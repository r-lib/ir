//! Integration tests for the public `ir` CLI.
//!
//! These tests mostly avoid mocked `Rscript`, `quarto`, `rig`, or package
//! executable shims. The end-to-end cases run real fixture scripts/documents
//! through the compiled binary and assert marker lines printed by those public
//! workflows.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static UNIQUE_ID: AtomicU64 = AtomicU64::new(0);

fn ir() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ir"))
}

fn rx() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rx"))
}

fn ir_bin_name() -> String {
    Path::new(env!("CARGO_BIN_EXE_ir"))
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn rx_bin_name() -> String {
    Path::new(env!("CARGO_BIN_EXE_rx"))
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn rscript() -> OsString {
    std::env::var_os("IR_RSCRIPT")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Rscript".into())
}

fn rscript_from_r_binary(binary: &Path) -> PathBuf {
    let mut name = OsString::from("Rscript");
    if let Some(ext) = binary.extension() {
        name.push(".");
        name.push(ext);
    }
    binary.with_file_name(name)
}

fn rig_list() -> Result<Vec<InstalledRigR>, String> {
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

fn normalize_cli_output(output: &[u8]) -> String {
    String::from_utf8_lossy(output)
        .replace("\r\n", "\n")
        .replace(&ir_bin_name(), "ir")
        .replace(&rx_bin_name(), "rx")
}

fn normalize_path_output(output: &Output) -> String {
    stdout(output).trim_end().replace('\\', "/")
}

fn renviron_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(unix)]
fn r_string(path: &Path) -> String {
    serde_json::to_string(&renviron_path(path)).unwrap()
}

fn assert_help_snapshot(name: &str, args: &[&str]) {
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

fn assert_rx_help_snapshot(name: &str, args: &[&str]) {
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

fn unique_path(prefix: &str, ext: &str) -> PathBuf {
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

fn unique_dir(prefix: &str) -> PathBuf {
    let dir = unique_path(prefix, "");
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(target_os = "macos")]
fn tree_contains_dir_named(root: &Path, name: &str) -> bool {
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

fn unique_dir_in(parent: &Path, prefix: &str) -> (PathBuf, OsString) {
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

fn pin_quarto_r(command: &mut Command) {
    let rscript = rscript();
    let looks_like_path = rscript.to_string_lossy().contains(['/', '\\']);
    if looks_like_path || Path::new(&rscript).exists() {
        command.env("QUARTO_R", rscript);
    }
}

fn current_utc_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn copy_dir_files(source: &Path, destination: &Path) {
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

fn fixture_copy(name: &str, prefix: &str) -> PathBuf {
    let source = fixture(name);
    let destination = unique_dir(prefix);
    copy_dir_files(&source, &destination);

    destination
}

fn docs_copy(prefix: &str) -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = manifest_dir.join("docs");
    let (destination, _) = unique_dir_in(manifest_dir, prefix);
    copy_dir_files(&source, &destination);

    destination
}

fn test_cache(prefix: &str) -> PathBuf {
    unique_dir(prefix)
}

fn write_r_source_package(root: &Path, name: &str, extra_description: &[String]) -> PathBuf {
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

fn output_text(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{}", output_text(output));
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

#[cfg(target_os = "macos")]
fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}

fn assert_stdout_contains(output: &Output, needle: &str) {
    let text = stdout(output);
    assert!(
        text.contains(needle),
        "missing {needle:?}\n{}",
        output_text(output)
    );
}

fn assert_command_success(mut command: Command, label: &str) {
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
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    make_executable(path);
}

fn python_executable() -> PathBuf {
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

fn python_minor_version() -> String {
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
fn default_r_version() -> Option<String> {
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
struct InstalledRigR {
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

#[test]
fn rig_test_prerequisites_match_ir_test_r_version() {
    let _ = rig_test_r_version("rig_test_prerequisites_match_ir_test_r_version");
}

fn rig_test_r_version(test_name: &str) -> Option<String> {
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

#[test]
fn ci_dependencies_are_available() {
    let r_expr = concat!(
        "pkgs <- c(",
        "'pak', 'renv', 'secretbase', 'cli', 'glue', 'jsonlite', ",
        "'dplyr', 'tidyr', 'reticulate', 'knitr', 'rmarkdown', 'quarto', ",
        "'btw', 'Rapp', 'docopt', 'pkgsearch', 'prettyunits', 'fansi', ",
        "'htmltools'); ",
        "missing <- pkgs[!vapply(pkgs, requireNamespace, logical(1), quietly = TRUE)]; ",
        "if (length(missing)) { ",
        "stop('missing R packages: ', paste(missing, collapse = ', '), call. = FALSE) ",
        "}; ",
        "cat('ir.fixture=ci-deps\\n')",
    );

    let mut r = Command::new(rscript());
    r.args(["-e", r_expr]);
    assert_command_success(r, "R dependency probe");

    let mut quarto = Command::new("quarto");
    quarto.arg("--version");
    assert_command_success(quarto, "quarto --version");

    let version = python_minor_version();
    assert!(!version.is_empty());
}

#[test]
fn version_flag_reports_version() {
    let out = ir().arg("--version").output().unwrap();
    assert_success(&out);
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("ir 0."));
}

#[test]
fn rx_version_flag_reports_version() {
    for flag in ["--version", "-V"] {
        let out = rx().arg(flag).output().unwrap();
        assert_success(&out);
        assert!(
            String::from_utf8_lossy(&out.stdout).starts_with("rx 0."),
            "{}",
            output_text(&out)
        );
    }
}

#[test]
fn help_outputs_match_snapshots() {
    for (name, args) in [
        ("help", &["--help"][..]),
        ("help", &["-h"]),
        ("run-help", &["run", "--help"]),
        ("run-help", &["run", "-h"]),
        ("render-help", &["render", "--help"]),
        ("render-help", &["render", "-h"]),
        ("tool-help", &["tool", "--help"]),
        ("tool-help", &["tool", "-h"]),
        ("tool-run-help", &["tool", "run", "--help"]),
        ("tool-run-help", &["tool", "run", "-h"]),
        ("tool-install-help", &["tool", "install", "--help"]),
        ("tool-install-help", &["tool", "install", "-h"]),
        ("cache-help", &["cache", "--help"]),
        ("cache-help", &["cache", "-h"]),
        ("cache-clean-help", &["cache", "clean", "--help"]),
        ("cache-clean-help", &["cache", "clean", "-h"]),
        ("cache-dir-help", &["cache", "dir", "--help"]),
        ("cache-dir-help", &["cache", "dir", "-h"]),
    ] {
        assert_help_snapshot(name, args);
    }
}

#[test]
fn rx_help_outputs_match_snapshots() {
    for (name, args) in [("rx-help", &["--help"][..]), ("rx-help", &["-h"])] {
        assert_rx_help_snapshot(name, args);
    }
}

#[test]
fn cli_help_honors_clap_color_env() {
    let out = ir()
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .arg("--help")
        .output()
        .unwrap();
    assert_success(&out);

    let colored_stdout = stdout(&out);
    assert!(colored_stdout.contains("\u{1b}["), "{colored_stdout}");
    assert!(
        colored_stdout.contains("\u{1b}[94mUsage:"),
        "{colored_stdout}"
    );
    assert!(colored_stdout.contains("\u{1b}[36mir"), "{colored_stdout}");
    assert!(
        colored_stdout.contains("\u{1b}[90m[COMMAND]"),
        "{colored_stdout}"
    );
    assert!(!colored_stdout.contains("\u{1b}[32m"), "{colored_stdout}");
    assert!(!colored_stdout.contains("\u{1b}[33m"), "{colored_stdout}");
    assert!(!colored_stdout.contains("\u{1b}[4m"), "{colored_stdout}");

    let out = ir()
        .env("NO_COLOR", "1")
        .env_remove("CLICOLOR_FORCE")
        .arg("--help")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = stdout(&out);
    assert!(!stdout.contains("\u{1b}["), "{stdout}");
}

#[test]
fn help_section_headings_are_colored() {
    let colored_examples = "\u{1b}[1m\u{1b}[94mExamples:\u{1b}[0m";
    for args in [
        &["--help"][..],
        &["run", "--help"],
        &["render", "--help"],
        &["tool", "run", "--help"],
        &["tool", "install", "--help"],
    ] {
        let out = ir()
            .env_remove("NO_COLOR")
            .env("CLICOLOR_FORCE", "1")
            .args(args)
            .output()
            .unwrap();
        assert_success(&out);
        let stdout = stdout(&out);
        assert!(stdout.contains(colored_examples), "{args:?}:\n{stdout}");
    }

    let out = ir()
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .args(["tool", "--help"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = stdout(&out);
    assert!(
        stdout.contains("\u{1b}[1m\u{1b}[94mTools:\u{1b}[0m"),
        "{stdout}"
    );
}

#[cfg(unix)]
#[test]
fn docs_website_has_dark_mode_and_colored_reference_output() {
    use std::os::unix::fs::PermissionsExt;

    let docs_dir = docs_copy("ir-docs-reference-project");
    let (output_dir, output_dir_name) = unique_dir_in(&docs_dir, "ir-docs-reference-output");
    let bin_dir = unique_dir("ir-docs-reference-bin");
    let fake_cargo = bin_dir.join("cargo");
    let stale_ir = bin_dir.join("ir");
    let cargo_marker = output_dir.join("cargo-called");

    fs::write(
        &fake_cargo,
        concat!(
            "#!/bin/sh\n",
            "touch \"$IR_CARGO_MARKER\"\n",
            "exec \"$REAL_CARGO\" \"$@\"\n",
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(&fake_cargo).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_cargo, perms).unwrap();

    fs::write(
        &stale_ir,
        concat!(
            "#!/bin/sh\n",
            "echo \"error: unrecognized subcommand 'render'\" >&2\n",
            "exit 2\n",
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(&stale_ir).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&stale_ir, perms).unwrap();

    let config = fs::read_to_string(docs_dir.join("_quarto.yml")).unwrap();
    assert!(config.contains("light:"), "{config}");
    assert!(config.contains("dark:"), "{config}");
    assert!(config.contains("dark:\n        - cosmo"), "{config}");
    assert!(config.contains("- dark.scss"), "{config}");
    assert!(!config.contains("- darkly"), "{config}");

    let styles = fs::read_to_string(docs_dir.join("styles.css")).unwrap();
    assert!(styles.contains("quarto-dark"), "{styles}");
    assert!(
        styles.contains("pre.ir-cli-help span[style*=\"#5555FF\"]"),
        "{styles}"
    );
    assert!(
        styles.contains("pre.ir-cli-help span[style*=\"#00BBBB\"]"),
        "{styles}"
    );
    assert!(
        styles.contains("pre.ir-cli-help span[style*=\"#555555\"]"),
        "{styles}"
    );

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let mut quarto = Command::new("quarto");
    quarto
        .current_dir(&docs_dir)
        .env("PATH", path)
        .env_remove("IR_BIN")
        .env(
            "REAL_CARGO",
            std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")),
        )
        .env("IR_CARGO_MARKER", &cargo_marker)
        .args(["render", "reference.qmd", "--to", "html"])
        .arg("--output-dir")
        .arg(&output_dir_name);
    pin_quarto_r(&mut quarto);
    let output = quarto.output().unwrap();
    assert_success(&output);
    assert!(
        cargo_marker.exists(),
        "reference render should build the current ir binary"
    );

    let html = fs::read_to_string(output_dir.join("reference.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered reference page: {e}"));
    assert!(html.contains("data-mode=\"dark\""), "{html}");
    assert!(
        html.contains("Render a Quarto document or script"),
        "{html}"
    );
    assert!(html.contains("Options:"), "{html}");
    assert!(html.contains("color: #5555FF"), "{html}");
    assert!(html.contains("color: #00BBBB"), "{html}");
    assert!(html.contains("color: #555555"), "{html}");
    assert!(html.contains("font-weight: bold"), "{html}");
    assert!(!html.contains("\u{1b}["), "{html}");

    let _ = fs::remove_dir_all(&output_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&docs_dir);
}

#[test]
fn docs_run_page_dark_mode_styles_console_blocks() {
    let docs_dir = docs_copy("ir-docs-run-project");
    let (output_dir, output_dir_name) = unique_dir_in(&docs_dir, "ir-docs-run-output");

    let mut quarto = Command::new("quarto");
    quarto
        .current_dir(&docs_dir)
        .args(["render", "run.qmd", "--to", "html"])
        .arg("--output-dir")
        .arg(&output_dir_name);
    pin_quarto_r(&mut quarto);
    let output = quarto.output().unwrap();
    assert_success(&output);

    let html = fs::read_to_string(output_dir.join("run.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered run page: {e}"));
    assert!(html.contains("$ ir run script.R"), "{html}");

    assert!(html.contains("data-mode=\"dark\""), "{html}");

    let styles = fs::read_to_string(output_dir.join("styles.css"))
        .unwrap_or_else(|e| panic!("failed to read rendered styles.css: {e}"));
    assert!(styles.contains("body.quarto-dark .navbar"), "{styles}");
    assert!(styles.contains("pre.console"), "{styles}");
    assert!(
        styles.contains("background-color: var(--ir-panel)"),
        "{styles}"
    );
    assert!(
        styles.contains("background-color: var(--ir-help-panel)"),
        "{styles}"
    );

    let _ = fs::remove_dir_all(&output_dir);
    let _ = fs::remove_dir_all(&docs_dir);
}

#[test]
fn install_scripts_configure_default_path_entries() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let sh = fs::read_to_string(manifest_dir.join("scripts/install.sh")).unwrap();
    assert!(sh.contains("ensure_install_dir_on_path"), "{sh}");
    assert!(sh.contains("IR_NO_MODIFY_PATH"), "{sh}");
    assert!(sh.contains("ZDOTDIR"), "{sh}");
    assert!(sh.contains("Added ~/.local/bin to PATH in"), "{sh}");
    assert!(sh.contains("profile_display"), "{sh}");
    assert!(
        sh.contains("add ${INSTALL_DIR} to your PATH to run ${commands}"),
        "{sh}"
    );

    let ps1 = fs::read_to_string(manifest_dir.join("scripts/install.ps1")).unwrap();
    assert!(ps1.contains("Ensure-InstallDirOnPath"), "{ps1}");
    assert!(ps1.contains("IR_NO_MODIFY_PATH"), "{ps1}");
    assert!(
        ps1.contains("[Environment]::ExpandEnvironmentVariables($PathEntry)"),
        "{ps1}"
    );
    assert!(ps1.contains("Set-ItemProperty -Type ExpandString"), "{ps1}");
    assert!(ps1.contains("32767"), "{ps1}");
    assert!(ps1.contains("added $installDir to user PATH"), "{ps1}");

    let tool_rs = fs::read_to_string(manifest_dir.join("src/tool.rs")).unwrap();
    assert!(
        tool_rs.contains("[Environment]::ExpandEnvironmentVariables($PathEntry)"),
        "{tool_rs}"
    );
}

#[test]
fn clap_reports_public_usage_errors() {
    let cases = [
        (vec!["frobnicate"], "unrecognized subcommand 'frobnicate'"),
        (
            vec!["cache", "clean", "--bogus"],
            "unexpected argument '--bogus'",
        ),
        (vec!["run"], "requires a script"),
        (vec!["run", "--from", "btw", "btw"], "ir tool run"),
        (vec!["run", "-e"], "a value is required for '--expr <EXPR>'"),
        (
            vec!["render"],
            "the following required arguments were not provided",
        ),
        (vec!["render", "-e", "1"], "unexpected argument '-e'"),
        (
            vec!["tool", "run", "--from", "btw"],
            "`--from` requires a command",
        ),
        (
            vec!["tool", "run", "--from", "btw", "path/to/tool"],
            "`--from` requires a command name",
        ),
        (
            vec!["tool", "install"],
            "the following required arguments were not provided",
        ),
        (
            vec!["tool", "install", "-e", "1"],
            "unexpected argument '-e'",
        ),
        (
            vec!["tool", "install", "--bogus", "cli"],
            "unexpected argument '--bogus'",
        ),
    ];

    for (args, expected) in cases {
        let out = ir().args(args.clone()).output().unwrap();
        assert!(
            !out.status.success(),
            "args {args:?} unexpectedly succeeded\n{}",
            output_text(&out)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(expected),
            "args {args:?}\n{}",
            output_text(&out)
        );
    }
}

#[test]
fn rx_reports_public_usage_errors() {
    let cases = [
        (vec!["--from", "btw"], "`--from` requires a command"),
        (
            vec!["--from", "btw", "path/to/tool"],
            "`--from` requires a command name",
        ),
        (vec!["-w"], "a value is required for '--with <PKG>'"),
        (vec!["-e", "1"], "`-e` is not supported by `rx`"),
    ];

    for (args, expected) in cases {
        let out = rx().args(args.clone()).output().unwrap();
        assert!(
            !out.status.success(),
            "args {args:?} unexpectedly succeeded\n{}",
            output_text(&out)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(expected),
            "args {args:?}\n{}",
            output_text(&out)
        );
    }
}

#[test]
fn run_with_missing_script_errors() {
    let out = ir().args(["run", "/no/such/ir-script.R"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read script"));
}

#[test]
fn run_quarto_source_reports_render_subcommand() {
    let source = unique_path("ir-run-qmd-uses-render", "qmd");
    fs::write(&source, "---\nir: [\n---\n").unwrap();

    let out = ir().args(["run"]).arg(&source).output().unwrap();
    let _ = fs::remove_file(&source);

    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("use `ir render <source>`"),
        "{}",
        output_text(&out)
    );
}

#[test]
fn malformed_frontmatter_errors_before_resolution() {
    let script = unique_path("ir-malformed-frontmatter", "R");
    fs::write(
        &script,
        "#!/usr/bin/env -S ir run\n#| packages: [dplyr\n\ncat('not reached')\n",
    )
    .unwrap();

    let out = ir()
        .args(["run", script.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = fs::remove_file(&script);

    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("could not parse script frontmatter as YAML"),
        "{}",
        output_text(&out)
    );
}

#[test]
fn frontmatter_packages_must_be_sequence() {
    let script = unique_path("ir-packages-scalar-frontmatter", "R");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| packages: ""

cat('not reached')
"#,
    )
    .unwrap();

    let out = ir()
        .args(["run", script.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = fs::remove_file(&script);

    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("frontmatter `packages` must be a YAML sequence"),
        "{}",
        output_text(&out)
    );
}

#[test]
fn frontmatter_packages_null_means_empty_sequence() {
    let script = unique_path("ir-packages-null-frontmatter", "R");
    let cache_dir = test_cache("ir-packages-null-cache");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| packages: null

cat("ir.fixture=packages-null\n")
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--vanilla", script.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&cache_dir);

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=packages-null");
}

#[test]
fn run_script_frontmatter_accepts_packages_and_isolated() {
    let script = unique_path("ir-packages-frontmatter", "R");
    let cache_dir = test_cache("ir-packages-frontmatter-cache");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| packages:
#|   - glue
#| isolated: true
#| sys-reqs:
#|   - ignored-future-key

suppressPackageStartupMessages(library(glue))
lib <- strsplit(Sys.getenv("R_LIBS"), .Platform$path.sep, fixed = TRUE)[[1]][[1]]
expected <- normalizePath(file.path(lib, "glue"), mustWork = TRUE)
cat("ir.fixture=packages-frontmatter\n")
cat("frontmatter.glue_in_cache=", tolower(normalizePath(path.package("glue"), mustWork = TRUE) == expected), "\n", sep = "")
cat("frontmatter.user_library=", Sys.getenv("R_LIBS_USER", unset = "<unset>"), "\n", sep = "")
"#,
    )
    .unwrap();

    let user_library = unique_dir("ir-packages-frontmatter-user-library");
    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_LIBS_USER", &user_library)
        .args(["run", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&user_library);

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=packages-frontmatter");
    assert_stdout_contains(&out, "frontmatter.glue_in_cache=true");
    assert_stdout_contains(&out, "frontmatter.user_library=NULL");
}

#[test]
fn cache_dir_reports_override_and_process_env_defaults() {
    let cache_dir = unique_dir("ir-cache-override");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["cache", "dir"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(stdout(&out), format!("{}\n", cache_dir.display()));

    let r_user_cache_dir = unique_dir("ir-cache-r-user");
    let out = ir()
        .env_remove("IR_CACHE_DIR")
        .env("R_USER_CACHE_DIR", &r_user_cache_dir)
        .args(["cache", "dir"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(
        normalize_path_output(&out),
        r_user_cache_dir
            .join("R")
            .join("ir")
            .to_string_lossy()
            .replace('\\', "/")
    );

    let xdg_cache_home = unique_dir("ir-cache-xdg-default");
    let out = ir()
        .env_remove("IR_CACHE_DIR")
        .env_remove("R_USER_CACHE_DIR")
        .env("XDG_CACHE_HOME", &xdg_cache_home)
        .args(["cache", "dir"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(
        normalize_path_output(&out),
        xdg_cache_home
            .join("R")
            .join("ir")
            .to_string_lossy()
            .replace('\\', "/")
    );

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&r_user_cache_dir);
    let _ = fs::remove_dir_all(&xdg_cache_home);
}

#[cfg(windows)]
#[test]
fn cache_dir_falls_back_to_userprofile_without_localappdata() {
    let user_profile = unique_dir("ir-cache-userprofile");

    let out = ir()
        .env_remove("IR_CACHE_DIR")
        .env_remove("R_USER_CACHE_DIR")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("LOCALAPPDATA")
        .env("USERPROFILE", &user_profile)
        .args(["cache", "dir"])
        .output()
        .unwrap();
    assert_success(&out);

    let expected = user_profile
        .join("AppData")
        .join("Local")
        .join("R")
        .join("cache")
        .join("R")
        .join("ir")
        .to_string_lossy()
        .replace('\\', "/");
    assert_eq!(normalize_path_output(&out), expected);

    let _ = fs::remove_dir_all(&user_profile);
}

#[test]
fn cache_dir_ignores_r_user_cache_dir_from_r_environ_user() {
    let xdg_cache_home = unique_dir("ir-cache-xdg");
    let renviron_cache = unique_dir("ir-cache-renviron");
    let renviron = unique_path("ir-cache-renviron", "Renviron");
    fs::write(
        &renviron,
        format!("R_USER_CACHE_DIR={}\n", renviron_path(&renviron_cache)),
    )
    .unwrap();

    let out = ir()
        .env_remove("IR_CACHE_DIR")
        .env_remove("R_USER_CACHE_DIR")
        .env("XDG_CACHE_HOME", &xdg_cache_home)
        .env("R_ENVIRON_USER", &renviron)
        .args(["cache", "dir"])
        .output()
        .unwrap();
    assert_success(&out);

    let expected = xdg_cache_home
        .join("R")
        .join("ir")
        .to_string_lossy()
        .replace('\\', "/");
    assert_eq!(normalize_path_output(&out), expected);

    let _ = fs::remove_file(&renviron);
    let _ = fs::remove_dir_all(&renviron_cache);
    let _ = fs::remove_dir_all(&xdg_cache_home);
}

#[test]
fn cache_clean_removes_cache_dir() {
    let cache_dir = unique_dir("ir-cache-clean");
    let library = cache_dir.join("libraries").join("library");
    fs::create_dir_all(&library).unwrap();
    fs::write(library.join("pkg"), "cached").unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["cache", "clean"])
        .output()
        .unwrap();

    assert_success(&out);
    assert!(!cache_dir.exists());
    assert_stdout_contains(&out, &format!("Clearing cache at: {}", cache_dir.display()));
    assert_stdout_contains(&out, "Removed 1 file");
}

#[test]
fn run_script_fixture_resolves_packages_and_isolates_user_library() {
    let script = fixture("run/packages.R");
    let cache_dir = test_cache("ir-run-script-cache");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .args(["--script-arg", "value"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=run-script");
    assert_stdout_contains(&out, "script.args=--script-arg|value");
    assert_stdout_contains(&out, "script.lib_in_cache=true");
    assert_stdout_contains(&out, "script.user_library=NULL");
    assert_stdout_contains(
        &out,
        "script.packages=dplyr:true,tidyr:true,glue:true,jsonlite:true",
    );
    assert_stdout_contains(&out, "script.result=a:4,b:2");
    assert_stdout_contains(&out, "script.json={\"ok\":true,\"rows\":1}");
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_script_uses_only_the_first_yaml_document() {
    let script = fixture("run/multiple-documents.R");
    let cache_dir = test_cache("ir-multi-doc-cache");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=multi-doc");
    assert_stdout_contains(&out, "multi.packages=glue:true");
    assert_stdout_contains(&out, "multi.ignored_package=false");
    assert_stdout_contains(&out, "multi.result=5");
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_inline_expression_resolves_with_dependencies() {
    let cache_dir = test_cache("ir-inline-cache");
    let expr = concat!(
        "{",
        "library(cli); ",
        "library(glue); ",
        "lib <- strsplit(Sys.getenv('R_LIBS'), .Platform$path.sep, fixed = TRUE)[[1]][[1]]; ",
        "expected <- normalizePath(file.path(lib, c('cli', 'glue')), mustWork = TRUE); ",
        "pkg_in_cache <- normalizePath(path.package(c('cli', 'glue')), mustWork = TRUE) == expected; ",
        "cat('ir.fixture=inline\\n'); ",
        "cat('inline.args=', paste(commandArgs(TRUE), collapse = '|'), '\\n', sep = ''); ",
        "cat('inline.lib_in_cache=', tolower(all(pkg_in_cache)), '\\n', sep = ''); ",
        "cat('inline.pkgs_in_cache=', tolower(all(pkg_in_cache)), '\\n', sep = ''); ",
        "cat(glue::glue('inline.glue={1 + 1}\\n'))",
        "}",
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli,glue",
            "--vanilla",
            "-e",
            expr,
            "inline-arg",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=inline");
    assert_stdout_contains(&out, "inline.args=inline-arg");
    assert_stdout_contains(&out, "inline.lib_in_cache=true");
    assert_stdout_contains(&out, "inline.pkgs_in_cache=true");
    assert_stdout_contains(&out, "inline.glue=2");
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_inline_expression_forwards_option_like_args_after_expr() {
    let cache_dir = test_cache("ir-inline-args-cache");
    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args([
            "run",
            "--isolated",
            "--vanilla",
            "-e",
            "cat('inline.args=', paste(commandArgs(TRUE), collapse = '|'), '\\n', sep = '')",
            "--script-flag",
            "value",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "inline.args=--script-flag|value");
    assert!(
        !output_text(&out).contains("unknown option '--script-flag'"),
        "{}",
        output_text(&out)
    );
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_normalizes_version_specs_before_resolution_cache_keying() {
    let cache_dir = unique_dir("ir-ref-normalized-cache");
    let expr = "{ library(cli); cat('ir.fixture=normalized-cache\\n') }";

    for dep in ["cli==3.6.6", "cli@3.6.6"] {
        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .args(["run", "--isolated", "--with", dep, "--vanilla", "-e", expr])
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, "ir.fixture=normalized-cache");
    }

    let resolution_dir = cache_dir.join("resolutions");
    let resolution_count = fs::read_dir(&resolution_dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", resolution_dir.display()))
        .count();
    let _ = fs::remove_dir_all(&cache_dir);

    assert_eq!(resolution_count, 1);
}

#[test]
fn run_frontmatter_github_ref_installs_github_package() {
    let cache_dir = unique_dir("ir-github-ref-cache");
    let script = unique_path("ir-github-ref", "R");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| packages:
#|   - github::rstudio/reticulate@fix-windows-pwsh-uv-bootstrap

library(reticulate)
lib <- strsplit(Sys.getenv("R_LIBS"), .Platform$path.sep, fixed = TRUE)[[1]][[1]]
expected <- normalizePath(file.path(lib, "reticulate"), mustWork = TRUE)
loaded <- normalizePath(path.package("reticulate"), mustWork = TRUE)
desc_file <- system.file("DESCRIPTION", package = "reticulate")
desc <- as.list(read.dcf(desc_file)[1, ])
stopifnot(
  identical(loaded, expected),
  identical(desc$RemoteType, "github"),
  identical(desc$RemoteUsername, "rstudio"),
  identical(desc$RemoteRepo, "reticulate"),
  identical(desc$RemoteRef, "fix-windows-pwsh-uv-bootstrap"),
  nzchar(desc$RemoteSha)
)
cat("ir.fixture=github-ref\n")
cat("github.remote=", paste(
  desc$RemoteType,
  desc$RemoteUsername,
  desc$RemoteRepo,
  desc$RemoteRef,
  sep = "/"
), "\n", sep = "")
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("R_PROFILE_USER")
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=github-ref");
    assert_stdout_contains(
        &out,
        "github.remote=github/rstudio/reticulate/fix-windows-pwsh-uv-bootstrap",
    );

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_frontmatter_github_subdir_ref_installs_subdir_package() {
    let cache_dir = unique_dir("ir-github-subdir-ref-cache");
    let script = unique_path("ir-github-subdir-ref", "R");
    let sha = "a7c16d1ea299853694af95b3cdd3b7ab3e97fb0e";
    fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env -S ir run
#| packages:
#|   - r-lib/pkgdepends/tests/testthat/fixtures/foo@{}

library(foo)
lib <- strsplit(Sys.getenv("R_LIBS"), .Platform$path.sep, fixed = TRUE)[[1]][[1]]
expected <- normalizePath(file.path(lib, "foo"), mustWork = TRUE)
loaded <- normalizePath(path.package("foo"), mustWork = TRUE)
desc_file <- system.file("DESCRIPTION", package = "foo")
desc <- as.list(read.dcf(desc_file)[1, ])
stopifnot(
  identical(loaded, expected),
  identical(desc$RemoteType, "github"),
  identical(desc$RemoteUsername, "r-lib"),
  identical(desc$RemoteRepo, "pkgdepends"),
  identical(desc$RemoteRef, "{}"),
  identical(desc$RemoteSubdir, "tests/testthat/fixtures/foo"),
  nzchar(desc$RemoteSha)
)
cat("ir.fixture=github-subdir-ref\n")
cat("github.remote=", paste(
  desc$RemoteType,
  desc$RemoteUsername,
  desc$RemoteRepo,
  desc$RemoteSubdir,
  sep = "/"
), "\n", sep = "")
"#,
            sha, sha
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("R_PROFILE_USER")
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=github-subdir-ref");
    assert_stdout_contains(
        &out,
        "github.remote=github/r-lib/pkgdepends/tests/testthat/fixtures/foo",
    );

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_frontmatter_preserves_transitive_source_refs() {
    let cache_dir = unique_dir("ir-transitive-source-cache");
    let package_dir = unique_dir("ir-transitive-source-packages");
    let dep = write_r_source_package(&package_dir, "irdep", &[]);
    let parent = write_r_source_package(
        &package_dir,
        "irparent",
        &[
            "Imports: irdep".to_string(),
            format!("Remotes: irdep=local::{}", renviron_path(&dep)),
        ],
    );
    let script = unique_path("ir-transitive-source", "R");
    fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env -S ir run
#| packages:
#|   - local::{}

library(irparent)
library(irdep)
lib <- strsplit(Sys.getenv("R_LIBS"), .Platform$path.sep, fixed = TRUE)[[1]][[1]]
expected <- normalizePath(file.path(lib, c("irparent", "irdep")), mustWork = TRUE)
loaded <- normalizePath(path.package(c("irparent", "irdep")), mustWork = TRUE)
stopifnot(identical(loaded, expected))
cat("ir.fixture=transitive-source\n")
"#,
            renviron_path(&parent)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("R_PROFILE_USER")
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=transitive-source");

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&package_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_frontmatter_local_ref_reruns_resolution_when_package_changes() {
    let cache_dir = unique_dir("ir-local-ref-cache");
    let package_dir = unique_dir("ir-local-ref-packages");
    let package = write_r_source_package(&package_dir, "irlocal", &[]);
    let script = unique_path("ir-local-ref", "R");
    fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env -S ir run
#| packages:
#|   - local::{}

library(irlocal)
cat("ir.fixture=local-ref\n")
cat("irlocal.version=", as.character(packageVersion("irlocal")), "\n", sep = "")
"#,
            renviron_path(&package)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("R_PROFILE_USER")
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=local-ref");
    assert_stdout_contains(&out, "irlocal.version=0.0.1");

    let description_path = package.join("DESCRIPTION");
    let description = fs::read_to_string(&description_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", description_path.display()));
    fs::write(
        &description_path,
        description.replace("Version: 0.0.1", "Version: 0.0.2"),
    )
    .unwrap_or_else(|e| panic!("failed to write {}: {e}", description_path.display()));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("R_PROFILE_USER")
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=local-ref");
    assert_stdout_contains(&out, "irlocal.version=0.0.2");

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&package_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_frontmatter_local_ref_with_pak_params_installs_local_package() {
    let cache_dir = unique_dir("ir-local-ref-params-cache");
    let package_dir = unique_dir("ir-local-ref-params-packages");
    let package = write_r_source_package(&package_dir, "irlocal", &[]);
    let script = unique_path("ir-local-ref-params", "R");
    fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env -S ir run
#| packages:
#|   - local::{}?reinstall

library(irlocal)
cat("ir.fixture=local-ref-params\n")
"#,
            renviron_path(&package)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("R_PROFILE_USER")
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=local-ref-params");

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&package_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_frontmatter_named_local_ref_installs_local_package() {
    let cache_dir = unique_dir("ir-named-local-ref-cache");
    let package_dir = unique_dir("ir-named-local-ref-packages");
    let package = write_r_source_package(&package_dir, "irlocal", &[]);
    let script = unique_path("ir-named-local-ref", "R");
    fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env -S ir run
#| packages:
#|   - irlocal=local::{}

library(irlocal)
cat("ir.fixture=named-local-ref\n")
"#,
            renviron_path(&package)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("R_PROFILE_USER")
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=named-local-ref");

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&package_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_frontmatter_sequence_entry_preserves_space_containing_local_ref() {
    let cache_dir = unique_dir("ir-local-ref-spaces-cache");
    let package_dir = unique_dir("ir local ref spaces packages");
    let package = write_r_source_package(&package_dir, "irlocal", &[]);
    let script = unique_path("ir-local-ref-spaces", "R");
    fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env -S ir run
#| packages:
#|   - local::{}

library(irlocal)
cat("ir.fixture=local-ref-spaces\n")
"#,
            renviron_path(&package)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("R_PROFILE_USER")
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=local-ref-spaces");

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&package_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_latest_resolution_cache_marker_truncates_fractional_creation_time() {
    let cache_dir = unique_dir("ir-latest-cache-fractional-time");
    let profile = unique_path("ir-fractional-systime", "R");
    fs::write(
        &profile,
        "Sys.time <- function() as.POSIXct(1.9, origin = '1970-01-01', tz = 'UTC')\n",
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_PROFILE_USER", &profile)
        .args([
            "run",
            "--isolated",
            "--vanilla",
            "-e",
            "cat('ir.fixture=fractional-latest-marker\\n')",
        ])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=fractional-latest-marker");

    let resolution_dir = cache_dir.join("resolutions");
    let markers = fs::read_dir(&resolution_dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", resolution_dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 1);
    let marker_text = fs::read_to_string(&markers[0])
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", markers[0].display()));
    assert_eq!(marker_text.lines().next(), Some("latest: 1"));

    let _ = fs::remove_file(&profile);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_latest_resolution_cache_refreshes_marker_value_in_place() {
    let cache_dir = unique_dir("ir-latest-cache-refresh");
    let expr = "{ library(cli); cat('ir.fixture=latest-cache-refresh\\n') }";

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            expr,
        ])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=latest-cache-refresh");

    let resolution_dir = cache_dir.join("resolutions");
    let markers = fs::read_dir(&resolution_dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", resolution_dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 1);

    let marker = &markers[0];
    let marker_text = fs::read_to_string(marker)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", marker.display()));
    let mut lines = marker_text.lines();
    let created_at = lines
        .next()
        .and_then(|line| line.strip_prefix("latest: "))
        .and_then(|timestamp| timestamp.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("{} should record a latest timestamp", marker.display()));
    assert!(created_at <= current_utc_seconds());
    assert!(current_utc_seconds() - created_at <= 1);
    let library = lines
        .next()
        .unwrap_or_else(|| panic!("{} should record a library path", marker.display()));
    assert!(
        Path::new(library).is_dir(),
        "{} should record an existing library path",
        marker.display()
    );

    let still_fresh_created_at = current_utc_seconds() - 2;
    let still_fresh_marker_text = format!("latest: {still_fresh_created_at}\n{library}\n");
    fs::write(marker, &still_fresh_marker_text)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", marker.display()));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_LATEST_RESOLUTION_MAX_AGE_SECONDS", "60")
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            expr,
        ])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=latest-cache-refresh");

    let marker_text = fs::read_to_string(marker)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", marker.display()));
    assert_eq!(marker_text, still_fresh_marker_text);

    let future_created_at = current_utc_seconds() + 3600;
    let future_marker_text = format!("latest: {future_created_at}\n{library}\n");
    fs::write(marker, &future_marker_text)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", marker.display()));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            expr,
        ])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=latest-cache-refresh");

    let marker_text = fs::read_to_string(marker)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", marker.display()));
    assert_ne!(marker_text, future_marker_text);
    let refreshed_from_future_at = marker_text
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("latest: "))
        .and_then(|timestamp| timestamp.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("{} should record a latest timestamp", marker.display()));
    assert!(refreshed_from_future_at < future_created_at);
    assert!(refreshed_from_future_at <= current_utc_seconds());

    let stale_created_at = current_utc_seconds() - 86_401;
    fs::write(marker, format!("latest: {stale_created_at}\n{library}\n"))
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", marker.display()));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            expr,
        ])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=latest-cache-refresh");

    let markers = fs::read_dir(&resolution_dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", resolution_dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(markers, vec![marker.clone()]);

    let marker_text = fs::read_to_string(marker)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", marker.display()));
    let mut lines = marker_text.lines();
    let refreshed_at = lines
        .next()
        .and_then(|line| line.strip_prefix("latest: "))
        .and_then(|timestamp| timestamp.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("{} should record a latest timestamp", marker.display()));
    assert!(refreshed_at > stale_created_at);
    assert!(refreshed_at <= current_utc_seconds());
    let refreshed_library = lines
        .next()
        .unwrap_or_else(|| panic!("{} should record a library path", marker.display()));
    assert!(
        Path::new(refreshed_library).is_dir(),
        "{} should record an existing library path",
        marker.display()
    );

    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn run_passes_rust_owned_cache_dir_to_resolver() {
    let xdg_cache_home = unique_dir("ir-rust-owned-cache-xdg");
    let renviron_cache = unique_dir("ir-rust-owned-cache-renviron");
    let renviron = unique_path("ir-rust-owned-cache", "Renviron");
    fs::write(
        &renviron,
        format!("R_USER_CACHE_DIR={}\n", renviron_path(&renviron_cache)),
    )
    .unwrap();
    let expr = "{ library(cli); cat('ir.fixture=rust-owned-cache\\n') }";

    let out = ir()
        .env_remove("IR_CACHE_DIR")
        .env_remove("R_USER_CACHE_DIR")
        .env("XDG_CACHE_HOME", &xdg_cache_home)
        .env("R_ENVIRON_USER", &renviron)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            expr,
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=rust-owned-cache");
    assert!(
        xdg_cache_home
            .join("R")
            .join("ir")
            .join("resolutions")
            .is_dir(),
        "resolver should write markers under the Rust-owned cache root"
    );
    assert!(
        !renviron_cache
            .join("R")
            .join("ir")
            .join("resolutions")
            .exists(),
        "R startup files should not redirect the resolver cache"
    );

    let _ = fs::remove_file(&renviron);
    let _ = fs::remove_dir_all(&renviron_cache);
    let _ = fs::remove_dir_all(&xdg_cache_home);
}

// report.qmd deliberately does NOT declare rmarkdown, so the render only
// succeeds because ir injects it quietly for the knitr engine.
#[test]
fn render_quarto_fixture_injects_rmarkdown_and_renders() {
    let fixture_dir = fixture_copy("run", "ir-e2e-qmd-fixture");
    let cache_dir = unique_dir("ir-e2e-qmd-cache");

    for _ in 0..2 {
        let out = ir()
            .current_dir(&fixture_dir)
            .env("IR_CACHE_DIR", &cache_dir)
            .args(["render", "--isolated"])
            .arg("report.qmd")
            .args(["--to", "html"])
            .output()
            .unwrap();

        assert_success(&out);

        let html = fs::read_to_string(fixture_dir.join("report.html")).unwrap_or_else(|e| {
            panic!("failed to read rendered report: {e}\n{}", output_text(&out))
        });
        assert!(html.contains("ir.fixture=qmd"), "{html}");
        assert!(html.contains("qmd.lib_in_cache=true"), "{html}");
        assert!(html.contains("qmd.pkgs_in_cache=true"), "{html}");
        assert!(html.contains("qmd.result=a:4,b:2"), "{html}");

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("using latest rmarkdown"),
            "rmarkdown injection should be quiet\n{}",
            output_text(&out)
        );

        let _ = fs::remove_file(fixture_dir.join("report.html"));
        let _ = fs::remove_dir_all(fixture_dir.join("report_files"));
    }

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&fixture_dir);
}

// report-pinned.qmd declares rmarkdown itself, so the resolver leaves it alone.
#[test]
fn render_quarto_fixture_with_declared_rmarkdown_skips_injection() {
    let fixture_dir = fixture_copy("run", "ir-e2e-qmd-pinned-fixture");
    let cache_dir = unique_dir("ir-e2e-qmd-pinned-cache");

    let out = ir()
        .current_dir(&fixture_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["render", "--isolated"])
        .arg("report-pinned.qmd")
        .args(["--to", "html"])
        .output()
        .unwrap();

    assert_success(&out);

    let html = fs::read_to_string(fixture_dir.join("report-pinned.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered report: {e}\n{}", output_text(&out)));
    assert!(html.contains("ir.fixture=qmd-pinned"), "{html}");
    // The declared rmarkdown must load from the resolved run library, with its
    // version read from that library's DESCRIPTION.
    assert!(html.contains("pinned.rmarkdown_in_cache=true"), "{html}");
    assert!(html.contains("pinned.rmarkdown_version="), "{html}");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("using latest rmarkdown"),
        "rmarkdown injection should be quiet when rmarkdown is declared\n{}",
        output_text(&out)
    );

    let _ = fs::remove_file(fixture_dir.join("report-pinned.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("report-pinned_files"));
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&fixture_dir);
}

// report-transitive.qmd declares `quarto`, which Imports rmarkdown. The
// resolver sees rmarkdown already in the resolved set and skips its own seed.
#[test]
fn render_quarto_fixture_with_transitive_rmarkdown_renders() {
    let fixture_dir = fixture_copy("run", "ir-e2e-qmd-transitive-fixture");
    let cache_dir = unique_dir("ir-e2e-qmd-transitive-cache");

    let out = ir()
        .current_dir(&fixture_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["render", "--isolated"])
        .arg("report-transitive.qmd")
        .args(["--to", "html"])
        .output()
        .unwrap();

    assert_success(&out);

    let html = fs::read_to_string(fixture_dir.join("report-transitive.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered report: {e}\n{}", output_text(&out)));
    assert!(html.contains("ir.fixture=qmd-transitive"), "{html}");
    // Both the declared `bookdown` and the transitively-pulled rmarkdown must be
    // materialised into the resolved run library, with rmarkdown's version read
    // from that library's DESCRIPTION.
    assert!(html.contains("transitive.bookdown_in_cache=true"), "{html}");
    assert!(
        html.contains("transitive.rmarkdown_in_cache=true"),
        "{html}"
    );
    assert!(html.contains("transitive.rmarkdown_version="), "{html}");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("using latest rmarkdown"),
        "rmarkdown injection should be quiet when rmarkdown is a transitive dependency\n{}",
        output_text(&out)
    );

    let _ = fs::remove_file(fixture_dir.join("report-transitive.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("report-transitive_files"));
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&fixture_dir);
}

// report-bare.qmd declares no dependencies at all, so the resolver must still
// inject rmarkdown quietly for the knitr engine to render.
#[test]
fn render_quarto_bare_fixture_injects_rmarkdown() {
    let fixture_dir = fixture_copy("run", "ir-e2e-qmd-bare-fixture");
    let cache_dir = unique_dir("ir-e2e-qmd-bare-cache");

    for run in ["fresh resolution", "cached resolution"] {
        let out = ir()
            .current_dir(&fixture_dir)
            .env("IR_CACHE_DIR", &cache_dir)
            .args(["render", "--isolated"])
            .arg("report-bare.qmd")
            .args(["--to", "html"])
            .output()
            .unwrap();

        assert_success(&out);

        let html = fs::read_to_string(fixture_dir.join("report-bare.html")).unwrap_or_else(|e| {
            panic!("failed to read rendered report: {e}\n{}", output_text(&out))
        });
        assert!(html.contains("ir.fixture=qmd-bare"), "{html}");
        // The injected rmarkdown must be materialised into the resolved run
        // library, with its version read from that library's DESCRIPTION.
        assert!(html.contains("bare.rmarkdown_in_cache=true"), "{html}");
        assert!(html.contains("bare.rmarkdown_version="), "{html}");

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("using latest rmarkdown"),
            "rmarkdown injection should be quiet for {run}\n{}",
            output_text(&out)
        );
    }

    let _ = fs::remove_file(fixture_dir.join("report-bare.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("report-bare_files"));
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&fixture_dir);
}

#[test]
fn render_quarto_script_fixture_renders_with_dependencies() {
    let fixture_dir = fixture_copy("run", "ir-e2e-render-script-fixture");
    let cache_dir = unique_dir("ir-e2e-render-script-cache");

    let out = ir()
        .current_dir(&fixture_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["render", "--isolated", "--vanilla"])
        .arg("report-script.R")
        .args(["--to", "html"])
        .output()
        .unwrap();

    assert_success(&out);

    let html = fs::read_to_string(fixture_dir.join("report-script.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered report: {e}\n{}", output_text(&out)));
    assert!(html.contains("ir.fixture=render-script"), "{html}");
    assert!(html.contains("render.script.glue_in_cache=true"), "{html}");
    assert!(html.contains("render.script.vanilla=true"), "{html}");
    assert!(html.contains("render.script.result=4"), "{html}");

    let _ = fs::remove_file(fixture_dir.join("report-script.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("report-script_files"));
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&fixture_dir);
}

#[cfg(unix)]
#[test]
fn resolver_tooling_uses_compatible_user_library_packages() {
    let cache_dir = unique_dir("ir-compatible-tooling-cache");
    let user_library = unique_dir("ir-compatible-tooling-user-library");
    let fake_load_marker = unique_path("ir-compatible-secretbase-loaded", "txt");
    let profile = unique_path("ir-compatible-tooling-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
ir_test_write_pkg <- function(lib, pkg, namespace, code,
                              built = as.character(getRversion())) {{
  path <- file.path(lib, pkg)
  dir.create(file.path(path, "R"), recursive = TRUE, showWarnings = FALSE)
  dir.create(file.path(path, "Meta"), recursive = TRUE, showWarnings = FALSE)

  built_field <- paste0(
    "R ", built, "; ; 2026-01-01 00:00:00 UTC; ", .Platform$OS.type
  )
  description <- c(
    Package = pkg,
    Version = "0.0.1",
    Title = pkg,
    Description = paste0(pkg, "."),
    License = "MIT",
    Built = built_field
  )

  writeLines(paste(names(description), description, sep = ": "),
             file.path(path, "DESCRIPTION"))
  writeLines(namespace, file.path(path, "NAMESPACE"))
  writeLines(code, file.path(path, "R", pkg))
  saveRDS(
    list(
      DESCRIPTION = description,
      Built = list(
        R = package_version(built),
        Platform = "",
        Date = "2026-01-01 00:00:00 UTC",
        OStype = .Platform$OS.type
      ),
      Depends = NULL,
      Imports = NULL,
      LinkingTo = NULL,
      Suggests = NULL
    ),
    file.path(path, "Meta", "package.rds")
  )
}}

ir_test_write_pkg(
  Sys.getenv("R_LIBS_USER"),
  "secretbase",
  "export(sha256)",
  paste(
    paste0(".onLoad <- function(...) writeLines('loaded', ", deparse({}), ")"),
    "sha256 <- function(x) 'ambienthash'",
    sep = "\n"
  )
)
ir_test_write_pkg(
  Sys.getenv("R_LIBS_USER"),
  "pak",
  "export(pkg_deps)",
  paste(
    "pkg_deps <- function(refs, dependencies = NA, upgrade = TRUE) {{",
    "  refs <- as.character(refs)",
    "  data.frame(",
    "    status = rep('OK', length(refs)),",
    "    ref = refs,",
    "    package = sub('@.*$', '', refs),",
    "    version = rep('0.0.1', length(refs)),",
    "    type = rep('standard', length(refs)),",
    "    priority = NA_character_,",
    "    direct = TRUE,",
    "    stringsAsFactors = FALSE",
    "  )",
    "}}",
    sep = "\n"
  )
)
ir_test_write_pkg(
  Sys.getenv("R_LIBS_USER"),
  "renv",
  "export(use)",
  paste(
    "use <- function(..., library, repos, attach, sandbox, isolate, verbose) {{",
    "  specs <- unlist(list(...), use.names = FALSE)",
    "  for (spec in specs) {{",
    "    pkg <- sub('@.*$', '', spec)",
    "    dir.create(file.path(library, pkg), recursive = TRUE, showWarnings = FALSE)",
    "  }}",
    "  invisible(TRUE)",
    "}}",
    sep = "\n"
  )
)

utils::assignInNamespace("install.packages", function(...) {{
  stop("resolver should use compatible R_LIBS_USER tooling", call. = FALSE)
}}, ns = "utils")
"#,
            r_string(&fake_load_marker)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_LIBS_USER", &user_library)
        .env("R_PROFILE_USER", &profile)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            "cat('ir.fixture=compatible-tooling\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=compatible-tooling");
    assert!(
        fake_load_marker.exists(),
        "resolver should load compatible secretbase from R_LIBS_USER"
    );

    let _ = fs::remove_file(&profile);
    let _ = fs::remove_file(&fake_load_marker);
    let _ = fs::remove_dir_all(&user_library);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[cfg(unix)]
#[test]
fn resolver_tooling_ignores_wrong_r_minor_user_library_package() {
    let cache_dir = unique_dir("ir-ambient-tooling-cache");
    let ambient_library = unique_dir("ir-ambient-tooling-user-library");
    let fake_secretbase_load_marker = unique_path("ir-ambient-secretbase-loaded", "txt");
    let fake_pillar_load_marker = unique_path("ir-ambient-pillar-loaded", "txt");
    let profile = unique_path("ir-tooling-install-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
ir_test_write_pkg <- function(lib, pkg, namespace, code, built = NULL) {{
  path <- file.path(lib, pkg)
  dir.create(file.path(path, "R"), recursive = TRUE, showWarnings = FALSE)

  description <- c(
    Package = pkg,
    Version = "0.0.1",
    Title = pkg,
    Description = paste0(pkg, "."),
    License = "MIT"
  )
  if (!is.null(built)) {{
    built_field <- paste0(
      "R ", built, "; ; 2026-01-01 00:00:00 UTC; ", .Platform$OS.type
    )
    description <- c(description, Built = built_field)
  }}

  writeLines(paste(names(description), description, sep = ": "),
             file.path(path, "DESCRIPTION"))
  writeLines(namespace, file.path(path, "NAMESPACE"))
  writeLines(code, file.path(path, "R", pkg))

  if (!is.null(built)) {{
    dir.create(file.path(path, "Meta"), recursive = TRUE, showWarnings = FALSE)
    saveRDS(
      list(
        DESCRIPTION = description,
        Built = list(
          R = package_version(built),
          Platform = "",
          Date = "2026-01-01 00:00:00 UTC",
          OStype = .Platform$OS.type
        ),
        Depends = NULL,
        Imports = NULL,
        LinkingTo = NULL,
        Suggests = NULL
      ),
      file.path(path, "Meta", "package.rds")
    )
  }}
}}

ir_test_private_lib <- file.path(
  Sys.getenv("IR_CACHE_DIR"),
  "tooling",
  paste0(getRversion(), "-", R.version$platform)
)
ir_test_r_parts <- strsplit(as.character(getRversion()), ".", fixed = TRUE)[[1]]
ir_test_wrong_minor <- if (identical(ir_test_r_parts[[2]], "0")) "1" else "0"
ir_test_wrong_r <- paste(ir_test_r_parts[[1]], ir_test_wrong_minor, "0", sep = ".")

ir_test_write_pkg(
  Sys.getenv("R_LIBS_USER"),
  "secretbase",
  "export(sha256)",
  paste(
    paste0(".onLoad <- function(...) writeLines('loaded', ", deparse({}), ")"),
    "sha256 <- function(x) 'ambienthash'",
    sep = "\n"
  ),
  built = ir_test_wrong_r
)
ir_test_write_pkg(
  Sys.getenv("R_LIBS_USER"),
  "pillar",
  "export(pillar_shaft)",
  paste(
    paste0(".onLoad <- function(...) writeLines('loaded', ", deparse({}), ")"),
    "pillar_shaft <- function(x, ...) x",
    sep = "\n"
  ),
  built = ir_test_wrong_r
)
ir_test_write_pkg(
  ir_test_private_lib,
  "pak",
  "export(pkg_deps)",
  paste(
    "pkg_deps <- function(refs, dependencies = NA, upgrade = TRUE) {{",
    "  invisible(requireNamespace('pillar', quietly = TRUE))",
    "  refs <- as.character(refs)",
    "  data.frame(",
    "    status = rep('OK', length(refs)),",
    "    ref = refs,",
    "    package = sub('@.*$', '', refs),",
    "    version = rep('0.0.1', length(refs)),",
    "    type = rep('standard', length(refs)),",
    "    priority = NA_character_,",
    "    direct = TRUE,",
    "    stringsAsFactors = FALSE",
    "  )",
    "}}",
    sep = "\n"
  )
)
ir_test_write_pkg(
  ir_test_private_lib,
  "renv",
  "export(use)",
  paste(
    "use <- function(..., library, repos, attach, sandbox, isolate, verbose) {{",
    "  specs <- unlist(list(...), use.names = FALSE)",
    "  for (spec in specs) {{",
    "    pkg <- sub('@.*$', '', spec)",
    "    dir.create(file.path(library, pkg), recursive = TRUE, showWarnings = FALSE)",
    "  }}",
    "  invisible(TRUE)",
    "}}",
    sep = "\n"
  )
)

utils::assignInNamespace("install.packages", function(pkgs, lib, repos, ...) {{
  dir.create(lib, recursive = TRUE, showWarnings = FALSE)
  for (pkg in pkgs) {{
    if (!identical(pkg, "secretbase"))
      stop("unexpected resolver tooling package: ", pkg, call. = FALSE)
    ir_test_write_pkg(
      lib,
      "secretbase",
      "export(sha256)",
      "sha256 <- function(x) 'privatehash'"
    )
  }}
}}, ns = "utils")
"#,
            r_string(&fake_secretbase_load_marker),
            r_string(&fake_pillar_load_marker)
        ),
    )
    .unwrap();

    let first = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_LIBS_USER", &ambient_library)
        .env("R_PROFILE_USER", &profile)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            "cat('ir.fixture=ambient-tooling\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&first);
    assert_stdout_contains(&first, "ir.fixture=ambient-tooling");
    assert!(
        !fake_secretbase_load_marker.exists(),
        "resolver should not load secretbase from ambient R_LIBS_USER"
    );
    assert!(
        !fake_pillar_load_marker.exists(),
        "resolver should remove wrong-R-minor R_LIBS_USER before pak loads auxiliary packages"
    );

    let second = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_LIBS_USER", &ambient_library)
        .env("R_PROFILE_USER", &profile)
        .args([
            "run",
            "--isolated",
            "--with",
            "glue",
            "--vanilla",
            "-e",
            "cat('ir.fixture=ambient-tooling-warm\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&second);
    assert_stdout_contains(&second, "ir.fixture=ambient-tooling-warm");
    assert!(
        !fake_pillar_load_marker.exists(),
        "resolver should prune wrong-R-minor R_LIBS_USER even when private tooling is warm"
    );

    let _ = fs::remove_file(&profile);
    let _ = fs::remove_file(&fake_secretbase_load_marker);
    let _ = fs::remove_file(&fake_pillar_load_marker);
    let _ = fs::remove_dir_all(&ambient_library);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn r_version_selection_covers_render_flag_and_run_frontmatter() {
    const FIXTURE_R_VERSION: &str = "4.4.3";

    // Opt-in: needs rig plus a non-default R installed (CI provisions both).
    // `ir`'s `--r-version` path resolves through rig unconditionally, so with a
    // single R there is nothing to select. The frontmatter fixture pins 4.4.3,
    // so CI sets the same value to cover both public version-selection paths.
    let Ok(target) = std::env::var("IR_TEST_R_VERSION") else {
        eprintln!(
            "SKIP r_version_selection_covers_render_flag_and_run_frontmatter: set IR_TEST_R_VERSION={FIXTURE_R_VERSION}"
        );
        return;
    };

    if target != FIXTURE_R_VERSION {
        eprintln!(
            "SKIP r_version_selection_covers_render_flag_and_run_frontmatter: IR_TEST_R_VERSION ({target}) must match the fixture's `#| r-version`"
        );
        return;
    }

    // Selecting the version the default path already uses would prove nothing.
    if default_r_version().as_deref() == Some(FIXTURE_R_VERSION) {
        eprintln!(
            "SKIP r_version_selection_covers_render_flag_and_run_frontmatter: the fixture's R ({FIXTURE_R_VERSION}) matches the default R; nothing to select"
        );
        return;
    }

    let fixture_dir = fixture_copy("run", "ir-r-version-render-fixture");
    let cache_dir = test_cache("ir-r-version-cache");

    let render = ir()
        .current_dir(&fixture_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["render", "--isolated", "--r-version"])
        .arg(&target)
        .arg("r-version-select.qmd")
        .args(["--to", "html"])
        .output()
        .unwrap();

    assert_success(&render);

    let html = fs::read_to_string(fixture_dir.join("r-version-select.html")).unwrap_or_else(|e| {
        panic!(
            "failed to read rendered report: {e}\n{}",
            output_text(&render)
        )
    });
    assert!(html.contains("ir.fixture=r-version"), "{html}");
    assert!(
        html.contains(&format!("version.r_version=[{target}]")),
        "rendered under a different R than the requested {target}\n{html}"
    );
    assert!(html.contains("version.lib_in_cache=true"), "{html}");
    assert!(html.contains("version.jsonlite_in_cache=true"), "{html}");

    let script = fixture("run/r-version-frontmatter.R");

    let run = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&run);
    assert_stdout_contains(&run, "ir.fixture=r-version-frontmatter");
    assert_stdout_contains(&run, &format!("version.r_version=[{FIXTURE_R_VERSION}]"));
    assert_stdout_contains(&run, "version.lib_in_cache=true");
    assert_stdout_contains(&run, "version.jsonlite_in_cache=true");

    let _ = fs::remove_file(fixture_dir.join("r-version-select.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("r-version-select_files"));
    let _ = fs::remove_dir_all(&fixture_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[cfg(unix)]
#[test]
fn run_without_r_version_uses_rscript_on_path_when_rig_has_default() {
    let cache_dir = unique_dir("ir-path-rscript-cache");
    let bin_dir = unique_dir("ir-path-rscript-bin");
    let rig_dir = unique_dir("ir-path-rscript-rig");

    let path_rscript = bin_dir.join("Rscript");
    write_executable(
        &path_rscript,
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=path\n",
        ),
    );

    let rig_binary = rig_dir.join("R");
    let rig_rscript = rig_dir.join("Rscript");
    write_executable(
        &rig_rscript,
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=rig\n",
        ),
    );
    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "cat <<'JSON'\n",
                r#"[{{"name":"rig-default","version":"4.4.3","aliases":[],"default":true,"binary":"{}"}}]"#,
                "\nJSON\n",
            ),
            rig_binary.display()
        ),
    );

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=path");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&rig_dir);
}

#[cfg(unix)]
#[test]
fn run_without_r_version_skips_non_executable_rscript_on_path() {
    let cache_dir = unique_dir("ir-path-rscript-executable-cache");
    let stale_dir = unique_dir("ir-path-rscript-stale-bin");
    let bin_dir = unique_dir("ir-path-rscript-valid-bin");

    fs::write(stale_dir.join("Rscript"), "not executable\n").unwrap();
    write_executable(
        &bin_dir.join("Rscript"),
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=path\n",
        ),
    );

    let path = std::env::join_paths(
        [
            stale_dir.as_os_str().to_owned(),
            bin_dir.as_os_str().to_owned(),
        ]
        .into_iter()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=path");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&stale_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(unix)]
#[test]
fn render_without_r_version_pins_quarto_to_rscript_on_path() {
    let cache_dir = unique_dir("ir-render-path-rscript-cache");
    let bin_dir = unique_dir("ir-render-path-rscript-bin");
    let doc = unique_path("ir-render-path-rscript", "qmd");

    let rscript = bin_dir.join("Rscript");
    write_executable(
        &rscript,
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=path\n",
        ),
    );
    write_executable(
        &bin_dir.join("quarto"),
        concat!(
            "#!/bin/sh\n",
            "if [ \"${QUARTO_R:-}\" != \"$IR_EXPECTED_QUARTO_R\" ]; then\n",
            "  echo \"QUARTO_R=${QUARTO_R:-}\"\n",
            "  echo \"expected=$IR_EXPECTED_QUARTO_R\"\n",
            "  exit 2\n",
            "fi\n",
            "echo quarto_r=$QUARTO_R\n",
        ),
    );
    fs::write(&doc, "---\ntitle: render path rscript\n---\n").unwrap();
    let expected_rscript = fs::canonicalize(&rscript).unwrap();

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env("IR_EXPECTED_QUARTO_R", &expected_rscript)
        .env_remove("IR_RSCRIPT")
        .arg("render")
        .arg(&doc)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, &format!("quarto_r={}", expected_rscript.display()));

    let _ = fs::remove_file(&doc);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn run_without_r_version_uses_rscript_bat_on_path() {
    let cache_dir = unique_dir("ir-path-rscript-bat-cache");
    let bin_dir = unique_dir("ir-path-rscript-bat-bin");

    fs::write(
        bin_dir.join("Rscript.bat"),
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  type NUL > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
            "  exit /B 0\r\n",
            ")\r\n",
            "echo selected=bat\r\n",
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=bat");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn run_without_r_version_ignores_extensionless_rscript_on_path() {
    let cache_dir = unique_dir("ir-path-rscript-extensionless-cache");
    let stale_dir = unique_dir("ir-path-rscript-extensionless-stale");
    let bin_dir = unique_dir("ir-path-rscript-extensionless-valid");

    fs::write(stale_dir.join("Rscript"), "extensionless stub\r\n").unwrap();
    fs::write(
        bin_dir.join("Rscript.bat"),
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  type NUL > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
            "  exit /B 0\r\n",
            ")\r\n",
            "echo selected=bat\r\n",
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        [
            stale_dir.as_os_str().to_owned(),
            bin_dir.as_os_str().to_owned(),
        ]
        .into_iter()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=bat");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&stale_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn run_without_r_version_skips_unsupported_pathext_rscript_on_path() {
    let cache_dir = unique_dir("ir-path-rscript-unsupported-pathext-cache");
    let stale_dir = unique_dir("ir-path-rscript-unsupported-pathext-stale");
    let bin_dir = unique_dir("ir-path-rscript-unsupported-pathext-valid");

    fs::write(stale_dir.join("Rscript.JS"), "WScript.Echo('stale')\r\n").unwrap();
    fs::write(
        bin_dir.join("Rscript.bat"),
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  type NUL > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
            "  exit /B 0\r\n",
            ")\r\n",
            "echo selected=bat\r\n",
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        [
            stale_dir.as_os_str().to_owned(),
            bin_dir.as_os_str().to_owned(),
        ]
        .into_iter()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env("PATHEXT", ".JS;.BAT")
        .env_remove("IR_RSCRIPT")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=bat");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&stale_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn run_with_extended_rscript_command_skips_pathext_expansion() {
    let cache_dir = unique_dir("ir-extended-rscript-command-cache");
    let stale_dir = unique_dir("ir-extended-rscript-command-stale");
    let bin_dir = unique_dir("ir-extended-rscript-command-valid");

    fs::write(
        stale_dir.join("Rscript.bat.CMD"),
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  type NUL > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
            "  exit /B 0\r\n",
            ")\r\n",
            "echo selected=cmd\r\n",
        ),
    )
    .unwrap();
    fs::write(
        bin_dir.join("Rscript.bat"),
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  type NUL > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
            "  exit /B 0\r\n",
            ")\r\n",
            "echo selected=bat\r\n",
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        [
            stale_dir.as_os_str().to_owned(),
            bin_dir.as_os_str().to_owned(),
        ]
        .into_iter()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", "Rscript.bat")
        .env("PATH", path)
        .env("PATHEXT", ".CMD")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=bat");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&stale_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn run_without_r_version_ignores_non_rscript_batch_targets() {
    let cache_dir = unique_dir("ir-path-rscript-helper-target-cache");
    let bin_dir = unique_dir("ir-path-rscript-helper-target-bin");
    let helper = bin_dir.join("helper.exe");

    fs::write(&helper, "not an executable\r\n").unwrap();
    fs::write(
        bin_dir.join("Rscript.bat"),
        format!(
            concat!(
                "@echo off\r\n",
                "\"{}\"\r\n",
                "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
                "  type NUL > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
                "  exit /B 0\r\n",
                ")\r\n",
                "echo selected=bat\r\n",
            ),
            helper.display()
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=bat");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn run_without_r_version_does_not_cache_unresolved_rscript_bat() {
    let cache_dir = unique_dir("ir-path-rscript-bat-cache-miss");
    let bin_dir = unique_dir("ir-path-rscript-bat-bin");
    let library = unique_dir("ir-path-rscript-bat-library");
    let resolver_marker = unique_path("ir-path-rscript-bat-resolver", "txt");
    let resolver_script = bin_dir.join("resolve.ps1");

    fs::write(
        &resolver_script,
        concat!(
            "$library = $env:IR_TEST_LIBRARY\n",
            "New-Item -ItemType Directory -Force -Path $library | Out-Null\n",
            "Add-Content -Path $env:IR_TEST_RESOLVER_MARKER -Value 'resolve'\n",
            "if ($env:IR_RESOLUTION_MARKER) {\n",
            "  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $env:IR_RESOLUTION_MARKER) | Out-Null\n",
            "  $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()\n",
            "  Set-Content -Path $env:IR_RESOLUTION_MARKER -Value @(\"latest: $now\", $library)\n",
            "}\n",
            "Set-Content -Path $env:IR_RESOLVE_RESULT_FILE -Value $library\n",
        ),
    )
    .unwrap();
    fs::write(
        bin_dir.join("Rscript.bat"),
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  powershell -NoProfile -ExecutionPolicy Bypass -File \"%IR_TEST_RESOLVER_SCRIPT%\"\r\n",
            "  exit /B %ERRORLEVEL%\r\n",
            ")\r\n",
            "echo selected=bat\r\n",
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    for _ in 0..2 {
        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .env("PATH", &path)
            .env("IR_TEST_LIBRARY", &library)
            .env("IR_TEST_RESOLVER_MARKER", &resolver_marker)
            .env("IR_TEST_RESOLVER_SCRIPT", &resolver_script)
            .env_remove("IR_RSCRIPT")
            .args(["run", "--with", "cli", "-e", "cat('ignored')"])
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, "selected=bat");
    }

    let resolver_runs = fs::read_to_string(&resolver_marker).unwrap();
    assert_eq!(
        resolver_runs.lines().count(),
        2,
        "unresolved batch Rscript wrappers should not key the warm resolution cache"
    );

    let _ = fs::remove_file(&resolver_marker);
    let _ = fs::remove_dir_all(&library);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn render_without_r_version_pins_quarto_to_rscript_bat_target() {
    let cache_dir = unique_dir("ir-render-rscript-bat-target-cache");
    let bin_dir = unique_dir("ir-render-rscript-bat-target-bin");
    let doc = unique_path("ir-render-rscript-bat-target", "qmd");
    let target_rscript = PathBuf::from(rscript());

    if !target_rscript.is_file() {
        eprintln!(
            "SKIP render_without_r_version_pins_quarto_to_rscript_bat_target: default test Rscript is not a path"
        );
        return;
    }
    let expected_rscript = std::path::absolute(&target_rscript).unwrap();

    fs::write(
        bin_dir.join("Rscript.bat"),
        format!(
            "::test\r\n@echo off\r\n@\"{}\" %*\r\n",
            target_rscript.display()
        ),
    )
    .unwrap();
    fs::write(
        bin_dir.join("quarto.bat"),
        concat!(
            "@echo off\r\n",
            "if \"%QUARTO_R%\"==\"%IR_EXPECTED_QUARTO_R%\" (\r\n",
            "  echo quarto_r=%QUARTO_R%\r\n",
            "  exit /B 0\r\n",
            ")\r\n",
            "echo QUARTO_R=%QUARTO_R%\r\n",
            "echo expected=%IR_EXPECTED_QUARTO_R%\r\n",
            "exit /B 2\r\n",
        ),
    )
    .unwrap();
    fs::write(
        &doc,
        "---\ntitle: render batch target\nir:\n  exclude-newer: 2026-06-01\n---\n",
    )
    .unwrap();

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env("IR_QUARTO", bin_dir.join("quarto.bat"))
        .env("IR_EXPECTED_QUARTO_R", &expected_rscript)
        .env_remove("IR_RSCRIPT")
        .arg("render")
        .arg(&doc)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, &format!("quarto_r={}", expected_rscript.display()));

    let _ = fs::remove_file(&doc);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[test]
fn run_reticulate_fixture_imports_python_module() {
    let script = fixture("run/reticulate.R");
    let cache_dir = test_cache("ir-reticulate-cache");
    let managed_reticulate = std::env::var_os("IR_TEST_RETICULATE_MANAGED").is_some();

    let mut cmd = ir();
    cmd.env("IR_CACHE_DIR", &cache_dir);

    if managed_reticulate {
        cmd.env("IR_TEST_RETICULATE_MANAGED", "1")
            .env("IR_TEST_PYTHON_VERSION", python_minor_version())
            .env("RETICULATE_PYTHON", "managed");
    } else {
        cmd.env("RETICULATE_PYTHON", python_executable());
    }

    let out = cmd
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=reticulate");
    assert_stdout_contains(&out, "reticulate.lib_in_cache=true");
    assert_stdout_contains(&out, "reticulate.ephemeral=");
    assert_stdout_contains(&out, "reticulate.json={\"ok\": true}");
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn tool_run_executes_real_package_entrypoint() {
    let cache_dir = test_cache("ir-tool-run-real-cache");
    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args([
            "tool",
            "run",
            "--with",
            "docopt,pkgsearch,prettyunits",
            "--from",
            "cli",
            "search",
            "--help",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Seach for CRAN packages on r-pkg.org");
    assert_stdout_contains(&out, "cransearch.R [-h | --help]");
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn rx_executes_real_package_entrypoint() {
    let cache_dir = test_cache("ir-rx-real-cache");
    let out = rx()
        .env("IR_CACHE_DIR", &cache_dir)
        .args([
            "-w",
            "docopt,pkgsearch,prettyunits",
            "--from",
            "cli",
            "search",
            "--help",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Seach for CRAN packages on r-pkg.org");
    assert_stdout_contains(&out, "cransearch.R [-h | --help]");
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn tool_install_installs_real_package_entrypoint() {
    let cache_dir = test_cache("ir-tool-install-real-cache");
    let bin_dir = unique_dir("ir-e2e-tool-install-bin");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args([
            "tool",
            "install",
            "--with",
            "docopt,pkgsearch,prettyunits",
            "--bin-dir",
        ])
        .arg(&bin_dir)
        .arg("cli")
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Installed");
    assert_stdout_contains(&out, "search");

    let launcher = launcher_path(&bin_dir, "search");
    let out = Command::new(&launcher).arg("--help").output().unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Seach for CRAN packages on r-pkg.org");
    assert_stdout_contains(&out, "cransearch.R [-h | --help]");

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[cfg(target_os = "macos")]
#[test]
fn tool_install_adds_default_macos_bin_dir_to_zprofile_once() {
    let cache_dir = unique_dir("ir-tool-install-macos-path-cache");
    let home = unique_dir("ir-tool-install-macos-path-home");
    let default_bin_dir = home.join(".local").join("bin");
    fs::create_dir_all(&default_bin_dir).unwrap();
    let package_dir = unique_dir("ir-tool-install-macos-path-packages");
    let package = write_r_source_package(&package_dir, "irmacpath", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("hello.R"),
        r#"#!/usr/bin/env Rscript
cat("mac.path.fixture=TRUE\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("ZDOTDIR")
        .env_remove("IR_TOOL_BIN_DIR")
        .env_remove("RAPP_BIN_DIR")
        .env_remove("XDG_BIN_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("IR_NO_MODIFY_PATH")
        .args(["tool", "install"])
        .arg(&package_ref)
        .output()
        .unwrap();

    assert_success(&out);
    let first_stderr = stderr(&out);
    assert!(
        first_stderr.contains("Added ~/.local/bin to PATH in ~/.zprofile"),
        "{}",
        output_text(&out)
    );
    assert_stdout_contains(&out, "Installed");
    assert!(launcher_path(&default_bin_dir, "hello").exists());
    assert!(
        !tree_contains_dir_named(&cache_dir, "Rapp"),
        "PATH setup should not add a hidden Rapp dependency"
    );

    let zprofile = fs::read_to_string(home.join(".zprofile")).unwrap();
    assert_eq!(
        zprofile,
        concat!(
            "\n",
            "case \":$PATH:\" in\n",
            "  *:\"$HOME/.local/bin\":*) ;;\n",
            "  *) export PATH=\"$HOME/.local/bin:$PATH\" ;;\n",
            "esac\n"
        )
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("ZDOTDIR")
        .env_remove("IR_TOOL_BIN_DIR")
        .env_remove("RAPP_BIN_DIR")
        .env_remove("XDG_BIN_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("IR_NO_MODIFY_PATH")
        .args(["tool", "install", "--force"])
        .arg(&package_ref)
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "Installed");
    let second_stderr = stderr(&out);
    assert!(
        !second_stderr.contains("PATH"),
        "reinstall should not rerun PATH setup:\n{second_stderr}"
    );
    assert_eq!(
        fs::read_to_string(home.join(".zprofile")).unwrap(),
        zprofile
    );

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&package_dir);
}

#[cfg(target_os = "macos")]
#[test]
fn tool_install_custom_bin_dir_skips_default_macos_path_setup() {
    let cache_dir = unique_dir("ir-tool-install-custom-path-cache");
    let home = unique_dir("ir-tool-install-custom-path-home");
    let bin_dir = unique_path("ir-tool-install-custom-path-bin", "");
    let package_dir = unique_dir("ir-tool-install-custom-path-packages");
    let package = write_r_source_package(&package_dir, "irmaccustompath", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("hello.R"),
        r#"#!/usr/bin/env Rscript
cat("mac.custom.path.fixture=TRUE\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("ZDOTDIR")
        .env_remove("IR_TOOL_BIN_DIR")
        .env_remove("RAPP_BIN_DIR")
        .env_remove("XDG_BIN_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("IR_NO_MODIFY_PATH")
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg(&package_ref)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Installed");
    assert!(launcher_path(&bin_dir, "hello").exists());
    assert!(!home.join(".local").join("bin").exists());
    assert!(!home.join(".zprofile").exists());
    assert!(!stderr(&out).contains("PATH"));

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[cfg(target_os = "macos")]
#[test]
fn tool_install_existing_launcher_does_not_modify_zprofile() {
    let cache_dir = unique_dir("ir-tool-install-collision-path-cache");
    let home = unique_dir("ir-tool-install-collision-path-home");
    let default_bin_dir = home.join(".local").join("bin");
    fs::create_dir_all(&default_bin_dir).unwrap();
    fs::write(
        launcher_path(&default_bin_dir, "hello"),
        "existing launcher\n",
    )
    .unwrap();
    let package_dir = unique_dir("ir-tool-install-collision-path-packages");
    let package = write_r_source_package(&package_dir, "irmacpathcollision", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("hello.R"),
        r#"#!/usr/bin/env Rscript
cat("mac.path.collision.fixture=TRUE\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("ZDOTDIR")
        .env_remove("IR_TOOL_BIN_DIR")
        .env_remove("RAPP_BIN_DIR")
        .env_remove("XDG_BIN_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("IR_NO_MODIFY_PATH")
        .args(["tool", "install"])
        .arg(&package_ref)
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    let text = output_text(&out);
    assert!(
        text.contains("already exists; pass --force to overwrite it"),
        "{text}"
    );
    assert!(
        !home.join(".zprofile").exists(),
        "failed install should not write .zprofile\n{text}"
    );

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&package_dir);
}

#[cfg(target_os = "macos")]
#[test]
fn tool_install_write_failure_does_not_modify_zprofile() {
    use std::os::unix::fs::PermissionsExt as _;

    let cache_dir = unique_dir("ir-tool-install-write-failure-cache");
    let home = unique_dir("ir-tool-install-write-failure-home");
    let default_bin_dir = home.join(".local").join("bin");
    fs::create_dir_all(&default_bin_dir).unwrap();
    let original_permissions = fs::metadata(&default_bin_dir).unwrap().permissions();
    fs::set_permissions(&default_bin_dir, fs::Permissions::from_mode(0o555)).unwrap();

    let package_dir = unique_dir("ir-tool-install-write-failure-packages");
    let package = write_r_source_package(&package_dir, "irmacpathwritefailure", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("hello.R"),
        r#"#!/usr/bin/env Rscript
cat("mac.path.write.failure.fixture=TRUE\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("ZDOTDIR")
        .env_remove("IR_TOOL_BIN_DIR")
        .env_remove("RAPP_BIN_DIR")
        .env_remove("XDG_BIN_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("IR_NO_MODIFY_PATH")
        .args(["tool", "install"])
        .arg(&package_ref)
        .output()
        .unwrap();

    fs::set_permissions(&default_bin_dir, original_permissions).unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    let text = output_text(&out);
    assert!(text.contains("failed to write launcher"), "{text}");
    assert!(
        !home.join(".zprofile").exists(),
        "failed install should not write .zprofile\n{text}"
    );

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&package_dir);
}

#[test]
fn tool_run_and_install_rapp_package_frontend() {
    let cache_dir = unique_dir("ir-rapp-frontend-cache");
    let bin_dir = unique_dir("ir-rapp-frontend-bin");
    let app = unique_path("ir-rapp-frontend-app", "R");
    fs::write(
        &app,
        "#!/usr/bin/env Rapp\ncat(\"ir.fixture=rapp-frontend\\n\")\n",
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("R_PROFILE_USER")
        .args(["tool", "run", "--from", "Rapp", "Rapp"])
        .arg(&app)
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=rapp-frontend");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("R_PROFILE_USER")
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg("Rapp")
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "Rapp");

    let out = Command::new(launcher_path(&bin_dir, "Rapp"))
        .arg(&app)
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=rapp-frontend");

    let _ = fs::remove_file(&app);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn tool_run_and_install_use_launcher_metadata() {
    let cache_dir = unique_dir("ir-tool-launcher-metadata-cache");
    let bin_dir = unique_dir("ir-tool-launcher-metadata-bin");
    let package_dir = unique_dir("ir-tool-launcher-metadata-packages");
    let package = write_r_source_package(&package_dir, "irtoolmeta", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("default-name.R"),
        r#"#!/usr/bin/env Rscript
#| name: ignored-top-level
#| launcher:
#|   name: custom-tool
cat("launcher.name=", Sys.getenv("RAPP_LAUNCHER_NAME"), "\n", sep = "")
cat("utils.attached=", tolower("package:utils" %in% search()), "\n", sep = "")
cat("package.function.exists=", tolower(exists("ok")), "\n", sep = "")
"#,
    )
    .unwrap();
    fs::write(
        exec_dir.join("old-name.R"),
        r#"#!/usr/bin/env Rscript
#| launcher:
#|   name: new-name
cat("launcher.name=", Sys.getenv("RAPP_LAUNCHER_NAME"), "\n", sep = "")
cat("selected=renamed\n")
"#,
    )
    .unwrap();
    fs::write(
        exec_dir.join("actual-old.R"),
        r#"#!/usr/bin/env Rscript
#| launcher:
#|   name: old-name
cat("launcher.name=", Sys.getenv("RAPP_LAUNCHER_NAME"), "\n", sep = "")
cat("selected=actual\n")
"#,
    )
    .unwrap();
    fs::write(
        exec_dir.join("top-level.R"),
        r#"#!/usr/bin/env Rscript
#| name: top-level-tool
cat("launcher.name=", Sys.getenv("RAPP_LAUNCHER_NAME"), "\n", sep = "")
cat("selected=top-level\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "custom-tool"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "launcher.name=custom-tool");
    assert_stdout_contains(&out, "utils.attached=true");
    assert_stdout_contains(&out, "package.function.exists=false");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "old-name"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "launcher.name=old-name");
    assert_stdout_contains(&out, "selected=actual");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "top-level-tool"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "launcher.name=top-level-tool");
    assert_stdout_contains(&out, "selected=top-level");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg(&package_ref)
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "custom-tool");
    assert_stdout_contains(&out, "new-name");
    assert_stdout_contains(&out, "old-name");
    assert_stdout_contains(&out, "top-level-tool");
    assert!(
        !launcher_path(&bin_dir, "default-name").exists(),
        "launcher should use package launcher metadata"
    );

    let out = Command::new(launcher_path(&bin_dir, "custom-tool"))
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "launcher.name=custom-tool");
    assert_stdout_contains(&out, "utils.attached=true");
    assert_stdout_contains(&out, "package.function.exists=false");

    let out = Command::new(launcher_path(&bin_dir, "top-level-tool"))
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "launcher.name=top-level-tool");
    assert_stdout_contains(&out, "selected=top-level");

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[test]
fn tool_run_rejects_duplicate_launcher_metadata_names() {
    let cache_dir = unique_dir("ir-tool-duplicate-launcher-cache");
    let package_dir = unique_dir("ir-tool-duplicate-launcher-packages");
    let package = write_r_source_package(&package_dir, "irtooldupe", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("foo.R"),
        r#"#!/usr/bin/env Rscript
cat("selected=basename\n")
"#,
    )
    .unwrap();
    fs::write(
        exec_dir.join("renamed.R"),
        r#"#!/usr/bin/env Rscript
#| launcher:
#|   name: foo
cat("selected=metadata\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "foo"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "duplicate launchers should fail\n{}",
        output_text(&out)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("multiple package executables map to launcher `foo`"),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[test]
fn tool_run_ignores_non_r_direct_file_for_metadata_name() {
    let cache_dir = unique_dir("ir-tool-non-r-direct-cache");
    let package_dir = unique_dir("ir-tool-non-r-direct-packages");
    let package = write_r_source_package(&package_dir, "irtoolnonr", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(exec_dir.join("picked"), "not an R launcher\n").unwrap();
    fs::write(
        exec_dir.join("metadata.R"),
        r#"#!/usr/bin/env Rscript
#| launcher:
#|   name: picked
cat("selected=metadata\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "picked"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "selected=metadata");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[cfg(unix)]
#[test]
fn tool_run_and_install_support_direct_package_scripts() {
    let cache_dir = unique_dir("ir-tool-direct-script-cache");
    let bin_dir = unique_dir("ir-tool-direct-script-bin");
    let package_dir = unique_dir("ir-tool-direct-script-packages");
    let package = write_r_source_package(&package_dir, "irtooldirect", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    write_executable(
        &exec_dir.join("direct-sh"),
        "#!/bin/sh\nprintf 'tool.fixture=sh\\n'\nprintf 'tool.args=%s\\n' \"$*\"\n",
    );
    write_executable(
        &exec_dir.join("direct-python"),
        "#!/usr/bin/env python3\nimport sys\nprint('tool.fixture=python')\nprint('tool.args=' + '|'.join(sys.argv[1:]))\n",
    );
    write_executable(&exec_dir.join("native-tool"), "not a script\n");
    let package_ref = format!("local::{}", renviron_path(&package));

    for executable in [
        ("direct-sh", &["run", "sh"][..], "tool.fixture=sh"),
        ("direct-python", &["run", "python"], "tool.fixture=python"),
    ] {
        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .args(["tool", "run", "--from", &package_ref])
            .arg(executable.0)
            .args(executable.1)
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, executable.2);
    }

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg(&package_ref)
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "Installed 2 executables");

    for executable in [
        ("direct-sh", &["install", "sh"][..], "tool.fixture=sh"),
        (
            "direct-python",
            &["install", "python"],
            "tool.fixture=python",
        ),
    ] {
        let out = Command::new(launcher_path(&bin_dir, executable.0))
            .args(executable.1)
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, executable.2);
    }

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[test]
fn tool_run_skips_binary_exec_files() {
    let cache_dir = unique_dir("ir-tool-binary-exec-cache");
    let package_dir = unique_dir("ir-tool-binary-exec-packages");
    let package = write_r_source_package(&package_dir, "irtoolbinary", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(exec_dir.join("helper.bin"), [0xff, 0xfe, b'\n']).unwrap();
    fs::write(
        exec_dir.join("valid-tool.R"),
        r#"#!/usr/bin/env Rscript
cat("selected=valid\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "valid-tool"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "selected=valid");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[test]
fn tool_install_rejects_invalid_metadata_launcher_names() {
    for (package_name, launcher_name) in [
        ("irtoolbadname", "bad?name"),
        ("irtoolpercentname", "foo%PATH%"),
        ("irtooldotname", "."),
        ("irtooldotdotname", ".."),
    ] {
        let cache_dir = unique_dir("ir-tool-invalid-launcher-cache");
        let bin_dir = unique_dir("ir-tool-invalid-launcher-bin");
        let package_dir = unique_dir("ir-tool-invalid-launcher-packages");
        let package = write_r_source_package(&package_dir, package_name, &[]);
        let exec_dir = package.join("exec");
        fs::create_dir_all(&exec_dir).unwrap();
        fs::write(
            exec_dir.join("invalid.R"),
            format!(
                r#"#!/usr/bin/env Rscript
#| launcher:
#|   name: {launcher_name}
cat("not reached\n")
"#
            ),
        )
        .unwrap();
        let package_ref = format!("local::{}", renviron_path(&package));

        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .args(["tool", "install", "--bin-dir"])
            .arg(&bin_dir)
            .arg(&package_ref)
            .output()
            .unwrap();
        assert!(
            !out.status.success(),
            "invalid launcher names should fail\n{}",
            output_text(&out)
        );
        assert!(
            String::from_utf8_lossy(&out.stderr)
                .contains(&format!("unsupported launcher name `{launcher_name}`")),
            "{}",
            output_text(&out)
        );

        let _ = fs::remove_dir_all(&bin_dir);
        let _ = fs::remove_dir_all(&cache_dir);
        let _ = fs::remove_dir_all(&package_dir);
    }
}

#[test]
fn tool_run_limits_metadata_lookup_to_primary_package() {
    let cache_dir = unique_dir("ir-tool-primary-package-cache");
    let package_dir = unique_dir("ir-tool-primary-package-packages");
    let dep = write_r_source_package(&package_dir, "irtooldep", &[]);
    let dep_exec_dir = dep.join("exec");
    fs::create_dir_all(&dep_exec_dir).unwrap();
    fs::write(
        dep_exec_dir.join("dep-tool.R"),
        r#"#!/usr/bin/env Rscript
#| launcher:
#|   name: picked
cat("selected=dependency\n")
"#,
    )
    .unwrap();

    let package = write_r_source_package(
        &package_dir,
        "irtoolprimary",
        &[
            "Imports: irtooldep".to_string(),
            format!("Remotes: irtooldep=local::{}", renviron_path(&dep)),
        ],
    );
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("picked.R"),
        r#"#!/usr/bin/env Rscript
cat("selected=primary\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "picked"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "selected=primary");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[test]
fn tool_run_and_install_apply_package_default_packages() {
    let cache_dir = unique_dir("ir-tool-default-packages-cache");
    let bin_dir = unique_dir("ir-tool-default-packages-bin");
    let package_dir = unique_dir("ir-tool-default-packages-packages");
    let package = write_r_source_package(
        &package_dir,
        "irtooldefaults",
        &["Imports: Rapp".to_string()],
    );
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("no-launcher.R"),
        r#"#!/usr/bin/env Rapp
#| name: no-launcher-app

cat("package.function=", ok(), "\n", sep = "")
"#,
    )
    .unwrap();
    fs::write(
        exec_dir.join("null-default.R"),
        r#"#!/usr/bin/env Rscript
#| launcher:
#|   default-packages: null
cat("base.attached=", tolower("package:base" %in% search()), "\n", sep = "")
cat("stats.attached=", tolower("package:stats" %in% search()), "\n", sep = "")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "no-launcher-app"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "package.function=TRUE");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "null-default"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "base.attached=true");
    assert_stdout_contains(&out, "stats.attached=false");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg(&package_ref)
        .output()
        .unwrap();
    assert_success(&out);

    let out = Command::new(launcher_path(&bin_dir, "no-launcher-app"))
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "package.function=TRUE");

    let out = Command::new(launcher_path(&bin_dir, "null-default"))
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "base.attached=true");
    assert_stdout_contains(&out, "stats.attached=false");

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[cfg(unix)]
#[test]
fn tool_install_warm_resolution_cache_skips_resolver_rscript() {
    let cache_dir = unique_dir("ir-warm-tool-install-cache");
    let bin_dir = unique_dir("ir-warm-tool-install-bin");
    let rscript = rscript();
    let profile = unique_path("ir-rprofile-fail", "R");
    fs::write(
        &profile,
        "stop('resolver Rscript should not be launched')\n",
    )
    .unwrap();

    let warm = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", &rscript)
        .env_remove("R_PROFILE_USER")
        .args([
            "tool",
            "install",
            "--with",
            "docopt,pkgsearch,prettyunits",
            "--bin-dir",
        ])
        .arg(&bin_dir)
        .arg("cli")
        .output()
        .unwrap();
    assert_success(&warm);

    let cached = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", &rscript)
        .env("R_PROFILE_USER", &profile)
        .args([
            "tool",
            "install",
            "--force",
            "--with",
            "docopt,pkgsearch,prettyunits",
            "--bin-dir",
        ])
        .arg(&bin_dir)
        .arg("cli")
        .output()
        .unwrap();

    assert_success(&cached);
    assert_stdout_contains(&cached, "Installed");

    let _ = fs::remove_file(&profile);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[cfg(unix)]
#[test]
fn tool_install_with_path_rscript_symlink_records_target() {
    let cache_dir = unique_dir("ir-tool-install-rscript-link-cache");
    let bin_dir = unique_dir("ir-tool-install-rscript-link-bin");
    let link_dir = unique_dir("ir-tool-install-rscript-link-path");
    let target_dir = unique_dir("ir-tool-install-rscript-link-target");
    let library = unique_dir("ir-tool-install-rscript-link-library");
    let package = library.join("irfake");
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    write_executable(
        &exec_dir.join("hello.R"),
        "#!/usr/bin/env Rscript\ncat('hello\\n')\n",
    );

    let target_rscript = target_dir.join("Rscript");
    write_executable(
        &target_rscript,
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  printf '%s\\n' \"$IR_TEST_LIBRARY\" > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  printf '%s\\n' irfake > \"$IR_RESOLVE_PACKAGE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo target-rscript\n",
        ),
    );
    let link_rscript = link_dir.join("Rscript");
    std::os::unix::fs::symlink(&target_rscript, &link_rscript).unwrap();

    let path = std::env::join_paths(
        std::iter::once(link_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg("irfake")
        .output()
        .unwrap();

    assert_success(&out);
    let launcher = fs::read_to_string(launcher_path(&bin_dir, "hello")).unwrap();
    let target = fs::canonicalize(&target_rscript).unwrap();
    assert!(
        launcher.contains(&target.to_string_lossy().into_owned()),
        "{launcher}"
    );
    assert!(
        !launcher.contains(&link_rscript.to_string_lossy().into_owned()),
        "{launcher}"
    );

    let _ = fs::remove_dir_all(&library);
    let _ = fs::remove_dir_all(&target_dir);
    let _ = fs::remove_dir_all(&link_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[cfg(unix)]
#[test]
fn tool_install_with_rscript_wrapper_records_primary_package_marker() {
    let cache_dir = unique_dir("ir-wrapper-tool-install-cache");
    let bin_dir = unique_dir("ir-wrapper-tool-install-bin");
    let wrapper = unique_path("ir-rscript-wrapper", "sh");
    fs::write(
        &wrapper,
        "#!/bin/sh\nexec \"$IR_TEST_RSCRIPT_TARGET\" \"$@\"\n",
    )
    .unwrap();
    make_executable(&wrapper);

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", &wrapper)
        .env("IR_TEST_RSCRIPT_TARGET", rscript())
        .args([
            "tool",
            "install",
            "--with",
            "docopt,pkgsearch,prettyunits",
            "--bin-dir",
        ])
        .arg(&bin_dir)
        .arg("cli")
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Installed");

    let _ = fs::remove_file(&wrapper);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

fn launcher_path(bin_dir: &Path, name: &str) -> PathBuf {
    #[cfg(unix)]
    {
        bin_dir.join(name)
    }

    #[cfg(not(unix))]
    {
        bin_dir.join(format!("{name}.cmd"))
    }
}
