//! Integration tests for the public `ir` CLI.

mod support;

use support::*;

use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn ci_dependencies_are_available() {
    let r_expr = concat!(
        "pkgs <- c(",
        "'pak', 'renv', 'secretbase', 'cli', 'glue', 'jsonlite', ",
        "'dplyr', 'tidyr', 'reticulate', 'knitr', 'rmarkdown', 'xfun', 'quarto', ",
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

#[cfg(target_os = "linux")]
fn r_tooling_lib(cache_dir: &Path) -> std::path::PathBuf {
    let out = Command::new(rscript())
        .env("IR_CACHE_DIR", cache_dir)
        .args([
            "--vanilla",
            "-e",
            "cat(file.path(Sys.getenv('IR_CACHE_DIR'), 'tooling', paste0(getRversion(), '-', R.version$platform)))",
        ])
        .output()
        .unwrap();
    assert_success(&out);
    Path::new(stdout(&out).trim()).to_path_buf()
}

#[cfg(target_os = "linux")]
fn install_fake_r_package(lib: &Path, name: &str, namespace: &str, r_code: &str) {
    let source_root = temp_dir(&format!("ir-fake-{name}-source"));
    let package = source_root.join(name);
    fs::create_dir_all(package.join("R")).unwrap();
    fs::write(
        package.join("DESCRIPTION"),
        format!(
            "Package: {name}\nVersion: 99.0.0\nTitle: Fake {name}\nDescription: Fake {name}.\nLicense: MIT\nEncoding: UTF-8\n"
        ),
    )
    .unwrap();
    fs::write(package.join("NAMESPACE"), namespace).unwrap();
    fs::write(package.join("R").join(format!("{name}.R")), r_code).unwrap();

    let out = Command::new(rscript())
        .args([
            "--vanilla",
            "-e",
            "args <- commandArgs(TRUE); dir.create(args[[1]], recursive = TRUE, showWarnings = FALSE); install.packages(args[[2]], lib = args[[1]], repos = NULL, type = 'source', INSTALL_opts = c('--no-byte-compile', '--no-help', '--no-docs'))",
        ])
        .arg(lib)
        .arg(&package)
        .output()
        .unwrap();
    assert_success(&out);
}

#[cfg(target_os = "linux")]
#[test]
fn run_uses_private_tooling_before_resolving_ppm_repos() {
    let cache_dir = temp_dir("ir-private-tooling-before-repos-cache");
    let user_library = temp_dir("ir-private-tooling-before-repos-user-library");
    let tooling_lib = r_tooling_lib(&cache_dir);
    let script = temp_path("ir-private-tooling-before-repos", "R");

    install_fake_r_package(
        &tooling_lib,
        "pak",
        "export(pkg_deps)\nexport(repo_resolve)\n",
        r#"
load_private_cli <- function() TRUE
repo_resolve <- function(spec) list(CRAN = "https://packagemanager.posit.co/cran/latest")
pkg_deps <- function(refs, ...) {
  data.frame(
    ref = character(),
    status = character(),
    package = character(),
    version = character(),
    type = character(),
    priority = character(),
    direct = logical()
  )
}
"#,
    );
    install_fake_r_package(
        &tooling_lib,
        "renv",
        "",
        "use <- function(...) stop('renv should not be used for an empty manifest')\n",
    );
    install_fake_r_package(
        &tooling_lib,
        "secretbase",
        "export(sha256)\n",
        "sha256 <- function(x) 'fake-resolution-key'\n",
    );
    install_fake_r_package(
        &user_library,
        "pak",
        "export(pkg_deps)\nexport(repo_resolve)\n",
        r#"
load_private_cli <- function() stop("bad ambient pak private cli")
repo_resolve <- function(spec) stop("bad ambient pak used before tooling bootstrap")
pkg_deps <- function(refs, ...) stop("bad ambient pak used before tooling bootstrap")
"#,
    );
    fs::write(
        &script,
        "cat('ir.fixture=private-tooling-before-repos\\n')\n",
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_LIBS_USER", &user_library)
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=private-tooling-before-repos");
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
    let source = temp_path("ir-run-qmd-uses-render", "qmd");
    fs::write(&source, "---\nir: [\n---\n").unwrap();

    let out = ir().args(["run"]).arg(&source).output().unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("use `ir render <source>`"),
        "{}",
        output_text(&out)
    );
}

#[test]
fn malformed_frontmatter_errors_before_resolution() {
    let script = temp_path("ir-malformed-frontmatter", "R");
    fs::write(
        &script,
        "#!/usr/bin/env -S ir run\n#| packages: [dplyr\n\ncat('not reached')\n",
    )
    .unwrap();

    let out = ir()
        .args(["run", script.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("could not parse script frontmatter as YAML"),
        "{}",
        output_text(&out)
    );
}

#[test]
fn frontmatter_packages_must_be_sequence() {
    let script = temp_path("ir-packages-scalar-frontmatter", "R");
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
    let script = temp_path("ir-packages-null-frontmatter", "R");
    let cache_dir = temp_cache("ir-packages-null-cache");
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

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=packages-null");
}

#[test]
fn run_script_frontmatter_accepts_packages_and_isolated() {
    let script = temp_path("ir-packages-frontmatter", "R");
    let cache_dir = temp_cache("ir-packages-frontmatter-cache");
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

    let user_library = temp_dir("ir-packages-frontmatter-user-library");
    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_LIBS_USER", &user_library)
        .args(["run", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=packages-frontmatter");
    assert_stdout_contains(&out, "frontmatter.glue_in_cache=true");
    assert_stdout_contains(&out, "frontmatter.user_library=NULL");
}

#[cfg(unix)]
#[test]
fn run_script_frontmatter_sets_reticulate_python_and_activates_env() {
    let cache_dir = temp_dir("ir-run-python-cache");
    let bin_dir = temp_dir("ir-run-python-bin");
    let script = temp_path("ir-run-python", "R");
    let venv = bin_dir.join("venv");
    let venv_bin = venv.join("bin");
    let fake_python = venv_bin.join("python");
    let rscript = bin_dir.join("Rscript");
    let r_deps = temp_path("ir-run-python-r-deps", "txt");
    let python_packages = temp_path("ir-run-python-packages", "txt");
    let python_env = temp_path("ir-run-python-env", "txt");
    let resolved_again = temp_path("ir-run-python-resolved-again", "txt");
    let _ = fs::remove_dir_all(cache_dir.join("python"));

    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| python-packages:
#|   - pandas
#| r-version: "4.4"
#| python-version: "3.11"
#| exclude-newer: "2026-06-01"

cat("reticulate_python=", Sys.getenv("RETICULATE_PYTHON"), "\n", sep = "")
"#,
    )
    .unwrap();
    fs::create_dir_all(&venv_bin).unwrap();
    write_executable(&fake_python, "#!/bin/sh\nexit 0\n");
    write_executable(
        &rscript,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  printf 'exclude_newer=%s\\n' \"${{IR_EXCLUDE_NEWER:-}}\" > {}\n\
  cat >> {}\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  if [ -n \"${{IR_RESOLUTION_MARKER:-}}\" ]; then\n\
    mkdir -p \"$(dirname \"$IR_RESOLUTION_MARKER\")\"\n\
    printf 'exclude-newer: %s\\n%s\\n' \"${{IR_EXCLUDE_NEWER:-}}\" \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLUTION_MARKER\"\n\
  fi\n\
  if [ -n \"${{IR_PYTHON_RESULT_FILE:-}}\" ]; then\n\
    if [ -z \"${{IR_PYTHON_PACKAGES_FILE:-}}\" ]; then\n\
      echo expected Python packages file in the R resolver invocation >&2\n\
      exit 1\n\
    fi\n\
    cat \"$IR_PYTHON_PACKAGES_FILE\" > {}\n\
    if [ ! -e {} ]; then\n\
      printf 'resolved_again\\n' > {}\n\
      mkdir -p {}\n\
      : > {}\n\
      chmod +x {}\n\
    fi\n\
    printf 'python_version=%s\\n' \"${{IR_PYTHON_VERSION:-}}\" > {}\n\
    printf 'exclude_newer=%s\\n' \"${{IR_PYTHON_EXCLUDE_NEWER:-}}\" >> {}\n\
    printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  fi\n\
  exit 0\n\
fi\n\
if [ -n \"${{IR_PYTHON_RESULT_FILE:-}}\" ]; then\n\
  echo Python resolution should not use a second resolver invocation >&2\n\
  exit 1\n\
fi\n\
printf 'reticulate_python=%s\\n' \"${{RETICULATE_PYTHON:-}}\"\n\
printf 'virtual_env=%s\\n' \"${{VIRTUAL_ENV:-}}\"\n\
printf 'path_first=%s\\n' \"${{PATH%%:*}}\"\n",
            r_deps.display(),
            r_deps.display(),
            python_packages.display(),
            fake_python.display(),
            resolved_again.display(),
            venv_bin.display(),
            fake_python.display(),
            fake_python.display(),
            python_env.display(),
            python_env.display(),
            fake_python.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--rscript"])
        .arg(&rscript)
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(
        &out,
        &format!("reticulate_python={}", fake_python.display()),
    );
    assert_stdout_contains(&out, &format!("virtual_env={}", venv.display()));
    assert_stdout_contains(&out, &format!("path_first={}", venv_bin.display()));

    let deps = fs::read_to_string(&r_deps).unwrap();
    assert!(deps.contains("exclude_newer=2026-06-01"), "{deps}");
    assert!(
        !deps.lines().any(|line| line == "reticulate"),
        "Python-only frontmatter should not inject user-library reticulate\n{deps}"
    );

    let packages = fs::read_to_string(&python_packages).unwrap();
    assert!(packages.contains("pandas"), "{packages}");
    assert!(!packages.contains("jupyter"), "{packages}");

    let env = fs::read_to_string(&python_env).unwrap();
    assert!(env.contains("python_version=3.11"), "{env}");
    assert!(env.contains("exclude_newer=2026-06-01"), "{env}");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--rscript"])
        .arg(&rscript)
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(
        &out,
        &format!("reticulate_python={}", fake_python.display()),
    );

    fs::remove_file(&fake_python).unwrap();
    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--rscript"])
        .arg(&rscript)
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert!(
        resolved_again.exists(),
        "missing cached Python path should invoke the shared resolver again"
    );
}

#[cfg(unix)]
#[test]
fn run_python_version_only_writes_empty_package_file_and_clears_pythonhome() {
    let cache_dir = temp_dir("ir-run-python-version-only-cache");
    let bin_dir = temp_dir("ir-run-python-version-only-bin");
    let script = temp_path("ir-run-python-version-only", "R");
    let venv = bin_dir.join("venv");
    let venv_bin = venv.join("bin");
    let fake_python = venv_bin.join("python");
    let rscript = bin_dir.join("Rscript");

    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| python-version: "3.11"

cat("ignored\n")
"#,
    )
    .unwrap();
    fs::create_dir_all(&venv_bin).unwrap();
    write_executable(&fake_python, "#!/bin/sh\nexit 0\n");
    write_executable(
        &rscript,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  if [ -n \"${{PYTHONHOME:-}}\" ]; then\n\
    echo \"resolver inherited PYTHONHOME=$PYTHONHOME\" >&2\n\
    exit 1\n\
  fi\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  if [ -z \"${{IR_PYTHON_PACKAGES_FILE:-}}\" ]; then\n\
    echo expected Python packages file >&2\n\
    exit 1\n\
  fi\n\
  if [ -s \"$IR_PYTHON_PACKAGES_FILE\" ]; then\n\
    echo expected empty Python packages file >&2\n\
    cat \"$IR_PYTHON_PACKAGES_FILE\" >&2\n\
    exit 1\n\
  fi\n\
  printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
printf 'pythonhome=%s\\n' \"${{PYTHONHOME:-<unset>}}\"\n\
printf 'virtual_env=%s\\n' \"${{VIRTUAL_ENV:-}}\"\n\
printf 'path_first=%s\\n' \"${{PATH%%:*}}\"\n",
            fake_python.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PYTHONHOME", "/old/python")
        .args(["run", "--rscript"])
        .arg(&rscript)
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "pythonhome=<unset>");
    assert_stdout_contains(&out, &format!("virtual_env={}", venv.display()));
    assert_stdout_contains(&out, &format!("path_first={}", venv_bin.display()));
}

#[cfg(unix)]
#[test]
fn run_python_packages_file_is_private_and_removed() {
    let cache_dir = temp_dir("ir-run-python-private-packages-file-cache");
    let bin_dir = temp_dir("ir-run-python-private-packages-file-bin");
    let script = temp_path("ir-run-python-private-packages-file", "R");
    let fake_python = bin_dir.join("python");
    let rscript = bin_dir.join("Rscript");
    let mode_file = temp_path("ir-run-python-private-packages-file-mode", "txt");
    let path_file = temp_path("ir-run-python-private-packages-file-path", "txt");

    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| python-packages:
#|   - pandas

cat("ignored\n")
"#,
    )
    .unwrap();
    write_executable(&fake_python, "#!/bin/sh\nexit 0\n");
    write_executable(
        &rscript,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  if [ -z \"${{IR_PYTHON_PACKAGES_FILE:-}}\" ]; then\n\
    echo expected Python packages file >&2\n\
    exit 1\n\
  fi\n\
  case \"$(uname -s)\" in\n\
    Darwin) mode=$(stat -f '%Lp' \"$IR_PYTHON_PACKAGES_FILE\") ;;\n\
    *) mode=$(stat -c '%a' \"$IR_PYTHON_PACKAGES_FILE\") ;;\n\
  esac\n\
  printf '%s\\n' \"$mode\" > {}\n\
  printf '%s\\n' \"$IR_PYTHON_PACKAGES_FILE\" > {}\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
printf 'ir.fixture=python-private-packages-file\\n'\n",
            mode_file.display(),
            path_file.display(),
            fake_python.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--rscript"])
        .arg(&rscript)
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=python-private-packages-file");

    let mode = fs::read_to_string(&mode_file).unwrap();
    assert_eq!(mode.trim(), "600", "{mode}");
    let packages_file = fs::read_to_string(&path_file).unwrap();
    let packages_file = Path::new(packages_file.trim());
    assert!(
        !packages_file.exists(),
        "Python packages temp file should be removed after resolver exits: {}",
        packages_file.display()
    );
}

#[cfg(unix)]
#[test]
fn run_python_packages_file_is_removed_when_rscript_spawn_fails() {
    let cache_dir = temp_dir("ir-run-python-packages-file-spawn-fail-cache");
    let tmp_dir = temp_dir("ir-run-python-packages-file-spawn-fail-tmp");
    let script = temp_path("ir-run-python-packages-file-spawn-fail", "R");
    let missing_rscript = temp_path("ir-run-python-packages-file-missing-rscript", "sh");

    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| python-packages:
#|   - pandas

cat("ignored\n")
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("TMPDIR", &tmp_dir)
        .args(["run", "--rscript"])
        .arg(&missing_rscript)
        .arg(&script)
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "missing Rscript should fail\n{}",
        output_text(&out)
    );

    let leftovers = fs::read_dir(&tmp_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("ir-python-packages-"))
        })
        .collect::<Vec<_>>();
    assert!(
        leftovers.is_empty(),
        "Python packages temp files should be removed after spawn failure: {leftovers:?}"
    );
}

#[cfg(unix)]
#[test]
fn run_python_exclude_newer_override_uses_normalized_latest() {
    for exclude_newer in [" \t ", "2999-01-01"] {
        let cache_dir = temp_dir("ir-run-python-exclude-newer-latest-cache");
        let bin_dir = temp_dir("ir-run-python-exclude-newer-latest-bin");
        let script = temp_path("ir-run-python-exclude-newer-latest", "R");
        let fake_python = bin_dir.join("python");
        let rscript = bin_dir.join("Rscript");

        fs::write(
            &script,
            r#"#!/usr/bin/env -S ir run
#| python-packages:
#|   - pandas
#| exclude-newer: "2024-01-01"

cat("ignored\n")
"#,
        )
        .unwrap();
        write_executable(&fake_python, "#!/bin/sh\nexit 0\n");
        write_executable(
            &rscript,
            &format!(
                "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  if [ -n \"${{IR_PYTHON_EXCLUDE_NEWER:-}}\" ]; then\n\
    echo \"unexpected Python exclude-newer: $IR_PYTHON_EXCLUDE_NEWER\" >&2\n\
    exit 1\n\
  fi\n\
  printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
printf 'ir.fixture=python-exclude-newer-latest\\n'\n",
                fake_python.display()
            ),
        );

        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .args(["run", "--exclude-newer", exclude_newer, "--rscript"])
            .arg(&rscript)
            .arg(&script)
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, "ir.fixture=python-exclude-newer-latest");

        let python_dir = cache_dir.join("python");
        let markers = fs::read_dir(&python_dir)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", python_dir.display()))
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(markers.len(), 1);
        let marker_text = fs::read_to_string(&markers[0])
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", markers[0].display()));
        assert!(
            marker_text
                .lines()
                .next()
                .is_some_and(|line| line.starts_with("latest: ")),
            "{marker_text}"
        );
    }
}

#[cfg(unix)]
#[test]
fn run_python_exclude_newer_can_override_r_exclude_newer() {
    struct Case<'a> {
        name: &'a str,
        python_exclude_newer: &'a str,
        cli_args: &'a [&'a str],
        expected_python_exclude_newer: &'a str,
    }

    let cases = [
        Case {
            name: "frontmatter",
            python_exclude_newer: r#"#| python-exclude-newer: "2024-02-02"
"#,
            cli_args: &[],
            expected_python_exclude_newer: "2024-02-02",
        },
        Case {
            name: "frontmatter-null",
            python_exclude_newer: "#| python-exclude-newer: null\n",
            cli_args: &[],
            expected_python_exclude_newer: "",
        },
        Case {
            name: "cli",
            python_exclude_newer: r#"#| python-exclude-newer: "2024-02-02"
"#,
            cli_args: &["--python-exclude-newer", "2024-03-03"],
            expected_python_exclude_newer: "2024-03-03",
        },
        Case {
            name: "cli-empty",
            python_exclude_newer: r#"#| python-exclude-newer: "2024-02-02"
"#,
            cli_args: &["--python-exclude-newer", " \t "],
            expected_python_exclude_newer: "",
        },
    ];

    for case in cases {
        let cache_dir = temp_dir(&format!("ir-run-python-exclude-newer-{}-cache", case.name));
        let bin_dir = temp_dir(&format!("ir-run-python-exclude-newer-{}-bin", case.name));
        let script = temp_path(&format!("ir-run-python-exclude-newer-{}", case.name), "R");
        let fake_python = bin_dir.join("python");
        let rscript = bin_dir.join("Rscript");

        fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env -S ir run
#| python-packages:
#|   - pandas
#| exclude-newer: "2024-01-01"
{}

cat("ignored\n")
"#,
                case.python_exclude_newer
            ),
        )
        .unwrap();
        write_executable(&fake_python, "#!/bin/sh\nexit 0\n");
        write_executable(
            &rscript,
            &format!(
                "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  if [ \"${{IR_EXCLUDE_NEWER:-}}\" != \"2024-01-01\" ]; then\n\
    echo \"unexpected R exclude-newer: $IR_EXCLUDE_NEWER\" >&2\n\
    exit 1\n\
  fi\n\
  if [ \"${{IR_PYTHON_EXCLUDE_NEWER:-}}\" != \"{}\" ]; then\n\
    echo \"unexpected Python exclude-newer: $IR_PYTHON_EXCLUDE_NEWER\" >&2\n\
    exit 1\n\
  fi\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
printf 'ir.fixture=python-exclude-newer-{}\\n'\n",
                case.expected_python_exclude_newer,
                fake_python.display(),
                case.name
            ),
        );

        let mut command = ir();
        command.env("IR_CACHE_DIR", &cache_dir).arg("run");
        command.args(case.cli_args);
        let out = command
            .arg("--rscript")
            .arg(&rscript)
            .arg(&script)
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(
            &out,
            &format!("ir.fixture=python-exclude-newer-{}", case.name),
        );
    }
}

