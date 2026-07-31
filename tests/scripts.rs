mod support;

use support::{rscript, temp_cache, temp_dir, temp_path};

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn output_text(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{}", output_text(output));
}

fn assert_stdout_contains(output: &Output, expected: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(expected),
        "stdout did not contain {expected:?}\n{}",
        output_text(output)
    );
}

#[test]
fn readme_install_commands_are_copyable() {
    let readme = fs::read_to_string(repo_root().join("README.md"))
        .unwrap()
        .replace("\r\n", "\n");
    let install = readme
        .split_once("## Install\n")
        .and_then(|(_, rest)| rest.split_once("\n## Development setup"))
        .map(|(section, _)| section)
        .unwrap_or_else(|| panic!("README.md should have an Install section"));

    assert!(
        !install
            .lines()
            .any(|line| line.starts_with("$ ") || line.starts_with("> ")),
        "{install}"
    );
}

#[test]
fn public_windows_install_guidance_recommends_scoop() {
    for file in ["README.md", "docs/config.qmd"] {
        let text = fs::read_to_string(repo_root().join(file)).unwrap();
        let add_bucket = text
            .find("scoop bucket add r-bucket https://github.com/cderv/r-bucket.git")
            .unwrap_or_else(|| panic!("{file} should add the Scoop bucket"));
        let install = text
            .find("scoop install ir")
            .unwrap_or_else(|| panic!("{file} should install ir with Scoop"));
        assert!(add_bucket < install, "{file}");
        assert!(
            text.contains("https://raw.githubusercontent.com/r-lib/ir/main/scripts/install.ps1"),
            "{file}"
        );
    }
}

#[test]
fn public_ir_links_use_r_lib_owner() {
    for file in [
        "README.md",
        "docs/config.qmd",
        "scripts/install.sh",
        "scripts/install.ps1",
    ] {
        let text = fs::read_to_string(repo_root().join(file)).unwrap();
        assert!(!text.contains("t-kalinowski/ir"), "{file}");
        assert!(!text.contains("t-kalinowski.github.io/ir"), "{file}");
        assert!(
            !text.contains("raw.githubusercontent.com/t-kalinowski/ir"),
            "{file}"
        );
    }

    let readme = fs::read_to_string(repo_root().join("README.md")).unwrap();
    assert!(readme.contains("https://r-lib.github.io/ir/"), "{readme}");
    assert!(
        readme.contains("https://raw.githubusercontent.com/r-lib/ir/main/scripts/install.sh"),
        "{readme}"
    );
    assert!(
        readme.contains("https://raw.githubusercontent.com/r-lib/ir/main/scripts/install.ps1"),
        "{readme}"
    );

    let sh = fs::read_to_string(repo_root().join("scripts/install.sh")).unwrap();
    assert!(sh.contains("OWNER=\"r-lib\""), "{sh}");

    let ps1 = fs::read_to_string(repo_root().join("scripts/install.ps1")).unwrap();
    assert!(ps1.contains("$Owner = \"r-lib\""), "{ps1}");
}

#[test]
fn public_installers_recommend_rig_when_missing() {
    let sh_path = repo_root().join("scripts/install.sh");
    let sh = fs::read_to_string(&sh_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", sh_path.display()));
    assert!(sh.contains("command -v rig"), "{sh}");
    assert!(sh.contains("rig was not found on PATH."), "{sh}");
    assert!(sh.contains("brew install --cask rig"), "{sh}");
    assert!(sh.contains("sudo apt install r-rig"), "{sh}");
    assert!(sh.contains("https://github.com/r-lib/rig#id-macos"), "{sh}");
    assert!(sh.contains("https://github.com/r-lib/rig#id-linux"), "{sh}");

    let ps1_path = repo_root().join("scripts/install.ps1");
    let ps1 = fs::read_to_string(&ps1_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", ps1_path.display()));
    assert!(ps1.contains("Get-Command rig"), "{ps1}");
    assert!(ps1.contains("rig was not found on PATH."), "{ps1}");
    assert!(ps1.contains("winget install --id posit.rig"), "{ps1}");
    assert!(
        ps1.contains("https://github.com/r-lib/rig#id-windows"),
        "{ps1}"
    );
}

#[cfg(unix)]
fn dev_deps_sh_plan(platform: &str) -> Output {
    Command::new("sh")
        .current_dir(repo_root())
        .args([
            "scripts/install-dev-deps.sh",
            "--dry-run",
            "--platform",
            platform,
        ])
        .output()
        .unwrap()
}

#[cfg(unix)]
fn dev_deps_sh_plan_with_args(args: &[&str]) -> Output {
    Command::new("sh")
        .current_dir(repo_root())
        .arg("scripts/install-dev-deps.sh")
        .args(args)
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn install_dev_deps_sh_prints_linux_plan() {
    let out = dev_deps_sh_plan("linux-deb");

    assert_success(&out);
    assert_stdout_contains(&out, "apt-get install");
    assert_stdout_contains(&out, "https://sh.rustup.rs");
    assert_stdout_contains(&out, "https://rig.r-pkg.org/deb/rig.gpg");
    assert_stdout_contains(&out, "quarto-linux-");
    assert_stdout_contains(&out, "rig add release");
    assert_stdout_contains(&out, "rig add 4.3");
    assert_stdout_contains(&out, "rig list --json");
    assert_stdout_contains(&out, "IR_TEST_R_VERSION=<resolved-4.3-version>");
    assert_stdout_contains(&out, "IR_TEST_R_EXCLUDE_NEWER=<release-date-for-4.3>");
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("rig default release"),
        "{}",
        output_text(&out)
    );
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("rig run -r 4.4.3"),
        "{}",
        output_text(&out)
    );
}

#[cfg(unix)]
#[test]
fn install_dev_deps_sh_prints_macos_plan() {
    let out = dev_deps_sh_plan("macos");

    assert_success(&out);
    assert_stdout_contains(&out, "xcode-select --install");
    assert_stdout_contains(&out, "https://sh.rustup.rs");
    assert_stdout_contains(
        &out,
        "https://github.com/r-lib/rig/releases/download/<latest-rig-tag>/rig-<latest-rig-version>-macOS-<macos-arch>.pkg",
    );
    assert_stdout_contains(&out, "installer -pkg /tmp/ir-rig.pkg -target /");
    assert_stdout_contains(&out, "brew install --cask quarto");
    assert_stdout_contains(&out, "rig add release");
    assert_stdout_contains(&out, "rig add 4.3");
    assert_stdout_contains(&out, "rig list --json");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("brew tap r-lib/rig"), "{stdout}");
    assert!(!stdout.contains("brew install --cask rig"), "{stdout}");
    assert!(
        !stdout.contains("rig default release"),
        "{}",
        output_text(&out)
    );
    assert!(
        !stdout.contains("rig run -r 4.4.3"),
        "{}",
        output_text(&out)
    );
}

