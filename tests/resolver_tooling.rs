#![cfg(unix)]

//! Resolver tooling integration tests for the public `ir` CLI.

mod support;

use support::*;

use std::fs;
use std::path::Path;
use std::process::Output;

fn resolver_tooling_fixture_source() -> String {
    format!("source({})", r_string(&fixture("resolver-tooling.R")))
}

fn run_tooling_probe(
    cache_dir: &Path,
    profile: &Path,
    r_libs_user: &Path,
    package: &str,
    label: &str,
) -> Output {
    ir().env("IR_CACHE_DIR", cache_dir)
        .env("R_LIBS_USER", r_libs_user)
        .env("R_PROFILE_USER", profile)
        .args(["run", "--isolated", "--with", package, "--vanilla", "-e"])
        .arg(format!("cat('ir.fixture={label}\\n')"))
        .output()
        .unwrap()
}

fn assert_pak_installed_resolver_tooling(install_marker: &Path, pak_marker: &Path) {
    let install_packages = fs::read_to_string(install_marker).unwrap();
    assert_eq!(install_packages.trim(), "pak");

    let pak_refs = fs::read_to_string(pak_marker).unwrap();
    assert!(pak_refs.lines().any(|line| line == "renv"), "{pak_refs}");
    assert!(
        pak_refs.lines().any(|line| line == "secretbase"),
        "{pak_refs}"
    );
    assert!(!pak_refs.lines().any(|line| line == "pak"), "{pak_refs}");
}

#[test]
fn resolver_configures_and_closes_pak_subprocess() {
    let cache_dir = temp_dir("ir-pak-subprocess-cache");
    let tmp_dir = temp_dir("ir-pak-subprocess-tmp");
    let user_library = temp_dir("ir-pak-subprocess-user-library");
    let temp_marker = temp_path("ir-pak-subprocess-temp-marker", "txt");
    let close_marker = temp_path("ir-pak-subprocess-close-marker", "txt");
    let profile = temp_path("ir-pak-subprocess-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
{}
ir_test_write_secretbase(Sys.getenv("R_LIBS_USER"))
ir_test_write_renv(Sys.getenv("R_LIBS_USER"))
ir_test_write_pak(
  Sys.getenv("R_LIBS_USER"),
  namespace = "export(pkg_deps)",
  code = paste(
    "pkg_data <- new.env(parent = emptyenv())",
    paste0(
      "pkg_data$remote <- list(close = function(grace = 1000) {{",
      "  writeLines(paste0('closed=', grace), ", deparse({}), ")",
      "}})"
    ),
    "pkg_deps <- function(refs, dependencies = NA, upgrade = TRUE) {{",
    "  writeLines(c(",
    "    paste0('tempdir=', normalizePath(tempdir(), winslash = '/', mustWork = TRUE)),",
    "    paste0('TMPDIR=', Sys.getenv('TMPDIR', unset = '')),",
    "    paste0('TMP=', Sys.getenv('TMP', unset = '')),",
    "    paste0('TEMP=', Sys.getenv('TEMP', unset = ''))",
    paste0("  ), ", deparse({}), ")"),
    ir_test_pak_deps_code(),
    "  pkg_deps(refs, dependencies = dependencies, upgrade = upgrade)",
    "}}",
    sep = "\n"
  )
)
"#,
            resolver_tooling_fixture_source(),
            r_string(&close_marker),
            r_string(&temp_marker)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("TMPDIR", &tmp_dir)
        .env("TMP", &tmp_dir)
        .env("TEMP", &tmp_dir)
        .env("R_LIBS_USER", &user_library)
        .env("R_PROFILE_USER", &profile)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            "cat('ir.fixture=pak-subprocess\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=pak-subprocess");

    let temp_settings = fs::read_to_string(temp_marker).unwrap();
    let lines: Vec<&str> = temp_settings.lines().collect();
    let resolver_temp = lines
        .iter()
        .find_map(|line| line.strip_prefix("tempdir="))
        .unwrap();
    assert!(resolver_temp.contains("Rtmp"), "{temp_settings}");
    for name in ["TMPDIR", "TMP", "TEMP"] {
        assert!(
            lines
                .iter()
                .any(|line| line == &format!("{name}={resolver_temp}")),
            "{temp_settings}"
        );
    }

    let close = fs::read_to_string(close_marker).unwrap();
    assert_eq!(close.trim(), "closed=5000");
}

