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
    let exclude_newer = temp_path("ir-render-cli-exclude-newer-precedence-value", "txt");
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
  writeLines(Sys.getenv("IR_EXCLUDE_NEWER"), Sys.getenv("IR_TEST_EXCLUDE_NEWER_FILE"))
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
        .env("IR_TEST_EXCLUDE_NEWER_FILE", &exclude_newer)
        .env("R_PROFILE_USER", &profile)
        .args(["render", "--exclude-newer", " 2024-03-01 "])
        .arg(&doc)
        .output()
        .unwrap();

    assert_success(&out);
    assert_eq!(
        fs::read_to_string(&exclude_newer)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", exclude_newer.display()))
            .trim_end(),
        "2024-03-01"
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
    let exclude_newer = temp_path("ir-render-future-frontmatter-exclude-newer-value", "txt");
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
  writeLines(Sys.getenv("IR_EXCLUDE_NEWER"), Sys.getenv("IR_TEST_EXCLUDE_NEWER_FILE"))
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
        .env("IR_TEST_EXCLUDE_NEWER_FILE", &exclude_newer)
        .env("R_PROFILE_USER", &profile)
        .args(["render"])
        .arg(&doc)
        .output()
        .unwrap();

    assert_success(&out);
    assert_eq!(
        fs::read_to_string(&exclude_newer)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", exclude_newer.display()))
            .trim_end(),
        ""
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

#[test]
fn run_uses_ppm_latest_for_default_repos_and_rewrites_ppm_snapshots() {
    let default_cache_dir = temp_dir("ir-linux-binary-repos-default-cache");
    let snapshot_cache_dir = temp_dir("ir-linux-binary-repos-snapshot-cache");
    let profile = temp_path("ir-linux-binary-repos-profile", "R");
    let default_repos = temp_path("ir-linux-binary-repos-default", "txt");
    let alias_repos = temp_path("ir-linux-binary-repos-alias", "txt");
    let alias_options = temp_path("ir-linux-binary-repos-alias-options", "txt");
    let alias_prefix = temp_path("ir-linux-binary-repos-alias-prefix", "txt");
    let snapshot_repos = temp_path("ir-linux-binary-repos-snapshot", "txt");
    let source_repos = temp_path("ir-linux-binary-repos-source", "txt");
    let source_options = temp_path("ir-linux-binary-repos-source-options", "txt");
    let binary_repos = temp_path("ir-linux-binary-repos-binary", "txt");
    let binary_options = temp_path("ir-linux-binary-repos-binary-options", "txt");

    fs::write(
        &profile,
        format!(
            "source({})\n{}",
            r_string(&fixture("resolver-tooling.R")),
            r#"
if (identical(Sys.getenv("IR_TEST_LINUX", unset = ""), "1"))
  Sys.info <- function() c(sysname = "Linux")
ir_test_glibc_version <- Sys.getenv("IR_TEST_GLIBC_VERSION", unset = "")
if (nzchar(ir_test_glibc_version)) {
  system2 <- function(command, args = character(), stdout = FALSE, stderr = FALSE, ...) {
    if (identical(command, "ldd") && "--version" %in% args)
      return(paste("ldd (GNU libc)", ir_test_glibc_version))
    base::system2(command, args = args, stdout = stdout, stderr = stderr, ...)
  }
}
ir_test_r_arch <- Sys.getenv("IR_TEST_R_ARCH", unset = "")
if (nzchar(ir_test_r_arch))
  R.version$arch <- ir_test_r_arch

ir_test_cache_platform <- function() {
  distro <- Sys.getenv("IR_TEST_PPM_LINUX_DISTRIBUTION", unset = "")
  if (nzchar(distro))
    paste0(R.version$platform, ";ppm-linux=", distro)
  else
    R.version$platform
}

ir_test_private_libs <- unique(file.path(
  Sys.getenv("IR_CACHE_DIR"),
  "tooling",
  paste0(getRversion(), "-", c(
    R.version$platform,
    ir_test_cache_platform(),
    paste0(R.version$platform, ";package-type=source"),
    paste0(ir_test_cache_platform(), ";package-type=binary")
  ))
))

for (ir_test_private_lib in ir_test_private_libs) {
  ir_test_write_secretbase(ir_test_private_lib)
  ir_test_write_pak(ir_test_private_lib)
  ir_test_write_renv(
    ir_test_private_lib,
    code = paste(
      "use <- function(..., library, repos, attach, sandbox, isolate, verbose) {",
      "  writeLines(paste(names(repos), unname(repos), sep = '='), Sys.getenv('IR_TEST_REPOS_FILE'))",
      "  options_file <- Sys.getenv('IR_TEST_OPTIONS_FILE', unset = '')",
      "  if (nzchar(options_file)) {",
      "    writeLines(c(",
      "      paste0('HTTPUserAgent=', getOption('HTTPUserAgent', '')),",
      "      paste0('download.file.extra=', getOption('download.file.extra', '')),",
      "      paste0('pkgType=', getOption('pkgType', '')),",
      "      paste0('pkg.platforms=', paste(getOption('pkg.platforms', ''), collapse = ',')),",
      "      paste0('PKG_PLATFORMS=', Sys.getenv('PKG_PLATFORMS', unset = ''))",
      "    ), options_file)",
      "  }",
      "  prefix_file <- Sys.getenv('IR_TEST_PREFIX_FILE', unset = '')",
      "  if (nzchar(prefix_file))",
      "    writeLines(Sys.getenv('RENV_PATHS_PREFIX', unset = ''), prefix_file)",
      "  specs <- unlist(list(...), use.names = FALSE)",
      "  for (spec in specs) {",
      "    pkg <- sub('@.*$', '', spec)",
      "    dir.create(file.path(library, pkg), recursive = TRUE, showWarnings = FALSE)",
      "  }",
      "  invisible(TRUE)",
      "}",
      sep = "\n"
    )
  )
}

ir_test_profile_repos <- Sys.getenv(
  "IR_TEST_PROFILE_REPOS",
  unset = "https://packagemanager.posit.co/cran/latest"
)
ir_test_ppm_alias <- Sys.getenv("IR_TEST_PPM_ALIAS", unset = "")
ir_test_repos <- if (nzchar(ir_test_ppm_alias)) {
  c(stats::setNames(ir_test_profile_repos, ir_test_ppm_alias),
    CRAN = "https://cran.r-project.org")
} else {
  c(CRAN = ir_test_profile_repos)
}
if (identical(Sys.getenv("IR_TEST_INCLUDE_INTERNAL_REPO", unset = ""), "1"))
  ir_test_repos <- c(ir_test_repos, Internal = "https://internal.example.test/repo")
options(repos = ir_test_repos)
if (identical(Sys.getenv("IR_TEST_SOURCE_STARTUP", unset = ""), "1")) {
  options(pkgType = "source", pkg.platforms = "source")
  Sys.setenv(PKG_PLATFORMS = "source")
}
ir_test_download_method <- Sys.getenv("IR_TEST_DOWNLOAD_METHOD", unset = "")
if (nzchar(ir_test_download_method))
  options(download.file.method = ir_test_download_method,
          download.file.extra = "--compressed")
"#,
        ),
    )
    .unwrap();

    let read_repos = |path: &Path| {
        fs::read_to_string(path)
            .unwrap()
            .replace("\r\n", "\n")
            .trim()
            .to_string()
    };
    let configure_manylinux = |cmd: &mut Command| {
        cmd.env("IR_TEST_LINUX", "1")
            .env("IR_TEST_GLIBC_VERSION", "2.36")
            .env("IR_TEST_R_ARCH", "x86_64")
            .env("IR_TEST_PPM_LINUX_DISTRIBUTION", "manylinux_2_28");
    };

    let mut default_cmd = ir();
    configure_manylinux(&mut default_cmd);
    let default = default_cmd
        .env("IR_CACHE_DIR", &default_cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("R_PROFILE_USER", &profile)
        .env("IR_TEST_PROFILE_REPOS", "@CRAN@")
        .env("IR_TEST_REPOS_FILE", &default_repos)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            "cat('ir.fixture=ppm-default\\n')",
        ])
        .output()
        .unwrap();
    assert_success(&default);
    assert_stdout_contains(&default, "ir.fixture=ppm-default");
    assert_eq!(
        read_repos(&default_repos),
        "CRAN=https://packagemanager.posit.co/cran/__linux__/manylinux_2_28/latest"
    );

    let mut snapshot_cmd = ir();
    configure_manylinux(&mut snapshot_cmd);
    let snapshot = snapshot_cmd
        .env("IR_CACHE_DIR", &snapshot_cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("R_PROFILE_USER", &profile)
        .env("IR_TEST_REPOS_FILE", &snapshot_repos)
        .args([
            "run",
            "--exclude-newer",
            "2024-03-15",
            "--isolated",
            "--with",
            "glue",
            "--vanilla",
            "-e",
            "cat('ir.fixture=linux-binary-snapshot\\n')",
        ])
        .output()
        .unwrap();
    assert_success(&snapshot);
    assert_stdout_contains(&snapshot, "ir.fixture=linux-binary-snapshot");
    assert_eq!(
        read_repos(&snapshot_repos),
        "CRAN=https://packagemanager.posit.co/cran/__linux__/manylinux_2_28/2024-03-15"
    );

    let alias_cache_dir = temp_dir("ir-linux-binary-repos-alias-cache");
    let mut alias_cmd = ir();
    configure_manylinux(&mut alias_cmd);
    let alias = alias_cmd
        .env("IR_CACHE_DIR", &alias_cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("R_PROFILE_USER", &profile)
        .env("IR_TEST_REPOS_FILE", &alias_repos)
        .env("IR_TEST_OPTIONS_FILE", &alias_options)
        .env("IR_TEST_PREFIX_FILE", &alias_prefix)
        .env("IR_TEST_DOWNLOAD_METHOD", "curl")
        .env("IR_TEST_PPM_ALIAS", "RSPM")
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            "cat('ir.fixture=alias-binary-latest\\n')",
        ])
        .output()
        .unwrap();
    assert_success(&alias);
    assert_stdout_contains(&alias, "ir.fixture=alias-binary-latest");
    assert_eq!(
        read_repos(&alias_repos),
        "RSPM=https://packagemanager.posit.co/cran/__linux__/manylinux_2_28/latest\nCRAN=https://cran.r-project.org"
    );
    let options = read_repos(&alias_options);
    assert!(options.contains("HTTPUserAgent=R/"));
    assert!(options.contains("download.file.extra=--compressed"));
    assert!(options.contains("--user-agent"));
    assert_eq!(read_repos(&alias_prefix), "manylinux_2_28");

    let source_cache_dir = temp_dir("ir-linux-binary-repos-source-cache");
    let source = ir()
        .env("IR_CACHE_DIR", &source_cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("R_PROFILE_USER", &profile)
        .env("IR_PACKAGE_TYPE", "source")
        .env("IR_TEST_REPOS_FILE", &source_repos)
        .env("IR_TEST_OPTIONS_FILE", &source_options)
        .env("IR_TEST_PPM_ALIAS", "RSPM")
        .env("IR_TEST_INCLUDE_INTERNAL_REPO", "1")
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            "cat('ir.fixture=source-package-type\\n')",
        ])
        .output()
        .unwrap();
    assert_success(&source);
    assert_stdout_contains(&source, "ir.fixture=source-package-type");
    assert_eq!(
        read_repos(&source_repos),
        "RSPM=https://packagemanager.posit.co/cran/latest\nCRAN=https://cran.r-project.org\nInternal=https://internal.example.test/repo"
    );
    let source_options = read_repos(&source_options);
    assert!(source_options.contains("pkgType=source"));
    assert!(source_options.contains("pkg.platforms=source"));
    assert!(source_options.contains("PKG_PLATFORMS=source"));

    let binary_cache_dir = temp_dir("ir-linux-binary-repos-binary-cache");
    let mut binary_cmd = ir();
    configure_manylinux(&mut binary_cmd);
    let binary = binary_cmd
        .env("IR_CACHE_DIR", &binary_cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("R_PROFILE_USER", &profile)
        .env("IR_PACKAGE_TYPE", "binary")
        .env("IR_TEST_REPOS_FILE", &binary_repos)
        .env("IR_TEST_OPTIONS_FILE", &binary_options)
        .env("IR_TEST_SOURCE_STARTUP", "1")
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            "cat('ir.fixture=binary-package-type\\n')",
        ])
        .output()
        .unwrap();
    assert_success(&binary);
    assert_stdout_contains(&binary, "ir.fixture=binary-package-type");
    assert_eq!(
        read_repos(&binary_repos),
        "CRAN=https://packagemanager.posit.co/cran/__linux__/manylinux_2_28/latest"
    );
    let binary_options = read_repos(&binary_options);
    assert!(binary_options.contains("pkgType=both"));
    assert!(binary_options.contains("pkg.platforms="));
    assert!(binary_options.contains("PKG_PLATFORMS="));
    assert!(!binary_options.contains("pkg.platforms=source"));
    assert!(!binary_options.contains("PKG_PLATFORMS=source"));
}

