//! Integration tests for the `ir` CLI.
//!
//! The cases here are offline and deterministic — they exercise argument
//! handling and error reporting, none of which reaches R. The R-side
//! resolution logic is covered by `tests/test-resolve.R`, which this file also
//! runs via `cargo test` when an R toolchain is available.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static UNIQUE_ID: AtomicU64 = AtomicU64::new(0);

fn ir() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ir"))
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
    let actual = String::from_utf8(out.stdout).unwrap();
    assert_eq!(actual, expected, "{args:?} changed {}", snapshot.display());
}

fn unique_path(prefix: &str, ext: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let id = UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{nanos}-{id}.{ext}",
        std::process::id()
    ))
}

/// Write `contents` to `path` as a fake `Rscript`. On Unix the file is marked
/// executable; on Windows a `.cmd` is runnable by extension alone.
fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}

#[test]
fn help_outputs_match_snapshots() {
    for (name, args) in [
        ("help", &["--help"][..]),
        ("help", &["-h"]),
        ("help", &[]),
        ("run-help", &["run", "--help"]),
        ("run-help", &["run", "-h"]),
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
fn version_flag_reports_version() {
    let out = ir().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("ir 0."));
}

#[test]
fn help_is_shown_for_help_flag_and_no_args() {
    for args in [vec!["--help"], vec![]] {
        let out = ir().args(&args).output().unwrap();
        assert!(out.status.success(), "args {args:?} should exit 0");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("self-describing R scripts"),
            "args {args:?}: {stdout}"
        );
        assert!(!stdout.contains("uv-style"), "args {args:?}: {stdout}");
        assert!(stdout.contains("USAGE"), "args {args:?}: {stdout}");
        assert!(stdout.contains("ir run"), "args {args:?}: {stdout}");
        assert!(
            stdout.contains(
                "\n    ir run [Rscript-options...] <script.R> [args...]\n    ir cache <command>\n"
            ),
            "args {args:?}: {stdout}"
        );
    }
}

#[test]
fn unknown_command_errors() {
    let out = ir().arg("frobnicate").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown command"));
}

#[test]
fn run_without_a_script_errors() {
    let out = ir().arg("run").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("requires a script"));
}

#[test]
fn run_help_flag_shows_help() {
    let out = ir().args(["run", "--help"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Run an R script"), "{stdout}");
    assert!(stdout.contains("USAGE"), "{stdout}");
    assert!(
        stdout.contains("ir run [Rscript-options...] <script.R> [args...]"),
        "{stdout}"
    );
    assert!(out.stderr.is_empty());
}

#[test]
fn run_with_missing_script_errors() {
    let out = ir().args(["run", "/no/such/ir-script.R"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read script"));
}

#[test]
fn cache_dir_reports_ir_cache_dir_override() {
    let cache_dir = unique_path("ir-cache", "dir");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["cache", "dir"])
        .output()
        .unwrap();

    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}\n", cache_dir.display())
    );
}

#[test]
fn cache_clean_removes_cache_dir() {
    let cache_dir = unique_path("ir-cache", "dir");
    let library = cache_dir.join("libraries").join("library");
    fs::create_dir_all(&library).unwrap();
    fs::write(library.join("pkg"), "cached").unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["cache", "clean"])
        .output()
        .unwrap();

    assert!(out.status.success());
    assert!(!cache_dir.exists());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&format!("Clearing cache at: {}", cache_dir.display())),
        "{stdout}"
    );
    assert!(stdout.contains("Removed 1 file"), "{stdout}");
}

#[test]
fn cache_clean_reports_missing_cache_dir() {
    let cache_dir = unique_path("ir-cache", "dir");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["cache", "clean"])
        .output()
        .unwrap();

    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("No cache found at: {}\n", cache_dir.display())
    );
}

