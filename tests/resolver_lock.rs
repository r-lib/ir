//! Resolver lock integration tests for the public `ir` CLI.

mod support;

use support::*;

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn write_resolver_lock_profile(profile: &Path) {
    fs::write(
        profile,
        r#"
if (nzchar(Sys.getenv("IR_RESOLVE_RESULT_FILE"))) {
  local({
    active <- Sys.getenv("IR_TEST_ACTIVE")
    if (!dir.create(active, recursive = TRUE, showWarnings = FALSE)) {
      writeLines("overlap", Sys.getenv("IR_TEST_OVERLAP"))
      stop("resolve.R overlapped", call. = FALSE)
    }
    on.exit(unlink(active, recursive = TRUE, force = TRUE), add = TRUE)
    cat(Sys.getpid(), "\n", file = Sys.getenv("IR_TEST_ENTERED"), append = TRUE)
    Sys.sleep(as.numeric(Sys.getenv("IR_TEST_SLEEP", "0")))
  })
}
"#,
    )
    .unwrap();
}

#[cfg(unix)]
fn write_python_resolver_lock_profile(profile: &Path) {
    fs::write(
        profile,
        r#"
if (nzchar(Sys.getenv("IR_RESOLVE_RESULT_FILE"))) {
  readLines("stdin", warn = FALSE)
  library <- file.path(Sys.getenv("IR_CACHE_DIR"), "fake-library")
  dir.create(library, recursive = TRUE, showWarnings = FALSE)
  cat(library, "\n", file = Sys.getenv("IR_RESOLVE_RESULT_FILE"))
  q("no", status = 0, runLast = FALSE)
}

if (nzchar(Sys.getenv("IR_PYTHON_RESULT_FILE"))) {
  readLines("stdin", warn = FALSE)
  local({
    active <- Sys.getenv("IR_TEST_ACTIVE")
    if (!dir.create(active, recursive = TRUE, showWarnings = FALSE)) {
      writeLines("overlap", Sys.getenv("IR_TEST_OVERLAP"))
      stop("resolve_python.R overlapped", call. = FALSE)
    }
    on.exit(unlink(active, recursive = TRUE, force = TRUE), add = TRUE)
    cat(Sys.getpid(), "\n", file = Sys.getenv("IR_TEST_ENTERED"), append = TRUE)
    Sys.sleep(as.numeric(Sys.getenv("IR_TEST_SLEEP", "0")))
    cat(Sys.getenv("IR_TEST_PYTHON"), "\n",
        file = Sys.getenv("IR_PYTHON_RESULT_FILE"))
  })
  q("no", status = 0, runLast = FALSE)
}
"#,
    )
    .unwrap();
}

struct ResolverLockProbe<'a> {
    user_cache_dir: &'a Path,
    profile: &'a Path,
    active: &'a Path,
    overlap: &'a Path,
    entered: &'a Path,
}

fn resolver_lock_command(
    cache_dir: &Path,
    probe: &ResolverLockProbe<'_>,
    package: Option<&str>,
    label: &str,
) -> Command {
    let mut cmd = ir();
    cmd.env("IR_CACHE_DIR", cache_dir)
        .env("R_USER_CACHE_DIR", probe.user_cache_dir)
        .env("R_PROFILE_USER", probe.profile)
        .env("IR_TEST_ACTIVE", probe.active)
        .env("IR_TEST_OVERLAP", probe.overlap)
        .env("IR_TEST_ENTERED", probe.entered)
        .env("IR_TEST_SLEEP", "1")
        .args(["run", "--isolated"]);
    if let Some(package) = package {
        cmd.args(["--with", package]);
    }
    cmd.args(["--vanilla", "-e"])
        .arg(format!("cat('ir.fixture={label}\\n')"));
    cmd
}

fn spawn_resolver_for_lock_test(
    cache_dir: &Path,
    probe: &ResolverLockProbe<'_>,
    package: Option<&str>,
    label: &str,
) -> std::process::Child {
    let mut cmd = resolver_lock_command(cache_dir, probe, package, label);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.spawn().unwrap()
}

fn wait_for_resolver_probe(mut child: std::process::Child, active: &Path) -> std::process::Child {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !active.exists() && Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            let output = child.wait_with_output().unwrap();
            panic!(
                "resolver exited before the test profile probe\n{}",
                output_text(&output)
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        active.exists(),
        "resolver should enter the test profile probe before the second run starts"
    );
    child
}

fn resolver_probe_count(entered: &Path) -> usize {
    fs::read_to_string(entered)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", entered.display()))
        .lines()
        .count()
}