#[cfg(unix)]
#[test]
fn run_python_local_package_bypasses_python_resolution_cache() {
    let cache_dir = temp_dir("ir-run-python-local-cache");
    let bin_dir = temp_dir("ir-run-python-local-bin");
    let script = temp_path("ir-run-python-local", "R");
    let fake_python = bin_dir.join("python");
    let rscript = bin_dir.join("Rscript");
    let resolver_count = temp_path("ir-run-python-local-count", "txt");
    let packages_seen = temp_path("ir-run-python-local-packages", "txt");

    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| python-packages:
#|   - ./local-python-package

cat("ignored\n")
"#,
    )
    .unwrap();
    write_executable(&fake_python, "#!/bin/sh\nexit 0\n");
    write_executable(
        &rscript,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  cat > /dev/null\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  if [ -n \"${{IR_RESOLUTION_MARKER:-}}\" ]; then\n\
    mkdir -p \"$(dirname \"$IR_RESOLUTION_MARKER\")\"\n\
    printf 'latest: %s\\n%s\\n' \"$(date +%s)\" \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLUTION_MARKER\"\n\
  fi\n\
  if [ -n \"${{IR_PYTHON_RESULT_FILE:-}}\" ]; then\n\
    cat \"$IR_PYTHON_PACKAGES_FILE\" > {}\n\
    printf 'python\\n' >> {}\n\
    printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  fi\n\
  exit 0\n\
fi\n\
if [ -n \"${{IR_PYTHON_RESULT_FILE:-}}\" ]; then\n\
  cat \"$IR_PYTHON_PACKAGES_FILE\" > {}\n\
  printf 'python\\n' >> {}\n\
  printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
printf 'ir.fixture=python-local-cache\\n'\n",
            packages_seen.display(),
            resolver_count.display(),
            fake_python.display(),
            packages_seen.display(),
            resolver_count.display(),
            fake_python.display()
        ),
    );

    for _ in 0..2 {
        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .args(["run", "--rscript"])
            .arg(&rscript)
            .arg(&script)
            .output()
            .unwrap();
        assert_success(&out);
        assert_stdout_contains(&out, "ir.fixture=python-local-cache");
    }

    let count = fs::read_to_string(&resolver_count).unwrap();
    assert_eq!(
        count.lines().count(),
        2,
        "local Python package refs should resolve on every run\n{count}"
    );
    let packages = fs::read_to_string(&packages_seen).unwrap();
    assert_eq!(packages.trim(), "./local-python-package");
}

