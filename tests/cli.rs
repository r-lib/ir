//! Integration tests for the `ir` CLI.
//!
//! The cases here are offline and deterministic — they exercise argument
//! handling and error reporting, none of which reaches R. The R-side
//! resolution logic is covered by `tests/test-resolve.R`, which this file also
//! runs via `cargo test` when an R toolchain is available.

use std::ffi::OsString;
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

fn prepend_path(dir: &Path) -> OsString {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(paths).unwrap()
}

#[cfg(unix)]
fn executable_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

#[cfg(unix)]
fn real_r_tools() -> Option<(PathBuf, PathBuf)> {
    let r = executable_on_path("R")?;
    let rscript = executable_on_path("Rscript")?;
    let probe = Command::new(&rscript)
        .args([
            "-e",
            "stopifnot(requireNamespace('secretbase', quietly = TRUE))",
        ])
        .output()
        .ok()?;
    if probe.status.success() {
        Some((r, rscript))
    } else {
        eprintln!("skipping real Rscript test: required R package `secretbase` unavailable");
        None
    }
}

#[cfg(unix)]
fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

#[cfg(unix)]
fn write_r_home_wrappers(r_home: &Path, real_r: &Path, real_rscript: &Path, marker: &str) {
    write_executable(
        &r_home.join("R"),
        &format!("#!/bin/sh\nexec {} \"$@\"\n", sh_quote(real_r)),
    );
    write_executable(
        &r_home.join("Rscript"),
        &format!(
            "#!/bin/sh\nIR_TEST_SELECTED_R={} exec {} \"$@\"\n",
            marker,
            sh_quote(real_rscript)
        ),
    );
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
            stdout.contains(concat!(
                "\n    ir run [Rscript-options...] [--isolated] [--with <pkg>]... [--r-version <spec>] <script.R> [args...]\n",
                "    ir run [Rscript-options...] [--isolated] [--with <pkg>]... [--r-version <spec>] -e <expr> [args...]\n",
                "    ir cache <command>\n"
            )),
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
        stdout.contains(
            "ir run [Rscript-options...] [--isolated] [--with <pkg>]... [--r-version <spec>] <script.R> [args...]"
        ),
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
fn run_passes_dependencies_on_resolver_stdin_and_forwards_pak_progress() {
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

if [ "${IR_RESOLVE_RESULT_FILE:-}" != "" ]; then
  if [ "${R_PKG_SHOW_PROGRESS:-}" != "true" ]; then
    echo "pak progress disabled" >&2
    exit 7
  fi
  if [ "$#" != "1" ]; then
    echo "unexpected resolver args: $*" >&2
    exit 10
  fi
  actual="$(mktemp)"
  expected="$(mktemp)"
  cat > "$actual"
  printf 'dplyr>=1.0\ntidyr\n' > "$expected"
  if ! cmp -s "$actual" "$expected"; then
    echo "unexpected resolver stdin" >&2
    exit 11
  fi
  if [ "${IR_EXCLUDE_NEWER:-}" != "2024-01-15" ]; then
    echo "missing IR_EXCLUDE_NEWER" >&2
    exit 13
  fi
  echo "pak progress stdout"
  echo "pak progress stderr" >&2
  echo "/tmp/ir-test-library" > "$IR_RESOLVE_RESULT_FILE"
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
#|   - tidyr
#| exclude-newer: "2024-01-15"

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
fn run_errors_on_malformed_frontmatter_before_resolver() {
    let fake_rscript = unique_path("ir-fake-rscript", "sh");
    let script = unique_path("ir-script", "R");

    write_executable(
        &fake_rscript,
        r#"#!/bin/sh
echo "resolver should not run" >&2
exit 17
"#,
    );
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| dependencies: [dplyr

cat('unused by fake Rscript\n')
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .args(["run", script.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);
    let _ = fs::remove_file(&script);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("could not parse script frontmatter as YAML"),
        "{stderr}"
    );
    assert!(!stderr.contains("resolver should not run"), "{stderr}");
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

if [ "${{IR_RESOLVE_RESULT_FILE:-}}" != "" ]; then
  for arg in "$@"; do
    if [ "$arg" = "--vanilla" ]; then
      echo "Rscript args leaked to resolver" >&2
      exit 9
    fi
  done
  test "$#" = "1"
  cat > /dev/null
  : > "$IR_RESOLVE_RESULT_FILE"
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

#[cfg(unix)]
#[test]
fn run_uses_r_version_key_to_select_installed_rscript_with_rig() {
    let Some((real_r, real_rscript)) = real_r_tools() else {
        return;
    };
    let bin_dir = unique_path("ir-bin", "dir");
    let r_home = unique_path("ir-r-45", "dir");
    let cache_dir = unique_path("ir-cache", "dir");
    let script = unique_path("ir-script", "R");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&r_home).unwrap();

    let rig = bin_dir.join("rig");
    let default_rscript = bin_dir.join("Rscript");
    let r_binary = r_home.join("R");

    write_executable(
        &rig,
        &format!(
            r#"#!/bin/sh
set -eu
case "$1 $2" in
  "list --json")
    cat <<'JSON'
[
  {{
    "name": "4.5-arm64",
    "default": false,
    "version": "4.5.3",
    "aliases": [],
    "path": "{}",
    "binary": "{}"
  }}
]
JSON
    ;;
  "available --json")
    echo "rig available should not run when an installed R satisfies the request" >&2
    exit 65
    ;;
  *)
    echo "unexpected rig args: $*" >&2
    exit 64
    ;;
esac
"#,
            r_home.display(),
            r_binary.display()
        ),
    );
    write_executable(
        &default_rscript,
        "#!/bin/sh\necho default Rscript should not run >&2\nexit 88\n",
    );
    write_r_home_wrappers(&r_home, &real_r, &real_rscript, "4.5");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| r-version: "4.5" # selected via rig

cat('selected R ', Sys.getenv('IR_TEST_SELECTED_R'), '\n', sep = '')
"#,
    )
    .unwrap();

    let out = ir()
        .env("PATH", prepend_path(&bin_dir))
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("IR_RSCRIPT")
        .args(["run", script.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&r_home);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_file(&script);

    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("selected R 4.5"),
        "{out:?}"
    );
}

