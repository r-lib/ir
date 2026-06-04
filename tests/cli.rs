//! Integration tests for the public `ir` CLI.
//!
//! These tests avoid mocked `Rscript`, `quarto`, `rig`, or package executable
//! shims. The end-to-end cases run real fixture scripts/documents through the
//! compiled binary and assert marker lines printed by those public workflows.

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

fn ir_bin_name() -> String {
    Path::new(env!("CARGO_BIN_EXE_ir"))
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn rscript() -> String {
    std::env::var("IR_RSCRIPT").unwrap_or_else(|_| "Rscript".into())
}

fn normalize_cli_output(output: &[u8]) -> String {
    String::from_utf8_lossy(output)
        .replace("\r\n", "\n")
        .replace(&ir_bin_name(), "ir")
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

fn python_minor_version() -> String {
    for command in ["python3", "python"] {
        let output = Command::new(command)
            .args([
                "-c",
                "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')",
            ])
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                return String::from_utf8(output.stdout).unwrap().trim().to_string();
            }
        }
    }

    panic!("python3 or python is required for the reticulate fixture");
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
    let r_expr = r#"
pkgs <- c(
  "pak", "renv", "secretbase", "cli", "glue", "jsonlite",
  "dplyr", "tidyr", "reticulate", "knitr", "rmarkdown",
  "btw", "Rapp", "docopt", "pkgsearch", "prettyunits"
)
missing <- pkgs[!vapply(pkgs, requireNamespace, logical(1), quietly = TRUE)]
if (length(missing)) {
  stop("missing R packages: ", paste(missing, collapse = ", "), call. = FALSE)
}
cat("ir.fixture=ci-deps\n")
"#;

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
fn help_outputs_match_snapshots() {
    for (name, args) in [
        ("help", &["--help"][..]),
        ("help", &["-h"]),
        ("run-help", &["run", "--help"]),
        ("run-help", &["run", "-h"]),
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
fn website_reference_page_runs_live_cli_help_chunks() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let reference = manifest_dir.join("docs").join("reference.qmd");
    let source = fs::read_to_string(&reference)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", reference.display()));

    assert!(
        source.contains("echo: false"),
        "{} should hide chunk source in rendered CLI help",
        reference.display()
    );

    for expected in [
        r#"system2("ir", c("--help")"#,
        r#"system2("ir", c("run", "--help")"#,
        r#"system2("ir", c("tool", "--help")"#,
        r#"system2("ir", c("tool", "run", "--help")"#,
        r#"system2("ir", c("tool", "install", "--help")"#,
        r#"system2("ir", c("cache", "--help")"#,
        r#"system2("ir", c("cache", "dir", "--help")"#,
        r#"system2("ir", c("cache", "clean", "--help")"#,
    ] {
        assert!(
            source.contains(expected),
            "{} should contain {expected}",
            reference.display()
        );
    }
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
fn run_with_missing_script_errors() {
    let out = ir().args(["run", "/no/such/ir-script.R"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read script"));
}

#[test]
fn malformed_frontmatter_errors_before_resolution() {
    let script = unique_path("ir-malformed-frontmatter", "R");
    fs::write(
        &script,
        "#!/usr/bin/env -S ir run\n#| dependencies: [dplyr\n\ncat('not reached')\n",
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
fn cache_dir_reports_override_and_real_r_default() {
    let cache_dir = unique_dir("ir-cache-override");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["cache", "dir"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(stdout(&out), format!("{}\n", cache_dir.display()));

    let expected = Command::new(rscript())
        .args(["-e", "writeLines(tools::R_user_dir(\"ir\", \"cache\"))"])
        .output()
        .expect("failed to run Rscript");
    assert_success(&expected);

    let out = ir()
        .env_remove("IR_CACHE_DIR")
        .args(["cache", "dir"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(stdout(&out), stdout(&expected));

    let _ = fs::remove_dir_all(&cache_dir);
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
    let cache_dir = unique_dir("ir-e2e-script-cache");
    let script = fixture("run/packages.R");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_EXPECT_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .args(["--script-arg", "value"])
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&cache_dir);

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
    let cache_dir = unique_dir("ir-e2e-multi-doc-cache");
    let script = fixture("run/multiple-documents.R");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&cache_dir);

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=multi-doc");
    assert_stdout_contains(&out, "multi.packages=glue:true");
    assert_stdout_contains(&out, "multi.ignored_package=false");
    assert_stdout_contains(&out, "multi.result=5");
}

#[test]
fn run_inline_expression_resolves_with_dependencies() {
    let _guard = e2e_lock();
    let cache_dir = unique_dir("ir-e2e-inline-cache");
    let expr = r#"
library(cli)
library(glue)
lib <- normalizePath(.libPaths()[[1]], winslash = "/", mustWork = TRUE)
expected <- normalizePath(Sys.getenv("IR_EXPECT_CACHE_DIR"), winslash = "/", mustWork = FALSE)
libraries <- file.path(expected, "libraries")
pkgs_in_cache <- startsWith(lib, libraries) &&
  all(file.exists(file.path(lib, c("cli", "glue"), "DESCRIPTION")))
cat("ir.fixture=inline\n")
cat("inline.args=", paste(commandArgs(TRUE), collapse = "|"), "\n", sep = "")
cat("inline.lib_in_cache=", tolower(startsWith(lib, libraries)), "\n", sep = "")
cat("inline.pkgs_in_cache=", tolower(pkgs_in_cache), "\n", sep = "")
cat(glue::glue("inline.glue={1 + 1}\n"))
"#;

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_EXPECT_CACHE_DIR", &cache_dir)
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

    let _ = fs::remove_dir_all(&cache_dir);

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=inline");
    assert_stdout_contains(&out, "inline.args=inline-arg");
    assert_stdout_contains(&out, "inline.lib_in_cache=true");
    assert_stdout_contains(&out, "inline.pkgs_in_cache=true");
    assert_stdout_contains(&out, "inline.glue=2");
}

#[test]
fn run_quarto_fixture_renders_html_with_resolved_packages() {
    let _guard = e2e_lock();
    let cache_dir = unique_dir("ir-e2e-qmd-cache");
    let output_dir = unique_dir("ir-e2e-qmd-output");
    let doc = fixture("run/report.qmd");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_EXPECT_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated"])
        .arg(&doc)
        .args(["--to", "html", "--output-dir"])
        .arg(&output_dir)
        .output()
        .unwrap();

    assert_success(&out);

    let html = fs::read_to_string(output_dir.join("report.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered report: {e}\n{}", output_text(&out)));
    assert!(html.contains("ir.fixture=qmd"), "{html}");
    assert!(html.contains("qmd.lib_in_cache=true"), "{html}");
    assert!(html.contains("qmd.pkgs_in_cache=true"), "{html}");
    assert!(html.contains("qmd.result=a:4,b:2"), "{html}");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&output_dir);
}

#[test]
fn run_quarto_selects_requested_r_version() {
    let _guard = e2e_lock();

    // Opt-in: needs rig plus a non-default R installed (CI provisions both).
    // `ir`'s `--r-version` path resolves through rig unconditionally, so with a
    // single R there is nothing to select.
    let Ok(target) = std::env::var("IR_TEST_R_VERSION") else {
        eprintln!(
            "SKIP run_quarto_selects_requested_r_version: set IR_TEST_R_VERSION to a rig-installed, non-default R version"
        );
        return;
    };

    // Selecting the version the default path already uses would prove nothing.
    if default_r_version().as_deref() == Some(target.as_str()) {
        eprintln!(
            "SKIP run_quarto_selects_requested_r_version: IR_TEST_R_VERSION ({target}) matches the default R; pick a different installed version"
        );
        return;
    }

    let cache_dir = unique_dir("ir-e2e-rversion-cache");
    let output_dir = unique_dir("ir-e2e-rversion-output");
    let doc = fixture("run/r-version-select.qmd");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_EXPECT_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated", "--r-version"])
        .arg(&target)
        .arg(&doc)
        .args(["--to", "html", "--output-dir"])
        .arg(&output_dir)
        .output()
        .unwrap();

    assert_success(&out);

    let html = fs::read_to_string(output_dir.join("r-version-select.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered report: {e}\n{}", output_text(&out)));
    assert!(html.contains("ir.fixture=r-version"), "{html}");
    assert!(
        html.contains(&format!("version.r_version=[{target}]")),
        "rendered under a different R than the requested {target}\n{html}"
    );
    assert!(html.contains("version.lib_in_cache=true"), "{html}");
    assert!(html.contains("version.jsonlite_in_cache=true"), "{html}");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&output_dir);
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

    let cache_dir = unique_dir("ir-e2e-rversion-fm-cache");
    let script = fixture("run/r-version-frontmatter.R");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_EXPECT_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&cache_dir);

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=r-version-frontmatter");
    assert_stdout_contains(&out, &format!("version.r_version=[{FIXTURE_R_VERSION}]"));
    assert_stdout_contains(&out, "version.lib_in_cache=true");
    assert_stdout_contains(&out, "version.jsonlite_in_cache=true");
}

#[test]
fn run_reticulate_fixture_uses_managed_ephemeral_venv() {
    let _guard = e2e_lock();
    let cache_dir = unique_dir("ir-e2e-reticulate-cache");
    let reticulate_cache = unique_dir("ir-e2e-reticulate-r-cache");
    let reticulate_data = unique_dir("ir-e2e-reticulate-r-data");
    let script = fixture("run/reticulate.R");
    let python_version = python_minor_version();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_EXPECT_CACHE_DIR", &cache_dir)
        .env("IR_TEST_PYTHON_VERSION", &python_version)
        .env("R_USER_CACHE_DIR", &reticulate_cache)
        .env("R_USER_DATA_DIR", &reticulate_data)
        .env("RETICULATE_PYTHON", "managed")
        .env("UV_PYTHON_PREFERENCE", "only-system")
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&reticulate_cache);
    let _ = fs::remove_dir_all(&reticulate_data);

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=reticulate");
    assert_stdout_contains(&out, "reticulate.lib_in_cache=true");
    assert_stdout_contains(&out, "reticulate.ephemeral=true");
    assert_stdout_contains(&out, "reticulate.json={\"ok\": true}");
}

#[test]
fn tool_run_executes_real_package_entrypoint() {
    let _guard = e2e_lock();
    let cache_dir = unique_dir("ir-e2e-tool-cache");

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

    let _ = fs::remove_dir_all(&cache_dir);

    assert_success(&out);
    assert_stdout_contains(&out, "Seach for CRAN packages on r-pkg.org");
    assert_stdout_contains(&out, "cransearch.R [-h | --help]");
}

#[test]
fn tool_install_installs_real_package_entrypoint() {
    let _guard = e2e_lock();
    let cache_dir = unique_dir("ir-e2e-tool-install-cache");
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

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);

    assert_success(&out);
    assert_stdout_contains(&out, "Seach for CRAN packages on r-pkg.org");
    assert_stdout_contains(&out, "cransearch.R [-h | --help]");
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
