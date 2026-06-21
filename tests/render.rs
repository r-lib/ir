//! Integration tests for the public `ir` CLI.

mod support;

use support::*;

use std::fs;
use std::process::Command;

#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::path::Path;

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

#[cfg(unix)]
#[test]
fn render_quarto_uv_frontmatter_sets_quarto_python() {
    let cache_dir = unique_dir("ir-render-uv-cache");
    let bin_dir = unique_dir("ir-render-uv-bin");
    let doc = unique_path("ir-render-uv", "qmd");
    let fake_python = bin_dir.join("python");
    let rscript = bin_dir.join("Rscript");
    let quarto = bin_dir.join("quarto");
    let r_deps = unique_path("ir-render-uv-r-deps", "txt");
    let r_driver = unique_path("ir-render-uv-r-driver", "txt");
    let py_driver = unique_path("ir-render-uv-py-driver", "txt");
    let uv_packages = unique_path("ir-render-uv-packages", "txt");
    let uv_env = unique_path("ir-render-uv-env", "txt");

    fs::write(
        &doc,
        r#"---
title: uv render
format: html
jupyter: python3
uv:
  packages:
    - pandas
  python-version: "3.11"
  exclude-newer: "2026-06-01"
---

```{python}
print("ok")
```
"#,
    )
    .unwrap();
    write_executable(&fake_python, "#!/bin/sh\nexit 0\n");
    write_executable(
        &rscript,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  printf '%s\\n' \"$1\" > {}\n\
  cat > {}\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
if [ -n \"${{IR_PYTHON_RESULT_FILE:-}}\" ]; then\n\
  printf '%s\\n' \"$1\" > {}\n\
  if grep -q 'ir_ensure_python_pak' \"$1\"; then\n\
    echo python resolver should use shared tooling bootstrap >&2\n\
    exit 1\n\
  fi\n\
  if grep -q 'ir_ensure_python_tooling' \"$1\"; then\n\
    echo python resolver should add reticulate through shared tooling >&2\n\
    exit 1\n\
  fi\n\
  if ! grep -q 'ir_ensure_tooling' \"$1\"; then\n\
    echo python resolver should include shared tooling bootstrap >&2\n\
    exit 1\n\
  fi\n\
  cat > {}\n\
  printf 'python_version=%s\\n' \"${{IR_UV_PYTHON_VERSION:-}}\" > {}\n\
  printf 'exclude_newer=%s\\n' \"${{IR_UV_EXCLUDE_NEWER:-}}\" >> {}\n\
  printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
echo unexpected Rscript invocation >&2\n\
exit 1\n",
            r_driver.display(),
            r_deps.display(),
            py_driver.display(),
            uv_packages.display(),
            uv_env.display(),
            uv_env.display(),
            fake_python.display()
        ),
    );
    write_executable(
        &quarto,
        "#!/bin/sh\nprintf 'quarto_python=%s\\n' \"$QUARTO_PYTHON\"\nprintf 'reticulate_python=%s\\n' \"$RETICULATE_PYTHON\"\n",
    );
    let expected_driver_dir = cache_dir.join("drivers");
    let expected_r_driver = expected_driver_dir.join("resolve.R");
    let expected_py_driver = expected_driver_dir.join("resolve-python.R");
    fs::create_dir_all(&expected_driver_dir).unwrap();
    fs::write(&expected_r_driver, "stale").unwrap();
    fs::write(&expected_py_driver, "stale").unwrap();
    fs::write(expected_driver_dir.join("resolve.R.version"), "old").unwrap();
    fs::write(expected_driver_dir.join("resolve-python.R.version"), "old").unwrap();
    let mut permissions = fs::metadata(&expected_r_driver).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&expected_r_driver, permissions).unwrap();
    let mut permissions = fs::metadata(&expected_py_driver).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&expected_py_driver, permissions).unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_QUARTO", &quarto)
        .args(["render", "--rscript"])
        .arg(&rscript)
        .arg(&doc)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, &format!("quarto_python={}", fake_python.display()));
    assert_stdout_contains(
        &out,
        &format!("reticulate_python={}", fake_python.display()),
    );

    let deps = fs::read_to_string(&r_deps).unwrap();
    assert!(
        !deps.lines().any(|line| line == "reticulate"),
        "uv resolution should not inject user-library reticulate\n{deps}"
    );
    let r_driver_path = Path::new(fs::read_to_string(&r_driver).unwrap().trim()).to_path_buf();
    let py_driver_path = Path::new(fs::read_to_string(&py_driver).unwrap().trim()).to_path_buf();
    assert_eq!(r_driver_path, expected_r_driver);
    assert_eq!(py_driver_path, expected_py_driver);
    assert!(fs::read_to_string(&r_driver_path)
        .unwrap()
        .contains("ir_ensure_tooling"));
    assert!(fs::read_to_string(&py_driver_path)
        .unwrap()
        .contains("ir_ensure_tooling"));
    assert_eq!(
        fs::read_to_string(expected_driver_dir.join("resolve.R.version")).unwrap(),
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(
        fs::read_to_string(expected_driver_dir.join("resolve-python.R.version")).unwrap(),
        env!("CARGO_PKG_VERSION")
    );
    assert!(fs::metadata(&r_driver_path)
        .unwrap()
        .permissions()
        .readonly());
    assert!(fs::metadata(&py_driver_path)
        .unwrap()
        .permissions()
        .readonly());

    let packages = fs::read_to_string(&uv_packages).unwrap();
    assert!(packages.contains("pandas"), "{packages}");
    assert!(packages.contains("jupyter"), "{packages}");

    let env = fs::read_to_string(&uv_env).unwrap();
    assert!(env.contains("python_version=3.11"), "{env}");
    assert!(env.contains("exclude_newer=2026-06-01"), "{env}");

    let _ = fs::remove_file(&doc);
    let _ = fs::remove_file(&r_deps);
    let _ = fs::remove_file(&r_driver);
    let _ = fs::remove_file(&py_driver);
    let _ = fs::remove_file(&uv_packages);
    let _ = fs::remove_file(&uv_env);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(unix)]
