//! Integration tests for the public `ir` CLI.

mod support;

use support::*;

use std::fs;
use std::path::PathBuf;
#[cfg(unix)]
use time::OffsetDateTime;

#[test]
fn rig_test_prerequisites_match_ir_test_r_version() {
    let _ = rig_test_r_version("rig_test_prerequisites_match_ir_test_r_version");
}

#[test]
fn r_version_selection_covers_render_flag_and_run_frontmatter() {
    // Opt-in: needs rig plus a non-default R installed (CI provisions both).
    // `ir`'s `--r-version` path resolves through rig unconditionally, so a
    // single R installation cannot cover selection.
    let Some(target) =
        rig_test_r_version("r_version_selection_covers_render_flag_and_run_frontmatter")
    else {
        return;
    };
    let target_exclude_newer = std::env::var("IR_TEST_R_EXCLUDE_NEWER")
        .unwrap_or_else(|_| panic!("IR_TEST_R_VERSION={target} requires IR_TEST_R_EXCLUDE_NEWER"));

    // Selecting the version the default path already uses would prove nothing.
    if default_r_version().as_deref() == Some(target.as_str()) {
        eprintln!(
            "SKIP r_version_selection_covers_render_flag_and_run_frontmatter: the test R ({target}) matches the default R; nothing to select"
        );
        return;
    }

    let fixture_dir = fixture_copy("run", "ir-r-version-render-fixture");
    let cache_dir = temp_cache("ir-r-version-cache");
    for filename in ["r-version-select.qmd", "r-version-frontmatter.R"] {
        let path = fixture_dir.join(filename);
        let frontmatter = fs::read_to_string(&path).unwrap();
        assert!(frontmatter.contains("exclude-newer: 2026-06-01"));
        let updated = frontmatter.replace(
            "exclude-newer: 2026-06-01",
            &format!("exclude-newer: {target_exclude_newer}"),
        );
        let updated = if filename.ends_with(".R") {
            assert!(updated.contains("#| r-version: 4.4.3"));
            updated.replace("#| r-version: 4.4.3", &format!("#| r-version: {target}"))
        } else {
            updated
        };
        fs::write(&path, updated).unwrap();
    }

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

    let script = fixture_dir.join("r-version-frontmatter.R");

    let run = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("IR_RSCRIPT")
        .env_remove("IR_R_VERSION")
        .args(["run", "--isolated", "--vanilla"])
        .arg(&script)
        .output()
        .unwrap();

    assert_success(&run);
    assert_stdout_contains(&run, "ir.fixture=r-version-frontmatter");
    assert_stdout_contains(&run, &format!("version.r_version=[{target}]"));
    assert_stdout_contains(&run, "version.lib_in_cache=true");
    assert_stdout_contains(&run, "version.jsonlite_in_cache=true");

    let _ = fs::remove_file(fixture_dir.join("r-version-select.html"));
    let _ = fs::remove_dir_all(fixture_dir.join("r-version-select_files"));
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
fn utc_today_string() -> String {
    let today = OffsetDateTime::now_utc().date();
    format!(
        "{:04}-{:02}-{:02}",
        today.year(),
        u8::from(today.month()),
        today.day()
    )
}

#[cfg(unix)]
fn assert_failure_contains(output: &std::process::Output, expected: &[&str]) {
    assert!(!output.status.success(), "{}", output_text(output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    for needle in expected {
        assert!(stderr.contains(needle), "{}", output_text(output));
    }
}

#[cfg(unix)]
fn assert_stderr_lacks(output: &std::process::Output, unexpected: &str) {
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains(unexpected),
        "{}",
        output_text(output)
    );
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
fn run_with_installed_r_versions(
    prefix: &str,
    versions: &[(&str, &str)],
    args: &[&str],
) -> std::process::Output {
    let cache_dir = temp_dir(&format!("{prefix}-cache"));
    let bin_dir = temp_dir(&format!("{prefix}-bin"));
    let mut r_dirs = Vec::new();
    let mut rows = Vec::new();

    for (version, label) in versions {
        let dir = temp_dir(&format!("{prefix}-{label}"));
        let binary = selected_r_binary(&dir, label);
        rows.push(format!(
            r#"{{"name":"{version}","version":"{version}","aliases":[],"binary":"{}"}}"#,
            binary.display()
        ));
        r_dirs.push(dir);
    }

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "case \"$1\" in\n",
                "  list)\n",
                "    [ \"$2\" = \"--json\" ]\n",
                "    cat <<'JSON'\n",
                "[\n{}\n]\n",
                "JSON\n",
                "    ;;\n",
                "  *) echo unexpected rig command >&2; exit 64 ;;\n",
                "esac\n",
            ),
            rows.join(",\n")
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(args)
        .output()
        .unwrap();

    out
}

#[cfg(unix)]
fn run_with_private_r_install(
    prefix: &str,
    args: &[&str],
    expected_install: &str,
    installed: (&str, &str),
    available_json: Option<&str>,
    resolved: Option<(&str, &str)>,
    runs: usize,
) -> std::process::Output {
    let cache_dir = temp_dir(&format!("{prefix}-cache"));
    let bin_dir = temp_dir(&format!("{prefix}-bin"));

    write_executable(
        &bin_dir.join("rig"),
        concat!(
            "#!/bin/sh\n",
            "set -eu\n",
            "if [ \"$1\" = \"--user\" ]; then shift; else [ \"${RIG_MODE:-}\" != \"user\" ]; fi\n",
            "case \"$1\" in\n",
            "  --version)\n",
            "    [ \"${RIG_MODE:-}\" = \"user\" ]\n",
            "    echo 'rig 0.10.0'\n",
            "    ;;\n",
            "  list)\n",
            "    [ \"$2\" = \"--json\" ]\n",
            "    if [ \"${RIG_MODE:-}\" != \"user\" ] || [ ! -f \"$RIG_R_INSTALL_DIR/installed\" ]; then\n",
            "      echo '[]'\n",
            "      exit 0\n",
            "    fi\n",
            "    printf '[{\"name\":\"%s\",\"version\":\"%s\",\"aliases\":[],\"binary\":\"%s\"}]\\n' \"$IR_TEST_INSTALLED_NAME\" \"$IR_TEST_INSTALLED_VERSION\" \"$RIG_R_INSTALL_DIR/$IR_TEST_INSTALLED_VERSION/bin/R\"\n",
            "    ;;\n",
            "  available)\n",
            "    [ \"${RIG_MODE:-}\" = \"user\" ]\n",
            "    [ \"$2\" = \"--all\" ]\n",
            "    [ \"$3\" = \"--json\" ]\n",
            "    [ -n \"${IR_TEST_AVAILABLE_JSON:-}\" ]\n",
            "    printf '%s\\n' \"$IR_TEST_AVAILABLE_JSON\"\n",
            "    ;;\n",
            "  resolve)\n",
            "    [ \"${RIG_MODE:-}\" = \"user\" ]\n",
            "    [ \"$2\" = \"--json\" ]\n",
            "    [ \"$3\" = \"--\" ]\n",
            "    [ \"$4\" = \"$IR_TEST_RESOLVE_SPEC\" ]\n",
            "    [ ! -f \"$RIG_R_INSTALL_DIR/resolve-ran\" ]\n",
            "    : > \"$RIG_R_INSTALL_DIR/resolve-ran\"\n",
            "    printf '[{\"version\":\"%s\"}]\\n' \"$IR_TEST_RESOLVED_VERSION\"\n",
            "    ;;\n",
            "  add)\n",
            "    [ \"${RIG_MODE:-}\" = \"user\" ]\n",
            "    [ \"$2\" = \"--without-pak\" ]\n",
            "    [ \"$3\" = \"--without-repos\" ]\n",
            "    [ \"$4\" = \"--\" ]\n",
            "    [ \"$5\" = \"$IR_TEST_EXPECT_INSTALL\" ]\n",
            "    [ ! -f \"$RIG_R_INSTALL_DIR/add-ran\" ]\n",
            "    mkdir -p \"$RIG_R_INSTALL_DIR/$IR_TEST_INSTALLED_VERSION/bin\"\n",
            "    : > \"$RIG_R_INSTALL_DIR/$IR_TEST_INSTALLED_VERSION/bin/R\"\n",
            "    cat > \"$RIG_R_INSTALL_DIR/$IR_TEST_INSTALLED_VERSION/bin/Rscript\" <<'RSCRIPT'\n",
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=private-install\n",
            "RSCRIPT\n",
            "    chmod +x \"$RIG_R_INSTALL_DIR/$IR_TEST_INSTALLED_VERSION/bin/Rscript\"\n",
            "    : > \"$RIG_R_INSTALL_DIR/add-ran\"\n",
            "    : > \"$RIG_R_INSTALL_DIR/installed\"\n",
            "    ;;\n",
            "  *) exit 64 ;;\n",
            "esac\n",
        ),
    );

    let mut command = ir();
    command
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env("IR_TEST_EXPECT_INSTALL", expected_install)
        .env("IR_TEST_INSTALLED_NAME", installed.0)
        .env("IR_TEST_INSTALLED_VERSION", installed.1)
        .env_remove("IR_RSCRIPT")
        .env_remove("RIG_MODE")
        .env_remove("RIG_R_INSTALL_DIR")
        .env_remove("RIG_BINARY_DIR")
        .args(args);
    if let Some(available_json) = available_json {
        command.env("IR_TEST_AVAILABLE_JSON", available_json);
    }
    if let Some((spec, version)) = resolved {
        command
            .env("IR_TEST_RESOLVE_SPEC", spec)
            .env("IR_TEST_RESOLVED_VERSION", version);
    }
    let mut output = None;
    for run in 0..runs {
        let current = command.output().unwrap();
        if run + 1 < runs {
            assert_success(&current);
            assert_stdout_contains(&current, "selected=private-install");
        }
        output = Some(current);
    }
    output.expect("run_with_private_r_install needs at least one run")
}

#[cfg(unix)]
#[test]
fn run_with_missing_r_version_installs_and_reuses_private_cached_r() {
    let cache_dir = temp_dir("ir-private-r-cache");
    let bin_dir = temp_dir("ir-private-r-bin");
    let rig_log = temp_path("ir-private-r-rig-log", "txt");
    let expected_r_root = cache_dir.join("rig").join("r");
    let expected_binary_dir = cache_dir.join("rig").join("bin");

    write_executable(
        &bin_dir.join("rig"),
        concat!(
            "#!/bin/sh\n",
            "set -eu\n",
            "if [ \"$1\" = \"--user\" ]; then shift; else [ \"${RIG_MODE:-}\" != \"user\" ]; fi\n",
            "case \"$1\" in\n",
            "  --version)\n",
            "    [ \"${RIG_MODE:-}\" = \"user\" ]\n",
            "    echo 'rig 0.10.0'\n",
            "    ;;\n",
            "  list)\n",
            "    [ \"$2\" = \"--json\" ]\n",
            "    if [ \"${RIG_MODE:-}\" != \"user\" ]; then\n",
            "      echo '[]'\n",
            "      exit 0\n",
            "    fi\n",
            "    [ \"$RIG_R_INSTALL_DIR\" = \"$IR_TEST_EXPECT_R_ROOT\" ]\n",
            "    [ \"$RIG_BINARY_DIR\" = \"$IR_TEST_EXPECT_BINARY_DIR\" ]\n",
            "    [ \"${PATH%%:*}\" = \"$RIG_BINARY_DIR\" ]\n",
            "    [ \"$HOME\" = \"$IR_TEST_EXPECT_HOME\" ]\n",
            "    [ \"$TMPDIR\" = \"$IR_TEST_EXPECT_TMP\" ]\n",
            "    [ \"$TMP\" = \"$IR_TEST_EXPECT_TMP\" ]\n",
            "    [ \"$TEMP\" = \"$IR_TEST_EXPECT_TMP\" ]\n",
            "    [ -z \"${XDG_CONFIG_HOME:-}\" ]\n",
            "    [ -z \"${XDG_DATA_HOME:-}\" ]\n",
            "    [ -z \"${XDG_CACHE_HOME:-}\" ]\n",
            "    [ -z \"${RIG_PLATFORM:-}\" ]\n",
            "    [ -z \"${RIG_RTOOLS_INSTALL_DIR:-}\" ]\n",
            "    if [ -f \"$RIG_R_INSTALL_DIR/installed\" ]; then\n",
            "      printf '[{\"name\":\"4.4.3\",\"version\":\"4.4.3\",\"aliases\":[],\"binary\":\"%s\"}]\\n' \"$RIG_R_INSTALL_DIR/4.4.3/bin/R\"\n",
            "    else\n",
            "      echo '[]'\n",
            "    fi\n",
            "    ;;\n",
            "  add)\n",
            "    [ \"${RIG_MODE:-}\" = \"user\" ]\n",
            "    [ \"$RIG_R_INSTALL_DIR\" = \"$IR_TEST_EXPECT_R_ROOT\" ]\n",
            "    [ \"$RIG_BINARY_DIR\" = \"$IR_TEST_EXPECT_BINARY_DIR\" ]\n",
            "    [ \"${PATH%%:*}\" = \"$RIG_BINARY_DIR\" ]\n",
            "    [ \"$HOME\" = \"$IR_TEST_EXPECT_HOME\" ]\n",
            "    [ \"$TMPDIR\" = \"$IR_TEST_EXPECT_TMP\" ]\n",
            "    [ \"$TMP\" = \"$IR_TEST_EXPECT_TMP\" ]\n",
            "    [ \"$TEMP\" = \"$IR_TEST_EXPECT_TMP\" ]\n",
            "    [ -z \"${XDG_CONFIG_HOME:-}\" ]\n",
            "    [ -z \"${XDG_DATA_HOME:-}\" ]\n",
            "    [ -z \"${XDG_CACHE_HOME:-}\" ]\n",
            "    [ -z \"${RIG_PLATFORM:-}\" ]\n",
            "    [ -z \"${RIG_RTOOLS_INSTALL_DIR:-}\" ]\n",
            "    [ \"$2\" = \"--without-pak\" ]\n",
            "    [ \"$3\" = \"--without-repos\" ]\n",
            "    [ \"$4\" = \"--\" ]\n",
            "    [ \"$5\" = \"4.4\" ]\n",
            "    mkdir -p \"$RIG_R_INSTALL_DIR/4.4.3/bin\"\n",
            "    : > \"$RIG_R_INSTALL_DIR/4.4.3/bin/R\"\n",
            "    cat > \"$RIG_R_INSTALL_DIR/4.4.3/bin/Rscript\" <<'RSCRIPT'\n",
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=private-cache\n",
            "RSCRIPT\n",
            "    chmod +x \"$RIG_R_INSTALL_DIR/4.4.3/bin/Rscript\"\n",
            "    : > \"$RIG_R_INSTALL_DIR/installed\"\n",
            "    echo add >> \"$IR_TEST_RIG_LOG\"\n",
            "    ;;\n",
            "  *) exit 64 ;;\n",
            "esac\n",
        ),
    );

    for _ in 0..2 {
        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .env("PATH", path_with_bin_dir(&bin_dir))
            .env("IR_TEST_EXPECT_R_ROOT", &expected_r_root)
            .env("IR_TEST_EXPECT_BINARY_DIR", &expected_binary_dir)
            .env("IR_TEST_EXPECT_HOME", cache_dir.join("rig").join("home"))
            .env("IR_TEST_EXPECT_TMP", cache_dir.join("rig").join("tmp"))
            .env("IR_TEST_RIG_LOG", &*rig_log)
            .env("RIG_PLATFORM", "linux-ubuntu-24.04")
            .env("RIG_RTOOLS_INSTALL_DIR", "/outside/rtools")
            .env("XDG_CONFIG_HOME", "/outside/config")
            .env("XDG_DATA_HOME", "/outside/data")
            .env("XDG_CACHE_HOME", "/outside/cache")
            .env_remove("IR_RSCRIPT")
            .env_remove("RIG_MODE")
            .env_remove("RIG_R_INSTALL_DIR")
            .env_remove("RIG_BINARY_DIR")
            .args(["run", "--r-version", "4.4", "-e", "cat('ignored')"])
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, "selected=private-cache");
    }

    assert_eq!(fs::read_to_string(&*rig_log).unwrap(), "add\n");
}

#[cfg(unix)]
#[test]
fn simultaneous_missing_r_runs_install_private_cached_r_once() {
    let cache_dir = temp_dir("ir-concurrent-private-r-cache");
    let bin_dir = temp_dir("ir-concurrent-private-r-bin");
    let barrier_dir = temp_dir("ir-concurrent-private-r-barrier");
    let rig_log = temp_path("ir-concurrent-private-r-rig-log", "txt");

    write_executable(
        &bin_dir.join("rig"),
        concat!(
            "#!/bin/sh\n",
            "set -eu\n",
            "if [ \"$1\" = \"--user\" ]; then shift; else [ \"${RIG_MODE:-}\" != \"user\" ]; fi\n",
            "case \"$1\" in\n",
            "  --version)\n",
            "    [ \"${RIG_MODE:-}\" = \"user\" ]\n",
            "    : > \"$IR_TEST_BARRIER_DIR/$IR_TEST_RUN_ID.preflight\"\n",
            "    echo 'rig 0.10.0'\n",
            "    ;;\n",
            "  list)\n",
            "    [ \"$2\" = \"--json\" ]\n",
            "    if [ \"${RIG_MODE:-}\" != \"user\" ]; then\n",
            "      echo '[]'\n",
            "      exit 0\n",
            "    fi\n",
            "    : > \"$IR_TEST_BARRIER_DIR/$IR_TEST_RUN_ID.private-list\"\n",
            "    if [ -f \"$RIG_R_INSTALL_DIR/installed\" ]; then\n",
            "      printf '[{\"name\":\"4.4.3\",\"version\":\"4.4.3\",\"aliases\":[],\"binary\":\"%s\"}]\\n' \"$RIG_R_INSTALL_DIR/4.4.3/bin/R\"\n",
            "    else\n",
            "      echo '[]'\n",
            "    fi\n",
            "    ;;\n",
            "  add)\n",
            "    [ \"${RIG_MODE:-}\" = \"user\" ]\n",
            "    [ \"$2\" = \"--without-pak\" ]\n",
            "    [ \"$3\" = \"--without-repos\" ]\n",
            "    [ \"$4\" = \"--\" ]\n",
            "    [ \"$5\" = \"4.4\" ]\n",
            "    : > \"$IR_TEST_BARRIER_DIR/$IR_TEST_RUN_ID.add\"\n",
            "    if [ \"$IR_TEST_RUN_ID\" = \"first\" ]; then\n",
            "      barrier_wait=0\n",
            "      while [ ! -f \"$IR_TEST_BARRIER_DIR/release-first-add\" ]; do\n",
            "        [ \"$barrier_wait\" -lt 500 ] || exit 70\n",
            "        barrier_wait=$((barrier_wait + 1))\n",
            "        sleep 0.01\n",
            "      done\n",
            "    fi\n",
            "    mkdir -p \"$RIG_R_INSTALL_DIR/4.4.3/bin\"\n",
            "    : > \"$RIG_R_INSTALL_DIR/4.4.3/bin/R\"\n",
            "    cat > \"$RIG_R_INSTALL_DIR/4.4.3/bin/Rscript\" <<'RSCRIPT'\n",
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo selected=private-cache\n",
            "RSCRIPT\n",
            "    chmod +x \"$RIG_R_INSTALL_DIR/4.4.3/bin/Rscript\"\n",
            "    : > \"$RIG_R_INSTALL_DIR/installed\"\n",
            "    echo add >> \"$IR_TEST_RIG_LOG\"\n",
            "    ;;\n",
            "  *) exit 64 ;;\n",
            "esac\n",
        ),
    );

    let spawn = |run_id: &str| {
        ir().env("IR_CACHE_DIR", &cache_dir)
            .env("PATH", path_with_bin_dir(&bin_dir))
            .env("IR_TEST_BARRIER_DIR", &barrier_dir)
            .env("IR_TEST_RUN_ID", run_id)
            .env("IR_TEST_RIG_LOG", &*rig_log)
            .env_remove("IR_RSCRIPT")
            .env_remove("RIG_MODE")
            .env_remove("RIG_R_INSTALL_DIR")
            .env_remove("RIG_BINARY_DIR")
            .args(["run", "--r-version", "4.4", "-e", "cat('ignored')"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };

    let mut first = spawn("first");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !barrier_dir.join("first.add").exists() {
        if let Some(status) = first.try_wait().unwrap() {
            panic!("first ir process exited before entering rig add with {status}");
        }
        if std::time::Instant::now() >= deadline {
            let _ = first.kill();
            panic!("first ir process did not enter rig add");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let mut second = spawn("second");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !barrier_dir.join("second.preflight").exists() {
        if let Some(status) = second.try_wait().unwrap() {
            fs::write(barrier_dir.join("release-first-add"), "").unwrap();
            let _ = first.kill();
            panic!("second ir process exited before its rig preflight with {status}");
        }
        if std::time::Instant::now() >= deadline {
            fs::write(barrier_dir.join("release-first-add"), "").unwrap();
            let _ = first.kill();
            let _ = second.kill();
            panic!("second ir process did not reach its rig preflight");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    std::thread::sleep(std::time::Duration::from_millis(500));
    if barrier_dir.join("second.private-list").exists() || barrier_dir.join("second.add").exists() {
        fs::write(barrier_dir.join("release-first-add"), "").unwrap();
        let _ = first.kill();
        let _ = second.kill();
        panic!(
            "the second ir process entered the private rig installation while the first add was blocked"
        );
    }
    fs::write(barrier_dir.join("release-first-add"), "").unwrap();

    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    for output in [&first, &second] {
        assert_success(output);
        assert_stdout_contains(output, "selected=private-cache");
    }
    assert_eq!(fs::read_to_string(&*rig_log).unwrap(), "add\n");
}

#[cfg(unix)]
#[test]
fn missing_r_with_old_rig_reports_development_version_requirement() {
    let cache_dir = temp_dir("ir-old-rig-cache");
    let bin_dir = temp_dir("ir-old-rig-bin");

    write_executable(
        &bin_dir.join("rig"),
        concat!(
            "#!/bin/sh\n",
            "if [ \"$1\" = \"list\" ]; then\n",
            "  echo '[]'\n",
            "  exit 0\n",
            "fi\n",
            "echo \"error: unexpected argument '--user' found\" >&2\n",
            "exit 2\n",
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "4.4", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_failure_contains(
        &out,
        &[
            "development version of rig",
            "cargo install --git https://github.com/r-lib/rig --locked --force rig",
        ],
    );
}

#[cfg(unix)]
#[test]
fn missing_r_without_rig_reports_development_version_install_command() {
    let cache_dir = temp_dir("ir-missing-rig-cache");
    let bin_dir = temp_dir("ir-missing-rig-bin");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", &bin_dir)
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "4.4", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_failure_contains(
        &out,
        &[
            "could not find `rig` on PATH",
            "cargo install --git https://github.com/r-lib/rig --locked --force rig",
        ],
    );
}

#[test]
fn real_rig_installs_private_cached_r_on_disposable_ci() {
    if std::env::var("GITHUB_ACTIONS").ok().as_deref() != Some("true")
        || std::env::var("RUNNER_ENVIRONMENT").ok().as_deref() != Some("github-hosted")
    {
        eprintln!(
            "SKIP real_rig_installs_private_cached_r_on_disposable_ci: this real rig installation test only runs on disposable GitHub-hosted runners"
        );
        return;
    }
    let Some(target) = rig_test_r_version("real_rig_installs_private_cached_r_on_disposable_ci")
    else {
        return;
    };
    let cache_dir = temp_dir("ir-real-private-r-cache");
    let empty_r_root = temp_dir("ir-real-empty-r-root");
    let empty_binary_dir = temp_dir("ir-real-empty-r-bin");

    let run = || {
        ir().env("IR_CACHE_DIR", &cache_dir)
            .env("RIG_MODE", "user")
            .env("RIG_R_INSTALL_DIR", &empty_r_root)
            .env("RIG_BINARY_DIR", &empty_binary_dir)
            .env_remove("IR_RSCRIPT")
            .args(["run", "--r-version", "oldrel/2"])
            .args([
                "-e",
                "cat('IR_REAL_R_HOME=', normalizePath(R.home(), winslash = '/', mustWork = TRUE), '\\nIR_REAL_R_VERSION=', as.character(getRversion()), '\\n', sep = '')",
            ])
            .output()
            .unwrap()
    };
    let installed = |output: &std::process::Output| {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let homes = stdout
            .lines()
            .filter_map(|line| line.strip_prefix("IR_REAL_R_HOME="))
            .collect::<Vec<_>>();
        let versions = stdout
            .lines()
            .filter_map(|line| line.strip_prefix("IR_REAL_R_VERSION="))
            .collect::<Vec<_>>();
        assert_eq!(homes.len(), 1, "unexpected R home output:\n{stdout}");
        assert_eq!(versions.len(), 1, "unexpected R version output:\n{stdout}");
        (
            fs::canonicalize(PathBuf::from(homes[0])).unwrap(),
            versions[0].to_string(),
        )
    };

    let first = run();
    assert_success(&first);
    let (first_home, first_version) = installed(&first);
    assert_eq!(first_version, target);
    let private_r_root = fs::canonicalize(cache_dir.join("rig").join("r")).unwrap();
    assert!(
        first_home.starts_with(&private_r_root),
        "selected R home `{}` is outside private cache root `{}`",
        first_home.display(),
        private_r_root.display()
    );
    assert_eq!(
        fs::read_to_string(cache_dir.join("rig").join("resolutions").join("oldrel-2"))
            .unwrap()
            .trim(),
        target
    );

    let sentinel = first_home.join("ir-cache-reuse-sentinel");
    fs::write(&sentinel, "preserved").unwrap();

    let second = run();
    assert_success(&second);
    let (second_home, second_version) = installed(&second);
    assert_eq!(second_home, first_home);
    assert_eq!(second_version, target);
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "preserved");
}

#[cfg(unix)]
#[test]
fn run_with_r_version_selects_highest_matching_installed_r() {
    let out = run_with_installed_r_versions(
        "ir-r-version-minor",
        &[("4.4.2", "old"), ("4.4.3", "new")],
        &["run", "--r-version", "4.4", "-e", "cat('ignored')"],
    );
    assert_success(&out);
    assert_stdout_contains(&out, "selected=new");

    let out = run_with_installed_r_versions(
        "ir-r-version-major",
        &[("4.3.3", "r43"), ("4.5.0", "r45")],
        &["run", "--r-version", "4", "-e", "cat('ignored')"],
    );
    assert_success(&out);
    assert_stdout_contains(&out, "selected=r45");

    let out = run_with_installed_r_versions(
        "ir-r-version-exact-major",
        &[("4.3.3", "r43"), ("4.5.0", "r45")],
        &["run", "--r-version", "== 4", "-e", "cat('ignored')"],
    );
    assert_success(&out);
    assert_stdout_contains(&out, "selected=r45");

    let out = run_with_installed_r_versions(
        "ir-r-version-exact-minor",
        &[("4.4.2", "old"), ("4.4.3", "new")],
        &["run", "--r-version", "== 4.4", "-e", "cat('ignored')"],
    );
    assert_success(&out);
    assert_stdout_contains(&out, "selected=new");

    let out = run_with_installed_r_versions(
        "ir-r-version-exact-minor-only",
        &[("4.4.2", "old")],
        &["run", "--r-version", "== 4.4", "-e", "cat('ignored')"],
    );
    assert_success(&out);
    assert_stdout_contains(&out, "selected=old");
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_prefers_rig_default_for_equal_versions() {
    let cache_dir = temp_dir("ir-exclude-newer-r-default-tie-cache");
    let bin_dir = temp_dir("ir-exclude-newer-r-default-tie-bin");
    let default_dir = temp_dir("ir-exclude-newer-r-default-tie-default");
    let alternate_dir = temp_dir("ir-exclude-newer-r-default-tie-alternate");

    let default_binary = selected_r_binary(&default_dir, "default");
    let alternate_binary = selected_r_binary(&alternate_dir, "alternate");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "case \"$1\" in\n",
                "  list)\n",
                "    cat <<'JSON'\n",
                r#"[
{{"name":"4.6","version":"4.6.0","default":true,"aliases":["release"],"binary":"{}"}},
{{"name":"4.6-arm64","version":"4.6.0","default":false,"aliases":["devel"],"binary":"{}"}}
]"#,
                "\nJSON\n",
                "    ;;\n",
                "  available) echo unexpected available >&2; exit 65 ;;\n",
                "  *) exit 64 ;;\n",
                "esac\n",
            ),
            default_binary.display(),
            alternate_binary.display()
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
    assert_stdout_contains(&out, "selected=default");
    assert_stderr_lacks(&out, "unexpected available");
}

#[cfg(all(unix, target_os = "macos", target_arch = "aarch64"))]
#[test]
fn run_with_exclude_newer_prefers_arm64_rig_install_for_equal_versions_without_default() {
    let cache_dir = temp_dir("ir-exclude-newer-r-native-tie-cache");
    let bin_dir = temp_dir("ir-exclude-newer-r-native-tie-bin");
    let arm64_dir = temp_dir("ir-exclude-newer-r-native-tie-arm");
    let alternate_dir = temp_dir("ir-exclude-newer-r-native-tie-alternate");

    let arm64_binary = selected_r_binary(&arm64_dir, "arm64");
    let alternate_binary = selected_r_binary(&alternate_dir, "alternate");
    let arm64_path = arm64_dir.join("R-4.6-arm64");
    let alternate_path = alternate_dir.join("R-4.6");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "case \"$1\" in\n",
                "  list)\n",
                "    cat <<'JSON'\n",
                r#"[
{{"name":"4.6-arm64","version":"4.6.0","default":false,"aliases":["devel"],"path":"{}","binary":"{}"}},
{{"name":"4.6","version":"4.6.0","default":false,"aliases":["release"],"path":"{}","binary":"{}"}}
]"#,
                "\nJSON\n",
                "    ;;\n",
                "  available) echo unexpected available >&2; exit 65 ;;\n",
                "  *) exit 64 ;;\n",
                "esac\n",
            ),
            arm64_path.display(),
            arm64_binary.display(),
            alternate_path.display(),
            alternate_binary.display()
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
    assert_stdout_contains(&out, "selected=arm64");
    assert_stderr_lacks(&out, "unexpected available");
}

#[cfg(all(unix, target_os = "macos", target_arch = "aarch64"))]
#[test]
fn run_with_exclude_newer_prefers_unsuffixed_rig_install_over_x86_64_on_arm_macos() {
    let cache_dir = temp_dir("ir-exclude-newer-r-unsuffixed-tie-cache");
    let bin_dir = temp_dir("ir-exclude-newer-r-unsuffixed-tie-bin");
    let native_dir = temp_dir("ir-exclude-newer-r-unsuffixed-tie-native");
    let x86_dir = temp_dir("ir-exclude-newer-r-unsuffixed-tie-x86");

    let native_binary = selected_r_binary(&native_dir, "native");
    let x86_binary = selected_r_binary(&x86_dir, "x86_64");
    let native_path = native_dir.join("R-4.6");
    let x86_path = x86_dir.join("R-4.6-x86_64");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "case \"$1\" in\n",
                "  list)\n",
                "    cat <<'JSON'\n",
                r#"[
{{"name":"4.6","version":"4.6.0","default":false,"aliases":["release"],"path":"{}","binary":"{}"}},
{{"name":"4.6-x86_64","version":"4.6.0","default":false,"aliases":["oldrel"],"path":"{}","binary":"{}"}}
]"#,
                "\nJSON\n",
                "    ;;\n",
                "  available) echo unexpected available >&2; exit 65 ;;\n",
                "  *) exit 64 ;;\n",
                "esac\n",
            ),
            native_path.display(),
            native_binary.display(),
            x86_path.display(),
            x86_binary.display()
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
    assert_stdout_contains(&out, "selected=native");
    assert_stderr_lacks(&out, "unexpected available");
}

#[cfg(unix)]
#[test]
fn run_with_exact_minor_r_version_installs_latest_patch() {
    let out = run_with_private_r_install(
        "ir-exact-minor-install",
        &["run", "--r-version", "== 4.4", "-e", "cat('ignored')"],
        "4.4",
        ("4.4.3", "4.4.3"),
        None,
        None,
        1,
    );
    assert_success(&out);
    assert_stdout_contains(&out, "selected=private-install");
}

#[cfg(unix)]
#[test]
fn run_with_exact_major_r_version_installs_highest_available_minor() {
    let available = r#"[
{"name":"3.6.3","version":"3.6.3"},
{"name":"4.4.3","version":"4.4.3"},
{"name":"4.5.1","version":"4.5.1"},
{"name":"5.0.0","version":"5.0.0"}
]"#;
    let out = run_with_private_r_install(
        "ir-exact-major-install",
        &["run", "--r-version", "== 4", "-e", "cat('ignored')"],
        "4.5.1",
        ("4.5.1", "4.5.1"),
        Some(available),
        None,
        1,
    );
    assert_success(&out);
    assert_stdout_contains(&out, "selected=private-install");
}

#[cfg(unix)]
#[test]
fn run_with_bare_major_r_version_installs_highest_available_minor() {
    let available = r#"[
{"name":"3.6.3","version":"3.6.3"},
{"name":"4.4.3","version":"4.4.3"},
{"name":"4.5.1","version":"4.5.1"},
{"name":"5.0.0","version":"5.0.0"}
]"#;
    let out = run_with_private_r_install(
        "ir-bare-major-install",
        &["run", "--r-version", "4", "-e", "cat('ignored')"],
        "4.5.1",
        ("4.5.1", "4.5.1"),
        Some(available),
        None,
        1,
    );
    assert_success(&out);
    assert_stdout_contains(&out, "selected=private-install");
}

#[cfg(unix)]
#[test]
fn comparison_r_versions_install_at_each_boundary() {
    for (label, selector, available, expected) in [
        (
            "gte",
            ">= 4.4.0",
            r#"[{"name":"4.3.9","version":"4.3.9"},{"name":"4.4.0","version":"4.4.0"},{"name":"devel","version":"4.7.0"}]"#,
            "4.4.0",
        ),
        (
            "gt",
            "> 4.4.0",
            r#"[{"name":"4.4.0","version":"4.4.0"},{"name":"4.4.1","version":"4.4.1"}]"#,
            "4.4.1",
        ),
        (
            "lte",
            "<= 4.4.0",
            r#"[{"name":"4.4.0","version":"4.4.0"},{"name":"4.4.1","version":"4.4.1"}]"#,
            "4.4.0",
        ),
        (
            "lt",
            "< 4.4.0",
            r#"[{"name":"4.3.9","version":"4.3.9"},{"name":"4.4.0","version":"4.4.0"}]"#,
            "4.3.9",
        ),
    ] {
        let prefix = format!("ir-r-version-{label}-boundary");
        let out = run_with_private_r_install(
            &prefix,
            &["run", "--r-version", selector, "-e", "cat('ignored')"],
            expected,
            (expected, expected),
            Some(available),
            None,
            1,
        );
        assert_success(&out);
        assert_stdout_contains(&out, "selected=private-install");
    }
}

#[cfg(unix)]
#[test]
fn strict_comparison_r_versions_reject_equal_boundary() {
    let available = r#"[{"name":"4.4.0","version":"4.4.0"}]"#;
    for (label, selector) in [("gt", "> 4.4.0"), ("lt", "< 4.4.0")] {
        let prefix = format!("ir-r-version-strict-{label}-boundary");
        let out = run_with_private_r_install(
            &prefix,
            &["run", "--r-version", selector, "-e", "cat('ignored')"],
            "not-installed",
            ("4.4.0", "4.4.0"),
            Some(available),
            None,
            1,
        );
        assert_failure_contains(
            &out,
            &["no R release available through rig matches", selector],
        );
    }
}

#[cfg(unix)]
#[test]
fn release_selectors_install_resolved_release_and_reuse_it() {
    for (selector, version) in [
        ("release", "4.6.1"),
        ("oldrel", "4.5.2"),
        ("oldrel/2", "4.4.3"),
    ] {
        let prefix = format!("ir-{}-install", selector.replace('/', "-"));
        let out = run_with_private_r_install(
            &prefix,
            &["run", "--r-version", selector, "-e", "cat('ignored')"],
            version,
            (version, version),
            None,
            Some((selector, version)),
            2,
        );
        assert_success(&out);
        assert_stdout_contains(&out, "selected=private-install");
    }
}

#[cfg(unix)]
#[test]
fn cached_release_resolution_wins_over_moved_ambient_alias() {
    let cache_dir = temp_dir("ir-release-pin-cache");
    let bin_dir = temp_dir("ir-release-pin-bin");
    let pinned_dir = temp_dir("ir-release-pin-r");
    let moved_dir = temp_dir("ir-release-moved-r");
    let pinned_binary = selected_r_binary(&pinned_dir, "pinned-release");
    let moved_binary = selected_r_binary(&moved_dir, "moved-release");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "set -eu\n",
                "if [ \"$1\" = \"--user\" ]; then shift; else [ \"${{RIG_MODE:-}}\" != \"user\" ]; fi\n",
                "case \"$1\" in\n",
                "  --version) echo 'rig 0.10.0' ;;\n",
                "  list)\n",
                "    [ \"$2\" = \"--json\" ]\n",
                "    if [ \"${{RIG_MODE:-}}\" = \"user\" ]; then\n",
                "      echo '[]'\n",
                "    elif [ \"$IR_TEST_RUN_ID\" = \"second\" ]; then\n",
                "      printf '[{{\"name\":\"4.5.1\",\"version\":\"4.5.1\",\"aliases\":[],\"binary\":\"{}\"}},{{\"name\":\"4.6.0\",\"version\":\"4.6.0\",\"aliases\":[\"release\"],\"binary\":\"{}\"}}]\\n'\n",
                "    else\n",
                "      printf '[{{\"name\":\"4.5.1\",\"version\":\"4.5.1\",\"aliases\":[],\"binary\":\"{}\"}},{{\"name\":\"4.6.0\",\"version\":\"4.6.0\",\"aliases\":[],\"binary\":\"{}\"}}]\\n'\n",
                "    fi\n",
                "    ;;\n",
                "  resolve)\n",
                "    [ \"$2\" = \"--json\" ]\n",
                "    [ \"$3\" = \"--\" ]\n",
                "    [ \"$4\" = \"release\" ]\n",
                "    [ ! -f \"$RIG_R_INSTALL_DIR/resolve-ran\" ]\n",
                "    : > \"$RIG_R_INSTALL_DIR/resolve-ran\"\n",
                "    echo '[{{\"version\":\"4.5.1\"}}]'\n",
                "    ;;\n",
                "  add) echo unexpected add >&2; exit 65 ;;\n",
                "  *) exit 64 ;;\n",
                "esac\n",
            ),
            pinned_binary.display(),
            moved_binary.display(),
            pinned_binary.display(),
            moved_binary.display()
        ),
    );

    for run_id in ["first", "second"] {
        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .env("PATH", path_with_bin_dir(&bin_dir))
            .env("IR_TEST_RUN_ID", run_id)
            .env_remove("IR_RSCRIPT")
            .env_remove("RIG_MODE")
            .args(["run", "--r-version", "release", "-e", "cat('ignored')"])
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, "selected=pinned-release");
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("moved-release"),
            "{}",
            output_text(&out)
        );
    }
    assert_eq!(
        fs::read_to_string(cache_dir.join("rig").join("resolutions").join("release")).unwrap(),
        "4.5.1\n"
    );
}

