//! Integration tests for the public `ir` CLI.
//!
//! These tests avoid mocked `Rscript`, `quarto`, `rig`, or package executable
//! shims. The end-to-end cases run real fixture scripts/documents through the
//! compiled binary and assert marker lines printed by those public workflows.

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

fn ir_bin_name() -> String {
    Path::new(env!("CARGO_BIN_EXE_ir"))
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
        "'btw', 'Rapp', 'docopt', 'pkgsearch', 'prettyunits'); ",
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
    assert!(
        source.contains(r#"output <- system2("ir", args, stdout = TRUE, stderr = TRUE)"#),
        "{} should run live CLI help from ir on PATH",
        reference.display()
    );
    assert!(
        source.contains(r#"status <- attr(output, "status")"#),
        "{} should inspect CLI help command status",
        reference.display()
    );
    assert!(
        source.contains(r#"` failed with status "#),
        "{} should fail the render when a CLI help command fails",
        reference.display()
    );

    for expected in [
        "cli_help()",
        r#"cli_help("run")"#,
        r#"cli_help("tool")"#,
        r#"cli_help("tool", "run")"#,
        r#"cli_help("tool", "install")"#,
        r#"cli_help("cache")"#,
        r#"cli_help("cache", "dir")"#,
        r#"cli_help("cache", "clean")"#,
    ] {
        assert!(
            source.contains(expected),
            "{} should contain {expected}",
            reference.display()
        );
    }
}

#[test]
fn readme_and_website_document_public_surface() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme_path = manifest_dir.join("README.md");
    let readme = fs::read_to_string(&readme_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", readme_path.display()));

    for stale in [
        "inline expression, isolated library",
        "empty isolated library",
    ] {
        assert!(
            !readme.contains(stale),
            "{} should not contain stale inline isolation wording: {stale}",
            readme_path.display()
        );
    }

    for expected in [
        "empty resolved library",
        "IR_QUARTO",
        "IR_TOOL_BIN_DIR",
        "RAPP_BIN_DIR",
        "XDG_BIN_HOME",
        "XDG_DATA_HOME",
        "LOCALAPPDATA",
        "rig's default R install",
    ] {
        assert!(
            readme.contains(expected),
            "{} should document {expected}",
            readme_path.display()
        );
    }

    let config_path = manifest_dir.join("docs").join("config.qmd");
    let config = fs::read_to_string(&config_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", config_path.display()));
    for expected in [
        "IR_CACHE_DIR",
        "IR_RSCRIPT",
        "IR_QUARTO",
        "IR_TOOL_BIN_DIR",
        "RAPP_BIN_DIR",
        "XDG_BIN_HOME",
        "XDG_DATA_HOME",
        "LOCALAPPDATA",
        "USERPROFILE",
    ] {
        assert!(
            config.contains(expected),
            "{} should document {expected}",
            config_path.display()
        );
    }

    let quarto_config = manifest_dir.join("docs").join("_quarto.yml");
    let quarto_config_source = fs::read_to_string(&quarto_config)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", quarto_config.display()));
    assert!(
        quarto_config_source.contains("config.qmd"),
        "{} should include the configuration page",
        quarto_config.display()
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

// report.qmd deliberately does NOT declare rmarkdown, so the render only
// succeeds because ir injects it (the knitr engine needs it). The advisory on
// stderr confirms the injected seed was used.
#[test]
fn run_quarto_fixture_injects_rmarkdown_and_renders() {
    let _guard = e2e_lock();
    let fixture_dir = fixture("run");
    let cache_dir = unique_dir("ir-e2e-qmd-cache");

    let out = ir()
        .current_dir(&fixture_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated"])
        .arg("report.qmd")
        .args(["--to", "html"])
        .output()
        .unwrap();

    assert_success(&out);

    let html = fs::read_to_string(fixture_dir.join("report.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered report: {e}\n{}", output_text(&out)));
    assert!(html.contains("ir.fixture=qmd"), "{html}");
    assert!(html.contains("qmd.lib_in_cache=true"), "{html}");
    assert!(html.contains("qmd.pkgs_in_cache=true"), "{html}");
    assert!(html.contains("qmd.result=a:4,b:2"), "{html}");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("using latest rmarkdown"),
        "expected the rmarkdown injection advisory\n{}",
        output_text(&out)
    );

    let _ = fs::remove_file(fixture_dir.join("report.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("report_files"));
    let _ = fs::remove_dir_all(&cache_dir);
}

// report-pinned.qmd declares rmarkdown itself, so the injected seed is
// deduped away and the advisory stays silent.
#[test]
fn run_quarto_fixture_with_declared_rmarkdown_skips_injection() {
    let _guard = e2e_lock();
    let fixture_dir = fixture("run");
    let cache_dir = unique_dir("ir-e2e-qmd-pinned-cache");

    let out = ir()
        .current_dir(&fixture_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated"])
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
        "advisory should stay silent when rmarkdown is declared\n{}",
        output_text(&out)
    );

    let _ = fs::remove_file(fixture_dir.join("report-pinned.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("report-pinned_files"));
    let _ = fs::remove_dir_all(&cache_dir);
}

// report-transitive.qmd declares `quarto`, which Imports rmarkdown. The
// resolver sees rmarkdown already in the resolved set and skips its own seed,
// so the advisory stays silent even though the document never names rmarkdown.
#[test]
fn run_quarto_fixture_with_transitive_rmarkdown_skips_advisory() {
    let _guard = e2e_lock();
    let fixture_dir = fixture("run");
    let cache_dir = unique_dir("ir-e2e-qmd-transitive-cache");

    let out = ir()
        .current_dir(&fixture_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated"])
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
        "advisory should stay silent when rmarkdown is a transitive dependency\n{}",
        output_text(&out)
    );

    let _ = fs::remove_file(fixture_dir.join("report-transitive.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("report-transitive_files"));
    let _ = fs::remove_dir_all(&cache_dir);
}

// report-bare.qmd declares no dependencies at all, so the resolver must still
// inject rmarkdown (with the advisory) for the knitr engine to render.
#[test]
fn run_quarto_bare_fixture_injects_rmarkdown() {
    let _guard = e2e_lock();
    let fixture_dir = fixture("run");
    let cache_dir = unique_dir("ir-e2e-qmd-bare-cache");

    let out = ir()
        .current_dir(&fixture_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated"])
        .arg("report-bare.qmd")
        .args(["--to", "html"])
        .output()
        .unwrap();

    assert_success(&out);

    let html = fs::read_to_string(fixture_dir.join("report-bare.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered report: {e}\n{}", output_text(&out)));
    assert!(html.contains("ir.fixture=qmd-bare"), "{html}");
    // The injected rmarkdown must be materialised into the resolved run
    // library, with its version read from that library's DESCRIPTION.
    assert!(html.contains("bare.rmarkdown_in_cache=true"), "{html}");
    assert!(html.contains("bare.rmarkdown_version="), "{html}");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("using latest rmarkdown"),
        "expected the rmarkdown injection advisory for a bare quarto doc\n{}",
        output_text(&out)
    );

    let _ = fs::remove_file(fixture_dir.join("report-bare.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("report-bare_files"));
    let _ = fs::remove_dir_all(&cache_dir);
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

    let fixture_dir = fixture("run");

    let out = ir()
        .current_dir(&fixture_dir)
        // The resolver inherits the environment, so an ambient R_LIBS_USER (CI's
        // setup-r-dependencies exports one) would point the selected R at a
        // library built for the *default* R, loading an ABI-mismatched
        // secretbase. A real `--r-version` user has no R_LIBS_USER exported; drop
        // it so the requested R uses its own toolchain.
        .env_remove("R_LIBS_USER")
        .args(["run", "--isolated", "--r-version"])
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
        // See run_quarto_selects_requested_r_version: drop the ambient
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
fn tool_run_and_install_support_package_exec_shebangs() {
    let _guard = e2e_lock();
    let fixture = dummy_tool_package();
    let bin_dir = unique_dir("ir-e2e-tool-shebang-bin");

    for executable in [
        ("bash-tool", &["run", "bash"][..], "tool.fixture=bash"),
        ("python-tool", &["run", "python"], "tool.fixture=python"),
        ("r-tool", &["run", "r"], "tool.fixture=r"),
        ("rapp-tool", &["--value", "run-rapp"], "tool.fixture=rapp"),
        ("sh-tool", &["run", "sh"], "tool.fixture=sh"),
    ] {
        let out = ir()
            .env("IR_CACHE_DIR", &fixture.cache_dir)
            .env("RENV_PATHS_CACHE", &fixture.renv_cache_dir)
            .args([
                "tool",
                "run",
                "--with",
                "Rapp",
                "--from",
                &fixture.package_ref,
            ])
            .arg(executable.0)
            .args(executable.1)
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, executable.2);
    }

    let out = ir()
        .env("IR_CACHE_DIR", &fixture.cache_dir)
        .env("RENV_PATHS_CACHE", &fixture.renv_cache_dir)
        .args(["tool", "install", "--with", "Rapp", "--bin-dir"])
        .arg(&bin_dir)
        .arg(&fixture.package_ref)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Installed 5 executables");

    for executable in [
        ("bash-tool", &["install", "bash"][..], "tool.fixture=bash"),
        ("python-tool", &["install", "python"], "tool.fixture=python"),
        ("r-tool", &["install", "r"], "tool.fixture=r"),
        (
            "rapp-tool",
            &["--value", "install-rapp"],
            "tool.fixture=rapp",
        ),
        ("sh-tool", &["install", "sh"], "tool.fixture=sh"),
    ] {
        let out = Command::new(launcher_path(&bin_dir, executable.0))
            .args(executable.1)
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, executable.2);
    }

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&fixture.root);
}

#[cfg(unix)]
struct DummyToolPackage {
    root: PathBuf,
    cache_dir: PathBuf,
    renv_cache_dir: PathBuf,
    package_ref: String,
}

#[cfg(unix)]
fn dummy_tool_package() -> DummyToolPackage {
    let root = unique_dir("ir-e2e-tool-shebang-pkg");
    let package = root.join("irtoolfixture");
    let exec = package.join("exec");
    fs::create_dir_all(&exec).unwrap();
    fs::create_dir_all(package.join("R")).unwrap();
    fs::write(
        package.join("DESCRIPTION"),
        concat!(
            "Package: irtoolfixture\n",
            "Type: Package\n",
            "Title: ir Tool Fixture\n",
            "Version: 0.0.1\n",
            "Authors@R: person(\"Test\", \"Fixture\", role = c(\"aut\", \"cre\"), email = \"test@example.com\")\n",
            "Description: Exercises package exec launchers.\n",
            "License: MIT\n",
            "Encoding: UTF-8\n"
        ),
    )
    .unwrap();
    fs::write(package.join("NAMESPACE"), "").unwrap();
    fs::write(package.join("R").join("placeholder.R"), "").unwrap();

    write_executable(
        &exec.join("bash-tool"),
        "#!/usr/bin/env bash\nprintf 'tool.fixture=bash\\n'\nprintf 'tool.args=%s\\n' \"$*\"\n",
    );
    write_executable(
        &exec.join("python-tool"),
        "#!/usr/bin/env python3\nimport sys\nprint('tool.fixture=python')\nprint('tool.args=' + '|'.join(sys.argv[1:]))\n",
    );
    write_executable(
        &exec.join("r-tool.R"),
        "#!/usr/bin/env Rscript\ncat('tool.fixture=r\\n')\ncat('tool.args=', paste(commandArgs(TRUE), collapse = '|'), '\\n', sep = '')\n",
    );
    write_executable(
        &exec.join("rapp-tool.R"),
        "#!/usr/bin/env Rapp\n#| name: rapp-tool\n#| description: Exercise Rapp dispatch.\n\n#| description: Value to echo\nvalue <- 'default'\n\ncat('tool.fixture=rapp\\n')\ncat('tool.value=', value, '\\n', sep = '')\n",
    );
    write_executable(
        &exec.join("sh-tool"),
        "#!/bin/sh\nprintf 'tool.fixture=sh\\n'\nprintf 'tool.args=%s\\n' \"$*\"\n",
    );

    DummyToolPackage {
        cache_dir: root.join("cache"),
        renv_cache_dir: root.join("renv-cache"),
        root,
        package_ref: format!("local::{}", package.display()),
    }
}

#[cfg(unix)]
fn write_executable(path: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
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