#[cfg(unix)]
#[test]
fn run_python_uv_resolver_env_bypasses_python_resolution_cache() {
    let cache_dir = temp_dir("ir-run-python-uv-env-cache");
    let bin_dir = temp_dir("ir-run-python-uv-env-bin");
    let script = temp_path("ir-run-python-uv-env", "R");
    let fake_python = bin_dir.join("python");
    let rscript = bin_dir.join("Rscript");
    let resolver_count = temp_path("ir-run-python-uv-env-count", "txt");
    let indexes_seen = temp_path("ir-run-python-uv-env-indexes", "txt");

    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| python-packages:
#|   - pandas

cat("ignored\n")
"#,
    )
    .unwrap();
    write_executable(&fake_python, "#!/bin/sh\nexit 0\n");
    write_executable(
        &rscript,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  cat > /dev/null\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  if [ -n \"${{IR_RESOLUTION_MARKER:-}}\" ]; then\n\
    mkdir -p \"$(dirname \"$IR_RESOLUTION_MARKER\")\"\n\
    printf 'latest: %s\\n%s\\n' \"$(date +%s)\" \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLUTION_MARKER\"\n\
  fi\n\
  if [ -n \"${{IR_PYTHON_RESULT_FILE:-}}\" ]; then\n\
    printf 'python\\n' >> {}\n\
    printf '%s\\n' \"${{UV_DEFAULT_INDEX:-}}\" >> {}\n\
    printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  fi\n\
  exit 0\n\
fi\n\
if [ -n \"${{IR_PYTHON_RESULT_FILE:-}}\" ]; then\n\
  printf 'python\\n' >> {}\n\
  printf '%s\\n' \"${{UV_DEFAULT_INDEX:-}}\" >> {}\n\
  printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
printf 'ir.fixture=python-uv-env-cache\\n'\n",
            resolver_count.display(),
            indexes_seen.display(),
            fake_python.display(),
            resolver_count.display(),
            indexes_seen.display(),
            fake_python.display()
        ),
    );

    for index in [
        "https://first.example/simple",
        "https://second.example/simple",
    ] {
        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .env("UV_DEFAULT_INDEX", index)
            .args(["run", "--rscript"])
            .arg(&rscript)
            .arg(&script)
            .output()
            .unwrap();
        assert_success(&out);
        assert_stdout_contains(&out, "ir.fixture=python-uv-env-cache");
    }

    let count = fs::read_to_string(&resolver_count).unwrap();
    assert_eq!(
        count.lines().count(),
        2,
        "uv resolver env should bypass the Python marker\n{count}"
    );
    let indexes = fs::read_to_string(&indexes_seen).unwrap();
    assert!(
        indexes.contains("https://first.example/simple"),
        "{indexes}"
    );
    assert!(
        indexes.contains("https://second.example/simple"),
        "{indexes}"
    );
}

