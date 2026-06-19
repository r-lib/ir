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
fn selected_r_binary(dir: &std::path::Path, label: &str) -> std::path::PathBuf {
    let binary = dir.join("R");
    write_executable(
        &dir.join("Rscript"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n",
                "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
                "  exit 0\n",
                "fi\n",
                "echo selected={}\n",
            ),
            label
        ),
    );
    binary
}

#[cfg(unix)]
fn path_with_bin_dir(bin_dir: &std::path::Path) -> std::ffi::OsString {
    std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap()
}

#[cfg(unix)]
fn write_selected_rscript(path: &std::path::Path, label: &str) {
    write_executable(
        path,
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n",
                "  if [ -n \"${{IR_TEST_EXPECT_EXCLUDE_NEWER:-}}\" ] && [ \"${{IR_EXCLUDE_NEWER:-}}\" != \"$IR_TEST_EXPECT_EXCLUDE_NEWER\" ]; then\n",
                "    echo \"unexpected exclude-newer: $IR_EXCLUDE_NEWER\" >&2\n",
                "    exit 66\n",
                "  fi\n",
                "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
                "  exit 0\n",
                "fi\n",
                "echo selected={}\n",
            ),
            label
        ),
    );
}

#[cfg(unix)]
#[test]
fn run_with_r_version_selects_highest_matching_installed_r() {
    let cache_dir = unique_dir("ir-r-version-cache");
    let bin_dir = unique_dir("ir-r-version-bin");
    let old_r_dir = unique_dir("ir-r-version-old");
    let new_r_dir = unique_dir("ir-r-version-new");

    let old_binary = selected_r_binary(&old_r_dir, "old");
    let new_binary = selected_r_binary(&new_r_dir, "new");

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

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
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
fn run_with_major_r_version_selects_highest_installed_minor() {
    let cache_dir = unique_dir("ir-major-version-cache");
    let bin_dir = unique_dir("ir-major-version-bin");
    let r43_dir = unique_dir("ir-major-version-r43");
    let r45_dir = unique_dir("ir-major-version-r45");

    let r43_binary = selected_r_binary(&r43_dir, "r43");
    let r45_binary = selected_r_binary(&r45_dir, "r45");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "cat <<'JSON'\n",
                r#"[
{{"name":"4.3.3","version":"4.3.3","aliases":[],"binary":"{}"}},
{{"name":"4.5.0","version":"4.5.0","aliases":[],"binary":"{}"}}
]"#,
                "\nJSON\n",
            ),
            r43_binary.display(),
            r45_binary.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "4", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=r45");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&r43_dir);
    let _ = fs::remove_dir_all(&r45_dir);
}

#[cfg(unix)]
#[test]
fn run_with_exact_major_r_version_selects_highest_installed_minor() {
    let cache_dir = unique_dir("ir-exact-major-highest-cache");
    let bin_dir = unique_dir("ir-exact-major-highest-bin");
    let r43_dir = unique_dir("ir-exact-major-highest-r43");
    let r45_dir = unique_dir("ir-exact-major-highest-r45");

    let r43_binary = selected_r_binary(&r43_dir, "r43");
    let r45_binary = selected_r_binary(&r45_dir, "r45");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "cat <<'JSON'\n",
                r#"[
{{"name":"4.3.3","version":"4.3.3","aliases":[],"binary":"{}"}},
{{"name":"4.5.0","version":"4.5.0","aliases":[],"binary":"{}"}}
]"#,
                "\nJSON\n",
            ),
            r43_binary.display(),
            r45_binary.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "== 4", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=r45");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&r43_dir);
    let _ = fs::remove_dir_all(&r45_dir);
}