#[cfg(unix)]
#[test]
fn concurrent_python_render_serializes_python_resolver_tooling() {
    let cache_dir = temp_dir("ir-python-resolution-lock-cache");
    let user_cache_dir = temp_dir("ir-python-resolution-lock-user-cache");
    let profile = temp_path("ir-python-resolution-lock-profile", "R");
    let active = temp_path("ir-python-resolution-lock-active", "");
    let entered = temp_path("ir-python-resolution-lock-entered", "txt");
    let overlap = temp_path("ir-python-resolution-lock-overlap", "txt");
    let bin_dir = temp_dir("ir-python-resolution-lock-bin");
    let python = bin_dir.join("python");
    let quarto = bin_dir.join("quarto");
    let doc = temp_path("ir-python-resolution-lock", "qmd");

    write_python_resolver_lock_profile(&profile);
    write_executable(&python, "#!/bin/sh\nexit 0\n");
    write_executable(&quarto, "#!/bin/sh\nexit 0\n");
    fs::write(
        &doc,
        r#"---
title: python lock
format: html
jupyter: python3
ir:
  python-packages:
    - pandas
---
"#,
    )
    .unwrap();

    let mut first = ir();
    first
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_USER_CACHE_DIR", &user_cache_dir)
        .env("R_PROFILE_USER", &profile)
        .env("IR_QUARTO", &quarto)
        .env("IR_TEST_ACTIVE", &active)
        .env("IR_TEST_OVERLAP", &overlap)
        .env("IR_TEST_ENTERED", &entered)
        .env("IR_TEST_SLEEP", "1")
        .env("IR_TEST_PYTHON", &python)
        .args(["render"])
        .arg(&doc)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let first = first.spawn().unwrap();
    let first = wait_for_resolver_probe(first, &active);

    let second = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_USER_CACHE_DIR", &user_cache_dir)
        .env("R_PROFILE_USER", &profile)
        .env("IR_QUARTO", &quarto)
        .env("IR_TEST_ACTIVE", &active)
        .env("IR_TEST_OVERLAP", &overlap)
        .env("IR_TEST_ENTERED", &entered)
        .env("IR_TEST_SLEEP", "1")
        .env("IR_TEST_PYTHON", &python)
        .args(["render"])
        .arg(&doc)
        .output()
        .unwrap();
    let first = first.wait_with_output().unwrap();

    assert_success(&first);
    assert_success(&second);
    assert!(!overlap.exists(), "resolve_python.R should not overlap");
    assert_eq!(
        resolver_probe_count(&entered),
        2,
        "both uv renders should resolve Python, but not concurrently"
    );
}

#[test]
fn concurrent_resolvers_serialize_same_dependency_resolution() {
    let cache_dir = temp_dir("ir-same-resolution-lock-cache");
    let user_cache_dir = temp_dir("ir-same-resolution-lock-user-cache");
    let profile = temp_path("ir-same-resolution-lock-profile", "R");
    let active = temp_path("ir-same-resolution-lock-active", "");
    let entered = temp_path("ir-same-resolution-lock-entered", "txt");
    let overlap = temp_path("ir-same-resolution-lock-overlap", "txt");
    let probe = ResolverLockProbe {
        user_cache_dir: &user_cache_dir,
        profile: &profile,
        active: &active,
        overlap: &overlap,
        entered: &entered,
    };

    write_resolver_lock_profile(&profile);

    let first = spawn_resolver_for_lock_test(&cache_dir, &probe, None, "resolution-lock-one");
    let first = wait_for_resolver_probe(first, &active);

    let second = resolver_lock_command(&cache_dir, &probe, None, "resolution-lock-two")
        .output()
        .unwrap();
    let first = first.wait_with_output().unwrap();

    assert_success(&first);
    assert_success(&second);
    assert_stdout_contains(&first, "ir.fixture=resolution-lock-one");
    assert_stdout_contains(&second, "ir.fixture=resolution-lock-two");
    assert!(!overlap.exists(), "resolve.R should not overlap");
    assert_eq!(
        resolver_probe_count(&entered),
        1,
        "second resolver should reuse the completed resolution marker"
    );
}

