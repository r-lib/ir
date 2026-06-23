#![cfg(unix)]

//! Resolver tooling integration tests for the public `ir` CLI.

mod support;

use support::*;

use std::fs;
use std::path::Path;

fn resolver_tooling_fixture_source() -> String {
    format!("source({})", r_string(&fixture("resolver-tooling.R")))
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
fn resolver_tooling_ignores_wrong_r_minor_user_library_package() {
    let cache_dir = temp_dir("ir-stale-tooling-cache");
    let user_library = temp_dir("ir-stale-tooling-user-library");
    let empty_library = temp_dir("ir-stale-tooling-empty-library");
    let install_marker = temp_path("ir-stale-tooling-install", "txt");
    let pak_marker = temp_path("ir-stale-tooling-pak", "txt");
    let secretbase_load_marker = temp_path("ir-stale-secretbase-loaded", "txt");
    let pillar_load_marker = temp_path("ir-stale-pillar-loaded", "txt");
    let profile = temp_path("ir-stale-tooling-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
{}
.libPaths(c(Sys.getenv("R_LIBS_USER"), Sys.getenv("IR_TEST_EMPTY_LIB")))

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

utils::assignInNamespace("install.packages", function(pkgs, lib, repos, ...) {{
  writeLines(as.character(pkgs), {})
  if (!identical(as.character(pkgs), "pak"))
    stop("resolver should bootstrap only pak with install.packages",
         call. = FALSE)
  ir_test_write_pak(
    lib,
    namespace = "export(pkg_deps)\nexport(pkg_install)",
    code = ir_test_fake_pak_code(
      install_marker = {},
      require_pillar = TRUE
    )
  )
}}, ns = "utils")
"#,
            resolver_tooling_fixture_source(),
            r_string(&secretbase_load_marker),
            r_string(&pillar_load_marker),
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
            "cat('ir.fixture=stale-tooling\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=stale-tooling");
    assert_pak_installed_resolver_tooling(&install_marker, &pak_marker);
    assert!(
        !secretbase_load_marker.exists(),
        "resolver should not load stale secretbase from R_LIBS_USER"
    );
    assert!(
        !pillar_load_marker.exists(),
        "resolver should prune stale R_LIBS_USER before pak loads auxiliary packages"
    );
}

#[cfg(unix)]
#[test]
fn resolver_tooling_restart_retries_after_stdin_broken_pipe() {
    let cache_dir = temp_dir("ir-restart-broken-pipe-cache");
    let bin_dir = temp_dir("ir-restart-broken-pipe-bin");
    let library = temp_dir("ir-restart-broken-pipe-library");
    let script = temp_path("ir-restart-broken-pipe-script", "R");
    let rscript = bin_dir.join("Rscript");
    let attempts = temp_path("ir-restart-broken-pipe-attempts", "txt");
    let first_attempt = temp_path("ir-restart-broken-pipe-first", "txt");

    let mut source = String::from("#!/usr/bin/env -S ir run\n#| packages:\n");
    for index in 0..20_000 {
        source.push_str(&format!("#|   - restartpipepkg{index}\n"));
    }
    source.push_str("\ncat(\"ir.fixture=restart-broken-pipe\\n\")\n");
    fs::write(&script, source).unwrap();

    write_executable(
        &rscript,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  printf 'attempt\\n' >> {}\n\
  if [ ! -f {} ]; then\n\
    printf 'seen\\n' > {}\n\
    if [ -z \"${{IR_TOOLING_RESTART_FILE:-}}\" ]; then\n\
      echo missing tooling restart file >&2\n\
      exit 1\n\
    fi\n\
    printf 'pak\\n' > \"$IR_TOOLING_RESTART_FILE\"\n\
    exit 86\n\
  fi\n\
  cat > /dev/null\n\
  printf '%s\\n' {} > \"$IR_RESOLVE_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
printf 'ir.fixture=restart-broken-pipe\\n'\n",
            attempts.display(),
            first_attempt.display(),
            first_attempt.display(),
            library.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", &rscript)
        .args(["run", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=restart-broken-pipe");
    let attempts = fs::read_to_string(&attempts).unwrap();
    assert_eq!(attempts.lines().count(), 2, "{attempts}");
}