#[cfg(unix)]
#[test]
fn cache_dir_resolves_default_with_r_user_dir() {
    let fake_rscript = unique_path("ir-fake-rscript", "sh");
    let cache_dir = unique_path("ir-r-cache", "dir");

    write_executable(
        &fake_rscript,
        &format!(
            r#"#!/bin/sh
set -eu
for arg in "$@"; do
  if [ "$arg" = "--vanilla" ]; then
    echo "unexpected --vanilla" >&2
    exit 9
  fi
done
test "$1" = "-e"
case "$2" in
  *"tools::R_user_dir"*) ;;
  *) echo "missing tools::R_user_dir" >&2; exit 8 ;;
esac
printf '%s\n' "{}"
"#,
            cache_dir.display()
        ),
    );

    let out = ir()
        .env_remove("IR_CACHE_DIR")
        .env("IR_RSCRIPT", &fake_rscript)
        .args(["cache", "dir"])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);

    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}\n", cache_dir.display())
    );
}

#[cfg(unix)]
#[test]
fn run_pipes_frontmatter_and_forwards_pak_progress_in_resolver() {
    let fake_rscript = unique_path("ir-fake-rscript", "sh");
    let script = unique_path("ir-script", "R");

    write_executable(
        &fake_rscript,
        r#"#!/bin/sh
set -eu
for arg in "$@"; do
  if [ "$arg" = "--vanilla" ]; then
    echo "unexpected --vanilla" >&2
    exit 9
  fi
done

if [ "$#" = "2" ]; then
  if [ "${R_PKG_SHOW_PROGRESS:-}" != "true" ]; then
    echo "pak progress disabled" >&2
    exit 7
  fi
  actual="$(mktemp)"
  expected="$(mktemp)"
  cat > "$actual"
  printf 'dependencies:\n  - dplyr>=1.0\nexclude after: "2024-01-15"\n' > "$expected"
  if ! cmp -s "$actual" "$expected"; then
    echo "unexpected resolver stdin" >&2
    echo "--- actual ---" >&2
    cat "$actual" >&2
    exit 10
  fi
  echo "pak progress stdout"
  echo "pak progress stderr" >&2
  echo "/tmp/ir-test-library" > "$2"
  exit 0
fi

if [ "${R_PKG_SHOW_PROGRESS:-}" = "true" ]; then
  echo "pak progress leaked to user script" >&2
  exit 8
fi

echo "user script stdout"
"#,
    );
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| dependencies:
#|   - dplyr>=1.0
#| exclude after: "2024-01-15"

cat('unused by fake Rscript\n')
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .env_remove("R_PKG_SHOW_PROGRESS")
        .args(["run", script.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);
    let _ = fs::remove_file(&script);

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("pak progress stdout"), "{stdout}");
    assert!(stdout.contains("user script stdout"), "{stdout}");
    assert!(stderr.contains("pak progress stderr"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn run_forwards_leading_rscript_args_to_user_script() {
    let fake_rscript = unique_path("ir-fake-rscript", "sh");
    let script = unique_path("ir-script", "R");
    fs::write(&script, "cat('unused by fake Rscript\\n')\n").unwrap();
    let canonical_script = fs::canonicalize(&script).unwrap();

    write_executable(
        &fake_rscript,
        &format!(
            r#"#!/bin/sh
set -eu

if [ "$1" != "--vanilla" ]; then
  for arg in "$@"; do
    if [ "$arg" = "--vanilla" ]; then
      echo "Rscript args leaked to resolver" >&2
      exit 9
    fi
  done
  test "$#" = "2"
  cat > /dev/null
  : > "$2"
  exit 0
fi

test "$#" = "5"
test "$1" = "--vanilla"
test "$2" = "--default-packages=utils"
test "$3" = "{}"
test "$4" = "--script-arg"
test "$5" = "value"
echo "user Rscript args received"
"#,
            canonical_script.display()
        ),
    );

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .args([
            "run",
            "--vanilla",
            "--default-packages=utils",
            script.to_str().unwrap(),
            "--script-arg",
            "value",
        ])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);
    let _ = fs::remove_file(&script);

    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("user Rscript args received"),
        "{:?}",
        out
    );
}

/// The user script's exit code surfaces unchanged as `ir`'s exit code. On Unix
/// this rides on the `exec` into R; the Windows spawn+wait fallback is covered
/// by the variant below.
#[cfg(unix)]
#[test]
fn run_propagates_user_script_exit_code() {
    let fake_rscript = unique_path("ir-fake-rscript", "sh");
    let script = unique_path("ir-script", "R");

    write_executable(
        &fake_rscript,
        r#"#!/bin/sh
set -eu
# Phase 1 (resolve) gets the driver path and output path.
if [ "$#" = "2" ]; then
  cat > /dev/null
  : > "$2"
  exit 0
fi
# Phase 2 (user script): exit with a distinctive code.
exit 42
"#,
    );
    fs::write(&script, "stop('unused by fake Rscript')\n").unwrap();

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .args(["run", script.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);
    let _ = fs::remove_file(&script);

    assert_eq!(out.status.code(), Some(42));
}

/// Windows has no `exec`, so `ir` runs R as a child and forwards its exit code
/// via `status.code()`. The fake distinguishes phases by the presence of the
/// resolver's output-file argument.
#[cfg(windows)]
#[test]
fn run_propagates_user_script_exit_code() {
    let fake_rscript = unique_path("ir-fake-rscript", "cmd");
    let script = unique_path("ir-script", "R");

    write_executable(
        &fake_rscript,
        concat!(
            "@echo off\r\n",
            // Phase 1 (resolve): an output-file arg is present — report an
            // empty library to its path and succeed.
            "if not \"%~2\"==\"\" (\r\n",
            "  type nul > \"%~2\"\r\n",
            "  exit /b 0\r\n",
            ")\r\n",
            // Phase 2 (user script): exit with a distinctive code.
            "exit /b 42\r\n",
        ),
    );
    fs::write(&script, "stop('unused by fake Rscript')\n").unwrap();

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .args(["run", script.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);
    let _ = fs::remove_file(&script);

    assert_eq!(out.status.code(), Some(42));
}

/// A user script killed by a signal is reported as signal death, not flattened
/// to a plain exit code — the fidelity `exec` preserves but a spawned child
/// (whose `status.code()` would be `None` → `1`) would lose. Unix-only: Windows
/// has no equivalent signal model and always takes the spawn+wait path.
#[cfg(unix)]
#[test]
fn run_propagates_user_script_signal_death() {
    use std::os::unix::process::ExitStatusExt;

    let fake_rscript = unique_path("ir-fake-rscript", "sh");
    let script = unique_path("ir-script", "R");

    write_executable(
        &fake_rscript,
        r#"#!/bin/sh
set -eu
if [ "$#" = "2" ]; then
  cat > /dev/null
  : > "$2"
  exit 0
fi
# Phase 2: after exec this shell *is* ir's process, so SIGKILL kills ir itself.
kill -KILL $$
"#,
    );
    fs::write(&script, "stop('unused by fake Rscript')\n").unwrap();

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .args(["run", script.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);
    let _ = fs::remove_file(&script);

    assert_eq!(out.status.code(), None, "signal death has no exit code");
    assert_eq!(out.status.signal(), Some(9), "killed by SIGKILL");
}

/// Run the comprehensive R resolution suite under `cargo test`. Skips (passes
/// as a no-op) when no usable R toolchain with the required R packages is
/// present.
#[test]
fn r_resolve_suite_passes() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let rscript = std::env::var("IR_RSCRIPT").unwrap_or_else(|_| "Rscript".into());

    let probe = Command::new(&rscript)
        .args([
            "-e",
            "stopifnot(requireNamespace('testthat', quietly = TRUE), \
                       requireNamespace('yaml12', quietly = TRUE), \
                       requireNamespace('withr', quietly = TRUE), \
                       requireNamespace('secretbase', quietly = TRUE))",
        ])
        .output();
    match probe {
        Err(_) => return eprintln!("skipping R suite: `{rscript}` not found"),
        Ok(o) if !o.status.success() => {
            return eprintln!("skipping R suite: required R packages unavailable");
        }
        Ok(_) => {}
    }

    let status = Command::new(&rscript)
        .arg("-e")
        .arg("testthat::test_file('tests/test-resolve.R', stop_on_failure = TRUE)")
        .current_dir(manifest)
        .env("IR_DRIVER", format!("{manifest}/driver/resolve.R"))
        .status()
        .expect("failed to launch Rscript");
    assert!(status.success(), "R resolution suite failed");
}