#[test]
fn concurrent_resolvers_serialize_different_dependency_resolution() {
    let cache_dir = temp_dir("ir-resolution-overlap-cache");
    let user_cache_dir = temp_dir("ir-resolution-overlap-user-cache");
    let profile = temp_path("ir-resolution-overlap-profile", "R");
    let active = temp_path("ir-resolution-overlap-active", "");
    let entered = temp_path("ir-resolution-overlap-entered", "txt");
    let overlap = temp_path("ir-resolution-overlap", "txt");
    let probe = ResolverLockProbe {
        user_cache_dir: &user_cache_dir,
        profile: &profile,
        active: &active,
        overlap: &overlap,
        entered: &entered,
    };

    write_resolver_lock_profile(&profile);

    let first = spawn_resolver_for_lock_test(&cache_dir, &probe, None, "resolution-one");
    let first = wait_for_resolver_probe(first, &active);

    let second = resolver_lock_command(&cache_dir, &probe, Some("cli"), "resolution-two")
        .output()
        .unwrap();
    let first = first.wait_with_output().unwrap();

    assert_success(&first);
    assert_success(&second);
    assert_stdout_contains(&first, "ir.fixture=resolution-one");
    assert_stdout_contains(&second, "ir.fixture=resolution-two");
    assert!(!overlap.exists(), "resolve.R should not overlap");
    assert_eq!(
        resolver_probe_count(&entered),
        2,
        "different dependencies should both resolve, but not concurrently"
    );
}

#[test]
fn concurrent_resolvers_serialize_shared_cache_with_different_user_cache_roots() {
    let cache_dir = temp_dir("ir-shared-cache-different-user-cache");
    let first_user_cache_dir = temp_dir("ir-shared-cache-user-one");
    let second_user_cache_dir = temp_dir("ir-shared-cache-user-two");
    let profile = temp_path("ir-shared-cache-different-user-cache-profile", "R");
    let active = temp_path("ir-shared-cache-different-user-cache-active", "");
    let entered = temp_path("ir-shared-cache-different-user-cache-entered", "txt");
    let overlap = temp_path("ir-shared-cache-different-user-cache-overlap", "txt");
    let first_probe = ResolverLockProbe {
        user_cache_dir: &first_user_cache_dir,
        profile: &profile,
        active: &active,
        overlap: &overlap,
        entered: &entered,
    };
    let second_probe = ResolverLockProbe {
        user_cache_dir: &second_user_cache_dir,
        profile: &profile,
        active: &active,
        overlap: &overlap,
        entered: &entered,
    };

    write_resolver_lock_profile(&profile);

    let first =
        spawn_resolver_for_lock_test(&cache_dir, &first_probe, None, "shared-cache-lock-one");
    let first = wait_for_resolver_probe(first, &active);

    let second = resolver_lock_command(&cache_dir, &second_probe, None, "shared-cache-lock-two")
        .output()
        .unwrap();
    let first = first.wait_with_output().unwrap();

    assert_success(&first);
    assert_success(&second);
    assert_stdout_contains(&first, "ir.fixture=shared-cache-lock-one");
    assert_stdout_contains(&second, "ir.fixture=shared-cache-lock-two");
    assert!(
        !overlap.exists(),
        "resolve.R should not overlap for a shared ir cache"
    );
    assert_eq!(
        resolver_probe_count(&entered),
        1,
        "second resolver should reuse the shared cache marker"
    );
}

#[test]
fn concurrent_resolvers_serialize_different_ir_caches_with_shared_user_cache() {
    let first_cache_dir = temp_dir("ir-different-cache-shared-user-cache-one");
    let second_cache_dir = temp_dir("ir-different-cache-shared-user-cache-two");
    let user_cache_dir = temp_dir("ir-different-cache-shared-user-cache-user");
    let profile = temp_path("ir-different-cache-shared-user-cache-profile", "R");
    let active = temp_path("ir-different-cache-shared-user-cache-active", "");
    let entered = temp_path("ir-different-cache-shared-user-cache-entered", "txt");
    let overlap = temp_path("ir-different-cache-shared-user-cache-overlap", "txt");
    let probe = ResolverLockProbe {
        user_cache_dir: &user_cache_dir,
        profile: &profile,
        active: &active,
        overlap: &overlap,
        entered: &entered,
    };

    write_resolver_lock_profile(&profile);

    let first =
        spawn_resolver_for_lock_test(&first_cache_dir, &probe, None, "shared-user-cache-one");
    let first = wait_for_resolver_probe(first, &active);

    let second = resolver_lock_command(
        &second_cache_dir,
        &probe,
        Some("cli"),
        "shared-user-cache-two",
    )
    .output()
    .unwrap();
    let first = first.wait_with_output().unwrap();

    assert_success(&first);
    assert_success(&second);
    assert_stdout_contains(&first, "ir.fixture=shared-user-cache-one");
    assert_stdout_contains(&second, "ir.fixture=shared-user-cache-two");
    assert!(
        !overlap.exists(),
        "resolve.R should not overlap for a shared R user cache"
    );
    assert_eq!(
        resolver_probe_count(&entered),
        2,
        "different IR caches should both resolve, but not concurrently"
    );
}