#[test]
fn run_source_parameter_uses_source_install_policy_and_distinct_library() {
    let cache_dir = temp_dir("ir-source-parameter-cache");
    let profile = temp_path("ir-source-parameter-profile", "R");
    let installs = temp_path("ir-source-parameter-installs", "txt");

    fs::write(
        &profile,
        r#"
ir_test_write_pkg <- function(lib, pkg, namespace, code) {
  path <- file.path(lib, pkg)
  dir.create(file.path(path, "R"), recursive = TRUE, showWarnings = FALSE)
  writeLines(c(
    paste("Package:", pkg),
    "Version: 0.0.1",
    paste("Title:", pkg),
    paste("Description:", pkg),
    "License: MIT"
  ), file.path(path, "DESCRIPTION"))
  writeLines(namespace, file.path(path, "NAMESPACE"))
  writeLines(code, file.path(path, "R", pkg))
}

ir_test_private_lib <- file.path(
  Sys.getenv("IR_CACHE_DIR"),
  "tooling",
  paste0(getRversion(), "-", R.version$platform)
)

ir_test_write_pkg(
  ir_test_private_lib,
  "secretbase",
  "export(sha256)",
  "sha256 <- function(x) paste(c('hash', nchar(paste(x, collapse = '\n'))), collapse = '-')"
)
ir_test_write_pkg(
  ir_test_private_lib,
  "pak",
  "export(pkg_deps)",
  paste(
    "pkg_deps <- function(refs, dependencies = NA, upgrade = TRUE) {",
    "  refs <- as.character(refs)",
    "  packages <- sub('[?].*$', '', refs)",
    "  packages <- sub('@.*$', '', packages)",
    "  data.frame(",
    "    status = rep('OK', length(refs)),",
    "    ref = refs,",
    "    package = packages,",
    "    version = rep('0.0.1', length(refs)),",
    "    type = rep('standard', length(refs)),",
    "    priority = NA_character_,",
    "    direct = TRUE,",
    "    stringsAsFactors = FALSE",
    "  )",
    "}",
    sep = "\n"
  )
)
ir_test_write_pkg(
  ir_test_private_lib,
  "renv",
  "export(use)",
  paste(
    "use <- function(..., library, repos, attach, sandbox, isolate, verbose) {",
    "  specs <- unlist(list(...), use.names = FALSE)",
    "  cat(",
    "    paste0('library=', library),",
    "    paste0('pkgType=', getOption('pkgType', '')),",
    "    paste0('pkg.platforms=', paste(getOption('pkg.platforms', ''), collapse = ',')),",
    "    paste0('PKG_PLATFORMS=', Sys.getenv('PKG_PLATFORMS', unset = '')),",
    "    paste0('specs=', paste(specs, collapse = ',')),",
    "    sep = '\n',",
    "    file = Sys.getenv('IR_TEST_INSTALLS_FILE'),",
    "    append = TRUE",
    "  )",
    "  cat('\n---\n', file = Sys.getenv('IR_TEST_INSTALLS_FILE'), append = TRUE)",
    "  for (spec in specs) {",
    "    pkg <- sub('@.*$', '', spec)",
    "    dir.create(file.path(library, pkg), recursive = TRUE, showWarnings = FALSE)",
    "  }",
    "  invisible(TRUE)",
    "}",
    sep = "\n"
  )
)
options(repos = c(CRAN = "https://packagemanager.posit.co/cran/latest"))
"#,
    )
    .unwrap();

    let plain = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("R_PROFILE_USER", &profile)
        .env("IR_TEST_INSTALLS_FILE", &installs)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            "cat(.libPaths()[1], '\\n')",
        ])
        .output()
        .unwrap();
    assert_success(&plain);

    let source = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("R_PROFILE_USER", &profile)
        .env("IR_TEST_INSTALLS_FILE", &installs)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli?source",
            "--vanilla",
            "-e",
            "cat(.libPaths()[1], '\\n')",
        ])
        .output()
        .unwrap();
    assert_success(&source);

    let plain_stdout = stdout(&plain);
    let source_stdout = stdout(&source);
    let plain_library = plain_stdout.trim();
    let source_library = source_stdout.trim();
    assert_ne!(plain_library, source_library);

    let installs = fs::read_to_string(&installs).unwrap().replace("\r\n", "\n");
    assert!(installs.contains("pkgType=source\n"));
    assert!(installs.contains("pkg.platforms=source\n"));
    assert!(installs.contains("PKG_PLATFORMS=source\n"));
    assert!(installs.contains("specs=cli@0.0.1\n"));
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
