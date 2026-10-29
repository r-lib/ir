//! Automatic R installation through the public `ir` CLI.

#![cfg(unix)]

mod support;

use std::fs;
use std::process::Command;

use serde_json::json;
use support::*;

struct RigFixture {
    root: TempPath,
    install_request: String,
}

impl RigFixture {
    fn new(install_request: &str, installed_version: &str) -> Self {
        let root = temp_dir("ir-rig-installation");
        let bin = root.join("bin");
        let r_bin = root.join("user R").join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&r_bin).unwrap();
        fs::write(root.join("calls"), "").unwrap();
        fs::write(
            root.join("installed.json"),
            json!({
                "name": installed_version,
                "version": installed_version,
                "aliases": ["release"],
                "default": false,
                "path": r_bin.parent().unwrap(),
                "binary": r_bin.join("R"),
                "new-install": true,
            })
            .to_string(),
        )
        .unwrap();
        fs::write(root.join("available.json"), "[]").unwrap();
        write_executable(
            &r_bin.join("Rscript"),
            concat!(
                "#!/bin/sh\n",
                "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
                "  : > \"$IR_RESOLVE_RESULT_FILE\"\n",
                "  exit 0\n",
                "fi\n",
                "printf 'selected=user\\n'\n",
            ),
        );
        write_executable(
            &bin.join("rig"),
            concat!(
                "#!/bin/sh\n",
                "set -eu\n",
                "printf '%s\\n' \"$*\" >> \"$IR_TEST_RIG_ROOT/calls\"\n",
                // IR must leave rig's default paths and explicit configuration alone.
                "test \"${RIG_R_INSTALL_DIR:-}\" = \"${IR_TEST_RIG_INSTALL_DIR:-}\"\n",
                "test \"${RIG_BINARY_DIR:-}\" = \"${IR_TEST_RIG_BINARY_DIR:-}\"\n",
                "case \"$*\" in\n",
                "  'list --json') printf '[]\\n' ;;\n",
                "  'list --json --user')\n",
                "    printf '['\n",
                "    if [ -f \"$IR_TEST_RIG_ROOT/installed\" ]; then\n",
                "      /bin/cat \"${IR_TEST_RIG_USER_JSON:-$IR_TEST_RIG_ROOT/installed.json}\"\n",
                "    fi\n",
                "    printf ']\\n' ;;\n",
                "  'available --all --json --user')\n",
                "    /bin/cat \"$IR_TEST_RIG_ROOT/available.json\" ;;\n",
                "  \"add --user --no-default --without-path --without-pak --json $IR_TEST_RIG_VERSION\")\n",
                "    if [ \"${IR_TEST_RIG_FAIL_INSTALL:-}\" = 1 ]; then\n",
                "      printf 'download failed for requested R\\n' >&2\n",
                "      exit 41\n",
                "    fi\n",
                "    : > \"$IR_TEST_RIG_ROOT/installed\"\n",
                "    /bin/cat \"$IR_TEST_RIG_ROOT/installed.json\" ;;\n",
                "  *) printf 'unexpected rig invocation: %s\\n' \"$*\" >&2; exit 42 ;;\n",
                "esac\n",
            ),
        );
        Self {
            root,
            install_request: install_request.into(),
        }
    }

    fn command(&self) -> Command {
        let mut command = ir();
        command
            // No ambient Rscript or real rig can be reached by these tests.
            .env("PATH", self.root.join("bin"))
            .env("IR_CACHE_DIR", self.root.join("cache"))
            .env("IR_TEST_RIG_ROOT", &self.root)
            .env("IR_TEST_RIG_VERSION", &self.install_request)
            .env("RIG_MODE", "admin")
            .env_remove("RIG_R_INSTALL_DIR")
            .env_remove("RIG_BINARY_DIR")
            .env_remove("IR_RSCRIPT")
            .env_remove("IR_R_VERSION")
            .env_remove("IR_EXCLUDE_NEWER");
        command
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.root.join("calls"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn available_releases(&self) {
        fs::write(
            self.root.join("available.json"),
            json!([
                {"name": "4.2.3", "version": "4.2.3", "type": "release", "url": "https://example.com/r-4.2.3"},
                {"name": "4.4.2", "version": "4.4.2", "type": "release", "url": "https://example.com/r-4.4.2"},
                {"name": "4.4.3", "version": "4.4.3", "type": "release", "url": "https://example.com/r-4.4.3"},
                {"name": "4.5.3", "version": "4.5.3", "type": "release", "url": "https://example.com/r-4.5.3"},
                {"name": "4.6.1", "version": "4.6.1", "type": "release", "url": "https://example.com/r-4.6.1"},
                {"name": "4.6.2", "version": "4.6.2", "type": "release", "url": null},
                {"name": "next", "version": "4.6.3", "type": "rc", "url": "https://example.com/r-next"},
                {"name": "devel", "version": "4.7.0", "type": "devel", "url": "https://example.com/r-devel"}
            ])
            .to_string(),
        )
        .unwrap();
    }
}

#[test]
fn missing_requested_r_is_installed_in_user_mode_and_reused() {
    for requirement in ["4.4", "== 4.4", "4.4.3"] {
        let install_request = requirement.trim_start_matches("== ");
        let fixture = RigFixture::new(install_request, "4.4.3");
        for _ in 0..2 {
            let out = fixture
                .command()
                .args(["run", "--r-version", requirement, "-e", "1"])
                .output()
                .unwrap();
            assert_success(&out);
            assert_stdout_contains(&out, "selected=user");
        }
        assert_eq!(
            fixture.calls(),
            [
                "list --json".into(),
                "list --json --user".into(),
                format!(
                    "add --user --no-default --without-path --without-pak --json {install_request}"
                ),
                "list --json".into(),
                "list --json --user".into(),
            ]
        );
    }
}

#[test]
fn no_r_on_path_installs_release_with_rigs_configured_directories() {
    let fixture = RigFixture::new("release", "4.6.1");
    let install_dir = fixture.root.join("configured R installs");
    let binary_dir = fixture.root.join("configured links");
    let out = fixture
        .command()
        .env("RIG_R_INSTALL_DIR", &install_dir)
        .env("RIG_BINARY_DIR", &binary_dir)
        .env("IR_TEST_RIG_INSTALL_DIR", &install_dir)
        .env("IR_TEST_RIG_BINARY_DIR", &binary_dir)
        .args(["run", "-e", "1"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "selected=user");
    assert_eq!(
        fixture.calls(),
        [
            "list --json",
            "list --json --user",
            "add --user --no-default --without-path --without-pak --json release"
        ]
    );
}

#[test]
fn missing_r_range_installs_highest_available_matching_release() {
    for (requirement, selected) in [
        (">= 4.4", "4.6.1"),
        ("> 4.4", "4.6.1"),
        ("< 4.5", "4.4.3"),
        ("<= 4.4.3", "4.4.3"),
        ("4", "4.6.1"),
        ("== 4", "4.6.1"),
    ] {
        let fixture = RigFixture::new(selected, selected);
        fixture.available_releases();
        let out = fixture
            .command()
            .args(["run", "--r-version", requirement, "-e", "1"])
            .output()
            .unwrap();
        assert_success(&out);
        assert_stdout_contains(&out, "selected=user");
        assert_eq!(fixture.calls()[2], "available --all --json --user");
        assert_eq!(
            fixture.calls()[3],
            format!("add --user --no-default --without-path --without-pak --json {selected}")
        );
    }
}

#[test]
fn broad_requirement_does_not_reuse_named_user_development_builds() {
    for (name, aliases) in [
        ("devel", vec![]),
        ("next", vec![]),
        ("devel-x86_64", vec!["devel-x86_64"]),
        ("next-x86_64", vec!["next-x86_64"]),
        ("4.7-x86_64", vec!["devel-x86_64"]),
    ] {
        let fixture = RigFixture::new("4.6.1", "4.6.1");
        fixture.available_releases();
        fs::write(fixture.root.join("installed"), "").unwrap();
        let user_json = fixture.root.join("user.json");
        fs::write(
            &user_json,
            json!({
                "name": name,
                "version": "4.7.0",
                "aliases": aliases,
                "binary": fixture.root.join("user R/bin/R"),
            })
            .to_string(),
        )
        .unwrap();
        let out = fixture
            .command()
            .env("IR_TEST_RIG_USER_JSON", &user_json)
            .args(["run", "--r-version", ">= 4.2", "-e", "1"])
            .output()
            .unwrap();
        assert_success(&out);
        assert_eq!(
            fixture.calls(),
            [
                "list --json",
                "list --json --user",
                "available --all --json --user",
                "add --user --no-default --without-path --without-pak --json 4.6.1",
            ],
            "selected development installation {name} for a broad requirement"
        );
    }
}

#[test]
fn missing_r_range_with_exclude_newer_installs_a_compatible_minor() {
    let fixture = RigFixture::new("4.4.3", "4.4.3");
    fixture.available_releases();
    let out = fixture
        .command()
        .args([
            "run",
            "--r-version",
            ">= 4.2",
            "--exclude-newer",
            "2024-06-20",
            "-e",
            "1",
        ])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "selected=user");
    assert_eq!(
        fixture.calls().last().unwrap(),
        "add --user --no-default --without-path --without-pak --json 4.4.3"
    );
}

#[test]
fn date_only_r_selection_installs_the_latest_minor_released_by_that_date() {
    let fixture = RigFixture::new("4.4", "4.4.3");
    let out = fixture
        .command()
        .args(["run", "--exclude-newer", "2024-06-20", "-e", "1"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "selected=user");
    assert_eq!(
        fixture.calls().last().unwrap(),
        "add --user --no-default --without-path --without-pak --json 4.4"
    );
}

#[test]
fn failed_r_installation_reports_rig_error_without_running_r() {
    let fixture = RigFixture::new("4.4", "4.4.3");
    let out = fixture
        .command()
        .env("IR_TEST_RIG_FAIL_INSTALL", "1")
        .args(["run", "--r-version", "4.4", "-e", "1"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", output_text(&out));
    assert!(out.stdout.is_empty(), "{}", output_text(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("download failed for requested R"),
        "{}",
        output_text(&out)
    );
}

#[test]
fn installed_r_must_match_the_requested_installation() {
    for (requirement, install_request, reported) in
        [("4.4", "4.4", "4.6.1"), (">= 4.4", "4.6.1", "4.5.3")]
    {
        let fixture = RigFixture::new(install_request, reported);
        fixture.available_releases();
        let out = fixture
            .command()
            .args(["run", "--r-version", requirement, "-e", "1"])
            .output()
            .unwrap();
        assert!(!out.status.success(), "{}", output_text(&out));
        assert!(out.stdout.is_empty(), "{}", output_text(&out));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(install_request), "{}", output_text(&out));
        assert!(stderr.contains(reported), "{}", output_text(&out));
        assert_eq!(
            fixture.calls().last().unwrap(),
            &format!(
                "add --user --no-default --without-path --without-pak --json {install_request}"
            )
        );
    }
}

#[test]
fn option_like_r_version_is_rejected_before_calling_rig() {
    let fixture = RigFixture::new("--admin", "4.6.1");
    let out = fixture
        .command()
        .args(["run", "--r-version=--admin", "-e", "1"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", output_text(&out));
    assert!(out.stdout.is_empty(), "{}", output_text(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--admin"),
        "{}",
        output_text(&out)
    );
    assert!(fixture.calls().is_empty(), "{:?}", fixture.calls());
}

#[test]
fn unavailable_r_range_does_not_attempt_installation() {
    let fixture = RigFixture::new("unexpected", "9.0.0");
    fixture.available_releases();
    let out = fixture
        .command()
        .args(["run", "--r-version", ">= 9", "-e", "1"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", output_text(&out));
    assert!(out.stdout.is_empty(), "{}", output_text(&out));
    assert_eq!(
        fixture.calls(),
        [
            "list --json",
            "list --json --user",
            "available --all --json --user"
        ]
    );
}

#[test]
fn missing_explicit_rscript_does_not_install_r() {
    let fixture = RigFixture::new("release", "4.6.1");
    for from_env in [false, true] {
        let missing = fixture.root.join("missing Rscript");
        let mut command = fixture.command();
        command.arg("run");
        if from_env {
            command.env("IR_RSCRIPT", &missing);
        } else {
            command.arg("--rscript").arg(&missing);
        }
        let out = command.args(["-e", "1"]).output().unwrap();
        assert!(!out.status.success(), "{}", output_text(&out));
        assert!(out.stdout.is_empty(), "{}", output_text(&out));
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("missing Rscript"),
            "{}",
            output_text(&out)
        );
        assert!(fixture.calls().is_empty());
    }
}

#[test]
fn render_uses_newly_installed_rscript() {
    let fixture = RigFixture::new("4.4", "4.4.3");
    let document = fixture.root.join("report.qmd");
    let quarto = fixture.root.join("bin").join("quarto");
    fs::write(
        &document,
        "---\ntitle: report\nir:\n  r-version: '4.4'\n---\n",
    )
    .unwrap();
    write_executable(
        &quarto,
        concat!(
            "#!/bin/sh\n",
            "test \"$1\" = render || exit 64\n",
            "test \"$QUARTO_R\" = \"$IR_TEST_EXPECTED_RSCRIPT\" || exit 65\n",
            "printf 'quarto selected=user\\n'\n",
        ),
    );
    let out = fixture
        .command()
        .env("IR_QUARTO", &quarto)
        .env(
            "IR_TEST_EXPECTED_RSCRIPT",
            fixture.root.join("user R/bin/Rscript"),
        )
        .arg("render")
        .arg(&document)
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "quarto selected=user");
    assert_eq!(
        fixture.calls().last().unwrap(),
        "add --user --no-default --without-path --without-pak --json 4.4"
    );
}