#[test]
fn resolver_tooling_uses_compatible_user_library_packages() {
    let cache_dir = temp_dir("ir-compatible-tooling-cache");
    let user_library = temp_dir("ir-compatible-tooling-user-library");
    let fake_load_marker = temp_path("ir-compatible-secretbase-loaded", "txt");
    let profile = temp_path("ir-compatible-tooling-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
{}
ir_test_write_secretbase(Sys.getenv("R_LIBS_USER"), marker = {})
ir_test_write_pak(Sys.getenv("R_LIBS_USER"))
ir_test_write_renv(Sys.getenv("R_LIBS_USER"))

utils::assignInNamespace("install.packages", function(...) {{
  stop("resolver should use compatible R_LIBS_USER tooling", call. = FALSE)
}}, ns = "utils")
"#,
            resolver_tooling_fixture_source(),
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
}

#[test]
fn resolver_tooling_bootstraps_only_pak_with_install_packages() {
    let cache_dir = temp_dir("ir-pak-tooling-cache");
    let empty_library = temp_dir("ir-pak-tooling-empty-library");
    let install_marker = temp_path("ir-pak-tooling-install", "txt");
    let pak_marker = temp_path("ir-pak-tooling-pak", "txt");
    let profile = temp_path("ir-pak-tooling-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
{}
.libPaths(Sys.getenv("IR_TEST_EMPTY_LIB"))

utils::assignInNamespace("install.packages", function(pkgs, lib, repos, ...) {{
  writeLines(as.character(pkgs), {})
  if (!identical(as.character(pkgs), "pak"))
    stop("resolver should bootstrap only pak with install.packages",
         call. = FALSE)
  ir_test_write_pak(
    lib,
    namespace = "export(pkg_deps)\nexport(pkg_install)",
    code = ir_test_fake_pak_code(install_marker = {})
  )
}}, ns = "utils")
"#,
            resolver_tooling_fixture_source(),
            r_string(&install_marker),
            r_string(&pak_marker)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_EMPTY_LIB", &empty_library)
        .env("R_LIBS_SITE", &empty_library)
        .env("R_LIBS_USER", &empty_library)
        .env("R_PROFILE_USER", &profile)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            "cat('ir.fixture=pak-tooling\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=pak-tooling");
    assert_pak_installed_resolver_tooling(&install_marker, &pak_marker);
}

#[test]
fn resolver_tooling_bootstraps_pak_after_pruning_bad_user_library() {
    let cache_dir = temp_dir("ir-pruned-pak-tooling-cache");
    let user_library = temp_dir("ir-pruned-pak-tooling-user-library");
    let empty_library = temp_dir("ir-pruned-pak-tooling-empty-library");
    let install_marker = temp_path("ir-pruned-pak-tooling-install", "txt");
    let pak_marker = temp_path("ir-pruned-pak-tooling-pak", "txt");
    let profile = temp_path("ir-pruned-pak-tooling-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
{}
.libPaths(c(Sys.getenv("R_LIBS_USER"), Sys.getenv("IR_TEST_EMPTY_LIB")))

ir_test_wrong_r <- ir_test_wrong_minor_version()
ir_test_write_pak(
  Sys.getenv("R_LIBS_USER"),
  code = "pkg_deps <- function(...) stop('pruned user pak should not load', call. = FALSE)"
)
ir_test_write_renv(
  Sys.getenv("R_LIBS_USER"),
  code = "use <- function(...) stop('pruned user renv should not load', call. = FALSE)",
  built = ir_test_wrong_r
)

utils::assignInNamespace("install.packages", function(pkgs, lib, repos, ...) {{
  writeLines(as.character(pkgs), {})
  if (!identical(as.character(pkgs), "pak"))
    stop("resolver should bootstrap only pak with install.packages",
         call. = FALSE)
  ir_test_write_pak(
    lib,
    namespace = "export(pkg_deps)\nexport(pkg_install)",
    code = ir_test_fake_pak_code(install_marker = {})
  )
}}, ns = "utils")
"#,
            resolver_tooling_fixture_source(),
            r_string(&install_marker),
            r_string(&pak_marker)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_EMPTY_LIB", &empty_library)
        .env("R_LIBS_SITE", &empty_library)
        .env("R_LIBS_USER", &user_library)
        .env("R_PROFILE_USER", &profile)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            "cat('ir.fixture=pruned-pak-tooling\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=pruned-pak-tooling");
    assert_pak_installed_resolver_tooling(&install_marker, &pak_marker);
}

#[test]
fn resolver_tooling_ignores_wrong_r_minor_user_library_package() {
    let cache_dir = temp_dir("ir-ambient-tooling-cache");
    let ambient_library = temp_dir("ir-ambient-tooling-user-library");
    let fake_secretbase_load_marker = temp_path("ir-ambient-secretbase-loaded", "txt");
    let fake_pillar_load_marker = temp_path("ir-ambient-pillar-loaded", "txt");
    let profile = temp_path("ir-tooling-install-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
{}
ir_test_private_lib <- file.path(
  Sys.getenv("IR_CACHE_DIR"),
  "tooling",
  paste0(getRversion(), "-", R.version$platform)
)
ir_test_wrong_r <- ir_test_wrong_minor_version()

ir_test_write_secretbase(
  Sys.getenv("R_LIBS_USER"),
  marker = {},
  hash = "ambienthash",
  built = ir_test_wrong_r
)
ir_test_write_pillar(
  Sys.getenv("R_LIBS_USER"),
  marker = {},
  built = ir_test_wrong_r
)
ir_test_write_pak(
  ir_test_private_lib,
  namespace = "export(pkg_deps)\nexport(pkg_install)",
  code = ir_test_fake_pak_code(
    allowed_installs = "secretbase",
    require_pillar = TRUE
  )
)
ir_test_write_renv(ir_test_private_lib)

utils::assignInNamespace("install.packages", function(pkgs, lib, repos, ...) {{
  stop("install.packages should not install resolver tooling when pak exists",
       call. = FALSE)
}}, ns = "utils")
"#,
            resolver_tooling_fixture_source(),
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

    let second = run_tooling_probe(
        &cache_dir,
        &profile,
        &ambient_library,
        "glue",
        "ambient-tooling-warm",
    );

    assert_success(&second);
    assert_stdout_contains(&second, "ir.fixture=ambient-tooling-warm");
    assert!(
        !fake_pillar_load_marker.exists(),
        "resolver should prune wrong-R-minor R_LIBS_USER even when private tooling is warm"
    );
}