#[cfg(unix)]
#[test]
fn run_r_version_arg_overrides_frontmatter_r_version() {
    let Some((real_r, real_rscript)) = real_r_tools() else {
        return;
    };
    let bin_dir = unique_path("ir-bin", "dir");
    let r_home = unique_path("ir-r-45", "dir");
    let cache_dir = unique_path("ir-cache", "dir");
    let script = unique_path("ir-script", "R");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&r_home).unwrap();

    let rig = bin_dir.join("rig");
    let default_rscript = bin_dir.join("Rscript");
    let r_binary = r_home.join("R");

    write_executable(
        &rig,
        &format!(
            r#"#!/bin/sh
set -eu
case "$1 $2" in
  "list --json")
    cat <<'JSON'
[
  {{
    "name": "4.5-arm64",
    "default": false,
    "version": "4.5.3",
    "aliases": [],
    "path": "{}",
    "binary": "{}"
  }}
]
JSON
    ;;
  "available --json")
    echo "rig available should not run when an installed R satisfies the request" >&2
    exit 65
    ;;
  *)
    echo "unexpected rig args: $*" >&2
    exit 64
    ;;
esac
"#,
            r_home.display(),
            r_binary.display()
        ),
    );
    write_executable(
        &default_rscript,
        "#!/bin/sh\necho default Rscript should not run >&2\nexit 88\n",
    );
    write_r_home_wrappers(&r_home, &real_r, &real_rscript, "4.5");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| r-version: "4.6"

cat('selected R ', Sys.getenv('IR_TEST_SELECTED_R'), '\n', sep = '')
"#,
    )
    .unwrap();

    let out = ir()
        .env("PATH", prepend_path(&bin_dir))
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "4.5", script.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&r_home);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_file(&script);

    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("selected R 4.5"),
        "{out:?}"
    );
}

