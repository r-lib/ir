#[cfg(unix)]
mod support;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

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
    assert_stdout_contains(&out, "rig add 4.4.3");
    assert_stdout_contains(&out, "rig list --json");
    assert_stdout_contains(&out, "IR_TEST_R_VERSION=4.4.3");
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
    assert_stdout_contains(&out, "brew tap r-lib/rig");
    assert_stdout_contains(&out, "brew install --cask rig");
    assert_stdout_contains(&out, "brew install --cask quarto");
    assert_stdout_contains(&out, "rig add release");
    assert_stdout_contains(&out, "rig add 4.4.3");
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
    assert_stdout_contains(&out, "https://rig.r-pkg.org/deb/rig.gpg");
    assert_stdout_contains(&out, "rig add 4.4.3");
    assert_stdout_contains(&out, "rig list --json");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("https://sh.rustup.rs"), "{stdout}");
    assert!(!stdout.contains("python3 python3-venv"), "{stdout}");
    assert!(!stdout.contains("quarto-linux-"), "{stdout}");
    assert!(!stdout.contains("rig add release"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn install_dev_deps_sh_sets_rig_default_only_when_requested() {
    let out =
        dev_deps_sh_plan_with_args(&["--dry-run", "--platform", "linux-deb", "--set-rig-default"]);

    assert_success(&out);
    assert_stdout_contains(&out, "rig default release");
}

#[test]
fn ci_uses_dev_deps_script_for_non_default_r_setup() {
    let path = repo_root().join(".github/workflows/ci.yml");
    let workflow = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    assert!(workflow.contains("scripts/install-dev-deps.sh"));
    assert!(workflow.contains("Keep the GitHub setup actions above"));
    assert!(workflow.contains("scripts\\install-dev-deps.ps1"));
    assert!(workflow.contains("any::bookdown"));
    assert!(workflow.contains("taiki-e/install-action@nextest"));
    assert!(!workflow.contains("Install rig for build metadata"));
    assert!(workflow.contains("Warm default R package cache"));
    assert!(workflow.contains("Warm snapshot R package cache"));
    assert!(workflow.contains("--repos https://packagemanager.posit.co/cran/2026-06-01"));
    assert!(workflow.contains("rmarkdown bookdown tinytex"));
    assert!(workflow.contains("shell: bash"));
    assert!(workflow.contains("R_PROFILE_USER"));
    assert!(workflow.contains("scripts/ci-rprofile.R"));
    assert!(workflow.contains("scripts/warm-renv-cache.R"));
    assert!(!workflow.contains("bookdown btw Rapp"));
    assert!(!workflow.contains("Warm default R package cache (Unix)"));
    assert!(!workflow.contains("Warm default R package cache (Windows)"));
    assert!(workflow.contains("cargo nextest run --verbose --no-fail-fast"));
    assert!(!workflow.contains("cargo build --verbose"));
    assert!(!workflow.contains("Warm non-default R package cache"));
    assert!(!workflow.contains("scripts/warm-r-version-cache.R"));
    assert!(!workflow.contains("cargo run --bin ir -- run --isolated --vanilla"));
    assert!(!workflow.contains("--r-version \"$IR_TEST_R_VERSION\""));
    assert!(workflow.contains("Install rig and non-default R (Unix)"));
    assert!(workflow.contains("Install rig and non-default R (Windows)"));
    assert!(workflow.contains("-Skip rust, python, quarto, r-release"));
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
}

#[test]
fn cli_tests_do_not_use_global_e2e_lock() {
    let tests = [
        "tests/run.rs",
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
fn docs_workflow_requires_all_ci_jobs() {
    let path = repo_root().join(".github/workflows/docs.yml");
    let workflow = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    assert!(workflow.contains("actions: read"));
    assert!(workflow.contains("Require CI jobs to have succeeded"));
    assert!(workflow.contains("All CI jobs succeeded; proceeding to publish."));
    assert!(!workflow.contains("Install rig for build metadata"));
    assert!(!workflow.contains("workflow_dispatch"));
    assert!(!workflow.contains("github.event_name == 'workflow_run'"));
    assert!(!workflow.contains("github.sha"));
    assert!(!workflow.contains("non-Windows"));
    assert!(!workflow.contains("known-broken"));
    assert!(!workflow.contains(r#"test("windows"; "i")"#));
}

#[test]
fn release_workflow_builds_without_rig_metadata_install() {
    let path = repo_root().join(".github/workflows/release.yml");
    let workflow = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    assert!(!workflow.contains("r-lib/actions/setup-r@v2"));
    assert!(!workflow.contains("Install rig for build metadata"));
    assert!(!workflow.contains("brew install --cask rig"));
    assert!(!workflow.contains("apt-get install -y --no-install-recommends r-rig"));
    assert!(!workflow.contains("choco install rig -y --no-progress"));
}

#[cfg(unix)]
#[test]
fn cargo_build_uses_checked_in_r_version_metadata() {
    let bin_dir = support::unique_dir("ir-build-date-bin");
    let target_dir = support::unique_dir("ir-build-date-target");

    support::write_executable(
        &bin_dir.join("Rscript"),
        concat!(
            "#!/bin/sh\n",
            "echo \"build should not invoke Rscript: $*\" >&2\n",
            "exit 64\n",
        ),
    );
    support::write_executable(
        &bin_dir.join("rig"),
        concat!(
            "#!/bin/sh\n",
            "echo \"build should not invoke rig: $*\" >&2\n",
            "exit 65\n",
        ),
    );

    let path = std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .current_dir(repo_root())
        .env("PATH", path)
        .arg("check")
        .arg("--quiet")
        .arg("--locked")
        .arg("--bin")
        .arg("ir")
        .arg("--target-dir")
        .arg(&target_dir)
        .output()
        .unwrap();

    assert_success(&out);

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&target_dir);
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
    assert_stdout_contains(&out, "rig add 4.4.3");
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("rig default release"),
        "{}",
        output_text(&out)
    );
    assert_stdout_contains(&out, "IR_TEST_R_VERSION=4.4.3");
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
    assert_stdout_contains(&out, "rig add 4.4.3");
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
fn install_dev_deps_ps1_sets_rig_default_only_when_requested() {
    let out = Command::new("powershell")
        .current_dir(repo_root())
        .env_remove("GITHUB_ACTIONS")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "& .\\scripts\\install-dev-deps.ps1 -DryRun -SetRigDefault",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "rig default release");
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
    assert!(script.contains("[switch]$SetRigDefault"));
    assert!(script.contains("if ($SetRigDefault)"));
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
    assert!(script.contains("IR_TEST_R_VERSION=4.4.3"));
}