#[test]
fn render_quarto_uv_frontmatter_clears_ambient_internal_uv_env() {
    let cache_dir = unique_dir("ir-render-uv-env-cache");
    let bin_dir = unique_dir("ir-render-uv-env-bin");
    let doc = unique_path("ir-render-uv-env", "qmd");
    let fake_python = bin_dir.join("python");
    let rscript = bin_dir.join("Rscript");
    let quarto = bin_dir.join("quarto");
    let uv_env = unique_path("ir-render-uv-env-observed", "txt");

    fs::write(
        &doc,
        r#"---
title: uv render env
format: html
jupyter: python3
uv:
  packages:
    - pandas
---
"#,
    )
    .unwrap();
    write_executable(&fake_python, "#!/bin/sh\nexit 0\n");
    write_executable(
        &rscript,
        &format!(
            "#!/bin/sh\n\
if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n\
  cat > /dev/null\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
if [ -n \"${{IR_PYTHON_RESULT_FILE:-}}\" ]; then\n\
  cat > /dev/null\n\
  printf 'python_version=%s\\n' \"${{IR_UV_PYTHON_VERSION:-}}\" > {}\n\
  printf 'exclude_newer=%s\\n' \"${{IR_UV_EXCLUDE_NEWER:-}}\" >> {}\n\
  printf '%s\\n' {} > \"$IR_PYTHON_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
echo unexpected Rscript invocation >&2\n\
exit 1\n",
            uv_env.display(),
            uv_env.display(),
            fake_python.display()
        ),
    );
    write_executable(&quarto, "#!/bin/sh\nexit 0\n");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_QUARTO", &quarto)
        .env("IR_UV_PYTHON_VERSION", "9.99")
        .env("IR_UV_EXCLUDE_NEWER", "1999-01-01")
        .args(["render", "--rscript"])
        .arg(&rscript)
        .arg(&doc)
        .output()
        .unwrap();

    assert_success(&out);
    let env = fs::read_to_string(&uv_env).unwrap();
    assert!(env.contains("python_version=\n"), "{env}");
    assert!(env.contains("exclude_newer=\n"), "{env}");

    let _ = fs::remove_file(&doc);
    let _ = fs::remove_file(&uv_env);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
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
