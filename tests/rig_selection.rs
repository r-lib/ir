//! Integration tests for the public `ir` CLI.

mod support;

use support::*;

use std::fs;

#[cfg(windows)]
use std::path::PathBuf;

#[test]
fn rig_test_prerequisites_match_ir_test_r_version() {
    let _ = rig_test_r_version("rig_test_prerequisites_match_ir_test_r_version");
}

#[test]
fn r_version_selection_covers_render_flag_and_run_frontmatter() {
    const FIXTURE_R_VERSION: &str = "4.4.3";

    // Opt-in: needs rig plus a non-default R installed (CI provisions both).
    // `ir`'s `--r-version` path resolves through rig unconditionally, so with a
    // single R there is nothing to select. The frontmatter fixture pins 4.4.3,
    // so CI sets the same value to cover both public version-selection paths.
    let Ok(target) = std::env::var("IR_TEST_R_VERSION") else {
        eprintln!(
            "SKIP r_version_selection_covers_render_flag_and_run_frontmatter: set IR_TEST_R_VERSION={FIXTURE_R_VERSION}"
        );
        return;
    };

    if target != FIXTURE_R_VERSION {
        eprintln!(
            "SKIP r_version_selection_covers_render_flag_and_run_frontmatter: IR_TEST_R_VERSION ({target}) must match the fixture's `#| r-version`"
        );
        return;
    }

    // Selecting the version the default path already uses would prove nothing.
    if default_r_version().as_deref() == Some(FIXTURE_R_VERSION) {
        eprintln!(
            "SKIP r_version_selection_covers_render_flag_and_run_frontmatter: the fixture's R ({FIXTURE_R_VERSION}) matches the default R; nothing to select"
        );
        return;
    }

    let fixture_dir = fixture_copy("run", "ir-r-version-render-fixture");
    let cache_dir = test_cache("ir-r-version-cache");

    let render = ir()
        .current_dir(&fixture_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["render", "--isolated", "--r-version"])
        .arg(&target)
        .arg("r-version-select.qmd")
        .args(["--to", "html"])
        .output()
        .unwrap();

    assert_success(&render);

    let html = fs::read_to_string(fixture_dir.join("r-version-select.html")).unwrap_or_else(|e| {
        panic!(
            "failed to read rendered report: {e}\n{}",
            output_text(&render)
        )
    });
    assert!(html.contains("ir.fixture=r-version"), "{html}");
    assert!(
        html.contains(&format!("version.r_version=[{target}]")),
        "rendered under a different R than the requested {target}\n{html}"
    );
    assert!(html.contains("version.lib_in_cache=true"), "{html}");
    assert!(html.contains("version.jsonlite_in_cache=true"), "{html}");

    let script = fixture("run/r-version-frontmatter.R");

    let run = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&run);
    assert_stdout_contains(&run, "ir.fixture=r-version-frontmatter");
    assert_stdout_contains(&run, &format!("version.r_version=[{FIXTURE_R_VERSION}]"));
    assert_stdout_contains(&run, "version.lib_in_cache=true");
    assert_stdout_contains(&run, "version.jsonlite_in_cache=true");

    let _ = fs::remove_file(fixture_dir.join("r-version-select.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("r-version-select_files"));
    let _ = fs::remove_dir_all(&fixture_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[cfg(unix)]
#[test]
fn run_with_r_version_selects_highest_matching_installed_r() {
    let cache_dir = unique_dir("ir-r-version-cache");
    let bin_dir = unique_dir("ir-r-version-bin");
    let old_r_dir = unique_dir("ir-r-version-old");
    let new_r_dir = unique_dir("ir-r-version-new");

    let old_binary = old_r_dir.join("R");
    let old_rscript = old_r_dir.join("Rscript");
    write_executable(
        &old_rscript,
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=old\n",
        ),
    );

    let new_binary = new_r_dir.join("R");
    let new_rscript = new_r_dir.join("Rscript");
    write_executable(
        &new_rscript,
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=new\n",
        ),
    );

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "cat <<'JSON'\n",
                r#"[
{{"name":"4.4.2","version":"4.4.2","aliases":[],"binary":"{}"}},
{{"name":"4.4.3","version":"4.4.3","aliases":[],"binary":"{}"}}
]"#,
                "\nJSON\n",
            ),
            old_binary.display(),
            new_binary.display()
        ),
    );

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "4.4", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=new");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&old_r_dir);
    let _ = fs::remove_dir_all(&new_r_dir);
}

