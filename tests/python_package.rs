mod support;

use support::temp_dir;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn output_text(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}\n{}",
        output_text(output)
    );
}

fn run_installed(bin_dir: &Path, command: &str) -> Output {
    let executable = bin_dir.join(format!("{command}{}", std::env::consts::EXE_SUFFIX));
    Command::new(&executable)
        .arg("--version")
        .output()
        .unwrap_or_else(|err| panic!("failed to run {}: {err}", executable.display()))
}

#[test]
fn uv_tool_install_exposes_ir_and_rx() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temp = temp_dir("ir-python-package-test");
    let wheel_dir = temp.join("wheels");
    let tool_dir = temp.join("tools");
    let bin_dir = temp.join("bin");
    let cache_dir = temp.join("uv-cache");
    fs::create_dir(&wheel_dir).unwrap();
    fs::create_dir(&cache_dir).unwrap();

    let build = Command::new("uv")
        .arg("build")
        .arg("--wheel")
        .arg("--out-dir")
        .arg(&wheel_dir)
        .current_dir(&manifest_dir)
        .output()
        .unwrap_or_else(|err| panic!("failed to run uv build: {err}"));
    assert_success(&build, "uv build failed");
    let wheels = fs::read_dir(&wheel_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "whl"))
        .collect::<Vec<_>>();
    assert_eq!(wheels.len(), 1, "expected one wheel, found {wheels:?}");

    let install = Command::new("uv")
        .arg("tool")
        .arg("install")
        .arg("--find-links")
        .arg(&wheel_dir)
        .arg("--no-index")
        .arg("r-lib-ir")
        .env("UV_TOOL_DIR", &tool_dir)
        .env("UV_TOOL_BIN_DIR", &bin_dir)
        .env("UV_CACHE_DIR", &cache_dir)
        .env("UV_PYTHON_DOWNLOADS", "never")
        .output()
        .unwrap_or_else(|err| panic!("failed to run uv tool install: {err}"));
    assert_success(&install, "uv tool install failed");

    for command in ["ir", "rx"] {
        let output = run_installed(&bin_dir, command);
        assert_success(&output, &format!("installed {command} --version failed"));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            format!("{command} {}", env!("CARGO_PKG_VERSION")),
            "installed {command} --version printed unexpected stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
