//! Real end-to-end tests for `ir run`.
//!
//! Unlike `tests/cli.rs` (offline, fake Rscript), these run a real R toolchain:
//! they prove `ir run` materialises a library, installs the declared packages,
//! and runs under the resolved R version. They are opt-in via `IR_E2E` and form
//! the basis of a Windows CI job.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// True when e2e tests are opted in. Unset => tests skip (loudly).
fn e2e_enabled() -> bool {
    std::env::var_os("IR_E2E").is_some()
}

fn ir() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ir"))
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Fresh temp dir for one test. Left in place on panic (for debugging); removed
/// explicitly at the end of a passing test.
fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ir-e2e-{tag}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Copy a fixture into the isolated temp dir so any rendered output (e.g.
/// quarto's `.md`) lands there and is cleaned up — keeping tests/fixtures
/// pristine. Returns the staged path.
fn stage_fixture(dir: &Path, name: &str) -> PathBuf {
    let dst = dir.join(name);
    fs::copy(fixtures_dir().join(name), &dst)
        .unwrap_or_else(|e| panic!("failed to stage fixture {name}: {e}"));
    dst
}

/// Normalise a path for cross-platform comparison: forward slashes, strip the
/// Windows `\\?\` verbatim prefix, lowercase (Windows paths are case-insensitive
/// and R may return a different case than the temp dir was created with).
fn norm(p: &str) -> String {
    let s = p.replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s);
    s.to_lowercase()
}

/// Read and parse the facts JSON written by a fixture.
fn read_facts(path: &Path) -> serde_json::Value {
    let raw = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("facts file {} not written: {e}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("facts {} not valid JSON: {e}\n{raw}", path.display()))
}

/// Assert that some `.libPaths()` entry lives under `<cache>/libraries/`,
/// proving `ir` injected the resolved library via `R_LIBS`.
fn assert_library_injected(facts: &serde_json::Value, cache: &Path) {
    let libraries = norm(&cache.join("libraries").to_string_lossy());
    let libpaths = facts["libpaths"]
        .as_array()
        .expect("facts.libpaths missing or not an array");
    let found = libpaths
        .iter()
        .filter_map(|v| v.as_str())
        .any(|p| norm(p).starts_with(&libraries));
    assert!(
        found,
        "no .libPaths() entry under {libraries}\nlibpaths = {libpaths:?}"
    );
}

/// Assert a non-empty `<cache>/libraries/` containing at least one non-empty
/// subdir (the materialised library).
fn assert_library_created(cache: &Path) {
    let libraries = cache.join("libraries");
    let mut subdirs = fs::read_dir(&libraries)
        .unwrap_or_else(|e| panic!("{} not created: {e}", libraries.display()))
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir());
    let first = subdirs
        .next()
        .unwrap_or_else(|| panic!("no library subdir under {}", libraries.display()));
    let non_empty = fs::read_dir(first.path()).unwrap().next().is_some();
    assert!(non_empty, "library {} is empty", first.path().display());
}

/// Version of the default R reported by rig (the `default: true` entry).
/// `None` when rig is absent or has no default.
fn rig_default_version() -> Option<String> {
    let out = Command::new("rig").args(["list", "--json"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let list: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    list.as_array()?
        .iter()
        .find(|e| e["default"].as_bool() == Some(true))?["version"]
        .as_str()
        .map(str::to_string)
}

#[test]
fn run_r_script_injects_resolved_library() {
    if !e2e_enabled() {
        eprintln!("SKIP run_r_script_injects_resolved_library: set IR_E2E=1 to run");
        return;
    }
    let cache = temp_dir("script");
    let facts_path = cache.join("facts.json");
    let script = stage_fixture(&cache, "e2e.R");

    let status = ir()
        .arg("run")
        .arg(&script)
        .env("IR_CACHE_DIR", &cache)
        .env("IR_E2E_FACTS", &facts_path)
        .status()
        .expect("failed to spawn ir");
    assert!(
        status.success(),
        "`ir run e2e.R` failed (exit {:?})",
        status.code()
    );

    let facts = read_facts(&facts_path);
    assert_library_injected(&facts, &cache);
    assert_library_created(&cache);
    assert_eq!(
        facts["jsonlite_version"].as_str().unwrap_or(""),
        "2.0.0",
        "jsonlite version drifted; exclude-newer 2026-06-01 no longer deterministic"
    );

    fs::remove_dir_all(&cache).ok();
}

#[test]
fn run_qmd_selects_r_version_and_injects_library() {
    if !e2e_enabled() {
        eprintln!("SKIP run_qmd_selects_r_version_and_injects_library: set IR_E2E=1 to run");
        return;
    }
    let target = std::env::var("IR_E2E_R_VERSION").unwrap_or_else(|_| "4.4.3".to_string());
    let default_v =
        rig_default_version().expect("rig unavailable or no default R; this test requires rig");
    assert_ne!(
        target, default_v,
        "vacuous: IR_E2E_R_VERSION ({target}) == default R ({default_v}); pick a different installed version"
    );

    let cache = temp_dir("qmd");
    let facts_path = cache.join("facts.json");
    let doc = stage_fixture(&cache, "e2e.qmd");

    let status = ir()
        .arg("run")
        .arg("--r-version")
        .arg(&target)
        .arg(&doc)
        .env("IR_CACHE_DIR", &cache)
        .env("IR_E2E_FACTS", &facts_path)
        .status()
        .expect("failed to spawn ir");
    assert!(
        status.success(),
        "`ir run --r-version {target} e2e.qmd` failed (exit {:?})",
        status.code()
    );

    let facts = read_facts(&facts_path);
    assert!(
        facts["r_version"]
            .as_str()
            .unwrap_or("")
            .starts_with(&target),
        "rendered under R {:?}, expected {target}",
        facts["r_version"]
    );
    assert_library_injected(&facts, &cache);
    assert_library_created(&cache);

    fs::remove_dir_all(&cache).ok();
}
