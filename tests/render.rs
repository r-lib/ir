//! Integration tests for the public `ir` CLI.

mod support;

use support::*;

use std::fs;
use std::process::Command;

#[cfg(unix)]
use std::ffi::OsString;

#[cfg(unix)]
#[test]
fn docs_website_has_dark_mode_and_colored_reference_output() {
    use std::os::unix::fs::PermissionsExt;

    let docs_dir = docs_copy("ir-docs-reference-project");
    let (output_dir, output_dir_name) = unique_dir_in(&docs_dir, "ir-docs-reference-output");
    let bin_dir = unique_dir("ir-docs-reference-bin");
    let fake_cargo = bin_dir.join("cargo");
    let stale_ir = bin_dir.join("ir");
    let cargo_marker = output_dir.join("cargo-called");

    fs::write(
        &fake_cargo,
        concat!(
            "#!/bin/sh\n",
            "touch \"$IR_CARGO_MARKER\"\n",
            "exec \"$REAL_CARGO\" \"$@\"\n",
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(&fake_cargo).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_cargo, perms).unwrap();

    fs::write(
        &stale_ir,
        concat!(
            "#!/bin/sh\n",
            "echo \"error: unrecognized subcommand 'render'\" >&2\n",
            "exit 2\n",
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(&stale_ir).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&stale_ir, perms).unwrap();

    let config = fs::read_to_string(docs_dir.join("_quarto.yml")).unwrap();
    assert!(config.contains("light:"), "{config}");
    assert!(config.contains("dark:"), "{config}");
    assert!(config.contains("dark:\n        - cosmo"), "{config}");
    assert!(config.contains("- dark.scss"), "{config}");
    assert!(!config.contains("- darkly"), "{config}");

    let styles = fs::read_to_string(docs_dir.join("styles.css")).unwrap();
    assert!(styles.contains("quarto-dark"), "{styles}");
    assert!(
        styles.contains("pre.ir-cli-help span[style*=\"#5555FF\"]"),
        "{styles}"
    );
    assert!(
        styles.contains("pre.ir-cli-help span[style*=\"#00BBBB\"]"),
        "{styles}"
    );
    assert!(
        styles.contains("pre.ir-cli-help span[style*=\"#555555\"]"),
        "{styles}"
    );

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let mut quarto = Command::new("quarto");
    quarto
        .current_dir(&docs_dir)
        .env("PATH", path)
        .env_remove("IR_BIN")
        .env(
            "REAL_CARGO",
            std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")),
        )
        .env("IR_CARGO_MARKER", &cargo_marker)
        .args(["render", "reference.qmd", "--to", "html"])
        .arg("--output-dir")
        .arg(&output_dir_name);
    pin_quarto_r(&mut quarto);
    let output = quarto.output().unwrap();
    assert_success(&output);
    assert!(
        cargo_marker.exists(),
        "reference render should build the current ir binary"
    );

    let html = fs::read_to_string(output_dir.join("reference.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered reference page: {e}"));
    assert!(html.contains("data-mode=\"dark\""), "{html}");
    assert!(
        html.contains("Render a Quarto document or script"),
        "{html}"
    );
    assert!(html.contains("Options:"), "{html}");
    assert!(html.contains("color: #5555FF"), "{html}");
    assert!(html.contains("color: #00BBBB"), "{html}");
    assert!(html.contains("color: #555555"), "{html}");
    assert!(html.contains("font-weight: bold"), "{html}");
    assert!(!html.contains("\u{1b}["), "{html}");

    let _ = fs::remove_dir_all(&output_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&docs_dir);
}

#[test]
fn docs_run_page_dark_mode_styles_console_blocks() {
    let docs_dir = docs_copy("ir-docs-run-project");
    let (output_dir, output_dir_name) = unique_dir_in(&docs_dir, "ir-docs-run-output");

    let mut quarto = Command::new("quarto");
    quarto
        .current_dir(&docs_dir)
        .args(["render", "run.qmd", "--to", "html"])
        .arg("--output-dir")
        .arg(&output_dir_name);
    pin_quarto_r(&mut quarto);
    let output = quarto.output().unwrap();
    assert_success(&output);

    let html = fs::read_to_string(output_dir.join("run.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered run page: {e}"));
    assert!(html.contains("$ ir run script.R"), "{html}");

    assert!(html.contains("data-mode=\"dark\""), "{html}");

    let styles = fs::read_to_string(output_dir.join("styles.css"))
        .unwrap_or_else(|e| panic!("failed to read rendered styles.css: {e}"));
    assert!(styles.contains("body.quarto-dark .navbar"), "{styles}");
    assert!(styles.contains("pre.console"), "{styles}");
    assert!(
        styles.contains("background-color: var(--ir-panel)"),
        "{styles}"
    );
    assert!(
        styles.contains("background-color: var(--ir-help-panel)"),
        "{styles}"
    );

    let _ = fs::remove_dir_all(&output_dir);
    let _ = fs::remove_dir_all(&docs_dir);
}

#[test]
fn render_quarto_fixture_injects_rmarkdown_and_renders() {
    let fixture_dir = fixture_copy("run", "ir-e2e-qmd-fixture");
    let cache_dir = unique_dir("ir-e2e-qmd-cache");

    for _ in 0..2 {
        let out = ir()
            .current_dir(&fixture_dir)
            .env("IR_CACHE_DIR", &cache_dir)
            .args(["render", "--isolated"])
            .arg("report.qmd")
            .args(["--to", "html"])
            .output()
            .unwrap();

        assert_success(&out);

        let html = fs::read_to_string(fixture_dir.join("report.html")).unwrap_or_else(|e| {
            panic!("failed to read rendered report: {e}\n{}", output_text(&out))
        });
        assert!(html.contains("ir.fixture=qmd"), "{html}");
        assert!(html.contains("qmd.lib_in_cache=true"), "{html}");
        assert!(html.contains("qmd.pkgs_in_cache=true"), "{html}");
        assert!(html.contains("qmd.result=a:4,b:2"), "{html}");

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("using latest rmarkdown"),
            "rmarkdown injection should be quiet\n{}",
            output_text(&out)
        );

        let _ = fs::remove_file(fixture_dir.join("report.html"));
        let _ = fs::remove_dir_all(fixture_dir.join("report_files"));
    }

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&fixture_dir);
}