#[cfg(unix)]
#[test]
fn development_selectors_install_symbolic_r_and_reuse_it() {
    for (selector, version) in [("devel", "4.7.0"), ("next", "4.6.2")] {
        let prefix = format!("ir-{selector}-install");
        let out = run_with_private_r_install(
            &prefix,
            &["run", "--r-version", selector, "-e", "cat('ignored')"],
            selector,
            (selector, version),
            None,
            None,
            2,
        );
        assert_success(&out);
        assert_stdout_contains(&out, "selected=private-install");
    }
}

#[cfg(unix)]
#[test]
fn oldrel_selector_reuses_matching_ambient_r_after_resolution() {
    let cache_dir = temp_dir("ir-oldrel-ambient-cache");
    let bin_dir = temp_dir("ir-oldrel-ambient-bin");
    let r_dir = temp_dir("ir-oldrel-ambient-r");
    let binary = selected_r_binary(&r_dir, "ambient-oldrel");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "set -eu\n",
                "if [ \"$1\" = \"--user\" ]; then shift; else [ \"${{RIG_MODE:-}}\" != \"user\" ]; fi\n",
                "case \"$1\" in\n",
                "  --version) echo 'rig 0.10.0' ;;\n",
                "  list)\n",
                "    if [ \"${{RIG_MODE:-}}\" = \"user\" ]; then\n",
                "      echo '[]'\n",
                "    else\n",
                "      printf '[{{\"name\":\"4.4.3\",\"version\":\"4.4.3\",\"aliases\":[],\"binary\":\"{}\"}}]\\n'\n",
                "    fi\n",
                "    ;;\n",
                "  resolve)\n",
                "    [ \"$2\" = \"--json\" ]\n",
                "    [ \"$3\" = \"--\" ]\n",
                "    [ \"$4\" = \"oldrel/2\" ]\n",
                "    [ ! -f \"$RIG_R_INSTALL_DIR/resolve-ran\" ]\n",
                "    : > \"$RIG_R_INSTALL_DIR/resolve-ran\"\n",
                "    echo '[{{\"version\":\"4.4.3\"}}]'\n",
                "    ;;\n",
                "  add) echo unexpected add >&2; exit 65 ;;\n",
                "  *) exit 64 ;;\n",
                "esac\n",
            ),
            binary.display()
        ),
    );

    for _ in 0..2 {
        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .env("PATH", path_with_bin_dir(&bin_dir))
            .env_remove("IR_RSCRIPT")
            .env_remove("RIG_MODE")
            .args(["run", "--r-version", "oldrel/2", "-e", "cat('ignored')"])
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, "selected=ambient-oldrel");
    }
}