/// `-e <expr>` runs an inline expression instead of a script file: the
/// user-code phase is invoked as `Rscript -e <expr>` with the resolved library
/// injected via `R_LIBS`. As elsewhere, the resolver phase is the one with
/// `IR_RESOLVE_RESULT_FILE` set; an inline expression declares no deps, so the
/// resolver receives empty stdin.
#[cfg(unix)]
#[test]
fn run_e_evaluates_inline_expression() {
    let fake_rscript = unique_path("ir-fake-rscript", "sh");

    write_executable(
        &fake_rscript,
        r#"#!/bin/sh
set -eu
if [ "${IR_RESOLVE_RESULT_FILE:-}" != "" ]; then
  actual="$(cat)"
  if [ -n "$actual" ]; then
    echo "unexpected resolver stdin: $actual" >&2
    exit 10
  fi
  echo "/tmp/ir-test-library" > "$IR_RESOLVE_RESULT_FILE"
  exit 0
fi
# Phase 2 (user code): an inline expression, not a script file.
test "$1" = "-e"
test "$2" = "1 + 1"
test "${R_LIBS:-}" = "/tmp/ir-test-library"
echo "ran inline expr"
"#,
    );

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .args(["run", "-e", "1 + 1"])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);

    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("ran inline expr"),
        "{out:?}"
    );
}

#[cfg(unix)]
#[test]
fn run_uses_latest_installed_r_without_rig_available() {
    let Some((real_r, real_rscript)) = real_r_tools() else {
        return;
    };
    let bin_dir = unique_path("ir-bin", "dir");
    let r45_home = unique_path("ir-r-45", "dir");
    let r46_home = unique_path("ir-r-46", "dir");
    let cache_dir = unique_path("ir-cache", "dir");
    let script = unique_path("ir-script", "R");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&r45_home).unwrap();
    fs::create_dir_all(&r46_home).unwrap();

    let rig = bin_dir.join("rig");
    let default_rscript = bin_dir.join("Rscript");
    let r45_binary = r45_home.join("R");
    let r46_binary = r46_home.join("R");

    write_executable(
        &rig,
        &format!(
            r#"#!/bin/sh
set -eu
case "$1 $2" in
  "list --json")
    cat <<'JSON'
[
  {{
    "name": "4.5-arm64",
    "default": false,
    "version": "4.5.3",
    "aliases": [],
    "path": "{}",
    "binary": "{}"
  }},
  {{
    "name": "4.6-arm64",
    "default": false,
    "version": "4.6.0",
    "aliases": [],
    "path": "{}",
    "binary": "{}"
  }}
]
JSON
    ;;
  "available --json")
    echo "rig available should not run when an installed R satisfies the request" >&2
    exit 65
    ;;
  *)
    echo "unexpected rig args: $*" >&2
    exit 64
    ;;
esac
"#,
            r45_home.display(),
            r45_binary.display(),
            r46_home.display(),
            r46_binary.display()
        ),
    );
    write_executable(
        &default_rscript,
        "#!/bin/sh\necho default Rscript should not run >&2\nexit 88\n",
    );
    write_r_home_wrappers(&r45_home, &real_r, &real_rscript, "4.5");
    write_r_home_wrappers(&r46_home, &real_r, &real_rscript, "4.6");
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| r-version: ">= 4.5"
#| exclude-newer: "2025-12-31"

cat('selected R ', Sys.getenv('IR_TEST_SELECTED_R'), '\n', sep = '')
"#,
    )
    .unwrap();

    let out = ir()
        .env("PATH", prepend_path(&bin_dir))
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("IR_RSCRIPT")
        .args(["run", script.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&r45_home);
    let _ = fs::remove_dir_all(&r46_home);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_file(&script);

    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("selected R 4.6"), "{stdout}");
    assert!(!stdout.contains("selected R 4.5"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn run_errors_with_rig_install_command_when_required_r_is_not_installed() {
    let bin_dir = unique_path("ir-bin", "dir");
    let script = unique_path("ir-script", "R");
    fs::create_dir_all(&bin_dir).unwrap();

    let rig = bin_dir.join("rig");
    let default_rscript = bin_dir.join("Rscript");
    write_executable(
        &rig,
        r#"#!/bin/sh
set -eu
case "$1 $2" in
  "list --json")
    printf '[]\n'
    ;;
  "available --json")
    cat <<'JSON'
[
  {
    "name": "4.5",
    "date": "2025-04-11",
    "version": "4.5.3",
    "type": "release",
    "url": "https://example.invalid/R-4.5.3.pkg"
  },
  {
    "name": "4.6",
    "date": "2026-04-10",
    "version": "4.6.0",
    "type": "release",
    "url": "https://example.invalid/R-4.6.0.pkg"
  }
]
JSON
    ;;
  *)
    echo "unexpected rig args: $*" >&2
    exit 64
    ;;
esac
"#,
    );
    write_executable(
        &default_rscript,
        "#!/bin/sh\necho default Rscript should not run >&2\nexit 88\n",
    );
    fs::write(
        &script,
        r#"#!/usr/bin/env -S ir run
#| r-version: ">= 4.5"
#| exclude-newer: "2025-12-31"

cat('unused by fake Rscript\n')
"#,
    )
    .unwrap();

    let out = ir()
        .env("PATH", prepend_path(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", script.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_file(&script);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("R 4.5.3 is required but is not installed"),
        "{stderr}"
    );
    assert!(stderr.contains("rig install 4.5"), "{stderr}");
    assert!(!stderr.contains("rig install 4.6"), "{stderr}");
}

