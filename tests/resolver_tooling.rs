#![cfg(unix)]

//! Resolver tooling integration tests for the public `ir` CLI.

mod support;

use support::*;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Command;

fn resolver_tooling_fixture_source() -> String {
    format!("source({})", r_string(&fixture("resolver-tooling.R")))
}

fn real_pak_library(prefix: &str) -> TempPath {
    let out = Command::new(rscript())
        .args([
            "-e",
            "cat(normalizePath(find.package('pak'), winslash = '/', mustWork = TRUE))",
        ])
        .output()
        .unwrap();
    assert_success(&out);

    let pak_path = PathBuf::from(stdout(&out));
    let pak_library = temp_dir(prefix);
    symlink(pak_path, pak_library.join("pak")).unwrap();
    pak_library
}

#[test]
fn resolver_tooling_uses_compatible_user_library_packages() {
    let cache_dir = temp_dir("ir-compatible-tooling-cache");
    let pak_library = real_pak_library("ir-compatible-tooling-pak-library");
    let user_library = temp_dir("ir-compatible-tooling-user-library");
    let fake_load_marker = temp_path("ir-compatible-secretbase-loaded", "txt");
    let profile = temp_path("ir-compatible-tooling-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
{}
.libPaths(c(Sys.getenv("IR_TEST_PAK_LIB"), Sys.getenv("R_LIBS_USER")))

ir_test_write_secretbase(Sys.getenv("R_LIBS_USER"), marker = {})
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
        .env("IR_TEST_PAK_LIB", &pak_library)
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
fn resolver_tooling_installs_missing_packages_with_real_pak() {
    let cache_dir = temp_dir("ir-real-pak-tooling-cache");
    let pak_library = real_pak_library("ir-real-pak-tooling-pak-library");
    let empty_library = temp_dir("ir-real-pak-tooling-empty-library");
    let profile = temp_path("ir-real-pak-tooling-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
{}
.libPaths(c(Sys.getenv("IR_TEST_PAK_LIB"), Sys.getenv("IR_TEST_EMPTY_LIB")))

utils::assignInNamespace("install.packages", function(...) {{
  stop("resolver should use real pak that is already available",
       call. = FALSE)
}}, ns = "utils")
"#,
            resolver_tooling_fixture_source()
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_PAK_LIB", &pak_library)
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
            "cat('ir.fixture=real-pak-tooling\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=real-pak-tooling");
}

#[test]
fn resolver_tooling_ignores_wrong_r_minor_user_library_package() {
    let cache_dir = temp_dir("ir-stale-tooling-cache");
    let pak_library = real_pak_library("ir-stale-tooling-pak-library");
    let user_library = temp_dir("ir-stale-tooling-user-library");
    let empty_library = temp_dir("ir-stale-tooling-empty-library");
    let secretbase_load_marker = temp_path("ir-stale-secretbase-loaded", "txt");
    let profile = temp_path("ir-stale-tooling-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
{}
.libPaths(c(Sys.getenv("R_LIBS_USER"),
            Sys.getenv("IR_TEST_PAK_LIB"),
            Sys.getenv("IR_TEST_EMPTY_LIB")))

ir_test_wrong_r <- ir_test_wrong_minor_version()
ir_test_write_secretbase(
  Sys.getenv("R_LIBS_USER"),
  marker = {},
  hash = "ambienthash",
  built = ir_test_wrong_r
)
ir_test_write_renv(
  Sys.getenv("R_LIBS_USER"),
  built = ir_test_wrong_r
)

utils::assignInNamespace("install.packages", function(...) {{
  stop("resolver should use real pak after pruning stale user tooling",
       call. = FALSE)
}}, ns = "utils")
"#,
            resolver_tooling_fixture_source(),
            r_string(&secretbase_load_marker)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_PAK_LIB", &pak_library)
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
    assert!(
        !secretbase_load_marker.exists(),
        "resolver should not load stale secretbase from R_LIBS_USER"
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
