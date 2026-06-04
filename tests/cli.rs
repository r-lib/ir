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

fn cold_cache() -> bool {
    std::env::var_os("IR_TEST_COLD_CACHE").is_some()
}

struct TestDir {
    path: PathBuf,
    cleanup: bool,
}

impl TestDir {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn persistent_test_dir(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(name);
    fs::create_dir_all(&path).unwrap();
    path
}

fn shared_e2e_dir(name: &str) -> PathBuf {
    let path = if std::env::var_os("CI").is_some() {
        std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
    } else {
        persistent_test_dir(name)
    };
    fs::create_dir_all(&path).unwrap();
    path
}

fn e2e_cache_dir(prefix: &str) -> TestDir {
    if cold_cache() {
        TestDir {
            path: unique_dir(prefix),
            cleanup: true,
        }
    } else {
        TestDir {
            path: shared_e2e_dir("ir-e2e-cache"),
            cleanup: false,
        }
    }
}

fn e2e_reticulate_dir(prefix: &str, warm_name: &str) -> TestDir {
    if cold_cache() {
        TestDir {
            path: unique_dir(prefix),
            cleanup: true,
        }
    } else {
        TestDir {
            path: shared_e2e_dir(warm_name),
            cleanup: false,
        }
    }
}

fn scratch_dir(prefix: &str) -> TestDir {
    TestDir {
        path: unique_dir(prefix),
        cleanup: true,
    }
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
    python_probe().0
}

fn python_executable() -> String {
    python_probe().1
}

fn python_probe() -> (String, String) {
    for command in ["python3", "python"] {
        let output = Command::new(command)
            .args([
                "-c",
                "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}'); print(sys.executable)",
            ])
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8(output.stdout).unwrap();
                let mut lines = stdout.lines();
                let version = lines.next().unwrap().to_string();
                let executable = lines.next().unwrap().to_string();
                return (version, executable);
            }
        }
    }

    panic!("python3 or python is required for the reticulate fixture");
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
    let cache_dir = e2e_cache_dir("ir-e2e-script-cache");
    let script = fixture("run/packages.R");

    let out = ir()
        .env("IR_CACHE_DIR", cache_dir.path())
        .env("IR_EXPECT_CACHE_DIR", cache_dir.path())
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
    let cache_dir = e2e_cache_dir("ir-e2e-multi-doc-cache");
    let script = fixture("run/multiple-documents.R");

    let out = ir()
        .env("IR_CACHE_DIR", cache_dir.path())
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
    let cache_dir = e2e_cache_dir("ir-e2e-inline-cache");
    let expr = r#"
library(cli)
library(glue)
lib <- normalizePath(.libPaths()[[1]], winslash = "/", mustWork = TRUE)
expected <- normalizePath(Sys.getenv("IR_EXPECT_CACHE_DIR"), winslash = "/", mustWork = FALSE)
cat("ir.fixture=inline\n")
cat("inline.args=", paste(commandArgs(TRUE), collapse = "|"), "\n", sep = "")
cat("inline.lib_in_cache=", tolower(startsWith(lib, file.path(expected, "libraries"))), "\n", sep = "")
cat(glue::glue("inline.glue={1 + 1}\n"))
"#;

    let out = ir()
        .env("IR_CACHE_DIR", cache_dir.path())
        .env("IR_EXPECT_CACHE_DIR", cache_dir.path())
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
    assert_stdout_contains(&out, "inline.glue=2");
}

#[test]
fn run_quarto_fixture_renders_html_with_resolved_packages() {
    let _guard = e2e_lock();
    let cache_dir = e2e_cache_dir("ir-e2e-qmd-cache");
    let output_dir = scratch_dir("ir-e2e-qmd-output");
    let doc = fixture("run/report.qmd");

    let out = ir()
        .env("IR_CACHE_DIR", cache_dir.path())
        .env("IR_EXPECT_CACHE_DIR", cache_dir.path())
        .args(["run", "--isolated"])
        .arg(&doc)
        .args(["--to", "html", "--output-dir"])
        .arg(output_dir.path())
        .output()
        .unwrap();

    assert_success(&out);

    let html = fs::read_to_string(output_dir.path().join("report.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered report: {e}\n{}", output_text(&out)));
    assert!(html.contains("ir.fixture=qmd"), "{html}");
    assert!(html.contains("qmd.lib_in_cache=true"), "{html}");
    assert!(html.contains("qmd.result=a:4,b:2"), "{html}");
}

#[test]
fn run_reticulate_fixture_imports_python_module() {
    let _guard = e2e_lock();
    let cache_dir = e2e_cache_dir("ir-e2e-reticulate-cache");
    let reticulate_cache =
        e2e_reticulate_dir("ir-e2e-reticulate-r-cache", "ir-e2e-reticulate-r-cache");
    let reticulate_data =
        e2e_reticulate_dir("ir-e2e-reticulate-r-data", "ir-e2e-reticulate-r-data");
    let script = fixture("run/reticulate.R");
    let managed_reticulate = cold_cache();

    let mut cmd = ir();
    cmd.env("IR_CACHE_DIR", cache_dir.path())
        .env("IR_EXPECT_CACHE_DIR", cache_dir.path())
        .env("R_USER_CACHE_DIR", reticulate_cache.path())
        .env("R_USER_DATA_DIR", reticulate_data.path());

    if managed_reticulate {
        cmd.env("IR_TEST_RETICULATE_MANAGED", "1")
            .env("IR_TEST_PYTHON_VERSION", python_minor_version())
            .env("RETICULATE_PYTHON", "managed")
            .env("UV_PYTHON_PREFERENCE", "only-system");
    } else {
        cmd.env("RETICULATE_PYTHON", python_executable())
            .env("RETICULATE_USE_MANAGED_VENV", "no");
    }

    let out = cmd
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=reticulate");
    assert_stdout_contains(&out, "reticulate.lib_in_cache=true");
    assert_stdout_contains(
        &out,
        if managed_reticulate {
            "reticulate.ephemeral=true"
        } else {
            "reticulate.ephemeral=false"
        },
    );
    assert_stdout_contains(&out, "reticulate.json={\"ok\": true}");
}

#[test]
fn tool_run_executes_real_package_entrypoint() {
    let _guard = e2e_lock();
    let cache_dir = e2e_cache_dir("ir-e2e-tool-cache");

    let out = ir()
        .env("IR_CACHE_DIR", cache_dir.path())
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
fn tool_install_installs_real_package_entrypoint() {
    let _guard = e2e_lock();
    let cache_dir = e2e_cache_dir("ir-e2e-tool-install-cache");
    let bin_dir = scratch_dir("ir-e2e-tool-install-bin");

    let out = ir()
        .env("IR_CACHE_DIR", cache_dir.path())
        .args([
            "tool",
            "install",
            "--with",
            "docopt,pkgsearch,prettyunits",
            "--bin-dir",
        ])
        .arg(bin_dir.path())
        .arg("cli")
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Installed");
    assert_stdout_contains(&out, "search");

    let launcher = launcher_path(bin_dir.path(), "search");
    let out = Command::new(&launcher).arg("--help").output().unwrap();

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