#[cfg(unix)]
#[test]
fn run_python_uv_config_changes_python_resolution_cache() {
    let cache_dir = temp_dir("ir-run-python-uv-config-cache");
    let config_home = temp_dir("ir-run-python-uv-config-home");
    let bin_dir = temp_dir("ir-run-python-uv-config-bin");
    let script = temp_path("ir-run-python-uv-config", "R");
    let fake_python = bin_dir.join("python");
    let rscript = bin_dir.join("Rscript");
    let resolver_count = temp_path("ir-run-python-uv-config-count", "txt");
    let configs_seen = temp_path("ir-run-python-uv-config-seen", "txt");
    let uv_dir = config_home.join("uv");
    let uv_config = uv_dir.join("uv.toml");

    fs::create_dir_all(&uv_dir).unwrap();
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| python-packages:
#|   - pandas

cat("ignored\n")
"#,
    )
    .unwrap();
    write_executable(&fake_python, "#!/bin/sh\nexit 0\n");
    write_executable(
        &rscript,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  cat > /dev/null\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  if [ -n \"${{IR_RESOLUTION_MARKER:-}}\" ]; then\n\
    mkdir -p \"$(dirname \"$IR_RESOLUTION_MARKER\")\"\n\
    printf 'latest: %s\\n%s\\n' \"$(date +%s)\" \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLUTION_MARKER\"\n\
  fi\n\
  if [ -n \"${{IR_PYTHON_RESULT_FILE:-}}\" ]; then\n\
    printf 'python\\n' >> {}\n\
    cat \"$XDG_CONFIG_HOME/uv/uv.toml\" >> {}\n\
    printf '\\n---\\n' >> {}\n\
    printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  fi\n\
  exit 0\n\
fi\n\
if [ -n \"${{IR_PYTHON_RESULT_FILE:-}}\" ]; then\n\
  printf 'python\\n' >> {}\n\
  cat \"$XDG_CONFIG_HOME/uv/uv.toml\" >> {}\n\
  printf '\\n---\\n' >> {}\n\
  printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
printf 'ir.fixture=python-uv-config-cache\\n'\n",
            resolver_count.display(),
            configs_seen.display(),
            configs_seen.display(),
            fake_python.display(),
            resolver_count.display(),
            configs_seen.display(),
            configs_seen.display(),
            fake_python.display()
        ),
    );

    for config in [
        "[[index]]\nurl = \"https://first.example/simple\"\ndefault = true\n",
        "[[index]]\nurl = \"https://second.example/simple\"\ndefault = true\n",
        "[[index]]\nurl = \"https://second.example/simple\"\ndefault = true\n",
    ] {
        fs::write(&uv_config, config).unwrap();

        let mut command = ir();
        remove_uv_resolver_env(&mut command);
        let out = command
            .env("IR_CACHE_DIR", &cache_dir)
            .env("XDG_CONFIG_HOME", &config_home)
            .args(["run", "--rscript"])
            .arg(&rscript)
            .arg(&script)
            .output()
            .unwrap();
        assert_success(&out);
        assert_stdout_contains(&out, "ir.fixture=python-uv-config-cache");
    }

    let count = fs::read_to_string(&resolver_count).unwrap();
    assert_eq!(
        count.lines().count(),
        2,
        "uv.toml changes should invalidate the Python marker, unchanged uv.toml should reuse it\n{count}"
    );
    let configs = fs::read_to_string(&configs_seen).unwrap();
    assert!(
        configs.contains("https://first.example/simple"),
        "{configs}"
    );
    assert!(
        configs.contains("https://second.example/simple"),
        "{configs}"
    );
}

