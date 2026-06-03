//! Integration tests for the `ir` CLI.
//!
//! The cases here are offline and deterministic — they exercise argument
//! handling and error reporting, none of which reaches R. The R-side
//! resolution logic is covered by `tests/test-resolve.R`, which this file also
//! runs via `cargo test` when an R toolchain is available.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static UNIQUE_ID: AtomicU64 = AtomicU64::new(0);

fn ir() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ir"))
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

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
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
            stdout.contains("\n    ir run <script.R> [args...]\n    ir cache <command>\n"),
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

#[test]
fn run_enables_and_forwards_pak_progress_in_resolver() {
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

if [ "$#" = "3" ]; then
  if [ "${R_PKG_SHOW_PROGRESS:-}" != "true" ]; then
    echo "pak progress disabled" >&2
    exit 7
  fi
  echo "pak progress stdout"
  echo "pak progress stderr" >&2
  echo "/tmp/ir-test-library" > "$3"
  exit 0
fi

if [ "${R_PKG_SHOW_PROGRESS:-}" = "true" ]; then
  echo "pak progress leaked to user script" >&2
  exit 8
fi

echo "user script stdout"
"#,
    );
    fs::write(&script, "cat('unused by fake Rscript\\n')\n").unwrap();

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
