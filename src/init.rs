use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use tempfile::NamedTempFile;

use crate::driver;
use crate::lock::{resolver_lock_path, FileLock};
use crate::runtime::{ir_cache_dir, nonempty_env, resolve_rscript_command, rscript_command};
use crate::script;

const INIT_DRIVER: &str = concat!(
    include_str!("../driver/tooling.R"),
    "\n",
    include_str!("../driver/init.R")
);
const TOOLING_RESTART_STATUS: i32 = 86;
const TOOLING_SAFE_MODE_ENV: &str = "IR_TOOLING_SAFE_MODE";

struct InitResult {
    refs: Vec<String>,
    r_version: String,
    lockfile: Option<PathBuf>,
}

pub(crate) fn cmd_init_script(file: &str, no_project: bool) -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(file);
    let metadata =
        fs::symlink_metadata(&path).map_err(|e| format!("cannot read script `{file}`: {e}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("cannot initialize symbolic link `{file}`").into());
    }
    if !metadata.is_file() {
        return Err(format!("script `{file}` is not a regular file").into());
    }
    if !is_r_script(&path) {
        return Err(format!("script `{file}` must have a `.R` extension").into());
    }

    let contents = fs::read(&path).map_err(|e| format!("cannot read script `{file}`: {e}"))?;
    if contents.starts_with(b"\xef\xbb\xbf") {
        return Err(
            format!("script `{file}` starts with an unsupported UTF-8 byte order mark").into(),
        );
    }
    let header = script::r_script_header(&contents);
    if header.frontmatter.is_some() {
        return Err(format!("`{file}` already contains ir frontmatter").into());
    }
    let metadata_start = header.shebang_end.unwrap_or(0);
    if contents[metadata_start..].starts_with(b"#|") {
        return Err(format!("`{file}` already starts with a #| metadata marker").into());
    }

    let absolute = absolute_path(&path)?;
    let lockfile = (!no_project)
        .then(|| nearest_renv_lockfile(&absolute))
        .flatten();
    let result = discover_dependencies(&absolute, lockfile.as_deref())?;
    let newline = newline_sequence(&contents);
    let replacement = initialized_contents(&contents, header.shebang_end, &result, newline);
    replace_file(&path, &replacement, metadata.permissions())?;

    if let Some(shebang_end) = header.shebang_end {
        let shebang = String::from_utf8_lossy(&contents[..shebang_end]);
        if !shebang_invokes_ir(&shebang) {
            eprintln!("warning: the existing shebang bypasses ir metadata");
            eprintln!("help: replace it with: #!/usr/bin/env -S ir run");
        }
    }
    if let Some(lockfile) = result.lockfile {
        eprintln!("Using renv lockfile at `{}`", lockfile.display());
    }
    println!("Initialized script at `{}`", path.display());
    Ok(())
}

fn is_r_script(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("r"))
}

fn absolute_path(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(env::current_dir()?.join(path))
}

fn nearest_renv_lockfile(script: &Path) -> Option<PathBuf> {
    script.parent()?.ancestors().find_map(|directory| {
        let candidate = directory.join("renv.lock");
        candidate.is_file().then_some(candidate)
    })
}

fn discover_dependencies(
    script: &Path,
    lockfile: Option<&Path>,
) -> Result<InitResult, Box<dyn Error>> {
    let cache_dir = ir_cache_dir()?;
    let _resolver_lock = FileLock::acquire(&resolver_lock_path(&cache_dir))?;
    let driver = driver::cached_path(&cache_dir, driver::INIT_FILE, INIT_DRIVER)?;
    let result_path = unique_path(&env::temp_dir(), "ir-init", "txt");
    let restart_path = unique_path(&env::temp_dir(), "ir-init-restart", "txt");
    let result_file = TempResultFile::new(result_path);
    let restart_file = TempResultFile::new(restart_path);
    let rscript = nonempty_env("IR_RSCRIPT")
        .map(|command| resolve_rscript_command(&command))
        .unwrap_or_else(rscript_command);
    let mut retried_restart = false;
    let mut retried_safe_mode = false;
    let mut safe_mode = false;

    let status = loop {
        result_file.clear();
        restart_file.clear();

        let mut command = Command::new(&rscript);
        command
            .args(["--vanilla"])
            .arg(&driver)
            .stdin(Stdio::null())
            .stdout(io::stderr())
            .stderr(Stdio::inherit())
            .env("IR_CACHE_DIR", &cache_dir)
            .env("IR_INIT_SCRIPT", script)
            .env("IR_INIT_RESULT_FILE", result_file.path())
            .env("IR_TOOLING_RESTART_FILE", restart_file.path())
            .env_remove("IR_INIT_LOCKFILE")
            .env_remove(TOOLING_SAFE_MODE_ENV);
        if let Some(lockfile) = lockfile {
            command.env("IR_INIT_LOCKFILE", lockfile);
        }
        if safe_mode {
            command.env(TOOLING_SAFE_MODE_ENV, "1");
        }

        let status = command.status().map_err(|e| spawn_error(&rscript, e))?;
        if tooling_restart_requested(&status, restart_file.path()) {
            if !retried_restart {
                retried_restart = true;
                continue;
            }
            let packages = fs::read_to_string(restart_file.path()).unwrap_or_default();
            let packages = packages.trim();
            let suffix = if packages.is_empty() {
                String::new()
            } else {
                format!(" for {packages}")
            };
            return Err(format!(
                "script initialization repeatedly requested a tooling restart{suffix}"
            )
            .into());
        }
        if process_crashed(&status) && !safe_mode && !retried_safe_mode {
            retried_safe_mode = true;
            safe_mode = true;
            continue;
        }
        break status;
    };

    if !status.success() {
        return Err("script dependency discovery failed".into());
    }
    parse_init_result(result_file.path(), lockfile)
}

