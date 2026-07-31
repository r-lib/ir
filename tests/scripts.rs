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
fn public_install_guidance_includes_uv_tool_install() {
    for file in ["README.md", "docs/config.qmd"] {
        let text = fs::read_to_string(repo_root().join(file)).unwrap();
        assert!(text.contains("uv tool install r-lib-ir"), "{file}");
    }
}

#[test]
fn release_workflow_publishes_pypi_wheels() {
    let workflow = fs::read_to_string(repo_root().join(".github/workflows/release.yml")).unwrap();

    for expected in [
        "PyO3/maturin-action@v1",
        "manylinux: \"2014\"",
        "name: pypi-${{ matrix.target }}",
        "environment: pypi",
        "id-token: write",
        "pattern: pypi-*",
        "uv publish",
        "--trusted-publishing always",
        "--check-url https://pypi.org/simple",
        "tag $TAG does not match Cargo.toml version v$package_version",
    ] {
        assert!(workflow.contains(expected), "missing {expected:?}");
    }
}

#[test]
fn workflows_pin_setup_uv_to_exact_release() {
    for file in [".github/workflows/ci.yml", ".github/workflows/release.yml"] {
        let workflow = fs::read_to_string(repo_root().join(file)).unwrap();
        let versions: Vec<_> = workflow
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let line = line.strip_prefix("- ").unwrap_or(line);
                line.strip_prefix("uses: astral-sh/setup-uv@v")
            })
            .collect();

        assert!(!versions.is_empty(), "{file} should set up uv");
        for version in versions {
            let parts: Vec<_> = version.split('.').collect();
            assert_eq!(parts.len(), 3, "{file} should pin setup-uv to vX.Y.Z");
            assert!(
                parts
                    .iter()
                    .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())),
                "{file} should pin setup-uv to vX.Y.Z"
            );
        }
    }
}

