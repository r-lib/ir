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

fn real_pak_path() -> PathBuf {
    let out = Command::new(rscript())
        .args([
            "-e",
            "cat(normalizePath(find.package('pak'), winslash = '/', mustWork = TRUE))",
        ])
        .output()
        .unwrap();
    assert_success(&out);

    PathBuf::from(stdout(&out))
}

fn real_pak_library(prefix: &str) -> TempPath {
    let pak_path = real_pak_path();
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
fn resolver_tooling_does_not_metadata_prune_user_library_packages_on_fast_path() {
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
  stop("resolver should try ambient user tooling before safe mode",
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
        secretbase_load_marker.exists(),
        "fast path should load ambient secretbase without metadata pruning"
    );
}

#[test]
fn resolver_tooling_safe_mode_ignores_user_library_packages() {
    let cache_dir = temp_dir("ir-safe-tooling-cache");
    let bin_dir = temp_dir("ir-safe-tooling-bin");
    let empty_library = temp_dir("ir-safe-tooling-empty-library");
    let user_library = temp_dir("ir-safe-tooling-user-library");
    let fake_load_marker = temp_path("ir-safe-secretbase-loaded", "txt");
    let attempts = temp_path("ir-safe-tooling-attempts", "txt");
    let first_attempt = temp_path("ir-safe-tooling-first", "txt");
    let profile = temp_path("ir-safe-tooling-profile", "R");
    let rscript_wrapper = bin_dir.join("Rscript");
    let real_rscript = PathBuf::from(rscript());
    let pak_path = real_pak_path();

    write_executable(
        &rscript_wrapper,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ] && [ ! -f {} ]; then\n\
  printf 'normal\\n' >> {}\n\
  printf 'seen\\n' > {}\n\
  kill -s SEGV $$\n\
fi\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  printf 'safe=%s\\n' \"${{IR_TOOLING_SAFE_MODE:-0}}\" >> {}\n\
fi\n\
exec '{}' \"$@\"\n",
            first_attempt.display(),
            attempts.display(),
            first_attempt.display(),
            attempts.display(),
            real_rscript.display()
        ),
    );

    fs::write(
        &profile,
        format!(
            r#"
{}
.libPaths(c(Sys.getenv("R_LIBS_USER"), Sys.getenv("IR_TEST_EMPTY_LIB")))

ir_test_write_secretbase(Sys.getenv("R_LIBS_USER"), marker = {})
ir_test_write_renv(Sys.getenv("R_LIBS_USER"))

if (nzchar(Sys.getenv("IR_RESOLVE_RESULT_FILE")) &&
    !identical(Sys.getenv("IR_TOOLING_SAFE_MODE"), "1")) {{
  stop("resolver should rerun in safe mode after crash", call. = FALSE)
}}

utils::assignInNamespace("install.packages", function(pkgs, lib, repos, ...) {{
  for (pkg in pkgs) {{
    if (!identical(pkg, "pak"))
      stop("safe mode should install non-pak tooling with private pak",
           call. = FALSE)
    unlink(file.path(lib, "pak"), recursive = TRUE, force = TRUE)
    ok <- file.symlink({}, file.path(lib, "pak"))
    if (!ok) stop("failed to link real pak", call. = FALSE)
  }}
  invisible(TRUE)
}}, ns = "utils")
"#,
            resolver_tooling_fixture_source(),
            r_string(&fake_load_marker),
            r_string(&pak_path)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", &rscript_wrapper)
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
            "cat('ir.fixture=safe-tooling\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=safe-tooling");
    let attempts = fs::read_to_string(&attempts).unwrap();
    assert_eq!(attempts, "normal\nsafe=1\n", "{attempts}");
    assert!(
        !fake_load_marker.exists(),
        "safe mode should not load secretbase from R_LIBS_USER"
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

#[cfg(unix)]
#[test]
fn resolver_crash_retries_once_in_tooling_safe_mode() {
    let cache_dir = temp_dir("ir-crash-safe-mode-cache");
    let bin_dir = temp_dir("ir-crash-safe-mode-bin");
    let library = temp_dir("ir-crash-safe-mode-library");
    let script = temp_path("ir-crash-safe-mode-script", "R");
    let rscript = bin_dir.join("Rscript");
    let attempts = temp_path("ir-crash-safe-mode-attempts", "txt");
    let first_attempt = temp_path("ir-crash-safe-mode-first", "txt");

    fs::write(
        &script,
        "#!/usr/bin/env -S ir run\n#| packages:\n#|   - cli\n\ncat(\"ir.fixture=crash-safe-mode\\n\")\n",
    )
    .unwrap();

    write_executable(
        &rscript,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  printf 'attempt safe=%s\\n' \"${{IR_TOOLING_SAFE_MODE:-0}}\" >> {}\n\
  if [ ! -f {} ]; then\n\
    printf 'seen\\n' > {}\n\
    if [ -n \"${{IR_TOOLING_SAFE_MODE:-}}\" ]; then\n\
      echo first attempt should not be safe mode >&2\n\
      exit 1\n\
    fi\n\
    kill -s SEGV $$\n\
  fi\n\
  if [ \"${{IR_TOOLING_SAFE_MODE:-}}\" != \"1\" ]; then\n\
    echo expected safe mode after resolver crash >&2\n\
    exit 1\n\
  fi\n\
  cat > /dev/null\n\
  printf '%s\\n' {} > \"$IR_RESOLVE_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
printf 'ir.fixture=crash-safe-mode\\n'\n",
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
    assert_stdout_contains(&out, "ir.fixture=crash-safe-mode");
    let attempts = fs::read_to_string(&attempts).unwrap();
    assert_eq!(attempts, "attempt safe=0\nattempt safe=1\n", "{attempts}");
}

#[cfg(unix)]
#[test]
fn resolver_normal_failure_does_not_retry_in_tooling_safe_mode() {
    let cache_dir = temp_dir("ir-no-safe-retry-cache");
    let bin_dir = temp_dir("ir-no-safe-retry-bin");
    let script = temp_path("ir-no-safe-retry-script", "R");
    let rscript = bin_dir.join("Rscript");
    let attempts = temp_path("ir-no-safe-retry-attempts", "txt");

    fs::write(
        &script,
        "#!/usr/bin/env -S ir run\n#| packages:\n#|   - cli\n\ncat(\"unreachable\\n\")\n",
    )
    .unwrap();

    write_executable(
        &rscript,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  printf 'attempt safe=%s\\n' \"${{IR_TOOLING_SAFE_MODE:-0}}\" >> {}\n\
  exit 1\n\
fi\n\
printf 'unreachable\\n'\n",
            attempts.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", &rscript)
        .args(["run", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "normal resolver failure should fail\n{}",
        output_text(&out)
    );
    let attempts = fs::read_to_string(&attempts).unwrap();
    assert_eq!(attempts, "attempt safe=0\n", "{attempts}");
}