#[cfg(unix)]
#[test]
fn run_with_exact_minor_r_version_selects_highest_installed_patch() {
    let cache_dir = unique_dir("ir-exact-minor-highest-cache");
    let bin_dir = unique_dir("ir-exact-minor-highest-bin");
    let old_r_dir = unique_dir("ir-exact-minor-highest-old");
    let new_r_dir = unique_dir("ir-exact-minor-highest-new");

    let old_binary = selected_r_binary(&old_r_dir, "old");
    let new_binary = selected_r_binary(&new_r_dir, "new");

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

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "== 4.4", "-e", "cat('ignored')"])
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
fn run_with_exact_minor_r_version_uses_only_installed_matching_patch() {
    let cache_dir = unique_dir("ir-exact-minor-only-cache");
    let bin_dir = unique_dir("ir-exact-minor-only-bin");
    let old_r_dir = unique_dir("ir-exact-minor-only-old");

    let old_binary = selected_r_binary(&old_r_dir, "old");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "cat <<'JSON'\n",
                r#"[
{{"name":"4.4.2","version":"4.4.2","aliases":[],"binary":"{}"}}
]"#,
                "\nJSON\n",
            ),
            old_binary.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "== 4.4", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=old");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&old_r_dir);
}

#[cfg(unix)]
#[test]
fn run_with_exact_minor_r_version_errors_when_no_installed_patch_matches() {
    let cache_dir = unique_dir("ir-exact-minor-missing-cache");
    let bin_dir = unique_dir("ir-exact-minor-missing-bin");
    let r43_dir = unique_dir("ir-exact-minor-missing-r43");
    let r45_dir = unique_dir("ir-exact-minor-missing-r45");

    let r43_binary = selected_r_binary(&r43_dir, "r43");
    let r45_binary = selected_r_binary(&r45_dir, "r45");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "case \"$1 $2\" in\n",
                "  \"list --json\")\n",
                "    cat <<'JSON'\n",
                r#"[
{{"name":"4.3.3","version":"4.3.3","aliases":[],"binary":"{}"}},
{{"name":"4.5.0","version":"4.5.0","aliases":[],"binary":"{}"}}
]"#,
                "\nJSON\n",
                "    ;;\n",
                "  \"available --json\") echo unexpected available >&2; exit 65 ;;\n",
                "  *) exit 64 ;;\n",
                "esac\n",
            ),
            r43_binary.display(),
            r45_binary.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "== 4.4", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("R 4.4 is required"),
        "{}",
        output_text(&out)
    );
    assert!(
        stderr.contains("Run `rig install 4.4`"),
        "{}",
        output_text(&out)
    );
    assert!(
        !stderr.contains("unexpected available"),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&r43_dir);
    let _ = fs::remove_dir_all(&r45_dir);
}

#[cfg(unix)]
#[test]
fn run_with_exact_major_r_version_errors_when_no_installed_minor_matches() {
    let cache_dir = unique_dir("ir-exact-major-missing-cache");
    let bin_dir = unique_dir("ir-exact-major-missing-bin");
    let r3_dir = unique_dir("ir-exact-major-missing-r3");
    let r5_dir = unique_dir("ir-exact-major-missing-r5");

    let r3_binary = selected_r_binary(&r3_dir, "r3");
    let r5_binary = selected_r_binary(&r5_dir, "r5");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "case \"$1 $2\" in\n",
                "  \"list --json\")\n",
                "    cat <<'JSON'\n",
                r#"[
{{"name":"3.6.3","version":"3.6.3","aliases":[],"binary":"{}"}},
{{"name":"5.0.0","version":"5.0.0","aliases":[],"binary":"{}"}}
]"#,
                "\nJSON\n",
                "    ;;\n",
                "  \"available --json\") echo unexpected available >&2; exit 65 ;;\n",
                "  *) exit 64 ;;\n",
                "esac\n",
            ),
            r3_binary.display(),
            r5_binary.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "== 4", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("R 4 is required"), "{}", output_text(&out));
    assert!(
        stderr.contains("Run `rig install 4`"),
        "{}",
        output_text(&out)
    );
    assert!(
        !stderr.contains("unexpected available"),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&r3_dir);
    let _ = fs::remove_dir_all(&r5_dir);
}

