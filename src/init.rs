use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::{NamedTempFile, TempDir};
use time::OffsetDateTime;

use crate::driver;
use crate::lock::{resolver_lock_path, FileLock};
use crate::runtime::{
    ir_cache_dir, nonempty_env, repeated_tooling_restart_error, resolve_rscript_command,
    rscript_command, tooling_process_crashed, tooling_restart_requested, TOOLING_SAFE_MODE_ENV,
};

const INIT_DRIVER: &str = concat!(
    include_str!("../driver/tooling.R"),
    "\n",
    include_str!("../driver/init.R")
);

pub(crate) fn cmd_init_script(file: &str) -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(file);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("cannot read script `{file}`: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("cannot initialize symbolic link `{file}`").into());
    }
    if !metadata.is_file() {
        return Err(format!("script `{file}` is not a regular file").into());
    }
    if !is_r_script(&path) {
        return Err(format!("script `{file}` must have a `.R` extension").into());
    }

    let contents =
        fs::read(&path).map_err(|error| format!("cannot read script `{file}`: {error}"))?;
    if contents.starts_with(b"\xef\xbb\xbf") {
        return Err(
            format!("script `{file}` starts with an unsupported UTF-8 byte order mark").into(),
        );
    }
    let header = r_script_header(&contents);
    if header.frontmatter {
        return Err(format!("`{file}` already contains ir frontmatter").into());
    }
    let body_start = header.shebang_end.unwrap_or(0);
    if contents[body_start..].starts_with(b"#|") {
        return Err(format!("`{file}` already starts with a #| metadata marker").into());
    }

    let absolute = absolute_path(&path)?;
    let result = discover_dependencies(&absolute)?;
    let exclude_newer = OffsetDateTime::now_utc().date().to_string();
    let replacement = initialized_contents(
        &contents,
        body_start,
        &result,
        &exclude_newer,
        newline_sequence(&contents),
    );
    replace_if_unchanged(&path, file, &contents, &replacement)?;

    println!("Initialized script at `{}`", path.display());
    Ok(())
}

struct ScriptHeader {
    shebang_end: Option<usize>,
    frontmatter: bool,
}

fn r_script_header(contents: &[u8]) -> ScriptHeader {
    let shebang_end = contents.starts_with(b"#!").then(|| line_end(contents, 0));
    let mut cursor = shebang_end.unwrap_or(0);
    let mut frontmatter = false;
    while contents
        .get(cursor..)
        .is_some_and(|rest| rest.starts_with(b"#| "))
    {
        frontmatter = true;
        cursor = line_end(contents, cursor);
    }
    ScriptHeader {
        shebang_end,
        frontmatter,
    }
}

fn line_end(contents: &[u8], start: usize) -> usize {
    contents[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(contents.len(), |position| start + position + 1)
}

fn is_r_script(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("r"))
}

fn absolute_path(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

struct InitResult {
    refs: Vec<String>,
    r_version: String,
}

fn discover_dependencies(script: &Path) -> Result<InitResult, Box<dyn Error>> {
    let cache_dir = ir_cache_dir()?;
    let _resolver_lock = FileLock::acquire(&resolver_lock_path(&cache_dir))?;
    let driver = driver::cached_path(&cache_dir, driver::INIT_FILE, INIT_DRIVER)?;
    let results = TempDir::new()?;
    let result_file = results.path().join("result.txt");
    let restart_file = results.path().join("restart.txt");
    let rscript = selected_rscript();
    let mut retried_restart = false;
    let mut retried_safe_mode = false;
    let mut safe_mode = false;

    let status = loop {
        let _ = fs::remove_file(&result_file);
        let _ = fs::remove_file(&restart_file);

        let mut command = Command::new(&rscript);
        command
            .arg("--vanilla")
            .arg(&driver)
            .stdin(Stdio::null())
            .stdout(io::stderr())
            .stderr(Stdio::inherit())
            .env("IR_CACHE_DIR", &cache_dir)
            .env("IR_INIT_SCRIPT", script)
            .env("IR_INIT_RESULT_FILE", &result_file)
            .env("IR_TOOLING_RESTART_FILE", &restart_file)
            .env_remove(TOOLING_SAFE_MODE_ENV);
        if safe_mode {
            command.env(TOOLING_SAFE_MODE_ENV, "1");
        }

        let status = command
            .status()
            .map_err(|error| init_spawn_error(&rscript, error))?;
        if tooling_restart_requested(&status, &restart_file) {
            if !retried_restart {
                retried_restart = true;
                continue;
            }
            return Err(
                repeated_tooling_restart_error("script initialization", &restart_file).into(),
            );
        }
        if tooling_process_crashed(&status) && !safe_mode && !retried_safe_mode {
            retried_safe_mode = true;
            safe_mode = true;
            continue;
        }
        break status;
    };

    if !status.success() {
        return Err("script dependency discovery failed".into());
    }
    parse_init_result(&result_file)
}

fn selected_rscript() -> OsString {
    nonempty_env("IR_RSCRIPT")
        .map(|command| resolve_rscript_command(&command))
        .unwrap_or_else(rscript_command)
}

fn init_spawn_error(rscript: &OsStr, error: io::Error) -> String {
    let rscript = Path::new(rscript).display();
    if error.kind() == io::ErrorKind::NotFound {
        format!("could not find `{rscript}` on PATH. Install R or set IR_RSCRIPT.")
    } else {
        format!("failed to start Rscript `{rscript}` for script dependency discovery: {error}")
    }
}

fn parse_init_result(path: &Path) -> Result<InitResult, Box<dyn Error>> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("script dependency discovery produced no result: {error}"))?;
    let mut lines = text.lines();
    let r_version = lines
        .next()
        .and_then(|line| line.strip_prefix("r-version="))
        .filter(|value| !value.is_empty())
        .ok_or("script dependency discovery produced an invalid R version")?
        .to_string();
    let refs = lines.map(str::to_string).collect::<Vec<_>>();
    if refs.iter().any(|reference| reference.is_empty()) {
        return Err("script dependency discovery produced an empty package ref".into());
    }
    Ok(InitResult { refs, r_version })
}