#[cfg(unix)]
#[test]
fn unsupported_r_selectors_are_not_forwarded_to_rig_add() {
    for selector in [
        "https://example.invalid/R.pkg",
        "--platform=linux-ubuntu-24.04",
    ] {
        let out = run_with_private_r_install(
            "ir-unsupported-r-selector",
            &["run", "--r-version", selector, "-e", "cat('ignored')"],
            selector,
            ("4.4.3", "4.4.3"),
            None,
            None,
            1,
        );
        assert_failure_contains(&out, &["cannot automatically install", selector]);
    }
}

#[cfg(unix)]
#[test]
fn unsupported_r_selector_can_select_existing_ambient_alias() {
    let cache_dir = temp_dir("ir-custom-r-selector-cache");
    let bin_dir = temp_dir("ir-custom-r-selector-bin");
    let r_dir = temp_dir("ir-custom-r-selector-r");
    let binary = selected_r_binary(&r_dir, "custom-alias");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "set -eu\n",
                "[ \"$1\" = \"list\" ]\n",
                "[ \"$2\" = \"--json\" ]\n",
                "printf '[{{\"name\":\"4.4.3\",\"version\":\"4.4.3\",\"aliases\":[\"custom\"],\"binary\":\"{}\"}}]\\n'\n",
            ),
            binary.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_bin_dir(&bin_dir))
        .env_remove("IR_RSCRIPT")
        .args(["run", "--r-version", "custom", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=custom-alias");
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_selects_latest_available_minor_r() {
    let cache_dir = temp_dir("ir-exclude-newer-r-cache");
    let bin_dir = temp_dir("ir-exclude-newer-r-bin");
    let r43_dir = temp_dir("ir-exclude-newer-r43");
    let r44_dir = temp_dir("ir-exclude-newer-r44");

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
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_on_release_date_selects_that_minor_r() {
    let cache_dir = temp_dir("ir-exclude-newer-r-release-date-cache");
    let bin_dir = temp_dir("ir-exclude-newer-r-release-date-bin");
    let r43_dir = temp_dir("ir-exclude-newer-r-release-date-r43");
    let r44_dir = temp_dir("ir-exclude-newer-r-release-date-r44");

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
{{"name":"4.4.0","version":"4.4.0","aliases":[],"binary":"{}"}}
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
            "2024-04-24",
            "-e",
            "cat('ignored')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=r44");
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_installs_latest_available_minor_r() {
    let out = run_with_private_r_install(
        "ir-exclude-newer-install-latest-minor",
        &[
            "run",
            "--exclude-newer",
            "2026-03-20",
            "-e",
            "cat('ignored')",
        ],
        "4.5",
        ("4.5.3", "4.5.3"),
        None,
        None,
        1,
    );
    assert_success(&out);
    assert_stdout_contains(&out, "selected=private-install");
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_selects_latest_installed_patch_within_minor() {
    let out = run_with_installed_r_versions(
        "ir-exclude-newer-patch-date",
        &[("4.2.3", "r42"), ("4.3.2", "r432"), ("4.3.3", "r433")],
        &[
            "run",
            "--exclude-newer",
            "2024-01-15",
            "-e",
            "cat('ignored')",
        ],
    );
    assert_success(&out);
    assert_stdout_contains(&out, "selected=r433");
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_selects_r_4_0_for_2021_snapshot() {
    let cache_dir = temp_dir("ir-exclude-newer-r40-cache");
    let bin_dir = temp_dir("ir-exclude-newer-r40-bin");
    let r40_dir = temp_dir("ir-exclude-newer-r40");
    let r41_dir = temp_dir("ir-exclude-newer-r41");

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
}

#[cfg(unix)]
#[test]
fn run_with_exclude_newer_after_metadata_fetch_caches_actual_fetch_date() {
    let cache_dir = temp_dir("ir-exclude-newer-r-cache-fetch-date-cache");
    let bin_dir = temp_dir("ir-exclude-newer-r-cache-fetch-date-bin");
    let r46_dir = temp_dir("ir-exclude-newer-r-cache-fetch-date-r46");
    let r47_dir = temp_dir("ir-exclude-newer-r-cache-fetch-date-r47");
    let available_called = temp_path("ir-exclude-newer-r-cache-fetch-date-available", "txt");

    let r46_binary = selected_r_binary(&r46_dir, "r46");
    let r47_binary = selected_r_binary(&r47_dir, "r47");

    write_executable(
        &bin_dir.join("rig"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "case \"$*\" in\n",
                "  \"list --json\")\n",
                "    cat <<'JSON'\n",
                r#"[
{{"name":"4.6.0","version":"4.6.0","aliases":[],"binary":"{}"}},
{{"name":"4.7.0","version":"4.7.0","aliases":[],"binary":"{}"}}
]"#,
                "\nJSON\n",
                "    ;;\n",
                "  \"available --all --json\")\n",
                "    : > '{}'\n",
                "    cat <<'JSON'\n",
                r#"[
{{"name":"4.6.0","version":"4.6.0","date":"2026-04-24T00:00:00Z"}},
{{"name":"4.7.0","version":"4.7.0","date":"2026-06-18T00:00:00Z"}}
]"#,
                "\nJSON\n",
                "    ;;\n",
                "  *) exit 64 ;;\n",
                "esac\n",
            ),
            r46_binary.display(),
            r47_binary.display(),
            available_called.display()
        ),
    );

    let run = || {
        ir().env("IR_CACHE_DIR", &cache_dir)
            .env("PATH", path_with_bin_dir(&bin_dir))
            .env_remove("IR_RSCRIPT")
            .args([
                "run",
                "--exclude-newer",
                "2026-06-18",
                "-e",
                "cat('ignored')",
            ])
            .output()
            .unwrap()
    };

    let out = run();
    assert_success(&out);
    assert_stdout_contains(&out, "selected=r47");
    assert!(available_called.exists(), "{}", output_text(&out));

    let cache = fs::read_to_string(cache_dir.join("rig").join("minor-releases.json")).unwrap();
    assert!(
        cache.contains(&format!(r#""fetched_at": "{}""#, utc_today_string())),
        "{cache}"
    );
    fs::remove_file(&available_called).unwrap();
    let out = run();
    assert_success(&out);
    assert_stdout_contains(&out, "selected=r47");
    assert!(
        !available_called.exists(),
        "date-only exclude-newer should reuse refreshed minor-release cache"
    );
}

#[cfg(unix)]
#[test]
fn run_with_ir_rscript_and_exclude_newer_skips_rig_selection() {
    let cache_dir = temp_dir("ir-env-rscript-exclude-newer-cache");
    let bin_dir = temp_dir("ir-env-rscript-exclude-newer-bin");
    let rscript_dir = temp_dir("ir-env-rscript-exclude-newer-r");

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
    assert_stderr_lacks(&out, "unexpected rig");
}

#[cfg(unix)]
#[test]
fn env_rscript_overrides_frontmatter_r_version_without_rig() {
    let cache_dir = temp_dir("ir-env-rscript-frontmatter-r-version-cache");
    let bin_dir = temp_dir("ir-env-rscript-frontmatter-r-version-bin");
    let rscript_dir = temp_dir("ir-env-rscript-frontmatter-r-version-r");
    let script = temp_path("ir-env-rscript-frontmatter-r-version", "R");

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
    assert_stderr_lacks(&out, "unexpected rig");
}

#[cfg(unix)]
#[test]
fn cli_rscript_overrides_env_r_version_and_frontmatter_r_version() {
    let cache_dir = temp_dir("ir-cli-rscript-precedence-cache");
    let bin_dir = temp_dir("ir-cli-rscript-precedence-bin");
    let rscript_dir = temp_dir("ir-cli-rscript-precedence-r");
    let script = temp_path("ir-cli-rscript-precedence", "R");

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
    assert_stderr_lacks(&out, "unexpected rig");
}

#[cfg(unix)]
#[test]
fn cli_r_version_overrides_env_rscript() {
    let cache_dir = temp_dir("ir-cli-r-version-env-rscript-cache");
    let bin_dir = temp_dir("ir-cli-r-version-env-rscript-bin");
    let rscript_dir = temp_dir("ir-cli-r-version-env-rscript-r");
    let rig_r_dir = temp_dir("ir-cli-r-version-env-rscript-rig-r");

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
}

#[cfg(unix)]
#[test]
fn cli_r_selection_conflict_errors() {
    let rscript_dir = temp_dir("ir-cli-r-selection-conflict-r");
    let rscript = rscript_dir.join("Rscript");
    write_selected_rscript(&rscript, "unused");

    let out = ir()
        .args(["run", "--r-version", "4.4", "--rscript"])
        .arg(&rscript)
        .args(["-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_failure_contains(&out, &["cannot set both `--r-version` and `--rscript`"]);
}

#[cfg(unix)]
#[test]
fn env_r_selection_conflict_errors() {
    let rscript_dir = temp_dir("ir-env-r-selection-conflict-r");
    let rscript = rscript_dir.join("Rscript");
    write_selected_rscript(&rscript, "unused");

    let out = ir()
        .env("IR_RSCRIPT", &rscript)
        .env("IR_R_VERSION", "4.4")
        .args(["run", "-e", "cat('ignored')"])
        .output()
        .unwrap();

    assert_failure_contains(&out, &["cannot set both `IR_R_VERSION` and `IR_RSCRIPT`"]);
}

#[cfg(unix)]
#[test]
fn run_frontmatter_rscript_errors() {
    let cache_dir = temp_dir("ir-frontmatter-rscript-cache");
    let rscript_dir = temp_dir("ir-frontmatter-rscript-r");
    let script = temp_path("ir-frontmatter-rscript", "R");
    let rscript = rscript_dir.join("Rscript");
    write_selected_rscript(&rscript, "unused");
    fs::write(
        &script,
        format!("#| rscript: {}\ncat('ignored')\n", r_string(&rscript)),
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

    assert_failure_contains(
        &out,
        &[
            "frontmatter `rscript` is no longer supported",
            "Use `--rscript` or `IR_RSCRIPT` instead",
        ],
    );
}

#[cfg(unix)]
#[test]
fn render_frontmatter_rscript_errors() {
    let cache_dir = temp_dir("ir-render-frontmatter-rscript-cache");
    let quarto_dir = temp_dir("ir-render-frontmatter-rscript-quarto");
    let rscript_dir = temp_dir("ir-render-frontmatter-rscript-r");
    let doc = temp_path("ir-render-frontmatter-rscript", "qmd");
    let rscript = rscript_dir.join("Rscript");

    write_selected_rscript(&rscript, "unused");
    write_executable(&quarto_dir.join("quarto"), "#!/bin/sh\nexit 0\n");
    fs::write(
        &doc,
        format!(
            "---\ntitle: rscript render\nir:\n  rscript: {}\n---\n",
            r_string(&rscript)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_QUARTO", quarto_dir.join("quarto"))
        .env_remove("IR_RSCRIPT")
        .env_remove("IR_R_VERSION")
        .arg("render")
        .arg(&doc)
        .output()
        .unwrap();

    assert_failure_contains(
        &out,
        &[
            "frontmatter `ir.rscript` is no longer supported",
            "Use `--rscript` or `IR_RSCRIPT` instead",
        ],
    );
}

#[cfg(unix)]
#[test]
fn cli_rscript_with_exclude_newer_uses_snapshot_without_rig_selection() {
    let cache_dir = temp_dir("ir-cli-rscript-exclude-newer-cache");
    let bin_dir = temp_dir("ir-cli-rscript-exclude-newer-bin");
    let rscript_dir = temp_dir("ir-cli-rscript-exclude-newer-r");

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
    assert_stderr_lacks(&out, "unexpected rig");
}

#[cfg(unix)]
#[test]
fn render_cli_rscript_sets_quarto_r() {
    let cache_dir = temp_dir("ir-render-cli-rscript-cache");
    let rscript_dir = temp_dir("ir-render-cli-rscript-r");
    let quarto_dir = temp_dir("ir-render-cli-rscript-quarto-dir");
    let doc = temp_path("ir-render-cli-rscript", "qmd");
    let observed = temp_path("ir-render-cli-rscript-quarto-r", "txt");

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
}

#[cfg(unix)]
#[test]
fn exact_minor_r_version_with_exclude_newer_installs_without_available_query() {
    let out = run_with_private_r_install(
        "ir-exclude-newer-exact-minor-install",
        &[
            "run",
            "--r-version",
            "== 4.4",
            "--exclude-newer",
            "2024-06-20",
            "-e",
            "cat('ignored')",
        ],
        "4.4",
        ("4.4.3", "4.4.3"),
        None,
        None,
        1,
    );
    assert_success(&out);
    assert_stdout_contains(&out, "selected=private-install");
}

#[cfg(unix)]
#[test]
fn run_without_r_version_uses_rscript_on_path_when_rig_has_default() {
    let cache_dir = temp_dir("ir-path-rscript-cache");
    let bin_dir = temp_dir("ir-path-rscript-bin");
    let rig_dir = temp_dir("ir-path-rscript-rig");

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
}

#[cfg(unix)]
#[test]
fn run_without_r_version_skips_non_executable_rscript_on_path() {
    let cache_dir = temp_dir("ir-path-rscript-executable-cache");
    let stale_dir = temp_dir("ir-path-rscript-stale-bin");
    let bin_dir = temp_dir("ir-path-rscript-valid-bin");

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
}

#[cfg(unix)]
#[test]
fn render_without_r_version_pins_quarto_to_rscript_on_path() {
    let cache_dir = temp_dir("ir-render-path-rscript-cache");
    let bin_dir = temp_dir("ir-render-path-rscript-bin");
    let doc = temp_path("ir-render-path-rscript", "qmd");

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
}

#[cfg(windows)]
#[test]
fn run_without_r_version_uses_rscript_bat_on_path() {
    let cache_dir = temp_dir("ir-path-rscript-bat-cache");
    let bin_dir = temp_dir("ir-path-rscript-bat-bin");

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
}

#[cfg(windows)]
#[test]
fn run_without_r_version_ignores_extensionless_rscript_on_path() {
    let cache_dir = temp_dir("ir-path-rscript-extensionless-cache");
    let stale_dir = temp_dir("ir-path-rscript-extensionless-stale");
    let bin_dir = temp_dir("ir-path-rscript-extensionless-valid");

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
}

#[cfg(windows)]
#[test]
fn run_without_r_version_skips_unsupported_pathext_rscript_on_path() {
    let cache_dir = temp_dir("ir-path-rscript-unsupported-pathext-cache");
    let stale_dir = temp_dir("ir-path-rscript-unsupported-pathext-stale");
    let bin_dir = temp_dir("ir-path-rscript-unsupported-pathext-valid");

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
}

#[cfg(windows)]
#[test]
fn run_with_extended_rscript_command_skips_pathext_expansion() {
    let cache_dir = temp_dir("ir-extended-rscript-command-cache");
    let stale_dir = temp_dir("ir-extended-rscript-command-stale");
    let bin_dir = temp_dir("ir-extended-rscript-command-valid");

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
}

#[cfg(windows)]
#[test]
fn run_without_r_version_ignores_non_rscript_batch_targets() {
    let cache_dir = temp_dir("ir-path-rscript-helper-target-cache");
    let bin_dir = temp_dir("ir-path-rscript-helper-target-bin");
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
}

#[cfg(windows)]
#[test]
fn run_without_r_version_does_not_cache_unresolved_rscript_bat() {
    let cache_dir = temp_dir("ir-path-rscript-bat-cache-miss");
    let bin_dir = temp_dir("ir-path-rscript-bat-bin");
    let library = temp_dir("ir-path-rscript-bat-library");
    let resolver_marker = temp_path("ir-path-rscript-bat-resolver", "txt");
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
}

#[cfg(windows)]
#[test]
fn render_without_r_version_pins_quarto_to_rscript_bat_target() {
    let cache_dir = temp_dir("ir-render-rscript-bat-target-cache");
    let bin_dir = temp_dir("ir-render-rscript-bat-target-bin");
    let doc = temp_path("ir-render-rscript-bat-target", "qmd");
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
}