#[cfg(unix)]
#[test]
fn run_with_missing_r_version_does_not_query_available_releases() {
    let cache_dir = unique_dir("ir-r-version-missing-cache");
    let bin_dir = unique_dir("ir-r-version-missing-bin");

    write_executable(
        &bin_dir.join("rig"),
        concat!(
            "#!/bin/sh\n",
            "case \"$1 $2\" in\n",
            "  \"list --json\") echo '[]' ;;\n",
            "  \"available --json\") echo unexpected available >&2; exit 65 ;;\n",
            "  *) exit 64 ;;\n",
            "esac\n",
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "4.4", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("R 4.4 is required"),
        "{}",
        output_text(&out)
    );
    assert!(
        stderr.contains("Run `rig install 4.4`"),
        "{}",
        output_text(&out)
    );
    assert!(
        !stderr.contains("unexpected available"),
        "{}",
        output_text(&out)
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "== 4.4", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("R 4.4 is required"),
        "{}",
        output_text(&out)
    );
    assert!(
        stderr.contains("Run `rig install 4.4`"),
        "{}",
        output_text(&out)
    );
    assert!(
        !stderr.contains("unexpected available"),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_selects_latest_available_minor_r() {
    let cache_dir = unique_dir("ir-exclude-newer-r-cache");
    let bin_dir = unique_dir("ir-exclude-newer-r-bin");
    let r43_dir = unique_dir("ir-exclude-newer-r43");
    let r44_dir = unique_dir("ir-exclude-newer-r44");

    let r43_binary = selected_r_binary(&r43_dir, "r43");
    let r44_binary = selected_r_binary(&r44_dir, "r44");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "case \"$1\" in\n",
                "  list)\n",
                "    cat <<'JSON'\n",
                r#"[
{{"name":"4.3.3","version":"4.3.3","aliases":[],"binary":"{}"}},
{{"name":"4.4.3","version":"4.4.3","aliases":[],"binary":"{}"}}
]"#,
                "\nJSON\n",
                "    ;;\n",
                "  available) echo unexpected available >&2; exit 65 ;;\n",
                "  *) exit 64 ;;\n",
                "esac\n",
            ),
            r43_binary.display(),
            r44_binary.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args([
            "run",
            "--exclude-newer",
            "2024-03-15",
            "-e",
            "cat('ignored')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=r43");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&r43_dir);
    let _ = fs::remove_dir_all(&r44_dir);
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_selects_r_4_0_for_2021_snapshot() {
    let cache_dir = unique_dir("ir-exclude-newer-r40-cache");
    let bin_dir = unique_dir("ir-exclude-newer-r40-bin");
    let r40_dir = unique_dir("ir-exclude-newer-r40");
    let r41_dir = unique_dir("ir-exclude-newer-r41");

    let r40_binary = selected_r_binary(&r40_dir, "r40");
    let r41_binary = selected_r_binary(&r41_dir, "r41");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "case \"$1\" in\n",
                "  list)\n",
                "    cat <<'JSON'\n",
                r#"[
{{"name":"4.0.5","version":"4.0.5","aliases":[],"binary":"{}"}},
{{"name":"4.1.3","version":"4.1.3","aliases":[],"binary":"{}"}}
]"#,
                "\nJSON\n",
                "    ;;\n",
                "  available) echo unexpected available >&2; exit 65 ;;\n",
                "  *) exit 64 ;;\n",
                "esac\n",
            ),
            r40_binary.display(),
            r41_binary.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args([
            "run",
            "--exclude-newer",
            "2021-03-31",
            "-e",
            "cat('ignored')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=r40");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&r40_dir);
    let _ = fs::remove_dir_all(&r41_dir);
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_and_newer_installed_r_uses_embedded_minor_without_live_lookup() {
    let cache_dir = unique_dir("ir-exclude-newer-r-newer-required-cache");
    let bin_dir = unique_dir("ir-exclude-newer-r-newer-required-bin");
    let r46_dir = unique_dir("ir-exclude-newer-r-newer-required-r46");
    let newer_dir = unique_dir("ir-exclude-newer-r-newer-required-newer");
    let available_called = unique_path("ir-exclude-newer-r-newer-required-available", "txt");

    let r46_binary = selected_r_binary(&r46_dir, "r46");
    let newer_binary = selected_r_binary(&newer_dir, "newer");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "case \"$1 $2 $3\" in\n",
                "  \"list --json \")\n",
                "    cat <<'JSON'\n",
                r#"[
{{"name":"4.6.0","version":"4.6.0","aliases":[],"binary":"{}"}},
{{"name":"99.0.0","version":"99.0.0","aliases":[],"binary":"{}"}}
]"#,
                "\nJSON\n",
                "    ;;\n",
                "  \"available --all --json\") : > '{}'; echo unexpected available >&2; exit 65 ;;\n",
                "  *) exit 64 ;;\n",
                "esac\n",
            ),
            r46_binary.display(),
            newer_binary.display(),
            available_called.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args([
            "run",
            "--exclude-newer",
            "2026-06-01",
            "-e",
            "cat('ignored')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=r46");
    assert!(
        !available_called.exists(),
        "date-only exclude-newer should not call `rig available` when the embedded minor is installed"
    );

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&r46_dir);
    let _ = fs::remove_dir_all(&newer_dir);
    let _ = fs::remove_file(&available_called);
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_after_metadata_fetch_uses_embedded_minor_without_live_lookup() {
    let cache_dir = unique_dir("ir-exclude-newer-r-fallback-cache");
    let bin_dir = unique_dir("ir-exclude-newer-r-fallback-bin");
    let r46_dir = unique_dir("ir-exclude-newer-r-fallback");
    let devel_dir = unique_dir("ir-exclude-newer-r-fallback-devel");
    let available_called = unique_path("ir-exclude-newer-r-fallback-available", "txt");

    let r46_binary = selected_r_binary(&r46_dir, "r46");
    let devel_binary = selected_r_binary(&devel_dir, "devel");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "case \"$1\" in\n",
                "  list)\n",
                "    cat <<'JSON'\n",
                r#"[
{{"name":"4.6.0","version":"4.6.0","aliases":[],"binary":"{}"}},
{{"name":"devel","version":"99.0.0","aliases":["next"],"binary":"{}"}}
]"#,
                "\nJSON\n",
                "    ;;\n",
                "  available) : > '{}'; echo unexpected available >&2; exit 65 ;;\n",
                "  *) exit 64 ;;\n",
                "esac\n",
            ),
            r46_binary.display(),
            devel_binary.display(),
            available_called.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args([
            "run",
            "--exclude-newer",
            "2026-06-01",
            "-e",
            "cat('ignored')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=r46");
    assert!(
        !available_called.exists(),
        "date-only exclude-newer should not call `rig available` when embedded metadata can select R"
    );

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&r46_dir);
    let _ = fs::remove_dir_all(&devel_dir);
    let _ = fs::remove_file(&available_called);
}