/// Multiple `-e` flags are forwarded in order as repeated `-e <expr>` pairs,
/// mirroring Rscript.
#[cfg(unix)]
#[test]
fn run_e_accepts_multiple_expressions() {
    let fake_rscript = unique_path("ir-fake-rscript", "sh");

    write_executable(
        &fake_rscript,
        r#"#!/bin/sh
set -eu
if [ "${IR_RESOLVE_RESULT_FILE:-}" != "" ]; then
  cat > /dev/null
  : > "$IR_RESOLVE_RESULT_FILE"
  exit 0
fi
test "$#" = "4"
test "$1" = "-e"
test "$2" = "a <- 1"
test "$3" = "-e"
test "$4" = "print(a)"
echo "ran two exprs"
"#,
    );

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .args(["run", "-e", "a <- 1", "-e", "print(a)"])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);

    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("ran two exprs"),
        "{out:?}"
    );
}

/// `-e` with no following expression is a usage error, reported before any R
/// session is launched.
#[test]
fn run_e_requires_an_expression() {
    let out = ir().args(["run", "-e"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("requires an expression"),
        "{out:?}"
    );
}

/// `--with` specs reach the resolver on stdin (comma-separated lists split into
/// individual specs, one per line) and are not forwarded to the user-code phase.
#[cfg(unix)]
#[test]
fn run_with_passes_dependencies_to_resolver() {
    let fake_rscript = unique_path("ir-fake-rscript", "sh");

    write_executable(
        &fake_rscript,
        r#"#!/bin/sh
set -eu
if [ "${IR_RESOLVE_RESULT_FILE:-}" != "" ]; then
  actual="$(mktemp)"
  cat > "$actual"
  expected="$(mktemp)"
  printf 'dplyr\ntidyr\n' > "$expected"
  if ! cmp -s "$actual" "$expected"; then
    echo "unexpected resolver stdin:" >&2
    cat "$actual" >&2
    exit 10
  fi
  echo "/tmp/ir-test-library" > "$IR_RESOLVE_RESULT_FILE"
  exit 0
fi
# Phase 2 (user code): --with must not leak here.
for arg in "$@"; do
  case "$arg" in
    --with*) echo "--with leaked to user code" >&2; exit 9 ;;
  esac
done
test "$1" = "-e"
echo "resolver saw deps"
"#,
    );

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .args(["run", "--with", "dplyr,tidyr", "-e", "library(dplyr)"])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);

    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("resolver saw deps"),
        "{out:?}"
    );
}

/// `--with` dependencies must reach the embedded R resolver through Rscript's
/// stdin connection, not only through shell fakes used by argument tests.
#[cfg(unix)]
#[test]
fn run_with_reaches_real_r_resolver_stdin() {
    let Some(rscript) = executable_on_path("Rscript") else {
        return eprintln!("skipping real Rscript stdin test: Rscript unavailable");
    };

    let probe = Command::new(&rscript)
        .args([
            "-e",
            "stopifnot(requireNamespace('pak', quietly = TRUE), \
                       requireNamespace('renv', quietly = TRUE), \
                       requireNamespace('secretbase', quietly = TRUE))",
        ])
        .output()
        .expect("failed to launch Rscript");
    if !probe.status.success() {
        return eprintln!("skipping real Rscript stdin test: resolver packages unavailable");
    }

    let cache_dir = unique_path("ir-real-r-resolver-cache", "dir");
    let out = ir()
        .env("IR_RSCRIPT", &rscript)
        .env("IR_CACHE_DIR", &cache_dir)
        .args([
            "run",
            "--with",
            "abd",
            "-e",
            r#"
lib <- .libPaths()[1]
stopifnot("abd" %in% rownames(installed.packages(lib.loc = lib)))
library(abd, lib.loc = lib)
cat("abd resolved from ", lib, "\n", sep = "")
"#,
        ])
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&cache_dir);

    assert!(
        out.status.success(),
        "real resolver did not materialise --with package:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("abd resolved from "),
        "{out:?}"
    );
}

