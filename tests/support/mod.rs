#![allow(dead_code)]

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn command_on_path(command: &str) -> Option<OsString> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(command))
        .find(|path| executable_file(path))
        .map(|path| path.into_os_string())
}

#[cfg(unix)]
fn executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;

    path.is_file()
        && fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn executable_file(path: &Path) -> bool {
    path.is_file()
}

static UNIQUE_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn ir() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ir"))
}

pub(crate) fn rx() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rx"))
}

pub(crate) fn ir_bin_name() -> String {
    Path::new(env!("CARGO_BIN_EXE_ir"))
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn rx_bin_name() -> String {
    Path::new(env!("CARGO_BIN_EXE_rx"))
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn rscript() -> OsString {
    std::env::var_os("IR_RSCRIPT")
        .filter(|value| !value.is_empty())
        .or_else(|| command_on_path("Rscript"))
        .unwrap_or_else(|| "Rscript".into())
}

pub(crate) fn rscript_from_r_binary(binary: &Path) -> PathBuf {
    let mut name = OsString::from("Rscript");
    if let Some(ext) = binary.extension() {
        name.push(".");
        name.push(ext);
    }
    binary.with_file_name(name)
}

pub(crate) fn rig_list() -> Result<Vec<InstalledRigR>, String> {
    let output = Command::new("rig")
        .args(["--quiet", "list", "--json"])
        .output()
        .map_err(|e| format!("failed to run `rig --quiet list --json`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`rig --quiet list --json` failed: {}",
            stderr.trim()
        ));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("failed to parse `rig --quiet list --json` JSON: {e}"))
}

pub(crate) fn normalize_cli_output(output: &[u8]) -> String {
    String::from_utf8_lossy(output)
        .replace("\r\n", "\n")
        .replace(&ir_bin_name(), "ir")
        .replace(&rx_bin_name(), "rx")
}

pub(crate) fn normalize_path_output(output: &Output) -> String {
    stdout(output).trim_end().replace('\\', "/")
}

pub(crate) fn renviron_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(unix)]
pub(crate) fn r_string(path: &Path) -> String {
    serde_json::to_string(&renviron_path(path)).unwrap()
}

pub(crate) fn assert_help_snapshot(name: &str, args: &[&str]) {
    let out = ir().args(args).output().unwrap();
    assert!(out.status.success(), "{args:?} should exit 0");
    assert!(out.stderr.is_empty(), "{args:?} should not write stderr");

    let snapshot = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(format!("{name}.stdout"));
    let expected = fs::read_to_string(&snapshot)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", snapshot.display()));
    let actual = normalize_cli_output(&out.stdout);
    assert_eq!(actual, expected, "{args:?} changed {}", snapshot.display());
}

pub(crate) fn assert_rx_help_snapshot(name: &str, args: &[&str]) {
    let out = rx().args(args).output().unwrap();
    assert!(out.status.success(), "{args:?} should exit 0");
    assert!(out.stderr.is_empty(), "{args:?} should not write stderr");

    let snapshot = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(format!("{name}.stdout"));
    let expected = fs::read_to_string(&snapshot)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", snapshot.display()));
    let actual = normalize_cli_output(&out.stdout);
    assert_eq!(actual, expected, "{args:?} changed {}", snapshot.display());
}

pub(crate) fn unique_path(prefix: &str, ext: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let id = UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    let mut path =
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}-{id}", std::process::id()));
    if !ext.is_empty() {
        path.set_extension(ext);
    }
    path
}

pub(crate) fn unique_dir(prefix: &str) -> PathBuf {
    let dir = unique_path(prefix, "");
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(target_os = "macos")]
pub(crate) fn tree_contains_dir_named(root: &Path, name: &str) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };

    entries.flatten().any(|entry| {
        let path = entry.path();
        path.is_dir()
            && (path.file_name() == Some(std::ffi::OsStr::new(name))
                || tree_contains_dir_named(&path, name))
    })
}

pub(crate) fn unique_dir_in(parent: &Path, prefix: &str) -> (PathBuf, OsString) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let id = UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    let name = OsString::from(format!("{prefix}-{}-{nanos}-{id}", std::process::id()));
    let dir = parent.join(&name);
    fs::create_dir_all(&dir).unwrap();
    (dir, name)
}

pub(crate) fn pin_quarto_r(command: &mut Command) {
    let rscript = rscript();
    let looks_like_path = rscript.to_string_lossy().contains(['/', '\\']);
    if looks_like_path || Path::new(&rscript).exists() {
        command.env("QUARTO_R", rscript);
    }
}

pub(crate) fn current_utc_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub(crate) fn copy_dir_files(source: &Path, destination: &Path) {
    for entry in fs::read_dir(source)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", source.display()))
    {
        let entry = entry.unwrap_or_else(|e| {
            panic!("failed to read fixture entry in {}: {e}", source.display())
        });
        let path = entry.path();
        if path.is_file() {
            fs::copy(&path, destination.join(entry.file_name())).unwrap_or_else(|e| {
                panic!(
                    "failed to copy fixture {} to {}: {e}",
                    path.display(),
                    destination.display()
                )
            });
        }
    }
}