#[cfg(unix)]
#[test]
fn run_with_ir_rscript_and_exclude_newer_skips_rig_selection() {
    let cache_dir = unique_dir("ir-env-rscript-exclude-newer-cache");
    let bin_dir = unique_dir("ir-env-rscript-exclude-newer-bin");
    let rscript_dir = unique_dir("ir-env-rscript-exclude-newer-r");

    write_executable(
        &bin_dir.join("rig"),
        concat!("#!/bin/sh\n", "echo unexpected rig >&2\n", "exit 65\n",),
    );
    let rscript = rscript_dir.join("Rscript");
    write_executable(
        &rscript,
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=env-rscript\n",
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_EXCLUDE_NEWER", "2024-03-15")
        .env("IR_RSCRIPT", &rscript)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=env-rscript");
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("unexpected rig"),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&rscript_dir);
}

#[cfg(unix)]
#[test]
fn env_rscript_overrides_frontmatter_r_version_without_rig() {
    let cache_dir = unique_dir("ir-env-rscript-frontmatter-r-version-cache");
    let bin_dir = unique_dir("ir-env-rscript-frontmatter-r-version-bin");
    let rscript_dir = unique_dir("ir-env-rscript-frontmatter-r-version-r");
    let script = unique_path("ir-env-rscript-frontmatter-r-version", "R");

    fs::write(&script, "#| r-version: \"4.4\"\ncat('ignored')\n").unwrap();
    write_executable(
        &bin_dir.join("rig"),
        concat!("#!/bin/sh\n", "echo unexpected rig >&2\n", "exit 65\n",),
    );
    let rscript = rscript_dir.join("Rscript");
    write_selected_rscript(&rscript, "env-rscript");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", &rscript)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .arg("run")
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=env-rscript");
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("unexpected rig"),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&rscript_dir);
}