#[cfg(unix)]
#[test]
fn install_dev_deps_sh_can_skip_action_managed_tools_for_ci() {
    let out = dev_deps_sh_plan_with_args(&[
        "--dry-run",
        "--platform",
        "linux-deb",
        "--skip",
        "rust",
        "--skip",
        "python",
        "--skip",
        "quarto",
        "--skip",
        "r-release",
    ]);

    assert_success(&out);
    assert_stdout_contains(&out, "https://rig.r-pkg.org/deb/rig.gpg");
    assert_stdout_contains(&out, "rig add 4.3");
    assert_stdout_contains(&out, "rig list --json");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("https://sh.rustup.rs"), "{stdout}");
    assert!(!stdout.contains("python3 python3-venv"), "{stdout}");
    assert!(!stdout.contains("quarto-linux-"), "{stdout}");
    assert!(!stdout.contains("rig add release"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn install_dev_deps_sh_can_skip_test_r() {
    let out =
        dev_deps_sh_plan_with_args(&["--dry-run", "--platform", "linux-deb", "--skip", "test-r"]);

    assert_success(&out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("rig add 4.3"), "{stdout}");
    assert!(!stdout.contains("IR_TEST_R_VERSION"), "{stdout}");
    assert!(!stdout.contains("IR_TEST_R_EXCLUDE_NEWER"), "{stdout}");
}

#[test]
fn ci_uses_dev_deps_script_for_non_default_r_setup() {
    let path = repo_root().join(".github/workflows/ci.yml");
    let workflow = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    assert!(workflow.contains("scripts/install-dev-deps.sh"));
    assert!(workflow.contains("Keep the GitHub setup actions above"));
    assert!(workflow.contains("scripts\\install-dev-deps.ps1"));
    assert!(workflow.contains("Install rig and non-default R (Unix)"));
    assert!(workflow.contains("Install rig and non-default R (Windows)"));
    assert!(workflow.contains("-Skip rust, python, quarto, r-release"));
    assert!(workflow.contains("GITHUB_TOKEN: ${{ github.token }}"));
    assert!(!workflow.contains("IR_TEST_R_VERSION=4.4.3"));
    assert!(!workflow.contains("IR_TEST_R_EXCLUDE_NEWER=2025-02-28"));
    assert!(workflow.contains("any::bookdown"));
    assert!(workflow.contains("any::xfun"));
    assert!(workflow.contains("${cran%/latest}/2024-02-01"));
    assert!(workflow.contains("tidyr@1.3.1"));
    assert!(workflow.contains("gt@0.10.1"));
    assert!(workflow.contains("taiki-e/install-action@nextest"));
    assert!(!workflow
        .lines()
        .any(|line| line == "  R_USER_CACHE_DIR: ${{ github.workspace }}/.cache"));
    assert!(!workflow
        .lines()
        .any(|line| line == "      R_USER_CACHE_DIR: ${{ runner.temp }}/ir-r-user-cache"));
    assert!(workflow.contains("id: r-cache-date"));
    assert!(workflow.contains("shell: Rscript {0}"));
    assert!(workflow.contains("Sys.getenv(\"RUNNER_TEMP\")"));
    assert!(workflow.contains("R_USER_CACHE_DIR="));
    assert!(workflow.contains("file = Sys.getenv(\"GITHUB_ENV\")"));
    assert!(workflow.contains("tz = \"UTC\""));
    assert!(workflow.contains("actions/cache@v4"));
    assert!(workflow.contains("path: ${{ runner.temp }}/ir-r-user-cache"));
    assert!(workflow
        .contains("key: r-user-cache-${{ runner.os }}-${{ steps.r-cache-date.outputs.date }}-"));
    assert!(workflow.contains("cache-version: ${{ steps.r-cache-date.outputs.date }}"));
    let r_user_cache = workflow
        .split("      - name: Cache R user cache")
        .nth(1)
        .and_then(|block| block.split("      # No DESCRIPTION").next())
        .expect("workflow should cache the R user cache before installing R dependencies");
    assert!(
        !r_user_cache.contains("restore-keys:"),
        "daily CI cache should not restore older cache buckets"
    );
    assert!(workflow.contains("Warm default R package cache"));
    assert!(workflow.contains("Warm snapshot R package cache"));
    assert!(workflow.contains("Warm non-default R package cache"));
    assert!(workflow.contains("cran=\"${RSPM:-https://packagemanager.posit.co/cran/latest}\""));
    assert!(workflow.contains("--repos \"${cran%/latest}/2026-06-01\""));
    assert!(workflow.contains("github::rstudio/reticulate fansi"));
    assert!(workflow.contains("rmarkdown xfun quarto"));
    assert!(workflow.contains("rmarkdown bookdown tinytex xfun"));
    assert!(workflow.contains("\"$IR_TEST_RSCRIPT\" scripts/warm-renv-cache.R"));
    assert!(workflow.contains("shell: bash"));
    assert!(workflow.contains("R_PROFILE_USER"));
    assert!(workflow.contains("scripts/ci-rprofile.R"));
    assert!(workflow.contains("scripts/warm-renv-cache.R"));
    let warm_non_default_cache = workflow
        .split("      - name: Warm non-default R package cache")
        .nth(1)
        .and_then(|block| block.split("      - run: cargo nextest").next())
        .expect("workflow should warm the non-default R package cache before tests");
    assert!(
        warm_non_default_cache.contains("--repos \"${cran%/latest}/${IR_TEST_R_EXCLUDE_NEWER}\"")
    );
    assert!(!warm_non_default_cache.contains("2026-06-01"));
    assert!(warm_non_default_cache.contains("R_LIBS_USER: ${{ runner.temp }}/ir-test-r-library"));
    let warm_default_cache = workflow
        .split("      - name: Warm default R package cache")
        .nth(1)
        .and_then(|block| {
            block
                .split("      - name: Warm snapshot R package cache")
                .next()
        })
        .expect(
            "workflow should have a default cache warm step before the snapshot cache warm step",
        );
    assert!(warm_default_cache.contains("GITHUB_PAT: ${{ github.token }}"));
    assert!(!warm_default_cache.contains("R_PROFILE_USER"));
    assert!(!workflow.contains("bookdown btw Rapp"));
    assert!(!workflow.contains("Warm default R package cache (Unix)"));
    assert!(!workflow.contains("Warm default R package cache (Windows)"));
    assert!(workflow.contains("cargo nextest run --verbose --no-fail-fast"));
    assert!(!workflow.contains("cargo build --verbose"));
    assert!(!workflow.contains("Warm GitHub R package cache"));
    assert!(!workflow.contains("withr@"));
    assert!(!workflow.contains("reticulate github::rstudio/reticulate"));
    assert!(!workflow.contains("github::rstudio/reticulate reticulate"));
    assert!(!workflow.contains("github::rstudio/reticulate@"));
    assert!(!workflow.contains("scripts/warm-r-version-cache.R"));
    assert!(!workflow.contains("cargo run --bin ir -- run --isolated --vanilla"));
    assert!(!workflow.contains("--r-version \"$IR_TEST_R_VERSION\""));
    assert!(
        !workflow.contains("-Skip rust `\n            -Skip python"),
        "PowerShell array parameters must be passed in one binding"
    );
    assert!(!workflow.contains("#32"));
    assert!(!workflow.contains(r"\\?\"));
    assert!(!workflow.contains("Install rig (Linux)"));
    assert!(!workflow.contains("Install rig (macOS)"));
    assert!(!workflow.contains("Warm resolver tooling for the non-default R"));
    assert!(!workflow.contains("pak::pkg_install(c(\"pak\", \"renv\", \"secretbase\"))"));

    let warm_script_path = repo_root().join("scripts/warm-renv-cache.R");
    let warm_script = fs::read_to_string(&warm_script_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", warm_script_path.display()));
    assert!(warm_script.contains("Sys.getenv(\"R_LIBS_USER\", unset = \"\")"));
    assert!(warm_script.contains("dir.create(user_lib, recursive = TRUE, showWarnings = FALSE)"));
    assert!(warm_script.contains(".libPaths(c(user_libs, .libPaths()))"));
    assert!(warm_script.contains("Sys.setenv(RENV_PATHS_SOURCE = source_cache)"));
    assert!(warm_script.contains("pak::repo_resolve(\"PPM@latest\")"));
    assert!(!warm_script.contains("https://cran.r-project.org"));
}

#[cfg(target_os = "linux")]
#[test]
fn warm_renv_cache_replaces_unnamed_at_cran_with_real_package() {
    let renv_cache = temp_cache("ir-warm-real-renv-cache");
    let user_library = temp_dir("ir-warm-real-user-library");
    let profile = temp_path("ir-warm-real-profile", "R");
    fs::write(&profile, "options(repos = \"@CRAN@\")\n").unwrap();

    let out = Command::new(rscript())
        .current_dir(repo_root())
        .env("RENV_PATHS_CACHE", &renv_cache)
        .env("R_LIBS_USER", &user_library)
        .env("R_PROFILE_USER", &profile)
        .env("CC", "false")
        .env("CXX", "false")
        .env("CXX11", "false")
        .env("CXX14", "false")
        .env("CXX17", "false")
        .env("CXX20", "false")
        .args(["scripts/warm-renv-cache.R", "zip"])
        .output()
        .unwrap();

    assert_success(&out);
}

#[test]
fn warm_renv_cache_ignores_corrupt_user_source_cache() {
    let renv_cache = temp_cache("ir-warm-corrupt-renv-cache");
    let user_library = temp_dir("ir-warm-corrupt-user-library");
    let user_cache = temp_dir("ir-warm-corrupt-user-cache");
    let profile = temp_path("ir-warm-corrupt-profile", "R");
    fs::write(&profile, "options(repos = \"@CRAN@\")\n").unwrap();

    let corrupt_archive = user_cache
        .join("R")
        .join("renv")
        .join("source")
        .join("repository")
        .join("cli")
        .join("cli_3.6.6.tar.gz");
    fs::create_dir_all(corrupt_archive.parent().unwrap()).unwrap();
    fs::write(&corrupt_archive, b"partial archive").unwrap();

    let out = Command::new(rscript())
        .current_dir(repo_root())
        .env("RENV_PATHS_CACHE", &renv_cache)
        .env("R_USER_CACHE_DIR", &user_cache)
        .env("R_LIBS_USER", &user_library)
        .env("R_PROFILE_USER", &profile)
        .env("CC", "false")
        .env("CXX", "false")
        .env("CXX11", "false")
        .env("CXX14", "false")
        .env("CXX17", "false")
        .env("CXX20", "false")
        .args(["scripts/warm-renv-cache.R", "zip"])
        .output()
        .unwrap();

    assert_success(&out);
}

#[cfg(target_os = "linux")]
#[test]
fn warm_renv_cache_rewrites_plain_ppm_latest_with_real_binary_package() {
    let renv_cache = temp_cache("ir-warm-real-ppm-latest-renv-cache");
    let user_library = temp_dir("ir-warm-real-ppm-latest-user-library");
    let profile = temp_path("ir-warm-real-ppm-latest-profile", "R");
    fs::write(
        &profile,
        r#"options(repos = c(CRAN = "https://packagemanager.posit.co/cran/latest"))"#,
    )
    .unwrap();

    let out = Command::new(rscript())
        .current_dir(repo_root())
        .env("RENV_PATHS_CACHE", &renv_cache)
        .env("R_LIBS_USER", &user_library)
        .env("R_PROFILE_USER", &profile)
        .env("CC", "false")
        .env("CXX", "false")
        .env("CXX11", "false")
        .env("CXX14", "false")
        .env("CXX17", "false")
        .env("CXX20", "false")
        .args(["scripts/warm-renv-cache.R", "zip"])
        .output()
        .unwrap();

    assert_success(&out);
}

#[test]
fn resolver_tooling_installs_do_not_force_source_packages() {
    let paths = [
        repo_root().join("driver/tooling.R"),
        repo_root().join("scripts/warm-renv-cache.R"),
    ];

    for path in paths {
        let tooling = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        assert!(
            !tooling.contains("PKG_PLATFORMS = \"source\""),
            "{}",
            path.display()
        );
        assert!(
            !tooling.contains("pkg.platforms = \"source\""),
            "{}",
            path.display()
        );
        assert!(!tooling.contains("type = \"source\""), "{}", path.display());
    }
}

#[test]
fn python_windows_uv_system_config_uses_programdata() {
    let path = repo_root().join("src/python.rs");
    let python = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    assert!(python.contains("env_os_nonempty(\"PROGRAMDATA\")"));
    assert!(!python.contains("env_os_nonempty(\"SYSTEMDRIVE\")"));
}

#[test]
fn install_dev_deps_scripts_persist_dynamic_test_r_metadata() {
    let sh_path = repo_root().join("scripts/install-dev-deps.sh");
    let sh = fs::read_to_string(&sh_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", sh_path.display()));
    assert!(sh.contains("TEST_R_SPEC=\"4.3\""));
    assert!(sh.contains("scripts/resolve-test-r.py \"$TEST_R_SPEC\""));
    assert!(sh.contains("sed -n '4p' \"$metadata_file\""));
    assert!(sh.contains("IR_TEST_R_EXCLUDE_NEWER"));
    assert!(sh.contains("IR_TEST_RSCRIPT"));
    assert!(
        !sh.contains("rig default release"),
        "setup should not mutate a user's configured rig default"
    );

    let ps1_path = repo_root().join("scripts/install-dev-deps.ps1");
    let ps1 = fs::read_to_string(&ps1_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", ps1_path.display()));
    assert!(ps1.contains("$TestRSpec = \"4.3\""));
    assert!(ps1.contains("scripts/resolve-test-r.py\" $TestRSpec"));
    assert!(ps1.contains("$fields = @($metadata)"));
    assert!(!ps1.contains(r#"-split "\s+""#));
    assert!(ps1.contains("IR_TEST_R_EXCLUDE_NEWER=$TestRExcludeNewer"));
    assert!(ps1.contains("IR_TEST_RSCRIPT=$TestRscript"));
    assert!(
        !ps1.contains("rig default release"),
        "setup should not mutate a user's configured rig default"
    );
}

#[test]
fn install_dev_deps_scripts_install_rig_from_upstream_release_without_pinned_version() {
    let sh_path = repo_root().join("scripts/install-dev-deps.sh");
    let sh = fs::read_to_string(&sh_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", sh_path.display()));
    assert!(sh.contains("https://github.com/r-lib/rig/releases/latest"));
    assert!(sh.contains("rig-${rig_version}-macOS-${rig_arch}.pkg"));
    assert!(sh.contains("releases/download/${rig_tag}/${rig_asset}"));
    assert!(sh.contains("installer -pkg"));
    assert!(!sh.contains("brew tap r-lib/rig"));
    assert!(!sh.contains("brew install --cask rig"));
    assert!(!sh.contains("0.8.1"));

    let ps1_path = repo_root().join("scripts/install-dev-deps.ps1");
    let ps1 = fs::read_to_string(&ps1_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", ps1_path.display()));
    assert!(ps1.contains("https://api.github.com/repos/r-lib/rig/releases/latest"));
    assert!(ps1.contains("rig-windows-$version.exe"));
    assert!(ps1.contains("rig-windows-arm64-$version.exe"));
    assert!(ps1.contains("Start-Process"));
    assert!(ps1.contains("-Wait"));
    assert!(ps1.contains("-PassThru"));
    assert!(ps1.contains("Install-WingetPackage \"posit.rig\""));
    assert!(!ps1.contains("choco install rig"));
    assert!(!ps1.contains("0.8.1"));
}

#[test]
fn cli_tests_do_not_use_global_e2e_lock() {
    let tests = [
        "tests/docs_examples.rs",
        "tests/run.rs",
        "tests/resolver_lock.rs",
        "tests/rig_selection.rs",
        "tests/render.rs",
        "tests/preview.rs",
        "tests/tool.rs",
        "tests/support/mod.rs",
    ]
    .into_iter()
    .map(|path| {
        let path = repo_root().join(path);
        fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
    })
    .collect::<String>();

    assert!(!tests.contains("static E2E_LOCK"), "use per-test isolation");
    assert!(!tests.contains("e2e_lock()"), "use per-test isolation");
}

#[test]
fn local_check_runs_all_ci_diagnostics_and_is_required_for_agents() {
    let check_path = repo_root().join("scripts/check.sh");
    let check = fs::read_to_string(&check_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", check_path.display()));
    let workflow_path = repo_root().join(".github/workflows/ci.yml");
    let workflow = fs::read_to_string(&workflow_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", workflow_path.display()));

    for command in [
        "cargo fmt --all -- --check",
        "cargo clippy --all-targets -- -D warnings",
        "cargo nextest run --verbose --no-fail-fast",
    ] {
        assert!(check.contains(command), "{check_path:?} omits `{command}`");
        assert!(
            workflow.contains(command),
            "{workflow_path:?} omits `{command}`"
        );
    }

    let agents_path = repo_root().join("AGENTS.md");
    let agents = fs::read_to_string(&agents_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", agents_path.display()));
    assert!(agents.contains("scripts/check.sh"), "{agents_path:?}");
}

#[test]
fn r_version_selection_test_uses_dynamic_test_r_version() {
    let path = repo_root().join("tests/rig_selection.rs");
    let test = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    assert!(!test.contains("FIXTURE_R_VERSION"));
    assert!(!test.contains("must match the fixture"));
    assert!(test.contains(
        "rig_test_r_version(\"r_version_selection_covers_render_flag_and_run_frontmatter\")"
    ));
    assert!(test.contains("replace(\"#| r-version: 4.4.3\""));
    assert!(test.contains("IR_TEST_R_EXCLUDE_NEWER"));
    assert!(test.contains("\"exclude-newer: 2026-06-01\""));
    assert!(test.contains("exclude-newer: {target_exclude_newer}"));
}

#[test]
fn docs_workflow_requires_all_ci_jobs() {
    let path = repo_root().join(".github/workflows/docs.yml");
    let workflow = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    assert!(workflow.contains("actions: read"));
    assert!(workflow.contains("Require CI jobs to have succeeded"));
    assert!(workflow.contains("All CI jobs succeeded; proceeding to publish."));
    assert!(!workflow.contains("workflow_dispatch"));
    assert!(!workflow.contains("github.event_name == 'workflow_run'"));
    assert!(!workflow.contains("github.sha"));
    assert!(!workflow.contains("non-Windows"));
    assert!(!workflow.contains("known-broken"));
    assert!(!workflow.contains(r#"test("windows"; "i")"#));
}

#[cfg(windows)]
#[test]
fn install_dev_deps_ps1_prints_windows_plan() {
    let out = Command::new("powershell")
        .current_dir(repo_root())
        .env_remove("GITHUB_ACTIONS")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            "scripts/install-dev-deps.ps1",
            "-DryRun",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(
        &out,
        "winget install --id Microsoft.VisualStudio.2022.BuildTools",
    );
    assert_stdout_contains(&out, "Invoke-WebRequest -Uri https://win.rustup.rs");
    assert_stdout_contains(&out, "rustup-init-");
    assert_stdout_contains(&out, "-y --default-toolchain stable");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("Rustlang.Rustup"));
    assert_stdout_contains(&out, "winget install --id posit.rig");
    assert_stdout_contains(&out, "winget install --id Posit.Quarto");
    assert_stdout_contains(&out, "rig add release");
    assert_stdout_contains(&out, "rig add 4.3");
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("rig default release"),
        "{}",
        output_text(&out)
    );
    assert_stdout_contains(&out, "IR_TEST_R_VERSION=<resolved-4.3-version>");
    assert_stdout_contains(&out, "IR_TEST_R_EXCLUDE_NEWER=<release-date-for-4.3>");
    assert_stdout_contains(&out, "IR_TEST_RSCRIPT='<Rscript-for-4.3>'");
}

#[cfg(windows)]
#[test]
fn install_dev_deps_ps1_uses_github_release_for_rig_on_github_actions() {
    let out = Command::new("powershell")
        .current_dir(repo_root())
        .env("GITHUB_ACTIONS", "true")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "& .\\scripts\\install-dev-deps.ps1 -DryRun -Skip rust, python, quarto, r-release",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(
        &out,
        "Invoke-RestMethod -Uri https://api.github.com/repos/r-lib/rig/releases/latest",
    );
    assert_stdout_contains(
        &out,
        "https://github.com/r-lib/rig/releases/download/<latest-rig-tag>/rig-windows-<latest-rig-version>.exe",
    );
    assert_stdout_contains(
        &out,
        "ir-rig-installer.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART",
    );
    assert_stdout_contains(&out, "rig add 4.3");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("choco install rig"), "{stdout}");
    assert!(
        !stdout.contains("winget install --id posit.rig"),
        "{stdout}"
    );
    assert!(!stdout.contains("rig add release"), "{stdout}");
    assert!(!stdout.contains("rig default release"), "{stdout}");
}

#[cfg(windows)]
#[test]
fn install_dev_deps_ps1_can_skip_test_r() {
    let out = Command::new("powershell")
        .current_dir(repo_root())
        .env_remove("GITHUB_ACTIONS")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "& .\\scripts\\install-dev-deps.ps1 -DryRun -Skip test-r",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("rig add 4.3"), "{stdout}");
    assert!(!stdout.contains("IR_TEST_R_VERSION"), "{stdout}");
    assert!(!stdout.contains("IR_TEST_R_EXCLUDE_NEWER"), "{stdout}");
    assert!(!stdout.contains("IR_TEST_RSCRIPT"), "{stdout}");
}

#[test]
fn install_dev_deps_ps1_documents_windows_bootstrap() {
    let path = repo_root().join("scripts/install-dev-deps.ps1");
    let script = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    assert!(script.contains("Microsoft.VisualStudio.2022.BuildTools"));
    assert!(script.contains("https://win.rustup.rs"));
    assert!(!script.contains("Rustlang.Rustup"));
    assert!(script.contains("posit.rig"));
    assert!(!script.contains("choco"));
    assert!(script.contains("https://api.github.com/repos/r-lib/rig/releases/latest"));
    assert!(script.contains("function Get-GitHubApiHeaders"));
    assert!(script.contains("$env:GITHUB_TOKEN"));
    assert!(script.contains("-Headers $headers"));
    assert!(script.contains("https://github.com/r-lib/rig/releases/download/$tag/$asset"));
    assert!(script.contains("Posit.Quarto"));
    assert!(script.contains("ProgramFiles \"rig\""));
    assert!(script.contains("ProgramFiles \"rig\\bin\""));
    assert!(script.contains("[string[]]$Skip"));
    assert!(script.contains("unsupported skip component"));
    assert!(script.contains("function Test-RunnableTool"));
    assert!(
        !script.contains("Require-Tool \"winget\"\nAdd-KnownInstallPaths"),
        "Windows CI must not require winget before honoring skipped components"
    );
    assert!(script.contains("Microsoft\\WindowsApps"));
    assert!(script.contains(r#"Test-AnyRunnableTool @("python", "python3")"#));
    assert!(!script.contains(r#"Test-AnyTool @("python", "python3")"#));
    assert!(!script.contains(r#"@("python", "python3", "py")"#));
    assert!(script.contains("R\\bin"));
    assert!(script.contains("$TestRSpec = \"4.3\""));
    assert!(script.contains("IR_TEST_R_VERSION=$TestRVersion"));
    assert!(script.contains("IR_TEST_R_EXCLUDE_NEWER=$TestRExcludeNewer"));
    assert!(
        !script.contains("exit 0"),
        "skip paths should return from the script without closing an interactive shell"
    );
    assert!(
        script.contains("IR_TEST_RSCRIPT='$TestRscript'"),
        "printed IR_TEST_RSCRIPT assignment should be pasteable when Rscript lives under Program Files"
    );
    assert!(script.contains("IR_TEST_RSCRIPT=$TestRscript"));
    assert!(
        !script.contains("rig default release"),
        "setup should not mutate a user's configured rig default"
    );
}

#[test]
fn test_r_metadata_resolution_is_shared() {
    let helper = repo_root().join("scripts/resolve-test-r.py");
    assert!(
        helper.exists(),
        "test R metadata resolution should live in a shared helper"
    );
    let helper_text = fs::read_to_string(&helper)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", helper.display()));
    assert!(
        helper_text.contains(r#"binary_path.with_name("Rscript.exe")"#),
        "test R metadata resolution should derive Rscript.exe from Windows R.exe"
    );
    assert!(helper_text.contains("stdin=\"\"\""));
    assert!(helper_text.contains("write.dcf"));
    assert!(helper_text.contains("from email.parser import Parser"));
    assert!(!helper_text.contains("\"--vanilla\""));
    assert!(!helper_text.contains("\"--slave\""));
    assert!(!helper_text.contains("cat(sprintf"));
    assert!(!helper_text.contains("def output_field"));
    assert!(!helper_text.contains("available\", \"--all\", \"--json"));
    assert!(!helper_text.contains("def version_parts"));

    for script in [
        "scripts/install-dev-deps.sh",
        "scripts/install-dev-deps.ps1",
        "scripts/setup_codex_universal.sh",
    ] {
        let path = repo_root().join(script);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        assert!(
            text.contains("scripts/resolve-test-r.py"),
            "{} should call the shared test R resolver",
            path.display()
        );
        assert!(
            !text.contains("def version_parts"),
            "{} should not duplicate the resolver's Python code",
            path.display()
        );
        assert!(
            !text.contains("function Get-TestRMetadata"),
            "{} should not duplicate the resolver's PowerShell code",
            path.display()
        );
    }
}

#[test]
fn universal_setup_uses_resolved_test_r_snapshot_date() {
    let path = repo_root().join("scripts/setup_codex_universal.sh");
    let script = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    assert!(script.contains("rig add 4.3"));
    assert!(script.contains("scripts/resolve-test-r.py 4.3"));
    assert!(script.contains("test_r_exclude_newer=\"${test_r_metadata[2]}\""));
    assert!(script.contains("https://packagemanager.posit.co/cran/${test_r_exclude_newer}"));
    assert!(script.contains("https://packagemanager.posit.co/cran/2024-02-01"));
    assert!(script.contains("\"tidyr@1.3.1\""));
    assert!(script.contains("\"gt@0.10.1\""));
    assert!(!script.contains("https://packagemanager.posit.co/cran/2026-06-01"));
}

#[cfg(unix)]
#[test]
fn test_r_metadata_resolver_delegates_oldrel_resolution_to_rig_resolve() {
    let temp = std::env::temp_dir().join(format!(
        "ir-fake-rig-oldrel-no-release-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir_all(&temp).unwrap();
    let rig = temp.join("rig");
    let r = temp.join("R dir").join("R");
    let rscript = temp.join("R dir").join("Rscript");
    fs::create_dir_all(r.parent().unwrap()).unwrap();
    fs::write(
        &rig,
        format!(
            r#"#!/usr/bin/env sh
set -eu
if [ "$1" = "-q" ] && [ "$2" = "resolve" ] && [ "$3" = "oldrel/2" ]; then
  echo '4.4.3 https://example.test/R-4.4.3.pkg'
elif [ "$1" = "-q" ] && [ "$2" = "list" ] && [ "$3" = "--json" ]; then
  cat <<'JSON'
[
  {{"name": "4.4-arm64", "version": "4.4.3", "aliases": [], "binary": "{r_binary}"}}
]
JSON
elif [ "$1" = "run" ]; then
  echo "metadata probe should invoke the resolved R binary directly" >&2
  exit 99
else
  echo "unexpected rig command: $*" >&2
  exit 99
fi
"#,
            r_binary = r.display(),
        ),
    )
    .unwrap();
    fs::write(
        &r,
        r#"#!/usr/bin/env sh
echo "metadata probe should invoke the resolved Rscript directly" >&2
exit 99
"#,
    )
    .unwrap();
    fs::write(
        &rscript,
        format!(
            r#"#!/usr/bin/env sh
set -eu
if [ "$#" -eq 1 ] && [ "$1" = "-" ]; then
  script="$(cat)"
  printf '%s\n' "$script" | grep -q 'write[.]dcf' || {{ echo "metadata script was not passed on stdin" >&2; exit 98; }}
  printf '%s\n' "$script" | grep -q 'width *= *100000' || {{ echo "metadata script should disable DCF wrapping" >&2; exit 98; }}
  printf '%s\n' "$script" | grep -q 'IR_TEST_METADATA_RSCRIPT' || {{ echo "metadata script should normalize the resolved Rscript path" >&2; exit 98; }}
  cat <<'EOF'
version: 4.4.3
date: 2025-02-28
rscript: {test_rscript}
EOF
else
  echo "unexpected R command: $*" >&2
  exit 99
fi
"#,
            test_rscript = rscript.display()
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&rig).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&rig, permissions).unwrap();
    let mut permissions = fs::metadata(&r).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&r, permissions).unwrap();
    let mut permissions = fs::metadata(&rscript).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&rscript, permissions).unwrap();

    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![temp.clone()];
    paths.extend(std::env::split_paths(&old_path));
    let path = std::env::join_paths(paths).unwrap();
    let out = Command::new("python3")
        .current_dir(repo_root())
        .env("PATH", path)
        .args(["scripts/resolve-test-r.py", "oldrel/2"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("4.4-arm64\n4.4.3\n2025-02-28\n{}\n", rscript.display())
    );

    let _ = fs::remove_dir_all(&temp);
}

#[cfg(all(
    any(target_os = "macos", target_os = "linux"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
mod release_script_tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use support::{command_on_path, make_executable, write_executable, TempPath};

    struct ReleaseScriptFixture {
        _root: TempPath,
        repo: PathBuf,
        origin: PathBuf,
        origin_fetch_url: PathBuf,
        origin_push_url: PathBuf,
        assets: PathBuf,
        state: PathBuf,
        log: PathBuf,
        path: OsString,
        real_git: OsString,
    }

    impl ReleaseScriptFixture {
        fn new() -> Self {
            let root = temp_dir("ir-release-script");
            let repo = root.join("repo");
            let origin = root.join("origin.git");
            let origin_fetch_url = root.join("origin-fetch.git");
            let origin_push_url = root.join("origin-push.git");
            let bin = root.join("bin");
            let assets = root.join("assets");
            let state = root.join("state");
            let log = root.join("release.log");
            fs::create_dir_all(repo.join("scripts")).unwrap();
            fs::create_dir_all(&bin).unwrap();
            fs::create_dir_all(&assets).unwrap();
            fs::create_dir_all(&state).unwrap();
            fs::write(&log, "").unwrap();

            let release_script = repo_root().join("scripts/release.sh");
            assert!(
                release_script.is_file(),
                "{} should exist before running its public behavior tests",
                release_script.display()
            );
            fs::copy(&release_script, repo.join("scripts/release.sh")).unwrap();
            make_executable(&repo.join("scripts/release.sh"));

            fs::write(
                repo.join("Cargo.toml"),
                "[package]\nname = \"ir\"\nversion = \"0.3.0+dev\"\ndescription = \"fixture\"\n",
            )
            .unwrap();
            fs::write(
                repo.join("Cargo.lock"),
                r#"version = 4

[[package]]
name = "helper"
version = "0.3.0+dev"

[[package]]
name = "ir"
version = "0.3.0+dev"
"#,
            )
            .unwrap();
            fs::write(repo.join(".gitignore"), "/target/\n").unwrap();
            write_executable(
                &repo.join("scripts/check.sh"),
                r#"#!/bin/sh
set -eu
printf 'check\n' >> "$IR_RELEASE_TEST_LOG"
if [ "${IR_RELEASE_TEST_FAIL_CHECK:-}" = "1" ]; then
  echo "fake local check failure" >&2
  exit 41
fi
version="$(awk '
  $0 == "[package]" { in_package = 1; next }
  in_package && /^version = "[^"]+"$/ {
    value = $0
    sub(/^version = "/, "", value)
    sub(/"$/, "", value)
    print value
    exit
  }
' Cargo.toml)"
mkdir -p target/debug
cat >target/debug/ir <<EOF
#!/bin/sh
printf 'local ir $version %s\n' "\$*" >> "\$IR_RELEASE_TEST_LOG"
if [ "\${1:-}" = "--version" ]; then
  printf 'ir $version\n'
  exit 0
fi
exit 98
EOF
cat >target/debug/rx <<EOF
#!/bin/sh
printf 'local rx $version %s\n' "\$*" >> "\$IR_RELEASE_TEST_LOG"
if [ "\${1:-}" = "--version" ]; then
  printf 'rx $version\n'
  exit 0
fi
exit 98
EOF
chmod +x target/debug/ir target/debug/rx
"#,
            );

            let real_git =
                command_on_path("git").expect("git should be available for release tests");
            write_executable(
                &bin.join("git"),
                r#"#!/bin/sh
set -eu
printf 'git' >> "$IR_RELEASE_TEST_LOG"
for arg in "$@"; do
  printf ' %s' "$arg" >> "$IR_RELEASE_TEST_LOG"
done
printf '\n' >> "$IR_RELEASE_TEST_LOG"
if [ "${1:-}" = "commit" ] && [ "${2:-}" = "-m" ] && [ "${3:-}" = "${IR_RELEASE_TEST_FAIL_GIT_COMMIT:-}" ]; then
  echo "fake git commit failure" >&2
  exit 42
fi
exec "$IR_RELEASE_REAL_GIT" "$@"
"#,
            );
            write_executable(
                &bin.join("gh"),
                r#"#!/bin/sh
set -eu
args="$*"
printf 'gh %s\n' "$args" >> "$IR_RELEASE_TEST_LOG"

require_repo() {
  repository=""
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "--repo" ]; then
      repository="$2"
      shift 2
    else
      shift
    fi
  done
  [ "$repository" = "github.example.test/release-owner/ir-fixture" ] || {
    echo "GitHub operation must use --repo github.example.test/release-owner/ir-fixture" >&2
    exit 98
  }
}

case "${1:-}:${2:-}" in
  repo:view)
    case "${3:-}" in
      "$IR_RELEASE_TEST_ORIGIN_FETCH_URL")
        printf 'https://github.example.test/release-owner/ir-fixture\n'
        ;;
      "$IR_RELEASE_TEST_ORIGIN_PUSH_URL")
        if [ "${IR_RELEASE_TEST_MISMATCH_PUSH_REPO:-}" = "1" ]; then
          printf 'https://github.example.test/other-owner/other-fixture\n'
        else
          printf 'https://github.example.test/release-owner/ir-fixture\n'
        fi
        ;;
      *)
        echo "repo view must be given an exact origin fetch or push URL" >&2
        exit 98
        ;;
    esac
    ;;
  run:list)
    require_repo "$@"
    workflow=""
    event=""
    branch=""
    commit=""
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --workflow)
          workflow="$2"
          shift 2
          ;;
        --event)
          event="$2"
          shift 2
          ;;
        --branch)
          branch="$2"
          shift 2
          ;;
        --commit)
          commit="$2"
          shift 2
          ;;
        *) shift ;;
      esac
    done
    [ "$event" = "push" ] || { echo "run lookup must use --event push" >&2; exit 98; }
    [ -n "$commit" ] || { echo "run lookup omitted --commit" >&2; exit 98; }
    case "$workflow" in
      *ci.yml | CI)
        [ "$branch" = "main" ] || { echo "CI run lookup must use --branch main" >&2; exit 98; }
        release_commit_file="$IR_RELEASE_TEST_STATE/release-commit"
        if [ ! -f "$release_commit_file" ]; then
          [ "$(git rev-parse origin/main)" = "$commit" ] || {
            echo "initial CI run lookup used the wrong commit" >&2
            exit 98
          }
          printf '%s\n' "$commit" > "$release_commit_file"
          run_id=101
        elif [ "$(sed -n '1p' "$release_commit_file")" = "$commit" ]; then
          run_id=101
        else
          dev_commit_file="$IR_RELEASE_TEST_STATE/dev-commit"
          if [ -f "$dev_commit_file" ]; then
            [ "$(sed -n '1p' "$dev_commit_file")" = "$commit" ] || {
              echo "post-release CI rerun lookup used the wrong commit" >&2
              exit 98
            }
          else
            [ "$(git rev-parse origin/main)" = "$commit" ] || {
              echo "post-release CI run lookup used the wrong commit" >&2
              exit 98
            }
            printf '%s\n' "$commit" > "$dev_commit_file"
          fi
          run_id=303
        fi
        ;;
      *release.yml | Release)
        [ "$branch" = "v0.4.0" ] || { echo "release run lookup must use --branch v0.4.0" >&2; exit 98; }
        [ "$(git rev-parse 'v0.4.0^{}')" = "$commit" ] || {
          echo "release run lookup used the wrong tag commit" >&2
          exit 98
        }
        release_commit_file="$IR_RELEASE_TEST_STATE/release-commit"
        [ -f "$release_commit_file" ] || { echo "release commit was not recorded" >&2; exit 98; }
        [ "$(sed -n '1p' "$release_commit_file")" = "$commit" ] || {
          echo "release run lookup used the wrong commit" >&2
          exit 98
        }
        run_id=202
        ;;
      *)
        echo "unexpected workflow: $workflow" >&2
        exit 98
        ;;
    esac
    case " $args " in
      *" --jq "*) printf '%s\n' "$run_id" ;;
      *) printf '[{"databaseId":%s,"status":"completed","conclusion":"success"}]\n' "$run_id" ;;
    esac
    ;;
  run:watch)
    require_repo "$@"
    case " $args " in
      *" --exit-status "*) ;;
      *) echo "run watch omitted --exit-status" >&2; exit 98 ;;
    esac
    if [ -n "${IR_RELEASE_TEST_FAIL_RUN_ID:-}" ]; then
      case " $args " in
        *" $IR_RELEASE_TEST_FAIL_RUN_ID "*)
          echo "fake gh watch failure" >&2
          exit 31
          ;;
      esac
    fi
    if [ -n "${IR_RELEASE_TEST_ADVANCE_ORIGIN_AFTER_303:-}" ]; then
      case " $args " in
        *" 303 "*)
          marker="$IR_RELEASE_TEST_STATE/origin-advanced-after-303"
          if [ ! -f "$marker" ]; then
            dev_commit="$(git rev-parse origin/main)"
            tree="$(git show -s --format=%T "$dev_commit")"
            case "$IR_RELEASE_TEST_ADVANCE_ORIGIN_AFTER_303" in
              1)
                new_tip="$(printf 'Concurrent descendant after post-release CI\n' | git commit-tree "$tree" -p "$dev_commit")"
                ;;
              wrong-first-parent-merge)
                release_commit="$(git rev-parse 'v0.4.0^{}')"
                new_tip="$(printf 'Concurrent merge with dev off first-parent mainline\n' | git commit-tree "$tree" -p "$release_commit" -p "$dev_commit")"
                ;;
              *)
                echo "unsupported origin advancement: $IR_RELEASE_TEST_ADVANCE_ORIGIN_AFTER_303" >&2
                exit 98
                ;;
            esac
            git push origin "$new_tip:refs/heads/main"
            printf '%s\n' "$new_tip" > "$marker"
          fi
          ;;
      esac
    fi
    ;;
  release:view)
    require_repo "$@"
    case "$args" in
      *assets*)
        case "${IR_RELEASE_TEST_ASSET_SET:-complete}" in
          complete)
            cat <<'EOF'