#[cfg(unix)]
#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn run_with_exclude_newer_uses_embedded_metadata_on_common_target() {
    let cache_dir = unique_dir("ir-r-version-embedded-common-cache");
    let bin_dir = unique_dir("ir-r-version-embedded-common-bin");
    let script = unique_path("ir-r-version-embedded-common", "R");

    fs::write(
        &script,
        concat!(
            "#| r-version: \"4.4.3\"\n",
            "#| exclude-newer: 2025-03-01\n",
            "cat('unused')\n",
        ),
    )
    .unwrap();

    write_executable(
        &bin_dir.join("rig"),
        concat!(
            "#!/bin/sh\n",
            "if [ \"$1\" = \"list\" ] && [ \"$2\" = \"--json\" ]; then\n",
            "  printf '[]\\n'\n",
            "  exit 0\n",
            "fi\n",
            "echo \"unexpected rig args: $*\" >&2\n",
            "exit 43\n",
        ),
    );
    let path = std::env::join_paths([bin_dir.as_os_str().to_owned()]).unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .arg("run")
        .arg(&script)
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    assert!(
        output_text(&out)
            .contains("R 4.4.3 is required but is not installed. Run `rig install 4.4.3`."),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_preserves_available_patch_releases() {
    let cache_dir = unique_dir("ir-r-version-embedded-all-cache");
    let bin_dir = unique_dir("ir-r-version-embedded-all-bin");
    let script = unique_path("ir-r-version-embedded-all", "R");

    fs::write(
        &script,
        concat!(
            "#| r-version: \"4.4.3\"\n",
            "#| exclude-newer: 2026-06-01\n",
            "cat('unused')\n",
        ),
    )
    .unwrap();

    write_executable(
        &bin_dir.join("rig"),
        concat!(
            "#!/bin/sh\n",
            "if [ \"$1\" = \"list\" ] && [ \"$2\" = \"--json\" ]; then\n",
            "  printf '[]\\n'\n",
            "  exit 0\n",
            "fi\n",
            "if [ \"$1\" = \"available\" ] && [ \"$2\" = \"--json\" ] && [ \"$3\" = \"--all\" ]; then\n",
            "  printf '%s\\n' '[{\"name\":\"4.4.3\",\"version\":\"4.4.3\",\"date\":\"2025-02-28T00:00:00Z\"}]'\n",
            "  exit 0\n",
            "fi\n",
            "echo \"unexpected rig args: $*\" >&2\n",
            "exit 43\n",
        ),
    );
    let path = std::env::join_paths([bin_dir.as_os_str().to_owned()]).unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .arg("run")
        .arg(&script)
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    assert!(
        output_text(&out)
            .contains("R 4.4.3 is required but is not installed. Run `rig install 4.4.3`."),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_uses_platform_available_metadata_for_embedded_dates() {
    let cache_dir = unique_dir("ir-r-version-platform-available-cache");
    let bin_dir = unique_dir("ir-r-version-platform-available-bin");
    let script = unique_path("ir-r-version-platform-available", "R");

    fs::write(
        &script,
        concat!(
            "#| r-version: \"3.6\"\n",
            "#| exclude-newer: 2021-01-01\n",
            "cat('unused')\n",
        ),
    )
    .unwrap();

    write_executable(
        &bin_dir.join("rig"),
        concat!(
            "#!/bin/sh\n",
            "if [ \"$1\" = \"list\" ] && [ \"$2\" = \"--json\" ]; then\n",
            "  printf '[]\\n'\n",
            "  exit 0\n",
            "fi\n",
            "if [ \"$1\" = \"available\" ] && [ \"$2\" = \"--json\" ] && [ \"$3\" = \"--all\" ]; then\n",
            "  printf '%s\\n' '[{\"name\":\"4.1\",\"version\":\"4.1.0\",\"date\":\"2021-05-18T00:00:00Z\"}]'\n",
            "  exit 0\n",
            "fi\n",
            "echo \"unexpected rig args: $*\" >&2\n",
            "exit 43\n",
        ),
    );
    let path = std::env::join_paths([bin_dir.as_os_str().to_owned()]).unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .arg("run")
        .arg(&script)
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    assert!(
        output_text(&out).contains(
            "could not resolve R version `3.6` with available R versions before or on 2021-01-01"
        ),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_after_embedded_metadata_refreshes_release_dates() {
    let cache_dir = unique_dir("ir-r-version-future-available-cache");
    let bin_dir = unique_dir("ir-r-version-future-available-bin");
    let script = unique_path("ir-r-version-future-available", "R");
    let metadata_calls = unique_path("ir-r-version-future-available-calls", "txt");

    fs::write(
        &script,
        concat!(
            "#| r-version: \"4.7\"\n",
            "#| exclude-newer: 2999-12-31\n",
            "cat('unused')\n",
        ),
    )
    .unwrap();

    write_executable(
        &bin_dir.join("rig"),
        concat!(
            "#!/bin/sh\n",
            "if [ \"$1\" = \"list\" ] && [ \"$2\" = \"--json\" ]; then\n",
            "  printf '[]\\n'\n",
            "  exit 0\n",
            "fi\n",
            "if [ \"$1\" = \"available\" ] && [ \"$2\" = \"--json\" ] && [ \"$3\" = \"--all\" ]; then\n",
            "  printf 'metadata\\n' >> \"$IR_TEST_R_VERSION_METADATA_CALLS\"\n",
            "  printf '%s\\n' '[{\"name\":\"4.7\",\"version\":\"4.7.0\",\"date\":\"2999-12-31T00:00:00Z\"}]'\n",
            "  exit 0\n",
            "fi\n",
            "echo \"unexpected rig args: $*\" >&2\n",
            "exit 43\n",
        ),
    );
    let path = std::env::join_paths([bin_dir.as_os_str().to_owned()]).unwrap();

    for _ in 0..2 {
        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .env("PATH", &path)
            .env("IR_TEST_R_VERSION_METADATA_CALLS", &metadata_calls)
            .env_remove("IR_RSCRIPT")
            .arg("run")
            .arg(&script)
            .output()
            .unwrap();

        assert!(!out.status.success(), "{}", output_text(&out));
        assert!(
            output_text(&out)
                .contains("R 4.7.0 is required but is not installed. Run `rig install 4.7`."),
            "{}",
            output_text(&out)
        );
    }

    let calls = fs::read_to_string(&metadata_calls).unwrap();
    assert_eq!(
        calls.lines().count(),
        1,
        "R version metadata should be cached after the first refreshed lookup"
    );

    let _ = fs::remove_file(&script);
    let _ = fs::remove_file(&metadata_calls);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(unix)]
#[test]
fn run_without_r_version_uses_rscript_on_path_when_rig_has_default() {
    let cache_dir = unique_dir("ir-path-rscript-cache");
    let bin_dir = unique_dir("ir-path-rscript-bin");
    let rig_dir = unique_dir("ir-path-rscript-rig");

    let path_rscript = bin_dir.join("Rscript");
    write_executable(
        &path_rscript,
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=path\n",
        ),
    );

    let rig_binary = rig_dir.join("R");
    let rig_rscript = rig_dir.join("Rscript");
    write_executable(
        &rig_rscript,
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=rig\n",
        ),
    );
    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "cat <<'JSON'\n",
                r#"[{{"name":"rig-default","version":"4.4.3","aliases":[],"default":true,"binary":"{}"}}]"#,
                "\nJSON\n",
            ),
            rig_binary.display()
        ),
    );

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=path");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&rig_dir);
}