fn initialized_contents(
    contents: &[u8],
    body_start: usize,
    result: &InitResult,
    exclude_newer: &str,
    newline: &[u8],
) -> Vec<u8> {
    let mut output = Vec::with_capacity(contents.len() + 160 + result.refs.len() * 24);
    push_line(&mut output, b"#!/usr/bin/env -S ir run", newline);
    if result.refs.is_empty() {
        push_line(&mut output, b"#| packages: []", newline);
    } else {
        push_line(&mut output, b"#| packages:", newline);
        for reference in &result.refs {
            output.extend_from_slice(b"#|   - ");
            serde_json::to_writer(&mut output, reference)
                .expect("serializing a string into memory cannot fail");
            output.extend_from_slice(newline);
        }
    }
    output.extend_from_slice(b"#| r-version: \"");
    output.extend_from_slice(result.r_version.as_bytes());
    output.extend_from_slice(b"\"");
    output.extend_from_slice(newline);
    push_line(&mut output, b"#| isolated: true", newline);
    output.extend_from_slice(b"#| exclude-newer: \"");
    output.extend_from_slice(exclude_newer.as_bytes());
    output.extend_from_slice(b"\"");
    output.extend_from_slice(newline);
    output.extend_from_slice(newline);
    output.extend_from_slice(&contents[body_start..]);
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

fn replace_if_unchanged(
    path: &Path,
    display_path: &str,
    original: &[u8],
    replacement: &[u8],
) -> Result<(), Box<dyn Error>> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = NamedTempFile::new_in(parent).map_err(|error| {
        format!(
            "failed to create temporary file beside `{}`: {error}",
            path.display()
        )
    })?;
    temporary.write_all(replacement)?;
    temporary.as_file().sync_all()?;

    // Portable filesystems do not provide a path-level compare-and-replace.
    // Stage and sync first so this check is immediately before the atomic
    // rename. It detects edits during dependency discovery; a writer racing
    // the final handoff remains outside the command's concurrency contract.
    let current = fs::read(path).map_err(|error| {
        format!("cannot recheck script `{display_path}` before replacing it: {error}")
    })?;
    if current != original {
        return Err(format!("script `{display_path}` changed during initialization").into());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!("cannot recheck script `{display_path}` before replacing it: {error}")
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("script `{display_path}` changed during initialization").into());
    }

    temporary
        .as_file()
        .set_permissions(executable_permissions(metadata.permissions()))?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| {
        format!(
            "failed to replace script `{}` atomically: {}",
            path.display(),
            error.error
        )
    })?;
    Ok(())
}

#[cfg(unix)]
fn executable_permissions(mut permissions: fs::Permissions) -> fs::Permissions {
    use std::os::unix::fs::PermissionsExt as _;

    permissions.set_mode(permissions.mode() | 0o111);
    permissions
}

#[cfg(not(unix))]
fn executable_permissions(permissions: fs::Permissions) -> fs::Permissions {
    permissions
}