pub(crate) fn fixture_copy(name: &str, prefix: &str) -> PathBuf {
    let source = fixture(name);
    let destination = unique_dir(prefix);
    copy_dir_files(&source, &destination);

    destination
}

pub(crate) fn docs_copy(prefix: &str) -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = manifest_dir.join("docs");
    let (destination, _) = unique_dir_in(manifest_dir, prefix);
    copy_dir_files(&source, &destination);

    destination
}

pub(crate) fn test_cache(prefix: &str) -> PathBuf {
    unique_dir(prefix)
}

pub(crate) fn write_r_source_package(
    root: &Path,
    name: &str,
    extra_description: &[String],
) -> PathBuf {
    let source = root.join(name);
    fs::create_dir_all(source.join("R")).unwrap();

    let mut description = vec![
        format!("Package: {name}"),
        "Version: 0.0.1".to_string(),
        format!("Title: {name} fixture"),
        format!("Description: {name} fixture."),
        "Authors@R: person(\"IR\", \"Fixture\", email = \"ir@example.com\", role = c(\"aut\", \"cre\"))".to_string(),
        "License: MIT".to_string(),
        "Encoding: UTF-8".to_string(),
    ];
    description.extend(extra_description.iter().cloned());
    fs::write(source.join("DESCRIPTION"), description.join("\n") + "\n").unwrap();
    fs::write(
        source.join("NAMESPACE"),
        "exportPattern(\"^[[:alpha:]]+\")\n",
    )
    .unwrap();
    fs::write(source.join("R").join("ok.R"), "ok <- function() TRUE\n").unwrap();

    source
}

pub(crate) fn output_text(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

pub(crate) fn assert_success(output: &Output) {
    assert!(output.status.success(), "{}", output_text(output));
}

pub(crate) fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

#[cfg(target_os = "macos")]
pub(crate) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}

pub(crate) fn assert_stdout_contains(output: &Output, needle: &str) {
    let text = stdout(output);
    assert!(
        text.contains(needle),
        "missing {needle:?}\n{}",
        output_text(output)
    );
}

pub(crate) fn assert_command_success(mut command: Command, label: &str) {
    let output = command
        .output()
        .unwrap_or_else(|e| panic!("failed to run {label}: {e}"));
    assert!(
        output.status.success(),
        "{label} failed\n{}",
        output_text(&output)
    );
}

#[cfg(unix)]
pub(crate) fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
pub(crate) fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    make_executable(path);
}

pub(crate) fn python_executable() -> PathBuf {
    for command in ["python3", "python"] {
        let output = Command::new(command)
            .args(["-c", "import sys; print(sys.executable)"])
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                let executable = String::from_utf8(output.stdout).unwrap().trim().to_string();
                if !executable.is_empty() {
                    return PathBuf::from(executable);
                }
            }
        }
    }

    panic!("python3 or python is required for the reticulate fixture");
}

pub(crate) fn python_minor_version() -> String {
    let output = Command::new(python_executable())
        .args([
            "-c",
            "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')",
        ])
        .output()
        .expect("failed to run python version probe");
    assert_success(&output);
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// Version of the default R on `PATH` — the one `ir` uses without `--r-version`.
/// `None` when that Rscript can't be run or reports nothing.
pub(crate) fn default_r_version() -> Option<String> {
    let out = Command::new(rscript())
        .args(["-e", "cat(as.character(getRversion()))"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!version.is_empty()).then_some(version)
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct InstalledRigR {
    name: String,
    version: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    binary: Option<PathBuf>,
}

impl InstalledRigR {
    fn matches(&self, req: &str) -> bool {
        self.name == req
            || self.version.as_deref() == Some(req)
            || self.aliases.iter().any(|alias| alias == req)
    }
}

pub(crate) fn rig_test_r_version(test_name: &str) -> Option<String> {
    let target = match std::env::var("IR_TEST_R_VERSION") {
        Ok(target) if !target.is_empty() => target,
        _ => {
            eprintln!("SKIP {test_name}: set IR_TEST_R_VERSION to a rig-installed R version");
            return None;
        }
    };

    let rig =
        rig_list().unwrap_or_else(|e| panic!("IR_TEST_R_VERSION={target} requires rig state: {e}"));
    let target_r = rig
        .iter()
        .find(|version| version.matches(&target))
        .unwrap_or_else(|| panic!("IR_TEST_R_VERSION={target} is not installed by rig"));
    let target_version = target_r
        .version
        .as_ref()
        .unwrap_or_else(|| panic!("IR_TEST_R_VERSION={target} has no version in rig state"));
    let ambient_r = default_r_version()
        .unwrap_or_else(|| panic!("IR_TEST_R_VERSION={target} requires a runnable ambient R"));
    assert_ne!(
        ambient_r.as_str(),
        target_version,
        "IR_TEST_R_VERSION={target} resolves to R {}, which matches the R used without --r-version",
        target_version
    );

    let binary = target_r
        .binary
        .as_ref()
        .unwrap_or_else(|| panic!("IR_TEST_R_VERSION={target} has no binary in rig state"));
    let rscript = rscript_from_r_binary(binary);
    assert!(
        rscript.exists(),
        "rig reports R {target} at `{}`, but `{}` does not exist",
        binary.display(),
        rscript.display()
    );

    Some(target)
}