#[cfg(unix)]
fn remove_uv_resolver_env(command: &mut Command) {
    for (name, _) in std::env::vars_os() {
        if name
            .to_str()
            .is_some_and(|name| name.starts_with("UV_") || name == "RETICULATE_UV")
        {
            command.env_remove(name);
        }
    }
}

#[cfg(unix)]
#[test]
fn run_python_only_resolution_clears_inherited_r_resolver_env() {
    let cache_dir = temp_dir("ir-run-python-only-clears-r-resolver-env-cache");
    let bin_dir = temp_dir("ir-run-python-only-clears-r-resolver-env-bin");
    let script = temp_path("ir-run-python-only-clears-r-resolver-env", "R");
    let fake_python = bin_dir.join("python");
    let rscript = bin_dir.join("Rscript");

    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| packages:
#|   - cli
#| python-packages:
#|   - pandas

cat("ignored\n")
"#,
    )
    .unwrap();
    write_executable(&fake_python, "#!/bin/sh\nexit 0\n");
    write_executable(
        &rscript,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_PYTHON_RESULT_FILE:-}}\" ]; then\n\
  if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
    if [ \"${{IR_RESOLVE_RESULT_FILE:-}}\" = \"/tmp/stale-r-result\" ]; then\n\
      echo \"unexpected inherited IR_RESOLVE_RESULT_FILE=$IR_RESOLVE_RESULT_FILE\" >&2\n\
      exit 1\n\
    fi\n\
    cat > /dev/null\n\
    mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
    printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
    if [ -n \"${{IR_RESOLUTION_MARKER:-}}\" ]; then\n\
      mkdir -p \"$(dirname \"$IR_RESOLUTION_MARKER\")\"\n\
      printf 'latest: %s\\n%s\\n' \"$(date +%s)\" \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLUTION_MARKER\"\n\
    fi\n\
    printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
    exit 0\n\
  fi\n\
  for name in IR_RESOLVE_RESULT_FILE IR_RESOLVE_PACKAGE_RESULT_FILE IR_RESOLUTION_MARKER IR_PRIMARY_PACKAGE_MARKER IR_QUARTO_RENDER; do\n\
    eval value=\\${{$name:-}}\n\
    if [ -n \"$value\" ]; then\n\
      echo \"unexpected inherited $name=$value\" >&2\n\
      exit 1\n\
    fi\n\
  done\n\
  printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ] && [ \"${{IR_RESOLVE_RESULT_FILE:-}}\" != \"/tmp/stale-r-result\" ]; then\n\
  cat > /dev/null\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
printf 'ir.fixture=cleared-r-resolver-env\\n'\n",
            fake_python.display(),
            fake_python.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--rscript"])
        .arg(&rscript)
        .arg(&script)
        .output()
        .unwrap();
    assert_success(&out);

    fs::remove_dir_all(cache_dir.join("python")).unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RESOLVE_RESULT_FILE", "/tmp/stale-r-result")
        .env("IR_RESOLVE_PACKAGE_RESULT_FILE", "/tmp/stale-r-package")
        .env("IR_RESOLUTION_MARKER", "/tmp/stale-r-marker")
        .env("IR_PRIMARY_PACKAGE_MARKER", "/tmp/stale-r-primary-marker")
        .env("IR_QUARTO_RENDER", "1")
        .args(["run", "--rscript"])
        .arg(&rscript)
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=cleared-r-resolver-env");
}

#[cfg(unix)]
#[test]
fn run_r_only_resolution_clears_inherited_python_resolver_env() {
    let cache_dir = temp_dir("ir-run-r-only-clears-python-resolver-env-cache");
    let bin_dir = temp_dir("ir-run-r-only-clears-python-resolver-env-bin");
    let script = temp_path("ir-run-r-only-clears-python-resolver-env", "R");
    let rscript = bin_dir.join("Rscript");

    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| packages:
#|   - cli

cat("ignored\n")
"#,
    )
    .unwrap();
    write_executable(
        &rscript,
        "#!/bin/sh\n\
if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n\
  for name in IR_PYTHON_RESULT_FILE IR_PYTHON_PACKAGES_FILE IR_PYTHON_VERSION IR_PYTHON_EXCLUDE_NEWER; do\n\
    eval value=\\${$name:-}\n\
    if [ -n \"$value\" ]; then\n\
      echo \"unexpected inherited $name=$value\" >&2\n\
      exit 1\n\
    fi\n\
  done\n\
  cat > /dev/null\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
printf 'ir.fixture=cleared-python-resolver-env\\n'\n",
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_PYTHON_RESULT_FILE", "/tmp/stale-python-result")
        .env("IR_PYTHON_PACKAGES_FILE", "/tmp/stale-python-packages")
        .env("IR_PYTHON_VERSION", "9.99")
        .env("IR_PYTHON_EXCLUDE_NEWER", "1999-01-01")
        .args(["run", "--rscript"])
        .arg(&rscript)
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=cleared-python-resolver-env");
}

#[test]
fn cache_dir_reports_override_and_process_env_defaults() {
    let cache_dir = temp_dir("ir-cache-override");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["cache", "dir"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(stdout(&out), format!("{}\n", cache_dir.display()));

    let r_user_cache_dir = temp_dir("ir-cache-r-user");
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

    let xdg_cache_home = temp_dir("ir-cache-xdg-default");
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
}

#[cfg(windows)]
#[test]
fn cache_dir_falls_back_to_userprofile_without_localappdata() {
    let user_profile = temp_dir("ir-cache-userprofile");

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
}

#[test]
fn cache_dir_ignores_r_user_cache_dir_from_r_environ_user() {
    let xdg_cache_home = temp_dir("ir-cache-xdg");
    let renviron_cache = temp_dir("ir-cache-renviron");
    let renviron = temp_path("ir-cache-renviron", "Renviron");
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
}

#[test]
fn run_with_ir_cache_dir_does_not_require_home_cache_env_for_resolver_lock() {
    let cache_dir = temp_dir("ir-cache-lock-override");
    let profile = temp_path("ir-cache-lock-override-profile", "R");
    fs::write(
        &profile,
        r#"
if (nzchar(Sys.getenv("IR_RESOLVE_RESULT_FILE"))) {
  writeLines("", Sys.getenv("IR_RESOLVE_RESULT_FILE"))
  q("no", status = 0L, runLast = FALSE)
}
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_PROFILE_USER", &profile)
        .env_remove("R_USER_CACHE_DIR")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("HOME")
        .env_remove("LOCALAPPDATA")
        .env_remove("USERPROFILE")
        .args(["run", "--isolated", "--vanilla", "-e"])
        .arg("cat('ir.fixture=cache-lock-override\n')")
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=cache-lock-override");
}

#[test]
fn run_with_ir_cache_dir_ignores_unusable_user_cache_for_resolver_lock() {
    let cache_dir = temp_dir("ir-cache-lock-unusable-override");
    let r_user_cache_file = temp_path("ir-cache-lock-unusable-r-cache", "txt");
    let profile = temp_path("ir-cache-lock-unusable-profile", "R");
    fs::write(&r_user_cache_file, "not a directory\n").unwrap();
    fs::write(
        &profile,
        r#"
if (nzchar(Sys.getenv("IR_RESOLVE_RESULT_FILE"))) {
  writeLines("", Sys.getenv("IR_RESOLVE_RESULT_FILE"))
  q("no", status = 0L, runLast = FALSE)
}
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_USER_CACHE_DIR", &r_user_cache_file)
        .env("R_PROFILE_USER", &profile)
        .args(["run", "--isolated", "--vanilla", "-e"])
        .arg("cat('ir.fixture=cache-lock-unusable-override\n')")
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=cache-lock-unusable-override");
}

#[test]
fn cache_clean_removes_cache_dir() {
    let cache_dir = temp_dir("ir-cache-clean");
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
    let cache_dir = temp_cache("ir-run-script-cache");

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
}

#[test]
fn run_script_uses_only_the_first_yaml_document() {
    let script = fixture("run/multiple-documents.R");
    let cache_dir = temp_cache("ir-multi-doc-cache");

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
}

#[test]
fn run_inline_expression_resolves_with_dependencies() {
    let cache_dir = temp_cache("ir-inline-cache");
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
}

#[test]
fn run_inline_expression_forwards_option_like_args_after_expr() {
    let cache_dir = temp_cache("ir-inline-args-cache");
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
}

#[test]
fn run_normalizes_version_specs_before_resolution_cache_keying() {
    let cache_dir = temp_dir("ir-ref-normalized-cache");
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

    assert_eq!(resolution_count, 1);
}

#[test]
fn run_trims_env_exclude_newer_before_resolution_cache_keying() {
    let cache_dir = temp_dir("ir-env-exclude-newer-normalized-cache");
    let profile = temp_path("ir-env-exclude-newer-normalized-profile", "R");
    let entered = temp_path("ir-env-exclude-newer-normalized-entered", "txt");
    fs::write(
        &profile,
        r#"
if (nzchar(Sys.getenv("IR_RESOLVE_RESULT_FILE"))) {
  cat(Sys.getpid(), "\n", file = Sys.getenv("IR_TEST_ENTERED"), append = TRUE)
}
"#,
    )
    .unwrap();
    let expr = "cat('ir.fixture=normalized-exclude-newer-cache\\n')";

    for _ in 0..2 {
        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .env("IR_EXCLUDE_NEWER", " 2024-06-01 ")
            .env("IR_RSCRIPT", rscript())
            .env("R_PROFILE_USER", &profile)
            .env("IR_TEST_ENTERED", &entered)
            .args(["run", "--isolated", "--vanilla", "-e", expr])
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, "ir.fixture=normalized-exclude-newer-cache");
    }

    assert_eq!(
        resolver_probe_count(&entered),
        1,
        "second run should reuse the Rust warm resolution cache"
    );

    let resolution_dir = cache_dir.join("resolutions");
    let markers = fs::read_dir(&resolution_dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", resolution_dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 1);
    let marker_text = fs::read_to_string(&markers[0])
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", markers[0].display()));
    assert_eq!(
        marker_text.lines().next(),
        Some("exclude-newer: 2024-06-01")
    );
}

#[test]
fn run_cli_exclude_newer_overrides_env_and_frontmatter() {
    let cache_dir = temp_dir("ir-cli-exclude-newer-precedence-cache");
    let script = temp_path("ir-cli-exclude-newer-precedence", "R");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| exclude-newer: 2024-01-01

cat("ir.fixture=cli-exclude-newer-precedence\n")
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_EXCLUDE_NEWER", " \t ")
        .args(["run", "--exclude-newer", " 2024-03-01 ", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=cli-exclude-newer-precedence");

    let marker_text = only_resolution_marker_text(&cache_dir);
    assert_eq!(
        marker_text.lines().next(),
        Some("exclude-newer: 2024-03-01")
    );
}

#[test]
fn run_empty_cli_exclude_newer_overrides_frontmatter_with_latest() {
    let cache_dir = temp_dir("ir-empty-cli-exclude-newer-cache");
    let script = temp_path("ir-empty-cli-exclude-newer", "R");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| exclude-newer: 2024-01-01

cat("ir.fixture=empty-cli-exclude-newer\n")
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--exclude-newer", " \t ", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=empty-cli-exclude-newer");

    let marker_text = only_resolution_marker_text(&cache_dir);
    assert!(
        marker_text
            .lines()
            .next()
            .is_some_and(|line| line.starts_with("latest: ")),
        "{marker_text}"
    );
}

#[test]
fn run_future_cli_exclude_newer_overrides_frontmatter_with_latest() {
    let cache_dir = temp_dir("ir-future-cli-exclude-newer-cache");
    let script = temp_path("ir-future-cli-exclude-newer", "R");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| exclude-newer: 2024-01-01

cat("ir.fixture=future-cli-exclude-newer\n")
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--exclude-newer", "2999-01-01", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=future-cli-exclude-newer");

    let marker_text = only_resolution_marker_text(&cache_dir);
    assert!(
        marker_text
            .lines()
            .next()
            .is_some_and(|line| line.starts_with("latest: ")),
        "{marker_text}"
    );
}

#[cfg(unix)]
#[test]
fn render_cli_exclude_newer_overrides_env_and_frontmatter() {
    let cache_dir = temp_dir("ir-render-cli-exclude-newer-precedence-cache");
    let library = temp_dir("ir-render-cli-exclude-newer-precedence-library");
    let doc = temp_path("ir-render-cli-exclude-newer-precedence", "qmd");
    let profile = temp_path("ir-render-cli-exclude-newer-precedence-profile", "R");
    let quarto = temp_path("ir-render-cli-exclude-newer-precedence-quarto", "");
    fs::write(
        &doc,
        r#"---
title: CLI exclude newer precedence
ir:
  exclude-newer: 2024-01-01
---

```{r}
cat("ir.fixture=render-cli-exclude-newer-precedence\n")
```
"#,
    )
    .unwrap();
    fs::write(
        &profile,
        r#"
if (nzchar(Sys.getenv("IR_RESOLVE_RESULT_FILE"))) {
  library <- Sys.getenv("IR_TEST_LIBRARY")
  dir.create(library, recursive = TRUE, showWarnings = FALSE)
  marker <- Sys.getenv("IR_RESOLUTION_MARKER")
  if (nzchar(marker)) {
    dir.create(dirname(marker), recursive = TRUE, showWarnings = FALSE)
    writeLines(c(
      paste("exclude-newer:", Sys.getenv("IR_EXCLUDE_NEWER")),
      library
    ), marker)
  }
  writeLines(library, Sys.getenv("IR_RESOLVE_RESULT_FILE"))
  q(save = "no", status = 0)
}
"#,
    )
    .unwrap();
    write_executable(&quarto, "#!/bin/sh\nexit 0\n");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_EXCLUDE_NEWER", " \t ")
        .env("IR_QUARTO", &quarto)
        .env("IR_RSCRIPT", rscript())
        .env("IR_TEST_LIBRARY", &library)
        .env("R_PROFILE_USER", &profile)
        .args(["render", "--exclude-newer", " 2024-03-01 "])
        .arg(&doc)
        .output()
        .unwrap();

    assert_success(&out);

    let marker_text = only_resolution_marker_text(&cache_dir);
    assert_eq!(
        marker_text.lines().next(),
        Some("exclude-newer: 2024-03-01")
    );
}

#[test]
fn run_empty_env_exclude_newer_overrides_frontmatter_with_latest() {
    let cache_dir = temp_dir("ir-empty-env-exclude-newer-cache");
    let script = temp_path("ir-empty-env-exclude-newer", "R");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| exclude-newer: 2024-01-01

cat("ir.fixture=empty-env-exclude-newer\n")
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_EXCLUDE_NEWER", "")
        .args(["run", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=empty-env-exclude-newer");

    let marker_text = only_resolution_marker_text(&cache_dir);
    assert!(
        marker_text
            .lines()
            .next()
            .is_some_and(|line| line.starts_with("latest: ")),
        "{marker_text}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn run_defaults_unset_cran_resolves_with_real_pak_ppm_repo() {
    let cache_dir = temp_dir("ir-real-pak-ppm-cache");
    let profile = temp_path("ir-real-pak-ppm-profile", "R");
    fs::write(&profile, r#"options(repos = "@CRAN@")"#).unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_PROFILE_USER", &profile)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "-e",
            "cat('ir.fixture=real-pak-ppm\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=real-pak-ppm");
}

#[cfg(target_os = "linux")]
#[test]
fn run_plain_ppm_latest_profile_resolves_with_real_pak_binary_repo() {
    let cache_dir = temp_dir("ir-real-pak-ppm-latest-cache");
    let renv_cache = temp_cache("ir-real-pak-ppm-latest-renv-cache");
    let profile = temp_path("ir-real-pak-ppm-latest-profile", "R");
    fs::write(
        &profile,
        r#"options(repos = c(CRAN = "https://packagemanager.posit.co/cran/latest"))"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("RENV_PATHS_CACHE", &renv_cache)
        .env("R_PROFILE_USER", &profile)
        .env("CC", "false")
        .env("CXX", "false")
        .env("CXX11", "false")
        .env("CXX14", "false")
        .env("CXX17", "false")
        .env("CXX20", "false")
        .args([
            "run",
            "--isolated",
            "--with",
            "zip",
            "-e",
            "library(zip); cat('ir.fixture=real-pak-ppm-latest\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=real-pak-ppm-latest");
}

#[test]
fn run_future_env_exclude_newer_overrides_frontmatter_with_latest() {
    let cache_dir = temp_dir("ir-future-env-exclude-newer-cache");
    let script = temp_path("ir-future-env-exclude-newer", "R");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| exclude-newer: 2024-01-01

cat("ir.fixture=future-env-exclude-newer\n")
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_EXCLUDE_NEWER", "2999-01-01")
        .args(["run", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=future-env-exclude-newer");

    let marker_text = only_resolution_marker_text(&cache_dir);
    assert!(
        marker_text
            .lines()
            .next()
            .is_some_and(|line| line.starts_with("latest: ")),
        "{marker_text}"
    );
}

#[test]
fn run_future_frontmatter_exclude_newer_resolves_latest() {
    let cache_dir = temp_dir("ir-future-frontmatter-exclude-newer-cache");
    let script = temp_path("ir-future-frontmatter-exclude-newer", "R");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| exclude-newer: 2999-01-01

cat("ir.fixture=future-frontmatter-exclude-newer\n")
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=future-frontmatter-exclude-newer");

    let marker_text = only_resolution_marker_text(&cache_dir);
    assert!(
        marker_text
            .lines()
            .next()
            .is_some_and(|line| line.starts_with("latest: ")),
        "{marker_text}"
    );
}

#[cfg(unix)]
#[test]
fn render_future_frontmatter_exclude_newer_resolves_latest() {
    let cache_dir = temp_dir("ir-render-future-frontmatter-exclude-newer-cache");
    let library = temp_dir("ir-render-future-frontmatter-exclude-newer-library");
    let doc = temp_path("ir-render-future-frontmatter-exclude-newer", "qmd");
    let profile = temp_path("ir-render-future-frontmatter-exclude-newer-profile", "R");
    let quarto = temp_path("ir-render-future-frontmatter-exclude-newer-quarto", "");
    fs::write(
        &doc,
        r#"---
title: Future frontmatter exclude newer
ir:
  exclude-newer: 2999-01-01
---

```{r}
cat("ir.fixture=render-future-frontmatter-exclude-newer\n")
```
"#,
    )
    .unwrap();
    fs::write(
        &profile,
        r#"
if (nzchar(Sys.getenv("IR_RESOLVE_RESULT_FILE"))) {
  library <- Sys.getenv("IR_TEST_LIBRARY")
  dir.create(library, recursive = TRUE, showWarnings = FALSE)
  marker <- Sys.getenv("IR_RESOLUTION_MARKER")
  if (nzchar(marker)) {
    dir.create(dirname(marker), recursive = TRUE, showWarnings = FALSE)
    source <- if (nzchar(Sys.getenv("IR_EXCLUDE_NEWER"))) {
      paste("exclude-newer:", Sys.getenv("IR_EXCLUDE_NEWER"))
    } else {
      "latest: 0"
    }
    writeLines(c(source, library), marker)
  }
  writeLines(library, Sys.getenv("IR_RESOLVE_RESULT_FILE"))
  q(save = "no", status = 0)
}
"#,
    )
    .unwrap();
    write_executable(&quarto, "#!/bin/sh\nexit 0\n");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_QUARTO", &quarto)
        .env("IR_RSCRIPT", rscript())
        .env("IR_TEST_LIBRARY", &library)
        .env("R_PROFILE_USER", &profile)
        .args(["render"])
        .arg(&doc)
        .output()
        .unwrap();

    assert_success(&out);

    let marker_text = only_resolution_marker_text(&cache_dir);
    assert!(
        marker_text
            .lines()
            .next()
            .is_some_and(|line| line.starts_with("latest: ")),
        "{marker_text}"
    );
}

#[test]
fn run_frontmatter_github_ref_installs_github_package() {
    let cache_dir = temp_dir("ir-github-ref-cache");
    let script = temp_path("ir-github-ref", "R");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| packages:
#|   - github::rstudio/reticulate

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
  nzchar(desc$RemoteSha)
)
cat("ir.fixture=github-ref\n")
cat("github.remote=", paste(
  desc$RemoteType,
  desc$RemoteUsername,
  desc$RemoteRepo,
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
    assert_stdout_contains(&out, "github.remote=github/rstudio/reticulate");
}

#[test]
fn run_frontmatter_github_subdir_ref_installs_subdir_package() {
    let cache_dir = temp_dir("ir-github-subdir-ref-cache");
    let script = temp_path("ir-github-subdir-ref", "R");
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
}

#[test]
fn run_frontmatter_preserves_transitive_source_refs() {
    let cache_dir = temp_dir("ir-transitive-source-cache");
    let package_dir = temp_dir("ir-transitive-source-packages");
    let dep = write_r_source_package(&package_dir, "irdep", &[]);
    let parent = write_r_source_package(
        &package_dir,
        "irparent",
        &[
            "Imports: irdep".to_string(),
            format!("Remotes: irdep=local::{}", renviron_path(&dep)),
        ],
    );
    let script = temp_path("ir-transitive-source", "R");
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
}

#[test]
fn run_frontmatter_local_ref_reruns_resolution_when_package_changes() {
    let cache_dir = temp_dir("ir-local-ref-cache");
    let package_dir = temp_dir("ir-local-ref-packages");
    let package = write_r_source_package(&package_dir, "irlocal", &[]);
    let script = temp_path("ir-local-ref", "R");
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
}

#[test]
fn run_frontmatter_local_ref_with_pak_params_installs_local_package() {
    let cache_dir = temp_dir("ir-local-ref-params-cache");
    let package_dir = temp_dir("ir-local-ref-params-packages");
    let package = write_r_source_package(&package_dir, "irlocal", &[]);
    let script = temp_path("ir-local-ref-params", "R");
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
}

#[test]
fn run_frontmatter_named_local_ref_installs_local_package() {
    let cache_dir = temp_dir("ir-named-local-ref-cache");
    let package_dir = temp_dir("ir-named-local-ref-packages");
    let package = write_r_source_package(&package_dir, "irlocal", &[]);
    let script = temp_path("ir-named-local-ref", "R");
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
}

#[test]
fn run_frontmatter_sequence_entry_preserves_space_containing_local_ref() {
    let cache_dir = temp_dir("ir-local-ref-spaces-cache");
    let package_dir = temp_dir("ir local ref spaces packages");
    let package = write_r_source_package(&package_dir, "irlocal", &[]);
    let script = temp_path("ir-local-ref-spaces", "R");
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
}

#[test]
fn run_latest_resolution_cache_marker_truncates_fractional_creation_time() {
    let cache_dir = temp_dir("ir-latest-cache-fractional-time");
    let profile = temp_path("ir-fractional-systime", "R");
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
}

#[test]
fn run_latest_resolution_cache_refreshes_marker_value_in_place() {
    let cache_dir = temp_dir("ir-latest-cache-refresh");
    let expr = "{ library(cli); cat('ir.fixture=latest-cache-refresh\\n') }";

    let before_first_run = current_utc_seconds();
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
    let after_first_run = current_utc_seconds();

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
    assert!(created_at >= before_first_run);
    assert!(created_at <= after_first_run);
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
}

#[test]
fn run_passes_rust_owned_cache_dir_to_resolver() {
    let xdg_cache_home = temp_dir("ir-rust-owned-cache-xdg");
    let renviron_cache = temp_dir("ir-rust-owned-cache-renviron");
    let renviron = temp_path("ir-rust-owned-cache", "Renviron");
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
}

fn resolver_probe_count(entered: &Path) -> usize {
    fs::read_to_string(entered)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", entered.display()))
        .lines()
        .count()
}

fn only_resolution_marker_text(cache_dir: &Path) -> String {
    let resolution_dir = cache_dir.join("resolutions");
    let markers = fs::read_dir(&resolution_dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", resolution_dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 1);
    fs::read_to_string(&markers[0])
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", markers[0].display()))
}

#[test]
fn run_reticulate_fixture_imports_python_module() {
    let script = fixture("run/reticulate.R");
    let cache_dir = temp_cache("ir-reticulate-cache");
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
}