ir-aarch64-apple-darwin.tar.gz
ir-x86_64-apple-darwin.tar.gz
ir-aarch64-unknown-linux-gnu.tar.gz
ir-x86_64-unknown-linux-gnu.tar.gz
ir-x86_64-pc-windows-msvc.zip
SHA256SUMS.txt
EOF
            ;;
          missing)
            cat <<'EOF'
ir-aarch64-apple-darwin.tar.gz
ir-x86_64-apple-darwin.tar.gz
ir-aarch64-unknown-linux-gnu.tar.gz
ir-x86_64-unknown-linux-gnu.tar.gz
ir-x86_64-pc-windows-msvc.zip
EOF
            ;;
          extra)
            cat <<'EOF'
ir-aarch64-apple-darwin.tar.gz
ir-x86_64-apple-darwin.tar.gz
ir-aarch64-unknown-linux-gnu.tar.gz
ir-x86_64-unknown-linux-gnu.tar.gz
ir-x86_64-pc-windows-msvc.zip
SHA256SUMS.txt
unexpected-debug-symbols.tar.gz
EOF
            ;;
          *) echo "unsupported fake asset set: $IR_RELEASE_TEST_ASSET_SET" >&2; exit 98 ;;
        esac
        ;;
      *tagName*isDraft*isPrerelease*publishedAt*)
        case "${IR_RELEASE_TEST_RELEASE_METADATA:-published}" in
          published) printf 'v0.4.0\tfalse\tfalse\t2026-07-31T12:10:26Z\n' ;;
          draft) printf 'v0.4.0\ttrue\tfalse\t2026-07-31T12:10:26Z\n' ;;
          prerelease) printf 'v0.4.0\tfalse\ttrue\t2026-07-31T12:10:26Z\n' ;;
          unpublished) printf 'v0.4.0\tfalse\tfalse\t\n' ;;
          *) echo "unsupported fake release metadata: $IR_RELEASE_TEST_RELEASE_METADATA" >&2; exit 98 ;;
        esac
        ;;
      *tagName*isDraft*isPrerelease* | *@tsv*)
        printf 'v0.4.0\tfalse\tfalse\n'
        ;;
      *tagName*) printf 'v0.4.0\n' ;;
      *isDraft*) printf 'false\n' ;;
      *isPrerelease*) printf 'false\n' ;;
      *)
        cat <<'EOF'
{"tagName":"v0.4.0","isDraft":false,"isPrerelease":false,"assets":[{"name":"ir-aarch64-apple-darwin.tar.gz"},{"name":"ir-x86_64-apple-darwin.tar.gz"},{"name":"ir-aarch64-unknown-linux-gnu.tar.gz"},{"name":"ir-x86_64-unknown-linux-gnu.tar.gz"},{"name":"ir-x86_64-pc-windows-msvc.zip"},{"name":"SHA256SUMS.txt"}]}
EOF
        ;;
    esac
    ;;
  release:download)
    require_repo "$@"
    destination=""
    while [ "$#" -gt 0 ]; do
      if [ "$1" = "--dir" ]; then
        destination="$2"
        shift 2
      else
        shift
      fi
    done
    [ -n "$destination" ] || { echo "release download omitted --dir" >&2; exit 98; }
    mkdir -p "$destination"
    cp "$IR_RELEASE_TEST_ASSETS"/* "$destination/"
    ;;
  *)
    echo "unexpected gh command: $args" >&2
    exit 98
    ;;
esac
"#,
            );
            write_executable(
                &bin.join("Rscript"),
                r#"#!/bin/sh
set -eu
printf 'Rscript %s\n' "$*" >> "$IR_RELEASE_TEST_LOG"
"#,
            );
            write_executable(
                &bin.join("cargo"),
                r#"#!/bin/sh
set -eu
printf 'cargo %s\n' "$*" >> "$IR_RELEASE_TEST_LOG"
[ "$*" = "build --locked --bins" ] || {
  echo "unexpected cargo command: $*" >&2
  exit 98
}
if [ "${IR_RELEASE_TEST_FAIL_CARGO_BUILD:-}" = "1" ]; then
  echo "fake cargo build failure" >&2
  exit 43
fi
version="$(awk '
  $0 == "[package]" { in_package = 1; next }
  in_package && /^version = "[^"]+"$/ {
    value = $0
    sub(/^version = "/, "", value)
    sub(/"$/, "", value)
    print value
    exit
  }
' Cargo.toml)"
mkdir -p target/debug
cat >target/debug/ir <<EOF
#!/bin/sh
printf 'local ir $version %s\n' "\$*" >> "\$IR_RELEASE_TEST_LOG"
if [ "\${1:-}" = "--version" ]; then
  printf 'ir $version\n'
  exit 0
fi
exit 98
EOF
cat >target/debug/rx <<EOF
#!/bin/sh
printf 'local rx $version %s\n' "\$*" >> "\$IR_RELEASE_TEST_LOG"
if [ "\${1:-}" = "--version" ]; then
  printf 'rx $version\n'
  exit 0
fi
exit 98
EOF
chmod +x target/debug/ir target/debug/rx
"#,
            );
            write_executable(
                &bin.join("sleep"),
                r#"#!/bin/sh
set -eu
printf 'sleep %s\n' "$*" >> "$IR_RELEASE_TEST_LOG"
"#,
            );
            write_executable(
                &bin.join("uname"),
                r#"#!/bin/sh
set -eu
case "${1:-}" in
  -s)
    case "$IR_RELEASE_TEST_HOST_TARGET" in
      *-apple-darwin) printf 'Darwin\n' ;;
      *-unknown-linux-gnu) printf 'Linux\n' ;;
      *) exit 98 ;;
    esac
    ;;
  -m)
    if [ -n "${IR_RELEASE_TEST_UNAME_ARCH:-}" ]; then
      printf '%s\n' "$IR_RELEASE_TEST_UNAME_ARCH"
    else
      case "$IR_RELEASE_TEST_HOST_TARGET" in
        aarch64-*) printf 'arm64\n' ;;
        x86_64-*) printf 'x86_64\n' ;;
        *) exit 98 ;;
      esac
    fi
    ;;
  *) echo "unexpected uname command: $*" >&2; exit 98 ;;
esac
"#,
            );
            let target = release_test_target();
            let package_name = format!("ir-{target}");
            let package_dir = root.join(&package_name);
            fs::create_dir_all(&package_dir).unwrap();
            write_executable(
                &package_dir.join("ir"),
                r#"#!/bin/sh
set -eu
printf 'artifact ir %s\n' "$*" >> "$IR_RELEASE_TEST_LOG"
case "${1:-}" in
  --version) printf 'ir %s\n' "${IR_RELEASE_TEST_DOWNLOADED_IR_VERSION:-0.4.0}" ;;
  --help) printf 'fake ir help\n' ;;
  run)
    [ "$#" -eq 5 ] || { echo "fake ir run expected five arguments: $*" >&2; exit 98; }
    [ "$2" = "--rscript" ] || { echo "fake ir run omitted --rscript: $*" >&2; exit 98; }
    [ -x "$3" ] || { echo "fake ir run Rscript is not executable: $3" >&2; exit 98; }
    [ "$4" = "-e" ] || { echo "fake ir run omitted -e: $*" >&2; exit 98; }
    [ "$5" = 'stopifnot(nzchar(as.character(getRversion())))' ] || {
      echo "fake ir run received the wrong R expression: $5" >&2
      exit 98
    }
    "$3" -e "$5"
    ;;
  *) echo "unexpected fake ir command: $*" >&2; exit 98 ;;
esac
"#,
            );
            write_executable(
                &package_dir.join("rx"),
                r#"#!/bin/sh
set -eu
printf 'artifact rx %s\n' "$*" >> "$IR_RELEASE_TEST_LOG"
case "${1:-}" in
  --version) printf 'rx 0.4.0\n' ;;
  --help) printf 'fake rx help\n' ;;
  *) echo "unexpected fake rx command: $*" >&2; exit 98 ;;
esac
"#,
            );
            let archive_name = format!("{package_name}.tar.gz");
            let archive = assets.join(&archive_name);
            let tar = Command::new("tar")
                .current_dir(&root)
                .args(["-czf", archive.to_str().unwrap(), &package_name])
                .output()
                .unwrap();
            assert_success(&tar);
            use sha2::{Digest as _, Sha256};
            let digest = Sha256::digest(fs::read(&archive).unwrap());
            let digest = digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            fs::write(
                assets.join("SHA256SUMS.txt"),
                format!("{digest}  {archive_name}\n"),
            )
            .unwrap();

            let old_path = std::env::var_os("PATH").unwrap_or_default();
            let mut paths = vec![bin.clone()];
            paths.extend(std::env::split_paths(&old_path));
            let path = std::env::join_paths(paths).unwrap();

            run_release_test_git(
                &real_git,
                &root,
                &["init", "--bare", origin.to_str().unwrap()],
            );
            symlink(&origin, &origin_fetch_url).unwrap();
            symlink(&origin, &origin_push_url).unwrap();
            run_release_test_git(&real_git, &repo, &["init", "-b", "main"]);
            run_release_test_git(
                &real_git,
                &repo,
                &["config", "user.email", "release-test@example.com"],
            );
            run_release_test_git(&real_git, &repo, &["config", "user.name", "Release Test"]);
            run_release_test_git(&real_git, &repo, &["add", "."]);
            run_release_test_git(&real_git, &repo, &["commit", "-m", "Initial development"]);
            run_release_test_git(
                &real_git,
                &repo,
                &[
                    "remote",
                    "add",
                    "origin",
                    origin_fetch_url.to_str().unwrap(),
                ],
            );
            run_release_test_git(
                &real_git,
                &repo,
                &[
                    "remote",
                    "set-url",
                    "--push",
                    "origin",
                    origin_push_url.to_str().unwrap(),
                ],
            );
            run_release_test_git(&real_git, &repo, &["push", "-u", "origin", "main"]);
            run_release_test_git(&real_git, &repo, &["checkout", "--detach"]);
            fs::write(repo.join("local-notes.txt"), "leave me untracked\n").unwrap();

            Self {
                _root: root,
                repo,
                origin,
                origin_fetch_url,
                origin_push_url,
                assets,
                state,
                log,
                path,
                real_git,
            }
        }

        fn run(&self, version: &str, fail_run_id: Option<u64>) -> Output {
            let fail_run_id = fail_run_id.map(|run_id| run_id.to_string());
            let mut environment = Vec::new();
            if let Some(fail_run_id) = fail_run_id.as_deref() {
                environment.push(("IR_RELEASE_TEST_FAIL_RUN_ID", fail_run_id));
            }
            self.run_with_environment(version, &environment)
        }

        fn run_with_downloaded_ir_version(
            &self,
            version: &str,
            fail_run_id: Option<u64>,
            downloaded_ir_version: Option<&str>,
        ) -> Output {
            let fail_run_id = fail_run_id.map(|run_id| run_id.to_string());
            let mut environment = Vec::new();
            if let Some(fail_run_id) = fail_run_id.as_deref() {
                environment.push(("IR_RELEASE_TEST_FAIL_RUN_ID", fail_run_id));
            }
            if let Some(downloaded_ir_version) = downloaded_ir_version {
                environment.push((
                    "IR_RELEASE_TEST_DOWNLOADED_IR_VERSION",
                    downloaded_ir_version,
                ));
            }
            self.run_with_environment(version, &environment)
        }

        fn run_with_environment(&self, version: &str, environment: &[(&str, &str)]) -> Output {
            let mut command = Command::new(self.repo.join("scripts/release.sh"));
            command
                .current_dir(&self.repo)
                .arg(version)
                .env("PATH", &self.path)
                .env("IR_RELEASE_REAL_GIT", &self.real_git)
                .env("IR_RELEASE_TEST_LOG", &self.log)
                .env("IR_RELEASE_TEST_STATE", &self.state)
                .env("IR_RELEASE_TEST_ASSETS", &self.assets)
                .env("IR_RELEASE_TEST_ORIGIN_FETCH_URL", &self.origin_fetch_url)
                .env("IR_RELEASE_TEST_ORIGIN_PUSH_URL", &self.origin_push_url)
                .env("IR_RELEASE_TEST_HOST_TARGET", release_test_target())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1");
            for (name, value) in environment {
                command.env(name, value);
            }
            command.output().unwrap()
        }

        fn corrupt_archive_checksum(&self) {
            let archive_name = format!("ir-{}.tar.gz", release_test_target());
            fs::write(
                self.assets.join("SHA256SUMS.txt"),
                format!("{}  {archive_name}\n", "0".repeat(64)),
            )
            .unwrap();
        }

        fn rewrite_project_version(&self, old: &str, new: &str) {
            let manifest_path = self.repo.join("Cargo.toml");
            let manifest = fs::read_to_string(&manifest_path).unwrap();
            let old_manifest = format!("version = \"{old}\"");
            assert_eq!(manifest.matches(&old_manifest).count(), 1, "{manifest}");
            fs::write(
                &manifest_path,
                manifest.replacen(&old_manifest, &format!("version = \"{new}\""), 1),
            )
            .unwrap();

            let lock_path = self.repo.join("Cargo.lock");
            let lock = fs::read_to_string(&lock_path).unwrap();
            let old_entry = format!("[[package]]\nname = \"ir\"\nversion = \"{old}\"");
            assert_eq!(lock.matches(&old_entry).count(), 1, "{lock}");
            fs::write(
                &lock_path,
                lock.replacen(
                    &old_entry,
                    &format!("[[package]]\nname = \"ir\"\nversion = \"{new}\""),
                    1,
                ),
            )
            .unwrap();
        }

        fn rewrite_description(&self, description: &str) {
            let manifest_path = self.repo.join("Cargo.toml");
            let manifest = fs::read_to_string(&manifest_path).unwrap();
            assert_eq!(
                manifest.matches("description = \"fixture\"").count(),
                1,
                "{manifest}"
            );
            fs::write(
                &manifest_path,
                manifest.replacen(
                    "description = \"fixture\"",
                    &format!("description = \"{description}\""),
                    1,
                ),
            )
            .unwrap();
        }

        fn commit_and_push(&self, message: &str) -> String {
            let output = self.git(&["add", "Cargo.toml", "Cargo.lock"]);
            assert_success(&output);
            let output = self.git(&["commit", "-m", message]);
            assert_success(&output);
            let commit = self.git_text(&["rev-parse", "HEAD"]);
            let output = self.git(&["push", "origin", "HEAD:main"]);
            assert_success(&output);
            commit
        }

        fn create_release_tag(&self, release_commit: &str) -> String {
            let output = self.git(&[
                "tag",
                "-a",
                "v0.4.0",
                release_commit,
                "-m",
                "Release v0.4.0",
            ]);
            assert_success(&output);
            let tag_object = self.git_text(&["rev-parse", "refs/tags/v0.4.0"]);
            let output = self.git(&["push", "origin", "v0.4.0"]);
            assert_success(&output);
            tag_object
        }

        fn git(&self, args: &[&str]) -> Output {
            release_test_git_command(&self.real_git)
                .current_dir(&self.repo)
                .args(args)
                .output()
                .unwrap()
        }

        fn git_text(&self, args: &[&str]) -> String {
            let output = self.git(args);
            assert_success(&output);
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        }

        fn origin_text(&self, args: &[&str]) -> String {
            let output = release_test_git_command(&self.real_git)
                .args(["--git-dir", self.origin.to_str().unwrap()])
                .args(args)
                .output()
                .unwrap();
            assert_success(&output);
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        }

        fn log(&self) -> String {
            fs::read_to_string(&self.log).unwrap()
        }

        fn assert_versions(&self, expected: &str) {
            let manifest = fs::read_to_string(self.repo.join("Cargo.toml")).unwrap();
            assert!(
                manifest.contains(&format!("version = \"{expected}\"")),
                "{manifest}"
            );

            let lock = fs::read_to_string(self.repo.join("Cargo.lock")).unwrap();
            assert!(
                lock.contains(&format!(
                    "[[package]]\nname = \"ir\"\nversion = \"{expected}\""
                )),
                "{lock}"
            );
            assert!(
                lock.contains("[[package]]\nname = \"helper\"\nversion = \"0.3.0+dev\""),
                "release version editing should not change another package\n{lock}"
            );
        }

        fn tag_exists(&self, tag: &str) -> bool {
            self.git(&[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/tags/{tag}"),
            ])
            .status
            .success()
        }

        fn origin_tag_exists(&self, tag: &str) -> bool {
            release_test_git_command(&self.real_git)
                .args([
                    "--git-dir",
                    self.origin.to_str().unwrap(),
                    "show-ref",
                    "--verify",
                    "--quiet",
                    &format!("refs/tags/{tag}"),
                ])
                .status()
                .unwrap()
                .success()
        }

        fn assert_completed_release(&self) -> (String, String) {
            self.assert_versions("0.4.0+dev");
            let dev_commit = self.git_text(&["rev-parse", "HEAD"]);
            let release_commit = self.git_text(&["rev-parse", "HEAD^"]);
            assert_eq!(
                self.origin_text(&["rev-parse", "refs/heads/main"]),
                dev_commit
            );
            assert_eq!(self.git_text(&["cat-file", "-t", "v0.4.0"]), "tag");
            assert_eq!(self.git_text(&["rev-parse", "v0.4.0^{}"]), release_commit);
            assert_eq!(
                self.origin_text(&["rev-parse", "v0.4.0^{}"]),
                release_commit
            );
            let subjects = self.git_text(&["log", "--format=%s"]);
            assert_eq!(
                subjects
                    .lines()
                    .filter(|subject| *subject == "Release v0.4.0")
                    .count(),
                1
            );
            assert_eq!(
            subjects
                .lines()
                .filter(|subject| *subject == "Mark post-release builds as development versions")
                .count(),
            1
        );
            assert_eq!(self.git_text(&["status", "--short"]), "?? local-notes.txt");
            (release_commit, dev_commit)
        }
    }

    fn release_test_git_command(real_git: &OsString) -> Command {
        let mut command = Command::new(real_git);
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        command
    }

    fn run_release_test_git(real_git: &OsString, cwd: &Path, args: &[&str]) {
        let output = release_test_git_command(real_git)
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert_success(&output);
    }

    fn release_test_target() -> &'static str {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => "aarch64-apple-darwin",
            ("macos", "x86_64") => "x86_64-apple-darwin",
            ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
            ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
            (os, arch) => panic!("release script tests do not support {os}/{arch}"),
        }
    }

    fn assert_release_event_order(log: &str, expected: &[&str]) {
        let mut rest = log;
        for event in expected {
            let position = rest
                .find(event)
                .unwrap_or_else(|| panic!("missing release event {event:?}\n{log}"));
            rest = &rest[position + event.len()..];
        }
    }

    fn assert_release_stopped_before_development_bump(fixture: &ReleaseScriptFixture) -> String {
        fixture.assert_versions("0.4.0");
        let release_commit = fixture.git_text(&["rev-parse", "HEAD"]);
        assert_eq!(
            fixture.git_text(&["log", "-1", "--format=%s"]),
            "Release v0.4.0"
        );
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            release_commit
        );
        assert_eq!(fixture.git_text(&["cat-file", "-t", "v0.4.0"]), "tag");
        assert_eq!(
            fixture.git_text(&["rev-parse", "v0.4.0^{}"]),
            release_commit
        );
        assert_eq!(
            fixture.origin_text(&["rev-parse", "v0.4.0^{}"]),
            release_commit
        );
        assert_eq!(
            fixture.git_text(&["status", "--short"]),
            "?? local-notes.txt"
        );

        let log = fixture.log();
        assert!(!log.contains("cargo build --locked --bins"), "{log}");
        assert!(
            !log.contains("Mark post-release builds as development versions"),
            "{log}"
        );
        release_commit
    }

    #[test]
    fn release_script_rejects_invalid_version_without_mutating_repository() {
        let fixture = ReleaseScriptFixture::new();
        let head = fixture.git_text(&["rev-parse", "HEAD"]);
        let origin_main = fixture.origin_text(&["rev-parse", "refs/heads/main"]);

        let output = fixture.run("v0.4.0", None);

        assert!(!output.status.success(), "{}", output_text(&output));
        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(stderr.contains("version"), "{}", output_text(&output));
        assert!(stderr.contains("0.4.0"), "{}", output_text(&output));
        assert_eq!(fixture.git_text(&["rev-parse", "HEAD"]), head);
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            origin_main
        );
        fixture.assert_versions("0.3.0+dev");
        assert!(!fixture.tag_exists("v0.4.0"));
        assert!(!fixture.origin_tag_exists("v0.4.0"));
        let log = fixture.log();
        assert!(!log.lines().any(|line| line == "check"), "{log}");
        assert!(!log.lines().any(|line| line.starts_with("gh ")), "{log}");
    }

    #[test]
    fn release_script_completes_release_and_restores_development_version() {
        let fixture = ReleaseScriptFixture::new();

        let output = fixture.run("0.4.0", None);

        assert_success(&output);
        fixture.assert_completed_release();

        let log = fixture.log();
        let fetch_repo_view = format!("gh repo view {} ", fixture.origin_fetch_url.display());
        let push_repo_view = format!("gh repo view {} ", fixture.origin_push_url.display());
        assert_eq!(
            log.lines()
                .filter(|line| line.starts_with(&fetch_repo_view))
                .count(),
            1,
            "repository discovery should resolve the exact origin fetch URL once\n{log}"
        );
        assert_eq!(
            log.lines()
                .filter(|line| line.starts_with(&push_repo_view))
                .count(),
            1,
            "repository discovery should resolve the exact origin push URL once\n{log}"
        );
        let github_operations = log
            .lines()
            .filter(|line| line.starts_with("gh run ") || line.starts_with("gh release "))
            .collect::<Vec<_>>();
        assert!(!github_operations.is_empty(), "{log}");
        for operation in github_operations {
            assert!(
                operation.contains(" --repo github.example.test/release-owner/ir-fixture"),
                "{operation}"
            );
        }
        assert_eq!(
            log.lines().filter(|line| *line == "check").count(),
            1,
            "{log}"
        );
        assert_release_event_order(
            &log,
            &[
                "check",
                "local ir 0.4.0 --version",
                "local rx 0.4.0 --version",
                "git commit -m Release v0.4.0",
                "git push origin HEAD:main",
                "gh run watch 101",
                "git push origin v0.4.0",
                "gh run watch 202",
                "gh release view v0.4.0",
                "gh release download v0.4.0",
                "artifact ir --version",
                "artifact rx --version",
                "artifact ir run",
                "Rscript -e stopifnot(nzchar(as.character(getRversion())))",
                "cargo build --locked --bins",
                "local ir 0.4.0+dev --version",
                "local rx 0.4.0+dev --version",
                "git commit -m Mark post-release builds as development versions",
                "git push origin HEAD:main",
                "gh run watch 303",
            ],
        );
    }

    #[test]
    fn release_script_rejects_mismatched_fetch_and_push_github_repositories() {
        let fixture = ReleaseScriptFixture::new();
        let initial_commit = fixture.git_text(&["rev-parse", "HEAD"]);
        assert_eq!(
            fixture.git_text(&["remote", "get-url", "origin"]),
            fixture.origin_fetch_url.to_string_lossy()
        );
        assert_eq!(
            fixture.git_text(&["remote", "get-url", "--push", "origin"]),
            fixture.origin_push_url.to_string_lossy()
        );

        let output =
            fixture.run_with_environment("0.4.0", &[("IR_RELEASE_TEST_MISMATCH_PUSH_REPO", "1")]);

        assert!(!output.status.success(), "{}", output_text(&output));
        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(
            stderr.contains("fetch and push"),
            "{}",
            output_text(&output)
        );
        assert!(
            stderr.contains("same github repository"),
            "{}",
            output_text(&output)
        );
        fixture.assert_versions("0.3.0+dev");
        assert_eq!(fixture.git_text(&["rev-parse", "HEAD"]), initial_commit);
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            initial_commit
        );
        assert!(!fixture.tag_exists("v0.4.0"));
        assert!(!fixture.origin_tag_exists("v0.4.0"));

        let log = fixture.log();
        let fetch_repo_view = format!("gh repo view {} ", fixture.origin_fetch_url.display());
        let push_repo_view = format!("gh repo view {} ", fixture.origin_push_url.display());
        assert!(
            log.lines().any(|line| line.starts_with(&fetch_repo_view)),
            "{log}"
        );
        assert!(
            log.lines().any(|line| line.starts_with(&push_repo_view)),
            "{log}"
        );
        assert!(!log.lines().any(|line| line == "check"), "{log}");
        assert!(!log.contains("git fetch"), "{log}");
        assert!(!log.contains("git commit"), "{log}");
        assert!(!log.contains("git push"), "{log}");
    }

    #[test]
    fn release_script_ci_failure_prevents_tag_and_development_bump() {
        let fixture = ReleaseScriptFixture::new();

        let output = fixture.run("0.4.0", Some(101));

        assert!(!output.status.success(), "{}", output_text(&output));
        fixture.assert_versions("0.4.0");
        let release_commit = fixture.git_text(&["rev-parse", "HEAD"]);
        assert_eq!(
            fixture.git_text(&["log", "-1", "--format=%s"]),
            "Release v0.4.0"
        );
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            release_commit
        );
        assert!(!fixture.tag_exists("v0.4.0"));
        assert!(!fixture.origin_tag_exists("v0.4.0"));
        assert_eq!(
            fixture.git_text(&["status", "--short"]),
            "?? local-notes.txt"
        );

        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(
            stderr.contains("release commit"),
            "{}",
            output_text(&output)
        );
        assert!(stderr.contains("tag"), "{}", output_text(&output));
        assert!(stderr.contains(&release_commit), "{}", output_text(&output));

        let log = fixture.log();
        assert_eq!(
            log.lines().filter(|line| *line == "check").count(),
            1,
            "{log}"
        );
        assert!(log.contains("gh run watch 101"), "{log}");
        assert!(!log.contains("git tag"), "{log}");
        assert!(!log.contains("release download"), "{log}");
        assert!(!log.contains("artifact "), "{log}");
        assert!(
            !log.contains("Mark post-release builds as development versions"),
            "{log}"
        );

        let log_before_resume = log.len();
        let output = fixture.run("0.4.0", None);

        assert_success(&output);
        let resume_output = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .to_lowercase();
        assert!(
            resume_output.contains("resum"),
            "a resumed release should identify itself\n{}",
            output_text(&output)
        );
        let (completed_release_commit, _) = fixture.assert_completed_release();
        assert_eq!(completed_release_commit, release_commit);

        let log = fixture.log();
        let resumed_log = &log[log_before_resume..];
        assert!(resumed_log.contains("gh run watch 101"), "{resumed_log}");
        assert!(resumed_log.contains("gh run watch 202"), "{resumed_log}");
        assert!(resumed_log.contains("gh run watch 303"), "{resumed_log}");
        assert!(resumed_log.contains("release download"), "{resumed_log}");
        assert!(resumed_log.contains("artifact ir run"), "{resumed_log}");
        assert!(
            resumed_log.contains("Rscript -e stopifnot(nzchar(as.character(getRversion())))"),
            "{resumed_log}"
        );
        assert!(
            !resumed_log.contains("git commit -m Release v0.4.0"),
            "{resumed_log}"
        );
        assert_eq!(
            log.lines().filter(|line| *line == "check").count(),
            1,
            "{log}"
        );
        assert!(
            resumed_log.contains("cargo build --locked --bins"),
            "{resumed_log}"
        );
        assert!(
            resumed_log.contains("local ir 0.4.0+dev --version"),
            "{resumed_log}"
        );
        assert!(
            resumed_log.contains("local rx 0.4.0+dev --version"),
            "{resumed_log}"
        );
    }

    #[test]
    fn release_script_release_workflow_failure_resumes_without_replacing_tag() {
        let fixture = ReleaseScriptFixture::new();

        let output = fixture.run("0.4.0", Some(202));

        assert!(!output.status.success(), "{}", output_text(&output));
        fixture.assert_versions("0.4.0");
        let release_commit = fixture.git_text(&["rev-parse", "HEAD"]);
        let tag_object = fixture.git_text(&["rev-parse", "refs/tags/v0.4.0"]);
        assert_eq!(
            fixture.git_text(&["log", "-1", "--format=%s"]),
            "Release v0.4.0"
        );
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            release_commit
        );
        assert_eq!(fixture.git_text(&["cat-file", "-t", "v0.4.0"]), "tag");
        assert_eq!(
            fixture.git_text(&["rev-parse", "v0.4.0^{}"]),
            release_commit
        );
        assert_eq!(
            fixture.origin_text(&["rev-parse", "v0.4.0^{}"]),
            release_commit
        );
        assert_eq!(
            fixture.git_text(&["status", "--short"]),
            "?? local-notes.txt"
        );

        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(stderr.contains("annotated tag"), "{}", output_text(&output));
        assert!(stderr.contains("publication"), "{}", output_text(&output));
        assert!(stderr.contains("v0.4.0"), "{}", output_text(&output));
        assert!(stderr.contains(&release_commit), "{}", output_text(&output));

        let log = fixture.log();
        assert!(log.contains("gh run watch 101"), "{log}");
        assert!(log.contains("git push origin v0.4.0"), "{log}");
        assert!(log.contains("gh run watch 202"), "{log}");
        assert!(!log.contains("release download"), "{log}");
        assert!(!log.contains("cargo build --locked --bins"), "{log}");
        assert!(
            !log.contains("Mark post-release builds as development versions"),
            "{log}"
        );

        let log_before_resume = log.len();
        let output = fixture.run("0.4.0", None);

        assert_success(&output);
        let resume_output = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .to_lowercase();
        assert!(
            resume_output.contains("resum"),
            "a resumed release should identify itself\n{}",
            output_text(&output)
        );
        let (completed_release_commit, _) = fixture.assert_completed_release();
        assert_eq!(completed_release_commit, release_commit);
        assert_eq!(
            fixture.git_text(&["rev-parse", "refs/tags/v0.4.0"]),
            tag_object,
            "resuming should not replace the annotated tag object"
        );

        let log = fixture.log();
        let resumed_log = &log[log_before_resume..];
        assert!(resumed_log.contains("gh run watch 202"), "{resumed_log}");
        assert!(resumed_log.contains("release download"), "{resumed_log}");
        assert!(resumed_log.contains("artifact ir run"), "{resumed_log}");
        assert!(
            resumed_log.contains("Rscript -e stopifnot(nzchar(as.character(getRversion())))"),
            "{resumed_log}"
        );
        assert!(resumed_log.contains("gh run watch 303"), "{resumed_log}");
        assert!(!resumed_log.contains("git tag"), "{resumed_log}");
        assert!(
            !resumed_log.contains("git push origin v0.4.0"),
            "{resumed_log}"
        );
        assert!(
            !resumed_log.contains("git commit -m Release v0.4.0"),
            "{resumed_log}"
        );
    }

    #[test]
    fn release_script_final_ci_failure_resumes_without_another_commit() {
        let fixture = ReleaseScriptFixture::new();

        let output = fixture.run("0.4.0", Some(303));

        assert!(!output.status.success(), "{}", output_text(&output));
        fixture.assert_versions("0.4.0+dev");
        let dev_commit = fixture.git_text(&["rev-parse", "HEAD"]);
        let release_commit = fixture.git_text(&["rev-parse", "HEAD^"]);
        let tag_object = fixture.git_text(&["rev-parse", "refs/tags/v0.4.0"]);
        let commit_count = fixture.git_text(&["rev-list", "--count", "HEAD"]);
        assert_eq!(
            fixture.git_text(&["log", "-2", "--format=%s"]),
            "Mark post-release builds as development versions\nRelease v0.4.0"
        );
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            dev_commit
        );
        assert_eq!(
            fixture.origin_text(&["rev-parse", "v0.4.0^{}"]),
            release_commit
        );
        assert_eq!(
            fixture.git_text(&["status", "--short"]),
            "?? local-notes.txt"
        );

        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(
            stderr.contains("post-release commit"),
            "{}",
            output_text(&output)
        );
        assert!(stderr.contains("final ci"), "{}", output_text(&output));
        assert!(stderr.contains(&dev_commit), "{}", output_text(&output));

        let log = fixture.log();
        assert!(log.contains("release download"), "{log}");
        assert!(log.contains("artifact ir run"), "{log}");
        assert!(log.contains("cargo build --locked --bins"), "{log}");
        assert!(
            log.contains("git commit -m Mark post-release builds as development versions"),
            "{log}"
        );
        assert!(log.contains("git push origin HEAD:main"), "{log}");
        assert!(log.contains("gh run watch 303"), "{log}");

        let log_before_resume = log.len();
        let output = fixture.run("0.4.0", None);

        assert_success(&output);
        let resume_output = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .to_lowercase();
        assert!(
            resume_output.contains("resum"),
            "a resumed release should identify itself\n{}",
            output_text(&output)
        );
        let (completed_release_commit, completed_dev_commit) = fixture.assert_completed_release();
        assert_eq!(completed_release_commit, release_commit);
        assert_eq!(completed_dev_commit, dev_commit);
        assert_eq!(
            fixture.git_text(&["rev-list", "--count", "HEAD"]),
            commit_count,
            "resuming final CI should not create another commit"
        );
        assert_eq!(
            fixture.git_text(&["rev-parse", "refs/tags/v0.4.0"]),
            tag_object,
            "resuming final CI should not replace the tag"
        );

        let log = fixture.log();
        let resumed_log = &log[log_before_resume..];
        assert!(resumed_log.contains("gh run watch 303"), "{resumed_log}");
        assert!(resumed_log.contains("release download"), "{resumed_log}");
        assert!(
            !resumed_log.lines().any(|line| line == "check"),
            "{resumed_log}"
        );
        assert!(
            !resumed_log.contains("cargo build --locked --bins"),
            "{resumed_log}"
        );
        assert!(!resumed_log.contains("git commit"), "{resumed_log}");
        assert!(!resumed_log.contains("git push"), "{resumed_log}");
    }

    #[test]
    fn release_script_rejects_draft_or_unpublished_release_metadata_before_dev_bump() {
        for (metadata, expected_error) in [
            ("draft", "draft release"),
            ("prerelease", "prerelease"),
            ("unpublished", "no publication timestamp"),
        ] {
            let fixture = ReleaseScriptFixture::new();

            let output = fixture
                .run_with_environment("0.4.0", &[("IR_RELEASE_TEST_RELEASE_METADATA", metadata)]);

            assert!(!output.status.success(), "{}", output_text(&output));
            let release_commit = assert_release_stopped_before_development_bump(&fixture);
            let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
            assert!(stderr.contains(expected_error), "{}", output_text(&output));
            assert!(stderr.contains(&release_commit), "{}", output_text(&output));
            let log = fixture.log();
            assert!(log.contains("gh run watch 202"), "{log}");
            assert!(log.contains("gh release view v0.4.0"), "{log}");
            assert!(!log.contains("gh release download"), "{log}");
        }
    }

    #[test]
    fn release_script_rejects_missing_or_extra_release_asset_before_dev_bump() {
        for asset_set in ["missing", "extra"] {
            let fixture = ReleaseScriptFixture::new();

            let output =
                fixture.run_with_environment("0.4.0", &[("IR_RELEASE_TEST_ASSET_SET", asset_set)]);

            assert!(!output.status.success(), "{}", output_text(&output));
            let release_commit = assert_release_stopped_before_development_bump(&fixture);
            let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
            assert!(
                stderr.contains("exact expected release asset set"),
                "{}",
                output_text(&output)
            );
            assert!(stderr.contains(&release_commit), "{}", output_text(&output));
            let log = fixture.log();
            assert!(log.contains("gh run watch 202"), "{log}");
            assert!(log.contains("gh release view v0.4.0"), "{log}");
            assert!(!log.contains("gh release download"), "{log}");
        }
    }

    #[test]
    fn release_script_checksum_failure_prevents_development_bump() {
        let fixture = ReleaseScriptFixture::new();
        fixture.corrupt_archive_checksum();

        let output = fixture.run("0.4.0", None);

        assert!(!output.status.success(), "{}", output_text(&output));
        let release_commit = assert_release_stopped_before_development_bump(&fixture);
        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(
            stderr.contains("downloaded-binary verification"),
            "{}",
            output_text(&output)
        );
        assert!(stderr.contains(&release_commit), "{}", output_text(&output));

        let log = fixture.log();
        assert!(log.contains("gh run watch 202"), "{log}");
        assert!(log.contains("gh release download v0.4.0"), "{log}");
        assert!(!log.contains("artifact "), "{log}");
        assert!(!log.contains("Rscript -e"), "{log}");
    }

    #[test]
    fn release_script_wrong_downloaded_version_prevents_development_bump() {
        let fixture = ReleaseScriptFixture::new();

        let output = fixture.run_with_downloaded_ir_version("0.4.0", None, Some("0.4.1"));

        assert!(!output.status.success(), "{}", output_text(&output));
        let release_commit = assert_release_stopped_before_development_bump(&fixture);
        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(
            stderr.contains("downloaded ir does not report version 0.4.0"),
            "{}",
            output_text(&output)
        );
        assert!(stderr.contains(&release_commit), "{}", output_text(&output));

        let log = fixture.log();
        assert!(log.contains("gh release download v0.4.0"), "{log}");
        assert!(log.contains("artifact ir --version"), "{log}");
        assert!(!log.contains("artifact rx --version"), "{log}");
        assert!(!log.contains("artifact ir run"), "{log}");
        assert!(!log.contains("Rscript -e"), "{log}");
    }

    #[test]
    fn release_script_resumes_after_local_check_leaves_unstaged_release_changes() {
        let fixture = ReleaseScriptFixture::new();
        let initial_commit = fixture.git_text(&["rev-parse", "HEAD"]);

        let output = fixture.run_with_environment("0.4.0", &[("IR_RELEASE_TEST_FAIL_CHECK", "1")]);

        assert!(!output.status.success(), "{}", output_text(&output));
        fixture.assert_versions("0.4.0");
        assert_eq!(fixture.git_text(&["rev-parse", "HEAD"]), initial_commit);
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            initial_commit
        );
        assert_eq!(
            fixture.git_text(&["diff", "--name-only"]),
            "Cargo.lock\nCargo.toml"
        );
        assert!(
            fixture
                .git(&["diff", "--cached", "--quiet"])
                .status
                .success(),
            "release changes should remain unstaged after the failed check"
        );
        assert!(!fixture.tag_exists("v0.4.0"));

        let output = fixture.run("0.4.0", None);

        assert_success(&output);
        fixture.assert_completed_release();
    }

    #[test]
    fn release_script_resumes_after_release_commit_failure_leaves_staged_changes() {
        let fixture = ReleaseScriptFixture::new();
        let initial_commit = fixture.git_text(&["rev-parse", "HEAD"]);

        let output = fixture.run_with_environment(
            "0.4.0",
            &[("IR_RELEASE_TEST_FAIL_GIT_COMMIT", "Release v0.4.0")],
        );

        assert!(!output.status.success(), "{}", output_text(&output));
        fixture.assert_versions("0.4.0");
        assert_eq!(fixture.git_text(&["rev-parse", "HEAD"]), initial_commit);
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            initial_commit
        );
        assert!(
            fixture.git(&["diff", "--quiet"]).status.success(),
            "release changes should have been staged before commit failed"
        );
        assert_eq!(
            fixture.git_text(&["diff", "--cached", "--name-only"]),
            "Cargo.lock\nCargo.toml"
        );
        assert!(!fixture.tag_exists("v0.4.0"));

        let output = fixture.run("0.4.0", None);

        assert_success(&output);
        fixture.assert_completed_release();
    }

    #[test]
    fn release_script_resumes_after_post_release_build_leaves_unstaged_dev_changes() {
        let fixture = ReleaseScriptFixture::new();

        let output =
            fixture.run_with_environment("0.4.0", &[("IR_RELEASE_TEST_FAIL_CARGO_BUILD", "1")]);

        assert!(!output.status.success(), "{}", output_text(&output));
        fixture.assert_versions("0.4.0+dev");
        let release_commit = fixture.git_text(&["rev-parse", "HEAD"]);
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            release_commit
        );
        assert_eq!(
            fixture.git_text(&["rev-parse", "v0.4.0^{}"]),
            release_commit
        );
        assert_eq!(
            fixture.git_text(&["diff", "--name-only"]),
            "Cargo.lock\nCargo.toml"
        );
        assert!(
            fixture
                .git(&["diff", "--cached", "--quiet"])
                .status
                .success(),
            "post-release development changes should remain unstaged"
        );
        assert_eq!(
            fixture.git_text(&["log", "-1", "--format=%s"]),
            "Release v0.4.0"
        );

        let output = fixture.run("0.4.0", None);

        assert_success(&output);
        let (resumed_release_commit, _) = fixture.assert_completed_release();
        assert_eq!(resumed_release_commit, release_commit);
    }

    #[test]
    fn release_script_rejects_release_commit_with_other_manifest_changes() {
        let fixture = ReleaseScriptFixture::new();
        fixture.rewrite_project_version("0.3.0+dev", "0.4.0");
        fixture.rewrite_description("unexpected release edit");
        let release_commit = fixture.commit_and_push("Release v0.4.0");

        let output = fixture.run("0.4.0", None);

        assert!(!output.status.success(), "{}", output_text(&output));
        fixture.assert_versions("0.4.0");
        assert_eq!(fixture.git_text(&["rev-parse", "HEAD"]), release_commit);
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            release_commit
        );
        assert!(!fixture.tag_exists("v0.4.0"));
        assert!(!fixture.origin_tag_exists("v0.4.0"));
        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(
            stderr.contains("release commit"),
            "{}",
            output_text(&output)
        );
        assert!(stderr.contains("cargo.toml"), "{}", output_text(&output));
        assert!(stderr.contains("version"), "{}", output_text(&output));
        assert!(
            !fixture
                .log()
                .lines()
                .any(|line| line.starts_with("gh run ")),
            "the invalid release commit should be rejected before CI lookup"
        );
    }

    #[test]
    fn release_script_rejects_dev_commit_with_other_manifest_changes() {
        let fixture = ReleaseScriptFixture::new();
        fixture.rewrite_project_version("0.3.0+dev", "0.4.0");
        let release_commit = fixture.commit_and_push("Release v0.4.0");
        fixture.create_release_tag(&release_commit);
        fixture.rewrite_project_version("0.4.0", "0.4.0+dev");
        fixture.rewrite_description("unexpected development edit");
        let dev_commit =
            fixture.commit_and_push("Mark post-release builds as development versions");

        let output = fixture.run("0.4.0", None);

        assert!(!output.status.success(), "{}", output_text(&output));
        fixture.assert_versions("0.4.0+dev");
        assert_eq!(fixture.git_text(&["rev-parse", "HEAD"]), dev_commit);
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            dev_commit
        );
        assert_eq!(
            fixture.git_text(&["rev-parse", "v0.4.0^{}"]),
            release_commit
        );
        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(stderr.contains("post-release"), "{}", output_text(&output));
        assert!(stderr.contains("cargo.toml"), "{}", output_text(&output));
        assert!(stderr.contains("version"), "{}", output_text(&output));
        assert!(
            !fixture
                .log()
                .lines()
                .any(|line| line.starts_with("gh run ")),
            "the invalid development commit should be rejected before CI lookup"
        );
    }

    #[test]
    fn release_script_rejects_unsupported_architecture_before_mutation() {
        let fixture = ReleaseScriptFixture::new();
        let initial_commit = fixture.git_text(&["rev-parse", "HEAD"]);

        let output =
            fixture.run_with_environment("0.4.0", &[("IR_RELEASE_TEST_UNAME_ARCH", "riscv64")]);

        assert!(!output.status.success(), "{}", output_text(&output));
        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(stderr.contains("unsupported"), "{}", output_text(&output));
        assert!(stderr.contains("riscv64"), "{}", output_text(&output));
        fixture.assert_versions("0.3.0+dev");
        assert_eq!(fixture.git_text(&["rev-parse", "HEAD"]), initial_commit);
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            initial_commit
        );
        assert!(!fixture.tag_exists("v0.4.0"));
        assert!(!fixture.origin_tag_exists("v0.4.0"));
        let log = fixture.log();
        assert!(!log.lines().any(|line| line == "check"), "{log}");
        assert!(!log.contains("git commit"), "{log}");
        assert!(!log.contains("git push"), "{log}");
    }

    #[test]
    fn release_script_accepts_descendant_main_commit_after_final_ci() {
        let fixture = ReleaseScriptFixture::new();

        let output = fixture.run_with_environment(
            "0.4.0",
            &[("IR_RELEASE_TEST_ADVANCE_ORIGIN_AFTER_303", "1")],
        );

        assert_success(&output);
        fixture.assert_versions("0.4.0+dev");
        let release_commit = fixture.origin_text(&["rev-parse", "v0.4.0^{}"]);
        let tag_object = fixture.origin_text(&["rev-parse", "refs/tags/v0.4.0"]);
        let origin_head = fixture.origin_text(&["rev-parse", "refs/heads/main"]);
        assert_eq!(
            fixture.origin_text(&["log", "-1", "--format=%s", "refs/heads/main"]),
            "Concurrent descendant after post-release CI"
        );
        let origin_commit_count = fixture.origin_text(&["rev-list", "--count", "refs/heads/main"]);
        let origin_subjects = fixture.origin_text(&["log", "--format=%s", "refs/heads/main"]);
        assert_eq!(
            origin_subjects
                .lines()
                .filter(|subject| *subject == "Release v0.4.0")
                .count(),
            1
        );
        assert_eq!(
            origin_subjects
                .lines()
                .filter(|subject| *subject == "Mark post-release builds as development versions")
                .count(),
            1
        );

        let output = fixture.run("0.4.0", None);

        assert_success(&output);
        fixture.assert_versions("0.4.0+dev");
        assert_eq!(fixture.git_text(&["rev-parse", "HEAD"]), origin_head);
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            origin_head
        );
        assert_eq!(
            fixture.origin_text(&["rev-list", "--count", "refs/heads/main"]),
            origin_commit_count
        );
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/tags/v0.4.0"]),
            tag_object
        );
        assert_eq!(
            fixture.origin_text(&["rev-parse", "v0.4.0^{}"]),
            release_commit
        );
        let origin_subjects = fixture.origin_text(&["log", "--format=%s", "refs/heads/main"]);
        assert_eq!(
            origin_subjects
                .lines()
                .filter(|subject| *subject == "Release v0.4.0")
                .count(),
            1
        );
        assert_eq!(
            origin_subjects
                .lines()
                .filter(|subject| *subject == "Mark post-release builds as development versions")
                .count(),
            1
        );
        assert_eq!(
            fixture.git_text(&["status", "--short"]),
            "?? local-notes.txt"
        );
    }

    #[test]
    fn release_script_rejects_dev_commit_off_first_parent_mainline() {
        let fixture = ReleaseScriptFixture::new();

        let output = fixture.run_with_environment(
            "0.4.0",
            &[(
                "IR_RELEASE_TEST_ADVANCE_ORIGIN_AFTER_303",
                "wrong-first-parent-merge",
            )],
        );

        assert!(!output.status.success(), "{}", output_text(&output));
        fixture.assert_versions("0.4.0+dev");
        let merge_commit = fixture.origin_text(&["rev-parse", "refs/heads/main"]);
        let parents = fixture.origin_text(&["rev-list", "--parents", "-1", &merge_commit]);
        let parents = parents.split_whitespace().collect::<Vec<_>>();
        assert_eq!(parents.len(), 3, "expected a two-parent merge: {parents:?}");
        assert_eq!(parents[0], merge_commit);
        let release_commit = parents[1];
        let dev_commit = parents[2];
        assert_eq!(
            fixture.origin_text(&["rev-parse", "v0.4.0^{}"]),
            release_commit
        );
        assert_eq!(fixture.git_text(&["rev-parse", "HEAD"]), dev_commit);
        assert_eq!(
            fixture.git_text(&["cat-file", "-t", "refs/tags/v0.4.0"]),
            "tag"
        );
        assert_eq!(
            fixture.origin_text(&["rev-list", "--count", "refs/heads/main"]),
            "4"
        );
        let subjects = fixture.origin_text(&["log", "--format=%s", "refs/heads/main"]);
        assert_eq!(
            subjects
                .lines()
                .filter(|subject| *subject == "Release v0.4.0")
                .count(),
            1
        );
        assert_eq!(
            subjects
                .lines()
                .filter(|subject| *subject == "Mark post-release builds as development versions")
                .count(),
            1
        );
        assert_eq!(
            fixture.git_text(&["status", "--short"]),
            "?? local-notes.txt"
        );

        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(stderr.contains("post-release"), "{}", output_text(&output));
        assert!(stderr.contains("first-parent"), "{}", output_text(&output));
    }
}