#[cfg(unix)]
#[test]
fn env_r_version_overrides_frontmatter_rscript() {
    let cache_dir = unique_dir("ir-env-r-version-frontmatter-rscript-cache");
    let bin_dir = unique_dir("ir-env-r-version-frontmatter-rscript-bin");
    let rscript_dir = unique_dir("ir-env-r-version-frontmatter-rscript-r");
    let rig_r_dir = unique_dir("ir-env-r-version-frontmatter-rscript-rig-r");
    let script = unique_path("ir-env-r-version-frontmatter-rscript", "R");

    let frontmatter_rscript = rscript_dir.join("Rscript");
    write_selected_rscript(&frontmatter_rscript, "frontmatter-rscript");
    fs::write(
        &script,
        format!(
            "#| rscript: {}\ncat('ignored')\n",
            r_string(&frontmatter_rscript)
        ),
    )
    .unwrap();

    let rig_binary = selected_r_binary(&rig_r_dir, "env-r-version");
    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "cat <<'JSON'\n",
                r#"[{{"name":"4.4.3","version":"4.4.3","aliases":[],"binary":"{}"}}]"#,
                "\nJSON\n",
            ),
            rig_binary.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_R_VERSION", "4.4")
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .arg("run")
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=env-r-version");

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&rscript_dir);
    let _ = fs::remove_dir_all(&rig_r_dir);
}

#[cfg(unix)]
#[test]
fn cli_rscript_overrides_env_r_version_and_frontmatter_r_version() {
    let cache_dir = unique_dir("ir-cli-rscript-precedence-cache");
    let bin_dir = unique_dir("ir-cli-rscript-precedence-bin");
    let rscript_dir = unique_dir("ir-cli-rscript-precedence-r");
    let script = unique_path("ir-cli-rscript-precedence", "R");

    fs::write(&script, "#| r-version: \"4.4\"\ncat('ignored')\n").unwrap();
    write_executable(
        &bin_dir.join("rig"),
        concat!("#!/bin/sh\n", "echo unexpected rig >&2\n", "exit 65\n",),
    );
    let rscript = rscript_dir.join("Rscript");
    write_selected_rscript(&rscript, "cli-rscript");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_R_VERSION", "4.4")
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", "--rscript"])
        .arg(&rscript)
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=cli-rscript");
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("unexpected rig"),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&rscript_dir);
}

#[cfg(unix)]
#[test]
fn cli_r_version_overrides_env_rscript() {
    let cache_dir = unique_dir("ir-cli-r-version-env-rscript-cache");
    let bin_dir = unique_dir("ir-cli-r-version-env-rscript-bin");
    let rscript_dir = unique_dir("ir-cli-r-version-env-rscript-r");
    let rig_r_dir = unique_dir("ir-cli-r-version-env-rscript-rig-r");

    let env_rscript = rscript_dir.join("Rscript");
    write_selected_rscript(&env_rscript, "env-rscript");
    let rig_binary = selected_r_binary(&rig_r_dir, "cli-r-version");
    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "cat <<'JSON'\n",
                r#"[{{"name":"4.4.3","version":"4.4.3","aliases":[],"binary":"{}"}}]"#,
                "\nJSON\n",
            ),
            rig_binary.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", &env_rscript)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .args(["run", "--r-version", "4.4", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=cli-r-version");

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&rscript_dir);
    let _ = fs::remove_dir_all(&rig_r_dir);
}