// report-pinned.qmd declares rmarkdown itself, so the resolver leaves it alone.
#[test]
fn render_quarto_fixture_with_declared_rmarkdown_skips_injection() {
    let fixture_dir = fixture_copy("run", "ir-e2e-qmd-pinned-fixture");
    let cache_dir = unique_dir("ir-e2e-qmd-pinned-cache");

    let out = ir()
        .current_dir(&fixture_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["render", "--isolated"])
        .arg("report-pinned.qmd")
        .args(["--to", "html"])
        .output()
        .unwrap();

    assert_success(&out);

    let html = fs::read_to_string(fixture_dir.join("report-pinned.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered report: {e}\n{}", output_text(&out)));
    assert!(html.contains("ir.fixture=qmd-pinned"), "{html}");
    // The declared rmarkdown must load from the resolved run library, with its
    // version read from that library's DESCRIPTION.
    assert!(html.contains("pinned.rmarkdown_in_cache=true"), "{html}");
    assert!(html.contains("pinned.rmarkdown_version="), "{html}");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("using latest rmarkdown"),
        "rmarkdown injection should be quiet when rmarkdown is declared\n{}",
        output_text(&out)
    );

    let _ = fs::remove_file(fixture_dir.join("report-pinned.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("report-pinned_files"));
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&fixture_dir);
}

