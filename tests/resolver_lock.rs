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

#[test]
fn concurrent_resolvers_serialize_same_dependency_resolution() {
    let cache_dir = unique_dir("ir-same-resolution-lock-cache");
    let user_cache_dir = unique_dir("ir-same-resolution-lock-user-cache");
    let profile = unique_path("ir-same-resolution-lock-profile", "R");
    let active = unique_path("ir-same-resolution-lock-active", "");
    let entered = unique_path("ir-same-resolution-lock-entered", "txt");
    let overlap = unique_path("ir-same-resolution-lock-overlap", "txt");
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

    let _ = fs::remove_file(&profile);
    let _ = fs::remove_file(&entered);
    let _ = fs::remove_file(&overlap);
    let _ = fs::remove_dir_all(&active);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&user_cache_dir);
}

#[test]
fn concurrent_resolvers_serialize_different_dependency_resolution() {
    let cache_dir = unique_dir("ir-resolution-overlap-cache");
    let user_cache_dir = unique_dir("ir-resolution-overlap-user-cache");
    let profile = unique_path("ir-resolution-overlap-profile", "R");
    let active = unique_path("ir-resolution-overlap-active", "");
    let entered = unique_path("ir-resolution-overlap-entered", "txt");
    let overlap = unique_path("ir-resolution-overlap", "txt");
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

    let _ = fs::remove_file(&profile);
    let _ = fs::remove_file(&entered);
    let _ = fs::remove_file(&overlap);
    let _ = fs::remove_dir_all(&active);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&user_cache_dir);
}

#[test]
fn concurrent_resolvers_serialize_shared_cache_with_different_user_cache_roots() {
    let cache_dir = unique_dir("ir-shared-cache-different-user-cache");
    let first_user_cache_dir = unique_dir("ir-shared-cache-user-one");
    let second_user_cache_dir = unique_dir("ir-shared-cache-user-two");
    let profile = unique_path("ir-shared-cache-different-user-cache-profile", "R");
    let active = unique_path("ir-shared-cache-different-user-cache-active", "");
    let entered = unique_path("ir-shared-cache-different-user-cache-entered", "txt");
    let overlap = unique_path("ir-shared-cache-different-user-cache-overlap", "txt");
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

    let _ = fs::remove_file(&profile);
    let _ = fs::remove_file(&entered);
    let _ = fs::remove_file(&overlap);
    let _ = fs::remove_dir_all(&active);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&first_user_cache_dir);
    let _ = fs::remove_dir_all(&second_user_cache_dir);
}