#[cfg(unix)]
#[test]
fn run_without_r_version_skips_non_executable_rscript_on_path() {
    let cache_dir = unique_dir("ir-path-rscript-executable-cache");
    let stale_dir = unique_dir("ir-path-rscript-stale-bin");
    let bin_dir = unique_dir("ir-path-rscript-valid-bin");

    fs::write(stale_dir.join("Rscript"), "not executable\n").unwrap();
    write_executable(
        &bin_dir.join("Rscript"),
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=path\n",
        ),
    );

    let path = std::env::join_paths(
        [
            stale_dir.as_os_str().to_owned(),
            bin_dir.as_os_str().to_owned(),
        ]
        .into_iter()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=path");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&stale_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(unix)]
#[test]
fn render_without_r_version_pins_quarto_to_rscript_on_path() {
    let cache_dir = unique_dir("ir-render-path-rscript-cache");
    let bin_dir = unique_dir("ir-render-path-rscript-bin");
    let doc = unique_path("ir-render-path-rscript", "qmd");

    let rscript = bin_dir.join("Rscript");
    write_executable(
        &rscript,
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=path\n",
        ),
    );
    write_executable(
        &bin_dir.join("quarto"),
        concat!(
            "#!/bin/sh\n",
            "if [ \"${QUARTO_R:-}\" != \"$IR_EXPECTED_QUARTO_R\" ]; then\n",
            "  echo \"QUARTO_R=${QUARTO_R:-}\"\n",
            "  echo \"expected=$IR_EXPECTED_QUARTO_R\"\n",
            "  exit 2\n",
            "fi\n",
            "echo quarto_r=$QUARTO_R\n",
        ),
    );
    fs::write(&doc, "---\ntitle: render path rscript\n---\n").unwrap();
    let expected_rscript = fs::canonicalize(&rscript).unwrap();

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env("IR_EXPECTED_QUARTO_R", &expected_rscript)
        .env_remove("IR_RSCRIPT")
        .arg("render")
        .arg(&doc)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, &format!("quarto_r={}", expected_rscript.display()));

    let _ = fs::remove_file(&doc);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn run_without_r_version_uses_rscript_bat_on_path() {
    let cache_dir = unique_dir("ir-path-rscript-bat-cache");
    let bin_dir = unique_dir("ir-path-rscript-bat-bin");

    fs::write(
        bin_dir.join("Rscript.bat"),
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  type NUL > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
            "  exit /B 0\r\n",
            ")\r\n",
            "echo selected=bat\r\n",
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=bat");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn run_without_r_version_ignores_extensionless_rscript_on_path() {
    let cache_dir = unique_dir("ir-path-rscript-extensionless-cache");
    let stale_dir = unique_dir("ir-path-rscript-extensionless-stale");
    let bin_dir = unique_dir("ir-path-rscript-extensionless-valid");

    fs::write(stale_dir.join("Rscript"), "extensionless stub\r\n").unwrap();
    fs::write(
        bin_dir.join("Rscript.bat"),
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  type NUL > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
            "  exit /B 0\r\n",
            ")\r\n",
            "echo selected=bat\r\n",
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        [
            stale_dir.as_os_str().to_owned(),
            bin_dir.as_os_str().to_owned(),
        ]
        .into_iter()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=bat");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&stale_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn run_without_r_version_skips_unsupported_pathext_rscript_on_path() {
    let cache_dir = unique_dir("ir-path-rscript-unsupported-pathext-cache");
    let stale_dir = unique_dir("ir-path-rscript-unsupported-pathext-stale");
    let bin_dir = unique_dir("ir-path-rscript-unsupported-pathext-valid");

    fs::write(stale_dir.join("Rscript.JS"), "WScript.Echo('stale')\r\n").unwrap();
    fs::write(
        bin_dir.join("Rscript.bat"),
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  type NUL > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
            "  exit /B 0\r\n",
            ")\r\n",
            "echo selected=bat\r\n",
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        [
            stale_dir.as_os_str().to_owned(),
            bin_dir.as_os_str().to_owned(),
        ]
        .into_iter()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env("PATHEXT", ".JS;.BAT")
        .env_remove("IR_RSCRIPT")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=bat");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&stale_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn run_with_extended_rscript_command_skips_pathext_expansion() {
    let cache_dir = unique_dir("ir-extended-rscript-command-cache");
    let stale_dir = unique_dir("ir-extended-rscript-command-stale");
    let bin_dir = unique_dir("ir-extended-rscript-command-valid");

    fs::write(
        stale_dir.join("Rscript.bat.CMD"),
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  type NUL > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
            "  exit /B 0\r\n",
            ")\r\n",
            "echo selected=cmd\r\n",
        ),
    )
    .unwrap();
    fs::write(
        bin_dir.join("Rscript.bat"),
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  type NUL > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
            "  exit /B 0\r\n",
            ")\r\n",
            "echo selected=bat\r\n",
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        [
            stale_dir.as_os_str().to_owned(),
            bin_dir.as_os_str().to_owned(),
        ]
        .into_iter()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", "Rscript.bat")
        .env("PATH", path)
        .env("PATHEXT", ".CMD")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=bat");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&stale_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn run_without_r_version_ignores_non_rscript_batch_targets() {
    let cache_dir = unique_dir("ir-path-rscript-helper-target-cache");
    let bin_dir = unique_dir("ir-path-rscript-helper-target-bin");
    let helper = bin_dir.join("helper.exe");

    fs::write(&helper, "not an executable\r\n").unwrap();
    fs::write(
        bin_dir.join("Rscript.bat"),
        format!(
            concat!(
                "@echo off\r\n",
                "\"{}\"\r\n",
                "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
                "  type NUL > \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
                "  exit /B 0\r\n",
                ")\r\n",
                "echo selected=bat\r\n",
            ),
            helper.display()
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=bat");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn run_without_r_version_does_not_cache_unresolved_rscript_bat() {
    let cache_dir = unique_dir("ir-path-rscript-bat-cache-miss");
    let bin_dir = unique_dir("ir-path-rscript-bat-bin");
    let library = unique_dir("ir-path-rscript-bat-library");
    let resolver_marker = unique_path("ir-path-rscript-bat-resolver", "txt");
    let resolver_script = bin_dir.join("resolve.ps1");

    fs::write(
        &resolver_script,
        concat!(
            "$library = $env:IR_TEST_LIBRARY\n",
            "New-Item -ItemType Directory -Force -Path $library | Out-Null\n",
            "Add-Content -Path $env:IR_TEST_RESOLVER_MARKER -Value 'resolve'\n",
            "if ($env:IR_RESOLUTION_MARKER) {\n",
            "  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $env:IR_RESOLUTION_MARKER) | Out-Null\n",
            "  $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()\n",
            "  Set-Content -Path $env:IR_RESOLUTION_MARKER -Value @(\"latest: $now\", $library)\n",
            "}\n",
            "Set-Content -Path $env:IR_RESOLVE_RESULT_FILE -Value $library\n",
        ),
    )
    .unwrap();
    fs::write(
        bin_dir.join("Rscript.bat"),
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\"==\"\" (\r\n",
            "  powershell -NoProfile -ExecutionPolicy Bypass -File \"%IR_TEST_RESOLVER_SCRIPT%\"\r\n",
            "  exit /B %ERRORLEVEL%\r\n",
            ")\r\n",
            "echo selected=bat\r\n",
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    for _ in 0..2 {
        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .env("PATH", &path)
            .env("IR_TEST_LIBRARY", &library)
            .env("IR_TEST_RESOLVER_MARKER", &resolver_marker)
            .env("IR_TEST_RESOLVER_SCRIPT", &resolver_script)
            .env_remove("IR_RSCRIPT")
            .args(["run", "--with", "cli", "-e", "cat('ignored')"])
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, "selected=bat");
    }

    let resolver_runs = fs::read_to_string(&resolver_marker).unwrap();
    assert_eq!(
        resolver_runs.lines().count(),
        2,
        "unresolved batch Rscript wrappers should not key the warm resolution cache"
    );

    let _ = fs::remove_file(&resolver_marker);
    let _ = fs::remove_dir_all(&library);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(windows)]
#[test]
fn render_without_r_version_pins_quarto_to_rscript_bat_target() {
    let cache_dir = unique_dir("ir-render-rscript-bat-target-cache");
    let bin_dir = unique_dir("ir-render-rscript-bat-target-bin");
    let doc = unique_path("ir-render-rscript-bat-target", "qmd");
    let target_rscript = PathBuf::from(rscript());

    if !target_rscript.is_file() {
        eprintln!(
            "SKIP render_without_r_version_pins_quarto_to_rscript_bat_target: default test Rscript is not a path"
        );
        return;
    }
    let expected_rscript = std::path::absolute(&target_rscript).unwrap();

    fs::write(
        bin_dir.join("Rscript.bat"),
        format!(
            "::test\r\n@echo off\r\n@\"{}\" %*\r\n",
            target_rscript.display()
        ),
    )
    .unwrap();
    fs::write(
        bin_dir.join("quarto.bat"),
        concat!(
            "@echo off\r\n",
            "if \"%QUARTO_R%\"==\"%IR_EXPECTED_QUARTO_R%\" (\r\n",
            "  echo quarto_r=%QUARTO_R%\r\n",
            "  exit /B 0\r\n",
            ")\r\n",
            "echo QUARTO_R=%QUARTO_R%\r\n",
            "echo expected=%IR_EXPECTED_QUARTO_R%\r\n",
            "exit /B 2\r\n",
        ),
    )
    .unwrap();
    fs::write(
        &doc,
        "---\ntitle: render batch target\nir:\n  exclude-newer: 2026-06-01\n---\n",
    )
    .unwrap();

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path)
        .env("IR_QUARTO", bin_dir.join("quarto.bat"))
        .env("IR_EXPECTED_QUARTO_R", &expected_rscript)
        .env_remove("IR_RSCRIPT")
        .arg("render")
        .arg(&doc)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, &format!("quarto_r={}", expected_rscript.display()));

    let _ = fs::remove_file(&doc);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}
