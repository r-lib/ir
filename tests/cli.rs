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
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

static UNIQUE_ID: AtomicU64 = AtomicU64::new(0);
static E2E_LOCK: Mutex<()> = Mutex::new(());

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
    if let Some(rscript) = std::env::var_os("IR_RSCRIPT").filter(|value| !value.is_empty()) {
        return rscript;
    }

    if let Some(rscript) = rig_default_rscript() {
        return rscript.into_os_string();
    }

    "Rscript".into()
}

fn rig_default_rscript() -> Option<PathBuf> {
    let output = Command::new("rig").args(["list", "--json"]).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let versions: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let default = versions
        .as_array()?
        .iter()
        .find(|version| version.get("default").and_then(|value| value.as_bool()) == Some(true))?;
    let rscript = rscript_from_r_binary(Path::new(default.get("binary")?.as_str()?));
    rscript.exists().then_some(rscript)
}

fn rscript_from_r_binary(binary: &Path) -> PathBuf {
    let mut name = OsString::from("Rscript");
    if let Some(ext) = binary.extension() {
        name.push(".");
        name.push(ext);
    }
    binary.with_file_name(name)
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

fn current_utc_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn e2e_lock() -> MutexGuard<'static, ()> {
    E2E_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
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
fn cli_help_honors_clicolor_force() {
    let out = ir()
        .env("CLICOLOR_FORCE", "1")
        .arg("--help")
        .output()
        .unwrap();
    assert_success(&out);

    let stdout = stdout(&out);
    assert!(stdout.contains("\u{1b}["), "{stdout}");
    assert!(stdout.contains("\u{1b}[32mUsage:"), "{stdout}");
    assert!(stdout.contains("\u{1b}[94mir"), "{stdout}");
    assert!(stdout.contains("\u{1b}[94m[COMMAND]"), "{stdout}");
    assert!(!stdout.contains("\u{1b}[36m"), "{stdout}");
    assert!(!stdout.contains("\u{1b}[33m"), "{stdout}");
    assert!(!stdout.contains("\u{1b}[4m"), "{stdout}");
}

#[test]
fn examples_help_headings_are_colored() {
    let colored_examples = "\u{1b}[1m\u{1b}[32mExamples:\u{1b}[0m";
    for args in [
        &["--help"][..],
        &["run", "--help"],
        &["render", "--help"],
        &["tool", "run", "--help"],
        &["tool", "install", "--help"],
    ] {
        let out = ir().env("CLICOLOR_FORCE", "1").args(args).output().unwrap();
        assert_success(&out);
        let stdout = stdout(&out);
        assert!(stdout.contains(colored_examples), "{args:?}:\n{stdout}");
    }
}

#[cfg(unix)]
#[test]
fn docs_website_has_dark_mode_and_colored_reference_output() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = e2e_lock();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs_dir = manifest_dir.join("docs");
    let output_dir = unique_dir("ir-docs-reference-output");
    let bin_dir = unique_dir("ir-docs-reference-bin");
    let stale_bin_dir = unique_dir("ir-docs-reference-stale-bin");
    let fake_ir = bin_dir.join("ir");
    let stale_ir = stale_bin_dir.join("ir");

    fs::write(
        &fake_ir,
        concat!(
            "#!/bin/sh\n",
            "printf '\\033[1;32mUsage:\\033[0m \\033[1;94mir\\033[0m \\033[94m[COMMAND]\\033[0m\\n\\n'\n",
            "printf '\\033[1;32mCommands:\\033[0m\\n'\n",
            "printf '  \\033[1;94mrender\\033[0m  Render a Quarto document or script\\n\\n'\n",
            "printf '\\033[1;32mOptions:\\033[0m\\n'\n",
            "printf '  \\033[1;94m-h\\033[0m, \\033[1;94m--help\\033[0m  Print help\\n'\n",
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(&fake_ir).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_ir, perms).unwrap();

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
    assert!(styles.contains("--ir-ansi-green"), "{styles}");
    assert!(
        styles.contains("pre.ir-cli-help span[style*=\"#00BB00\"]"),
        "{styles}"
    );
    assert!(
        styles.contains("pre.ir-cli-help span[style*=\"#5555FF\"]"),
        "{styles}"
    );

    let path = std::env::join_paths(
        std::iter::once(stale_bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let output = Command::new("quarto")
        .current_dir(&docs_dir)
        .env("PATH", path)
        .env("IR_BIN", &fake_ir)
        .args(["render", "reference.qmd", "--to", "html"])
        .arg("--output-dir")
        .arg(&output_dir)
        .output()
        .unwrap();
    assert_success(&output);

    let html = fs::read_to_string(output_dir.join("reference.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered reference page: {e}"));
    assert!(html.contains("data-mode=\"dark\""), "{html}");
    assert!(
        html.contains("Render a Quarto document or script"),
        "{html}"
    );
    assert!(html.contains("Options:"), "{html}");
    assert!(html.contains("color: #00BB00"), "{html}");
    assert!(html.contains("color: #5555FF"), "{html}");
    assert!(html.contains("font-weight: bold"), "{html}");
    assert!(!html.contains("\u{1b}["), "{html}");

    let _ = fs::remove_dir_all(&output_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&stale_bin_dir);
}

#[test]
fn docs_run_page_dark_mode_styles_console_blocks() {
    let _guard = e2e_lock();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs_dir = manifest_dir.join("docs");
    let output_dir = unique_dir("ir-docs-run-output");

    let output = Command::new("quarto")
        .current_dir(&docs_dir)
        .args(["render", "run.qmd", "--to", "html"])
        .arg("--output-dir")
        .arg(&output_dir)
        .output()
        .unwrap();
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
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| packages: null

cat("ir.fixture=packages-null\n")
"#,
    )
    .unwrap();

    let out = ir()
        .args(["run", "--vanilla", script.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = fs::remove_file(&script);

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=packages-null");
}

#[test]
fn run_script_frontmatter_accepts_packages_and_isolated() {
    let _guard = e2e_lock();
    let script = unique_path("ir-packages-frontmatter", "R");
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
        .env("R_LIBS_USER", &user_library)
        .args(["run", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    let _ = fs::remove_file(&script);
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
    let _guard = e2e_lock();
    let script = fixture("run/packages.R");

    let out = ir()
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
}

#[test]
fn run_script_uses_only_the_first_yaml_document() {
    let _guard = e2e_lock();
    let script = fixture("run/multiple-documents.R");

    let out = ir()
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=multi-doc");
    assert_stdout_contains(&out, "multi.packages=glue:true");
    assert_stdout_contains(&out, "multi.ignored_package=false");
    assert_stdout_contains(&out, "multi.result=5");
}

#[test]
fn run_inline_expression_resolves_with_dependencies() {
    let _guard = e2e_lock();
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
}

#[test]
fn run_inline_expression_forwards_option_like_args_after_expr() {
    let _guard = e2e_lock();
    let out = ir()
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
}

#[test]
fn run_normalizes_version_specs_before_resolution_cache_keying() {
    let _guard = e2e_lock();
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
    let _guard = e2e_lock();
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
    let _guard = e2e_lock();
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
    let _guard = e2e_lock();
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
    let _guard = e2e_lock();
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
    let _guard = e2e_lock();
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
    let _guard = e2e_lock();
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
    let _guard = e2e_lock();
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
    let _guard = e2e_lock();
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
    let _guard = e2e_lock();
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
    let _guard = e2e_lock();
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
    let _guard = e2e_lock();
    let fixture_dir = fixture("run");
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
}

// report-pinned.qmd declares rmarkdown itself, so the resolver leaves it alone.
#[test]
fn render_quarto_fixture_with_declared_rmarkdown_skips_injection() {
    let _guard = e2e_lock();
    let fixture_dir = fixture("run");
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
}

// report-transitive.qmd declares `quarto`, which Imports rmarkdown. The
// resolver sees rmarkdown already in the resolved set and skips its own seed.
#[test]
fn render_quarto_fixture_with_transitive_rmarkdown_renders() {
    let _guard = e2e_lock();
    let fixture_dir = fixture("run");
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
    // Both the declared `quarto` and the transitively-pulled rmarkdown must be
    // materialised into the resolved run library, with rmarkdown's version read
    // from that library's DESCRIPTION.
    assert!(html.contains("transitive.quarto_in_cache=true"), "{html}");
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
}

// report-bare.qmd declares no dependencies at all, so the resolver must still
// inject rmarkdown quietly for the knitr engine to render.
#[test]
fn render_quarto_bare_fixture_injects_rmarkdown() {
    let _guard = e2e_lock();
    let fixture_dir = fixture("run");
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
}

#[test]
fn render_quarto_script_fixture_renders_with_dependencies() {
    let _guard = e2e_lock();
    let fixture_dir = fixture("run");
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
}

#[test]
fn render_quarto_selects_requested_r_version() {
    let _guard = e2e_lock();

    // Opt-in: needs rig plus a non-default R installed (CI provisions both).
    // `ir`'s `--r-version` path resolves through rig unconditionally, so with a
    // single R there is nothing to select.
    let Ok(target) = std::env::var("IR_TEST_R_VERSION") else {
        eprintln!(
            "SKIP render_quarto_selects_requested_r_version: set IR_TEST_R_VERSION to a rig-installed, non-default R version"
        );
        return;
    };

    // Selecting the version the default path already uses would prove nothing.
    if default_r_version().as_deref() == Some(target.as_str()) {
        eprintln!(
            "SKIP render_quarto_selects_requested_r_version: IR_TEST_R_VERSION ({target}) matches the default R; pick a different installed version"
        );
        return;
    }

    let fixture_dir = fixture("run");

    let out = ir()
        .current_dir(&fixture_dir)
        // The resolver inherits the environment, so an ambient R_LIBS_USER (CI's
        // setup-r-dependencies exports one) would point the selected R at a
        // library built for the *default* R, loading an ABI-mismatched
        // secretbase. A real `--r-version` user has no R_LIBS_USER exported; drop
        // it so the requested R uses its own toolchain.
        .env_remove("R_LIBS_USER")
        .args(["render", "--isolated", "--r-version"])
        .arg(&target)
        .arg("r-version-select.qmd")
        .args(["--to", "html"])
        .output()
        .unwrap();

    assert_success(&out);

    let html = fs::read_to_string(fixture_dir.join("r-version-select.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered report: {e}\n{}", output_text(&out)));
    assert!(html.contains("ir.fixture=r-version"), "{html}");
    assert!(
        html.contains(&format!("version.r_version=[{target}]")),
        "rendered under a different R than the requested {target}\n{html}"
    );
    assert!(html.contains("version.lib_in_cache=true"), "{html}");
    assert!(html.contains("version.jsonlite_in_cache=true"), "{html}");

    let _ = fs::remove_file(fixture_dir.join("r-version-select.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("r-version-select_files"));
}

#[test]
fn run_script_frontmatter_selects_r_version() {
    let _guard = e2e_lock();

    // The fixture pins `#| r-version` to this version, so the test only runs
    // when CI has provisioned that exact R through rig (signalled by
    // IR_TEST_R_VERSION). Unlike the flag, the frontmatter value can't come from
    // the environment because it lives in the static fixture.
    const FIXTURE_R_VERSION: &str = "4.4.3";
    if std::env::var("IR_TEST_R_VERSION").ok().as_deref() != Some(FIXTURE_R_VERSION) {
        eprintln!(
            "SKIP run_script_frontmatter_selects_r_version: set IR_TEST_R_VERSION={FIXTURE_R_VERSION} (rig plus that R) to match the fixture's `#| r-version`"
        );
        return;
    }

    // Selecting the version the default path already uses would prove nothing.
    if default_r_version().as_deref() == Some(FIXTURE_R_VERSION) {
        eprintln!(
            "SKIP run_script_frontmatter_selects_r_version: the fixture's R ({FIXTURE_R_VERSION}) matches the default R; nothing to select"
        );
        return;
    }

    let script = fixture("run/r-version-frontmatter.R");

    let out = ir()
        // See render_quarto_selects_requested_r_version: drop the ambient
        // R_LIBS_USER so the frontmatter-selected R resolves against its own
        // toolchain rather than the default R's (ABI-mismatched) library.
        .env_remove("R_LIBS_USER")
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=r-version-frontmatter");
    assert_stdout_contains(&out, &format!("version.r_version=[{FIXTURE_R_VERSION}]"));
    assert_stdout_contains(&out, "version.lib_in_cache=true");
    assert_stdout_contains(&out, "version.jsonlite_in_cache=true");
}

#[test]
fn run_reticulate_fixture_imports_python_module() {
    let _guard = e2e_lock();
    let script = fixture("run/reticulate.R");
    let managed_reticulate = std::env::var_os("IR_TEST_RETICULATE_MANAGED").is_some();

    let mut cmd = ir();

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
}

#[test]
fn tool_run_executes_real_package_entrypoint() {
    let _guard = e2e_lock();

    let out = ir()
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
}

#[test]
fn rx_executes_real_package_entrypoint() {
    let _guard = e2e_lock();

    let out = rx()
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
}

#[test]
fn tool_install_installs_real_package_entrypoint() {
    let _guard = e2e_lock();
    let bin_dir = unique_dir("ir-e2e-tool-install-bin");

    let out = ir()
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
}

#[cfg(unix)]
#[test]
fn tool_install_warm_resolution_cache_skips_resolver_rscript() {
    let _guard = e2e_lock();
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
fn tool_install_with_rscript_wrapper_records_primary_package_marker() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = e2e_lock();
    let cache_dir = unique_dir("ir-wrapper-tool-install-cache");
    let bin_dir = unique_dir("ir-wrapper-tool-install-bin");
    let wrapper = unique_path("ir-rscript-wrapper", "sh");
    fs::write(
        &wrapper,
        "#!/bin/sh\nexec \"$IR_TEST_RSCRIPT_TARGET\" \"$@\"\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&wrapper).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&wrapper, permissions).unwrap();

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