// report-transitive.qmd declares `quarto`, which Imports rmarkdown. The
// resolver sees rmarkdown already in the resolved set and skips its own seed.
#[test]
fn render_quarto_fixture_with_transitive_rmarkdown_renders() {
    let fixture_dir = fixture_copy("run", "ir-e2e-qmd-transitive-fixture");
    let cache_dir = unique_dir("ir-e2e-qmd-transitive-cache");

    let out = ir()
        .current_dir(&fixture_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["render", "--isolated"])
        .arg("report-transitive.qmd")
        .args(["--to", "html"])
        .output()
        .unwrap();

    assert_success(&out);

    let html = fs::read_to_string(fixture_dir.join("report-transitive.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered report: {e}\n{}", output_text(&out)));
    assert!(html.contains("ir.fixture=qmd-transitive"), "{html}");
    // Both the declared `bookdown` and the transitively-pulled rmarkdown must be
    // materialised into the resolved run library, with rmarkdown's version read
    // from that library's DESCRIPTION.
    assert!(html.contains("transitive.bookdown_in_cache=true"), "{html}");
    assert!(
        html.contains("transitive.rmarkdown_in_cache=true"),
        "{html}"
    );
    assert!(html.contains("transitive.rmarkdown_version="), "{html}");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("using latest rmarkdown"),
        "rmarkdown injection should be quiet when rmarkdown is a transitive dependency\n{}",
        output_text(&out)
    );

    let _ = fs::remove_file(fixture_dir.join("report-transitive.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("report-transitive_files"));
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&fixture_dir);
}

// report-bare.qmd declares no dependencies at all, so the resolver must still
// inject rmarkdown quietly for the knitr engine to render.
#[test]
fn render_quarto_bare_fixture_injects_rmarkdown() {
    let fixture_dir = fixture_copy("run", "ir-e2e-qmd-bare-fixture");
    let cache_dir = unique_dir("ir-e2e-qmd-bare-cache");

    for run in ["fresh resolution", "cached resolution"] {
        let out = ir()
            .current_dir(&fixture_dir)
            .env("IR_CACHE_DIR", &cache_dir)
            .args(["render", "--isolated"])
            .arg("report-bare.qmd")
            .args(["--to", "html"])
            .output()
            .unwrap();

        assert_success(&out);

        let html = fs::read_to_string(fixture_dir.join("report-bare.html")).unwrap_or_else(|e| {
            panic!("failed to read rendered report: {e}\n{}", output_text(&out))
        });
        assert!(html.contains("ir.fixture=qmd-bare"), "{html}");
        // The injected rmarkdown must be materialised into the resolved run
        // library, with its version read from that library's DESCRIPTION.
        assert!(html.contains("bare.rmarkdown_in_cache=true"), "{html}");
        assert!(html.contains("bare.rmarkdown_version="), "{html}");

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("using latest rmarkdown"),
            "rmarkdown injection should be quiet for {run}\n{}",
            output_text(&out)
        );
    }

    let _ = fs::remove_file(fixture_dir.join("report-bare.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("report-bare_files"));
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&fixture_dir);
}

#[test]
fn render_quarto_script_fixture_renders_with_dependencies() {
    let fixture_dir = fixture_copy("run", "ir-e2e-render-script-fixture");
    let cache_dir = unique_dir("ir-e2e-render-script-cache");

    let out = ir()
        .current_dir(&fixture_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["render", "--isolated", "--vanilla"])
        .arg("report-script.R")
        .args(["--to", "html"])
        .output()
        .unwrap();

    assert_success(&out);

    let html = fs::read_to_string(fixture_dir.join("report-script.html"))
        .unwrap_or_else(|e| panic!("failed to read rendered report: {e}\n{}", output_text(&out)));
    assert!(html.contains("ir.fixture=render-script"), "{html}");
    assert!(html.contains("render.script.glue_in_cache=true"), "{html}");
    assert!(html.contains("render.script.vanilla=true"), "{html}");
    assert!(html.contains("render.script.result=4"), "{html}");

    let _ = fs::remove_file(fixture_dir.join("report-script.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("report-script_files"));
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&fixture_dir);
}
