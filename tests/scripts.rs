#[cfg(target_os = "linux")]
mod support;

#[cfg(target_os = "linux")]
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
    assert_stdout_contains(&out, "api.github.com/repos/r-lib/rig/releases/latest");
    assert_stdout_contains(&out, "r-rig_");
    assert_stdout_contains(&out, ".deb");
    assert_stdout_contains(&out, "quarto-linux-");
    assert_stdout_contains(&out, "rig add release");
    assert_stdout_contains(&out, "rig add oldrel/2");
    assert_stdout_contains(&out, "rig list --json");
    assert_stdout_contains(&out, "IR_TEST_R_VERSION=<resolved-oldrel/2-version>");
    assert_stdout_contains(&out, "IR_TEST_R_EXCLUDE_NEWER=<release-date-for-oldrel/2>");
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
    assert_stdout_contains(&out, "api.github.com/repos/r-lib/rig/releases/latest");
    assert_stdout_contains(&out, "macOS-");
    assert_stdout_contains(&out, "installer -pkg");
    assert_stdout_contains(&out, "brew install --cask quarto");
    assert_stdout_contains(&out, "rig add release");
    assert_stdout_contains(&out, "rig add oldrel/2");
    assert_stdout_contains(&out, "rig list --json");
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
    assert_stdout_contains(&out, "api.github.com/repos/r-lib/rig/releases/latest");
    assert_stdout_contains(&out, "r-rig_");
    assert_stdout_contains(&out, "rig add oldrel/2");
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
    assert!(!stdout.contains("rig add oldrel/2"), "{stdout}");
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
    assert!(!workflow.contains("IR_TEST_R_VERSION=4.4.3"));
    assert!(!workflow.contains("IR_TEST_R_EXCLUDE_NEWER=2025-02-28"));
    assert!(workflow.contains("any::bookdown"));
    assert!(workflow.contains("any::xfun"));
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
    let install_unix = workflow
        .split("      - name: Install rig and non-default R (Unix)")
        .nth(1)
        .and_then(|block| {
            block
                .split("      - name: Install rig and non-default R (Windows)")
                .next()
        })
        .expect("workflow should install rig and non-default R on Unix");
    assert!(install_unix.contains("GITHUB_TOKEN: ${{ github.token }}"));
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
    assert!(sh.contains("TEST_R_SPEC=\"oldrel/2\""));
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
    assert!(ps1.contains("$TestRSpec = \"oldrel/2\""));
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
fn install_dev_deps_sh_uses_github_token_for_release_lookup() {
    let sh_path = repo_root().join("scripts/install-dev-deps.sh");
    let sh = fs::read_to_string(&sh_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", sh_path.display()));

    assert!(sh.contains("GITHUB_TOKEN"));
    assert!(sh.contains("Authorization"));
    assert!(sh.contains("Bearer"));
}

#[test]
fn cli_tests_do_not_use_global_e2e_lock() {
    let tests = [
        "tests/run.rs",
        "tests/resolver_lock.rs",
        "tests/rig_selection.rs",
        "tests/render.rs",
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
    assert_stdout_contains(&out, "rig add oldrel/2");
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("rig default release"),
        "{}",
        output_text(&out)
    );
    assert_stdout_contains(&out, "IR_TEST_R_VERSION=<resolved-oldrel/2-version>");
    assert_stdout_contains(&out, "IR_TEST_R_EXCLUDE_NEWER=<release-date-for-oldrel/2>");
    assert_stdout_contains(&out, "IR_TEST_RSCRIPT='<Rscript-for-oldrel/2>'");
}

#[cfg(windows)]
#[test]
fn install_dev_deps_ps1_uses_choco_for_rig_on_github_actions() {
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
    assert_stdout_contains(&out, "choco install rig -y --no-progress");
    assert_stdout_contains(&out, "rig add oldrel/2");
    let stdout = String::from_utf8_lossy(&out.stdout);
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
    assert!(!stdout.contains("rig add oldrel/2"), "{stdout}");
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
    assert!(script.contains("choco"));
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
    assert!(script.contains("$TestRSpec = \"oldrel/2\""));
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

    assert!(script.contains("test_r_exclude_newer=\"${test_r_metadata[2]}\""));
    assert!(script.contains("https://packagemanager.posit.co/cran/${test_r_exclude_newer}"));
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