#[test]
fn release_process_documents_pypi_setup_and_verification() {
    let release = fs::read_to_string(repo_root().join("RELEASE.md")).unwrap();

    for expected in [
        "r-lib-ir",
        "pending Trusted Publisher",
        "environment `pypi`",
        "workflow `release.yml`",
        "uv tool install",
    ] {
        assert!(release.contains(expected), "missing {expected:?}");
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
    assert_stdout_contains(&out, "https://astral.sh/uv/install.sh");
    assert_stdout_contains(&out, "uv --version");
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
    assert_stdout_contains(&out, "https://astral.sh/uv/install.sh");
    assert_stdout_contains(&out, "uv --version");
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
    assert!(
        !stdout.contains("https://astral.sh/uv/install.sh"),
        "{stdout}"
    );
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
    assert_stdout_contains(&out, "winget install --id astral-sh.uv");
    assert_stdout_contains(&out, "uv --version");
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
        !stdout.contains("winget install --id astral-sh.uv"),
        "{stdout}"
    );
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
    assert!(script.contains("Install-WingetPackage \"astral-sh.uv\""));
    assert!(script.contains("Invoke-Step \"uv\" @(\"--version\")"));
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
    assert!(script.contains("Microsoft\\WinGet\\Links"));
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
    use std::path::PathBuf;
    use support::{make_executable, write_executable, TempPath};

    struct ReleaseFixture {
        _root: TempPath,
        repo: PathBuf,
        origin: PathBuf,
        assets: PathBuf,
        log: PathBuf,
        path: OsString,
    }

    impl ReleaseFixture {
        fn new() -> Self {
            let root = temp_dir("ir-release-script");
            let repo = root.join("repo");
            let origin = root.join("origin.git");
            let bin = root.join("bin");
            let assets = root.join("assets");
            fs::create_dir_all(repo.join("scripts")).unwrap();
            fs::create_dir_all(&bin).unwrap();
            fs::create_dir_all(&assets).unwrap();

            let release_script = repo_root().join("scripts/release.sh");
            fs::copy(&release_script, repo.join("scripts/release.sh")).unwrap();
            make_executable(&repo.join("scripts/release.sh"));
            fs::write(
                repo.join("Cargo.toml"),
                "[package]\nname = \"ir\"\nversion = \"0.3.0+dev\"\n",
            )
            .unwrap();
            fs::write(
                repo.join("Cargo.lock"),
                concat!(
                    "version = 4\n\n",
                    "[[package]]\nname = \"helper\"\nversion = \"0.4.0\"\n\n",
                    "[[package]]\nname = \"ir\"\nversion = \"0.3.0+dev\"\n",
                ),
            )
            .unwrap();

            let log = root.join("release.log");
            fs::write(&log, "").unwrap();
            write_executable(
                &repo.join("scripts/check.sh"),
                "#!/bin/sh\nset -eu\nprintf 'check\\n' >> \"$IR_RELEASE_TEST_LOG\"\n",
            );
            write_executable(
                &bin.join("gh"),
                r#"#!/bin/sh
set -eu
printf 'gh %s\n' "$*" >> "$IR_RELEASE_TEST_LOG"
case "$1:$2" in
  repo:view) printf 'r-lib/ir\n' ;;
  run:list)
    case " $* " in
      *" release.yml "*) printf '202\n' ;;
      *) printf '101\n' ;;
    esac
    ;;
  run:watch) ;;
  release:view) printf 'https://example.test/v0.4.0\n' ;;
  release:download)
    destination=""
    while [ "$#" -gt 0 ]; do
      if [ "$1" = "--dir" ]; then
        destination="$2"
        shift 2
      else
        shift
      fi
    done
    cp "$IR_RELEASE_TEST_ASSETS"/* "$destination/"
    ;;
  *) echo "unexpected gh command: $*" >&2; exit 98 ;;
esac
"#,
            );
            write_executable(
                &bin.join("Rscript"),
                "#!/bin/sh\nset -eu\nprintf 'Rscript %s\\n' \"$*\" >> \"$IR_RELEASE_TEST_LOG\"\n",
            );
            write_executable(&bin.join("sleep"), "#!/bin/sh\nset -eu\n");
            write_executable(
                &bin.join("uv"),
                r#"#!/bin/sh
set -eu
printf 'uv %s\n' "$*" >> "$IR_RELEASE_TEST_LOG"
attempts="$(grep -c '^uv ' "$IR_RELEASE_TEST_LOG" || true)"
if [ "$attempts" -le "${IR_RELEASE_TEST_UV_FAILURES:-0}" ]; then
  exit 97
fi
case "$1:$2" in
  tool:install)
    mkdir -p "$UV_TOOL_BIN_DIR"
    cp "$IR_RELEASE_TEST_ASSETS/pypi-ir" "$UV_TOOL_BIN_DIR/ir"
    cp "$IR_RELEASE_TEST_ASSETS/pypi-rx" "$UV_TOOL_BIN_DIR/rx"
    ;;
  *) echo "unexpected uv command: $*" >&2; exit 98 ;;
esac
"#,
            );

            write_executable(
                &assets.join("pypi-ir"),
                r#"#!/bin/sh
set -eu
printf 'pypi ir %s\n' "$*" >> "$IR_RELEASE_TEST_LOG"
case "$1" in
  --version) printf 'ir 0.4.0\n' ;;
  --help) ;;
  run) "$3" -e "$5" ;;
  *) exit 98 ;;
esac
"#,
            );
            write_executable(
                &assets.join("pypi-rx"),
                r#"#!/bin/sh
set -eu
printf 'pypi rx %s\n' "$*" >> "$IR_RELEASE_TEST_LOG"
case "$1" in
  --version) printf 'rx 0.4.0\n' ;;
  --help) ;;
  *) exit 98 ;;
esac
"#,
            );

            let target = release_target();
            let package = format!("ir-{target}");
            let package_dir = root.join(&package);
            fs::create_dir_all(&package_dir).unwrap();
            write_executable(
                &package_dir.join("ir"),
                r#"#!/bin/sh
set -eu
printf 'artifact ir %s\n' "$*" >> "$IR_RELEASE_TEST_LOG"
case "$1" in
  --version) printf 'ir 0.4.0\n' ;;
  --help) ;;
  run) "$3" -e "$5" ;;
  *) exit 98 ;;
esac
"#,
            );
            write_executable(
                &package_dir.join("rx"),
                r#"#!/bin/sh
set -eu
printf 'artifact rx %s\n' "$*" >> "$IR_RELEASE_TEST_LOG"
case "$1" in
  --version) printf 'rx 0.4.0\n' ;;
  --help) ;;
  *) exit 98 ;;
esac
"#,
            );
            let archive_name = format!("{package}.tar.gz");
            let archive = assets.join(&archive_name);
            let output = Command::new("tar")
                .current_dir(&root)
                .args(["-czf", archive.to_str().unwrap(), &package])
                .output()
                .unwrap();
            assert_success(&output);
            use sha2::{Digest as _, Sha256};
            let digest = Sha256::digest(fs::read(&archive).unwrap())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            fs::write(
                assets.join("SHA256SUMS.txt"),
                format!("{digest}  {archive_name}\n"),
            )
            .unwrap();

            run_git(&root, &["init", "--bare", origin.to_str().unwrap()]);
            run_git(&repo, &["init", "-b", "main"]);
            run_git(&repo, &["config", "user.email", "release-test@example.com"]);
            run_git(&repo, &["config", "user.name", "Release Test"]);
            run_git(&repo, &["add", "."]);
            run_git(&repo, &["commit", "-m", "Initial development"]);
            run_git(
                &repo,
                &["remote", "add", "origin", origin.to_str().unwrap()],
            );
            run_git(&repo, &["push", "-u", "origin", "main"]);

            let mut paths = vec![bin];
            paths.extend(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            ));

            Self {
                _root: root,
                repo,
                origin,
                assets,
                log,
                path: std::env::join_paths(paths).unwrap(),
            }
        }

        fn run(&self, version: &str) -> Output {
            self.command(version).output().unwrap()
        }

        fn run_with_uv_failure(&self, version: &str) -> Output {
            self.run_with_uv_failures(version, 100)
        }

        fn run_with_uv_failures(&self, version: &str, failures: usize) -> Output {
            self.command(version)
                .env("IR_RELEASE_TEST_UV_FAILURES", failures.to_string())
                .output()
                .unwrap()
        }

        fn command(&self, version: &str) -> Command {
            let mut command = Command::new(self.repo.join("scripts/release.sh"));
            command
                .current_dir(&self.repo)
                .arg(version)
                .env("PATH", &self.path)
                .env("IR_RELEASE_TEST_LOG", &self.log)
                .env("IR_RELEASE_TEST_ASSETS", &self.assets);
            command
        }

        fn git_text(&self, args: &[&str]) -> String {
            command_text(Command::new("git").current_dir(&self.repo).args(args))
        }

        fn origin_text(&self, args: &[&str]) -> String {
            command_text(
                Command::new("git")
                    .args(["--git-dir", self.origin.to_str().unwrap()])
                    .args(args),
            )
        }
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert_success(&output);
    }

    fn command_text(command: &mut Command) -> String {
        let output = command.output().unwrap();
        assert_success(&output);
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn release_target() -> &'static str {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => "aarch64-apple-darwin",
            ("macos", "x86_64") => "x86_64-apple-darwin",
            ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
            ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
            (os, arch) => panic!("unsupported release test host: {os}/{arch}"),
        }
    }

    fn assert_event_order(log: &str, events: &[&str]) {
        let mut rest = log;
        for event in events {
            let position = rest
                .find(event)
                .unwrap_or_else(|| panic!("missing event {event:?}\n{log}"));
            rest = &rest[position + event.len()..];
        }
    }

    #[test]
    fn release_script_rejects_invalid_version_before_preflight() {
        let output = Command::new(repo_root().join("scripts/release.sh"))
            .arg("v0.4.0")
            .output()
            .unwrap();

        assert!(!output.status.success(), "{}", output_text(&output));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("stable version like 0.4.0"),
            "{}",
            output_text(&output)
        );
    }

    #[test]
    fn release_script_runs_the_release_sequence() {
        let fixture = ReleaseFixture::new();

        let output = fixture.run("0.4.0");

        assert_success(&output);
        assert!(fs::read_to_string(fixture.repo.join("Cargo.toml"))
            .unwrap()
            .contains("version = \"0.4.0+dev\""));
        assert!(fs::read_to_string(fixture.repo.join("Cargo.lock"))
            .unwrap()
            .contains("name = \"ir\"\nversion = \"0.4.0+dev\""));
        assert!(fs::read_to_string(fixture.repo.join("Cargo.lock"))
            .unwrap()
            .contains("name = \"helper\"\nversion = \"0.4.0\""));
        assert_eq!(
            fixture.git_text(&["log", "-2", "--format=%s"]),
            "Mark post-release builds as development versions\nRelease v0.4.0"
        );
        let release_commit = fixture.git_text(&["rev-parse", "HEAD^"]);
        assert_eq!(
            fixture.git_text(&["rev-parse", "v0.4.0^{}"]),
            release_commit
        );
        assert_eq!(fixture.git_text(&["cat-file", "-t", "v0.4.0"]), "tag");
        assert_eq!(
            fixture.origin_text(&["rev-parse", "v0.4.0^{}"]),
            release_commit
        );
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            fixture.git_text(&["rev-parse", "HEAD"])
        );

        let log = fs::read_to_string(&fixture.log).unwrap();
        assert_event_order(
            &log,
            &[
                "gh repo view",
                "gh repo view",
                "check",
                "gh run watch 101",
                "gh run watch 202",
                "gh release view v0.4.0",
                "gh release download v0.4.0",
                "artifact ir --version",
                "artifact rx --version",
                "artifact ir run",
                "Rscript -e",
                "uv tool install --no-cache r-lib-ir==0.4.0",
                "pypi ir --version",
                "pypi rx --version",
                "pypi ir --help",
                "pypi rx --help",
                "pypi ir run",
                "Rscript -e",
                "gh run watch 101",
            ],
        );
    }

    #[test]
    fn failed_pypi_smoke_stops_before_development_version_commit() {
        let fixture = ReleaseFixture::new();

        let output = fixture.run_with_uv_failure("0.4.0");

        assert!(!output.status.success(), "{}", output_text(&output));
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("failed to install r-lib-ir==0.4.0 from PyPI"),
            "{}",
            output_text(&output)
        );
        assert!(fs::read_to_string(fixture.repo.join("Cargo.toml"))
            .unwrap()
            .contains("version = \"0.4.0\""));
        assert_eq!(
            fixture.git_text(&["log", "-1", "--format=%s"]),
            "Release v0.4.0"
        );
        assert_eq!(
            fixture.origin_text(&["rev-parse", "refs/heads/main"]),
            fixture.git_text(&["rev-parse", "HEAD"])
        );
    }

    #[test]
    fn transient_pypi_visibility_is_retried() {
        let fixture = ReleaseFixture::new();

        let output = fixture.run_with_uv_failures("0.4.0", 2);

        assert_success(&output);
        let log = fs::read_to_string(&fixture.log).unwrap();
        assert_eq!(
            log.lines()
                .filter(|line| line == &"uv tool install --no-cache r-lib-ir==0.4.0")
                .count(),
            3
        );
        assert!(fs::read_to_string(fixture.repo.join("Cargo.toml"))
            .unwrap()
            .contains("version = \"0.4.0+dev\""));
    }
}