fn parse_init_result(path: &Path, lockfile: Option<&Path>) -> Result<InitResult, Box<dyn Error>> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("script dependency discovery produced no result: {e}"))?;
    let mut lines = text.lines();
    let r_version = lines
        .next()
        .and_then(|line| line.strip_prefix("r-version="))
        .filter(|value| !value.is_empty())
        .ok_or("script dependency discovery produced an invalid R version")?
        .to_string();
    let reported_lockfile = lines
        .next()
        .and_then(|line| line.strip_prefix("lockfile="))
        .ok_or("script dependency discovery produced an invalid lockfile result")?;
    if lockfile.is_some() != !reported_lockfile.is_empty() {
        return Err("script dependency discovery reported the wrong lockfile state".into());
    }
    let refs: Vec<String> = lines.map(str::to_string).collect();
    if refs.iter().any(|reference| reference.is_empty()) {
        return Err("script dependency discovery produced an empty package ref".into());
    }
    Ok(InitResult {
        refs,
        r_version,
        lockfile: lockfile.map(Path::to_path_buf),
    })
}

fn initialized_contents(
    contents: &[u8],
    shebang_end: Option<usize>,
    result: &InitResult,
    newline: &[u8],
) -> Vec<u8> {
    let shebang_end = shebang_end.unwrap_or(0);
    let mut output = Vec::with_capacity(contents.len() + 160 + result.refs.len() * 24);
    if shebang_end == 0 {
        push_line(&mut output, b"#!/usr/bin/env -S ir run", newline);
    } else {
        output.extend_from_slice(&contents[..shebang_end]);
        if !output.ends_with(b"\n") {
            output.extend_from_slice(newline);
        }
    }

    if result.refs.is_empty() {
        push_line(&mut output, b"#| packages: []", newline);
    } else {
        push_line(&mut output, b"#| packages:", newline);
        for reference in &result.refs {
            output.extend_from_slice(b"#|   - ");
            output.extend_from_slice(reference.as_bytes());
            output.extend_from_slice(newline);
        }
    }
    output.extend_from_slice(b"#| r-version: \"");
    output.extend_from_slice(result.r_version.as_bytes());
    output.extend_from_slice(b"\"");
    output.extend_from_slice(newline);
    push_line(&mut output, b"#| isolated: true", newline);
    output.extend_from_slice(newline);
    output.extend_from_slice(&contents[shebang_end..]);
    output
}

fn push_line(output: &mut Vec<u8>, line: &[u8], newline: &[u8]) {
    output.extend_from_slice(line);
    output.extend_from_slice(newline);
}

fn newline_sequence(contents: &[u8]) -> &'static [u8] {
    let first_newline = contents.iter().position(|byte| *byte == b'\n');
    if first_newline.is_some_and(|position| position > 0 && contents[position - 1] == b'\r') {
        b"\r\n"
    } else {
        b"\n"
    }
}

fn shebang_invokes_ir(shebang: &str) -> bool {
    shebang
        .trim_end_matches(['\r', '\n'])
        .split_ascii_whitespace()
        .any(|word| word == "ir" || word.ends_with("/ir"))
}

fn replace_file(
    path: &Path,
    contents: &[u8],
    permissions: fs::Permissions,
) -> Result<(), Box<dyn Error>> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = NamedTempFile::new_in(parent).map_err(|e| {
        format!(
            "failed to create temporary file beside `{}`: {e}",
            path.display()
        )
    })?;
    temporary.write_all(contents)?;
    temporary.as_file().set_permissions(permissions)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| {
        format!(
            "failed to replace script `{}` atomically: {}",
            path.display(),
            e.error
        )
    })?;
    Ok(())
}

fn unique_path(parent: &Path, prefix: &str, extension: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    parent.join(format!(
        "{prefix}-{}-{nanos}.{extension}",
        std::process::id()
    ))
}

struct TempResultFile(PathBuf);

impl TempResultFile {
    fn new(path: PathBuf) -> Self {
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn clear(&self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl Drop for TempResultFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn tooling_restart_requested(status: &ExitStatus, restart_file: &Path) -> bool {
    status.code() == Some(TOOLING_RESTART_STATUS) && restart_file.exists()
}

fn process_crashed(status: &ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if status.signal().is_some() {
            return true;
        }
    }

    #[cfg(windows)]
    if let Some(code) = status.code() {
        let code = code as u32;
        if matches!(
            code,
            0xC0000005 | 0xC00000FD | 0xC000001D | 0xC0000374 | 0xC0000409
        ) {
            return true;
        }
    }

    status.code().is_none()
}

fn spawn_error(rscript: &OsStr, error: io::Error) -> String {
    let rscript = Path::new(rscript).display();
    format!("failed to start Rscript `{rscript}` for script dependency discovery: {error}")
}