#[cfg(unix)]
#[test]
fn cli_r_selection_conflict_errors() {
    let rscript_dir = unique_dir("ir-cli-r-selection-conflict-r");
    let rscript = rscript_dir.join("Rscript");
    write_selected_rscript(&rscript, "unused");

    let out = ir()
        .args(["run", "--r-version", "4.4", "--rscript"])
        .arg(&rscript)
        .args(["-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("cannot set both `--r-version` and `--rscript`"),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_dir_all(&rscript_dir);
}

#[cfg(unix)]
#[test]
fn env_r_selection_conflict_errors() {
    let rscript_dir = unique_dir("ir-env-r-selection-conflict-r");
    let rscript = rscript_dir.join("Rscript");
    write_selected_rscript(&rscript, "unused");

    let out = ir()
        .env("IR_RSCRIPT", &rscript)
        .env("IR_R_VERSION", "4.4")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("cannot set both `IR_R_VERSION` and `IR_RSCRIPT`"),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_dir_all(&rscript_dir);
}

#[cfg(unix)]
#[test]
fn frontmatter_r_selection_conflict_errors() {
    let cache_dir = unique_dir("ir-frontmatter-r-selection-conflict-cache");
    let rscript_dir = unique_dir("ir-frontmatter-r-selection-conflict-r");
    let script = unique_path("ir-frontmatter-r-selection-conflict", "R");
    let rscript = rscript_dir.join("Rscript");
    write_selected_rscript(&rscript, "unused");
    fs::write(
        &script,
        format!(
            "#| r-version: \"4.4\"\n#| rscript: {}\ncat('ignored')\n",
            r_string(&rscript)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("IR_RSCRIPT")
        .env_remove("IR_R_VERSION")
        .arg("run")
        .arg(&script)
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("frontmatter cannot set both `r-version` and `rscript`"),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&rscript_dir);
}

#[cfg(unix)]
#[test]
fn cli_rscript_with_exclude_newer_uses_snapshot_without_rig_selection() {
    let cache_dir = unique_dir("ir-cli-rscript-exclude-newer-cache");
    let bin_dir = unique_dir("ir-cli-rscript-exclude-newer-bin");
    let rscript_dir = unique_dir("ir-cli-rscript-exclude-newer-r");

    write_executable(
        &bin_dir.join("rig"),
        concat!("#!/bin/sh\n", "echo unexpected rig >&2\n", "exit 65\n",),
    );
    let rscript = rscript_dir.join("Rscript");
    write_selected_rscript(&rscript, "cli-rscript");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_EXPECT_EXCLUDE_NEWER", "2024-03-15")
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", "--rscript"])
        .arg(&rscript)
        .args(["--exclude-newer", "2024-03-15", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=cli-rscript");
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("unexpected rig"),
        "{}",
        output_text(&out)
    );

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&rscript_dir);
}

#[cfg(unix)]
#[test]
fn render_cli_rscript_sets_quarto_r() {
    let cache_dir = unique_dir("ir-render-cli-rscript-cache");
    let rscript_dir = unique_dir("ir-render-cli-rscript-r");
    let quarto_dir = unique_dir("ir-render-cli-rscript-quarto-dir");
    let doc = unique_path("ir-render-cli-rscript", "qmd");
    let observed = unique_path("ir-render-cli-rscript-quarto-r", "txt");

    fs::write(&doc, "---\ntitle: rscript render\n---\n").unwrap();
    let rscript = rscript_dir.join("Rscript");
    write_selected_rscript(&rscript, "unused");
    write_executable(
        &quarto_dir.join("quarto"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "printf '%s\\n' \"$QUARTO_R\" > {}\n",
                "exit 0\n",
            ),
            observed.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_QUARTO", quarto_dir.join("quarto"))
        .env_remove("IR_RSCRIPT")
        .args(["render", "--rscript"])
        .arg(&rscript)
        .arg(&doc)
        .output()
        .unwrap();

    assert_success(&out);
    let quarto_r = fs::read_to_string(&observed).unwrap();
    assert_eq!(
        quarto_r.trim(),
        std::path::absolute(&rscript).unwrap().to_string_lossy()
    );

    let _ = fs::remove_file(&doc);
    let _ = fs::remove_file(&observed);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&rscript_dir);
    let _ = fs::remove_dir_all(&quarto_dir);
}

#[cfg(unix)]
#[test]
fn missing_exact_minor_r_version_with_exclude_newer_does_not_query_available_releases() {
    let cache_dir = unique_dir("ir-exclude-newer-missing-r-cache");
    let bin_dir = unique_dir("ir-exclude-newer-missing-r-bin");

    write_executable(
        &bin_dir.join("rig"),
        concat!(
            "#!/bin/sh\n",
            "case \"$1\" in\n",
            "  list) echo '[]' ;;\n",
            "  available) echo unexpected available >&2; exit 65 ;;\n",
            "  *) exit 64 ;;\n",
            "esac\n",
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args([
            "run",
            "--r-version",
            "== 4.4",
            "--exclude-newer",
            "2024-06-20",
            "-e",
            "cat('ignored')",
        ])
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("R 4.4 is required but is not installed"),
        "{}",
        output_text(&out)
    );
    assert!(
        stderr.contains("Run `rig install 4.4`"),
        "{}",
        output_text(&out)
    );
    assert!(
        !stderr.contains("unexpected available"),
        "{}",
        output_text(&out)
    );

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
        .env("IR_RSCRIPT", "Rscript.bat")
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