/// `--with` works alongside a script file: its specs are appended to the
/// frontmatter dependencies on the resolver's stdin (frontmatter first), while
/// the user-code phase still runs the script (not `-e`).
#[cfg(unix)]
#[test]
fn run_with_applies_to_script_files() {
    let fake_rscript = unique_path("ir-fake-rscript", "sh");
    let script = unique_path("ir-script", "R");
    fs::write(
        &script,
        "#!/usr/bin/env -S ir run\n#| dependencies:\n#|   - dplyr\n\ncat('unused by fake Rscript\\n')\n",
    )
    .unwrap();

    write_executable(
        &fake_rscript,
        r#"#!/bin/sh
set -eu
if [ "${IR_RESOLVE_RESULT_FILE:-}" != "" ]; then
  actual="$(mktemp)"
  cat > "$actual"
  expected="$(mktemp)"
  printf 'dplyr\ncli\n' > "$expected"
  if ! cmp -s "$actual" "$expected"; then
    echo "unexpected resolver stdin:" >&2
    cat "$actual" >&2
    exit 10
  fi
  echo "/tmp/ir-test-library" > "$IR_RESOLVE_RESULT_FILE"
  exit 0
fi
# Phase 2 (user code): runs the script file, not -e.
test "$1" != "-e"
case "$1" in
  *.R) ;;
  *) echo "expected a script path, got $1" >&2; exit 9 ;;
esac
echo "ran script with --with"
"#,
    );

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .args(["run", "--with", "cli", script.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);
    let _ = fs::remove_file(&script);

    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("ran script with --with"),
        "{out:?}"
    );
}

/// `--isolated` is an `ir`-level flag: it reaches neither the resolver nor the
/// user-code phase as an argument. For the user-code phase it sets
/// `R_LIBS_USER=NULL` — R's value for disabling the user library — while the
/// resolved library still arrives via `R_LIBS`.
#[cfg(unix)]
#[test]
fn run_isolated_disables_the_user_library() {
    let fake_rscript = unique_path("ir-fake-rscript", "sh");

    write_executable(
        &fake_rscript,
        r#"#!/bin/sh
set -eu
if [ "${IR_RESOLVE_RESULT_FILE:-}" != "" ]; then
  # Resolver phase: --isolated must not leak here, and only the driver runs.
  test "$#" = "1"
  cat > /dev/null
  echo "/tmp/ir-test-library" > "$IR_RESOLVE_RESULT_FILE"
  exit 0
fi
# Phase 2 (user code): --isolated must not be forwarded to Rscript.
for arg in "$@"; do
  case "$arg" in
    --isolated) echo "--isolated leaked to user code" >&2; exit 9 ;;
  esac
done
# The resolved library arrives via R_LIBS; the user library is disabled.
test "${R_LIBS:-}" = "/tmp/ir-test-library"
test "${R_LIBS_USER:-}" = "NULL"
echo "ran isolated"
"#,
    );

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .args(["run", "--isolated", "-e", "1 + 1"])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);

    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("ran isolated"),
        "{out:?}"
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
# Phase 1 (resolve) gets the driver path and IR_RESOLVE_RESULT_FILE.
if [ "${IR_RESOLVE_RESULT_FILE:-}" != "" ]; then
  : > "$IR_RESOLVE_RESULT_FILE"
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
/// via `status.code()`. The fake distinguishes phases by the presence of
/// `IR_RESOLVE_RESULT_FILE`.
#[cfg(windows)]
#[test]
fn run_propagates_user_script_exit_code() {
    let fake_rscript = unique_path("ir-fake-rscript", "cmd");
    let script = unique_path("ir-script", "R");

    write_executable(
        &fake_rscript,
        concat!(
            "@echo off\r\n",
            // Phase 1 (resolve): report an empty library to its output path
            // and succeed.
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  type nul > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
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
if [ "${IR_RESOLVE_RESULT_FILE:-}" != "" ]; then
  : > "$IR_RESOLVE_RESULT_FILE"
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
