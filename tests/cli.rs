//! Integration tests for the `ir` CLI.
//!
//! The cases here are offline and deterministic — they exercise argument
//! handling and error reporting, none of which reaches R. The R-side
//! resolution logic (every version operator and error) is covered
//! comprehensively by `tests/test-resolve.R`, which this file also runs via
//! `cargo test` when an R toolchain is available.

use std::process::Command;

fn ir() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ir"))
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
        assert!(stdout.contains("USAGE"), "args {args:?}: {stdout}");
        assert!(stdout.contains("ir run"), "args {args:?}: {stdout}");
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

/// Run the comprehensive R resolution suite under `cargo test`. Skips (passes
/// as a no-op) when no usable R toolchain with testthat + yaml12 is present.
#[test]
fn r_resolve_suite_passes() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let rscript = std::env::var("IR_RSCRIPT").unwrap_or_else(|_| "Rscript".into());

    let probe = Command::new(&rscript)
        .args([
            "-e",
            "stopifnot(requireNamespace('testthat', quietly = TRUE), \
                       requireNamespace('yaml12',  quietly = TRUE))",
        ])
        .output();
    match probe {
        Err(_) => return eprintln!("skipping R suite: `{rscript}` not found"),
        Ok(o) if !o.status.success() => {
            return eprintln!("skipping R suite: testthat/yaml12 unavailable");
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
