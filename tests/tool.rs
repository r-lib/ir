//! Integration tests for the public `ir` CLI.

mod support;

use support::*;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn install_scripts_configure_default_path_entries() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let sh = fs::read_to_string(manifest_dir.join("scripts/install.sh")).unwrap();
    assert!(sh.contains("ensure_install_dir_on_path"), "{sh}");
    assert!(sh.contains("IR_NO_MODIFY_PATH"), "{sh}");
    assert!(sh.contains("ZDOTDIR"), "{sh}");
    assert!(sh.contains("Added ~/.local/bin to PATH in"), "{sh}");
    assert!(sh.contains("profile_display"), "{sh}");
    assert!(
        sh.contains("add ${INSTALL_DIR} to your PATH to run ${commands}"),
        "{sh}"
    );

    let ps1 = fs::read_to_string(manifest_dir.join("scripts/install.ps1")).unwrap();
    assert!(ps1.contains("Ensure-InstallDirOnPath"), "{ps1}");
    assert!(ps1.contains("IR_NO_MODIFY_PATH"), "{ps1}");
    assert!(
        ps1.contains("[Environment]::ExpandEnvironmentVariables($PathEntry)"),
        "{ps1}"
    );
    assert!(ps1.contains("Set-ItemProperty -Type ExpandString"), "{ps1}");
    assert!(ps1.contains("32767"), "{ps1}");
    assert!(ps1.contains("added $installDir to user PATH"), "{ps1}");

    let tool_rs = fs::read_to_string(manifest_dir.join("src/tool.rs")).unwrap();
    assert!(
        tool_rs.contains("[Environment]::ExpandEnvironmentVariables($PathEntry)"),
        "{tool_rs}"
    );
}

#[test]
fn tool_run_executes_real_package_entrypoint() {
    let cache_dir = temp_cache("ir-tool-run-real-cache");
    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args([
            "tool",
            "run",
            "--with",
            "docopt,pkgsearch,prettyunits",
            "--from",
            "cli",
            "search",
            "--help",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Seach for CRAN packages on r-pkg.org");
    assert_stdout_contains(&out, "cransearch.R [-h | --help]");
}

#[test]
fn rx_executes_real_package_entrypoint() {
    let cache_dir = temp_cache("ir-rx-real-cache");
    let out = rx()
        .env("IR_CACHE_DIR", &cache_dir)
        .args([
            "-w",
            "docopt,pkgsearch,prettyunits",
            "--from",
            "cli",
            "search",
            "--help",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Seach for CRAN packages on r-pkg.org");
    assert_stdout_contains(&out, "cransearch.R [-h | --help]");
}

#[cfg(unix)]
#[test]
fn rx_preserves_quickstart_package_shorthand() {
    let cache_dir = temp_dir("ir-rx-quickstart-package-cache");
    let library = temp_dir("ir-rx-quickstart-package-library");
    let rscript_dir = temp_dir("ir-rx-quickstart-package-rscript");
    let exec_dir = library.join("quickstart").join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    write_executable(&exec_dir.join("quickstart.R"), "#!/usr/bin/env Rscript\n");

    let rscript = rscript_dir.join("Rscript");
    write_executable(
        &rscript,
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  cat >/dev/null\n",
            "  printf '%s\\n' \"$IR_TEST_LIBRARY\" > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  printf '%s\\n' quickstart > \"$IR_RESOLVE_PACKAGE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "printf '%s\\n' quickstart.package.help\n",
        ),
    );

    let out = rx()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env("IR_RSCRIPT", &rscript)
        .args(["quickstart", "--help"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "quickstart.package.help");
}

#[cfg(windows)]
#[test]
fn rx_preserves_windows_child_exit_code() {
    let cache_dir = temp_dir("ir-rx-windows-exit-code-cache");
    let library = temp_dir("ir-rx-windows-exit-code-library");
    let rscript_dir = temp_dir("ir-rx-windows-exit-code-rscript");
    let exec_dir = library.join("irhighstatus").join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("irhighstatus.cmd"),
        "@echo off\r\nexit /b 300\r\n",
    )
    .unwrap();

    let rscript = rscript_dir.join("Rscript.cmd");
    fs::write(
        &rscript,
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\" == \"\" (\r\n",
            "  more > nul\r\n",
            "  echo %IR_TEST_LIBRARY%> \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
            "  echo irhighstatus> \"%IR_RESOLVE_PACKAGE_RESULT_FILE%\"\r\n",
            "  exit /b 0\r\n",
            ")\r\n",
            "exit /b 0\r\n",
        ),
    )
    .unwrap();

    let out = rx()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env("IR_RSCRIPT", &rscript)
        .arg("irhighstatus")
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(300), "{}", output_text(&out));
}

#[test]
fn tool_install_installs_real_package_entrypoint() {
    let cache_dir = temp_cache("ir-tool-install-real-cache");
    let bin_dir = temp_dir("ir-e2e-tool-install-bin");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args([
            "tool",
            "install",
            "--with",
            "docopt,pkgsearch,prettyunits",
            "--bin-dir",
        ])
        .arg(&bin_dir)
        .arg("cli")
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Installed");
    assert_stdout_contains(&out, "search");

    let launcher = launcher_path(&bin_dir, "search");
    let out = Command::new(&launcher).arg("--help").output().unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Seach for CRAN packages on r-pkg.org");
    assert_stdout_contains(&out, "cransearch.R [-h | --help]");
}

#[test]
fn tool_run_rx_and_install_support_package_bin_executables() {
    let cache_dir = temp_dir("ir-tool-bin-executable-cache");
    let bin_dir = temp_dir("ir-tool-bin-executable-bin");
    let package_dir = temp_dir("ir-tool-bin-executable-package");
    let package = package_dir.join("rustbinpkg");
    copy_dir_tree(&fixture("tool/rustbinpkg"), &package);
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "irrustbin"])
        .args(["run", "arg"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.location=bin");
    assert_stdout_contains(&out, "tool.args=run arg");

    let out = rx()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["--from", &package_ref, "irrustbin-arch"])
        .args(["rx", "arg"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.location=bin/");
    assert_stdout_contains(&out, "tool.args=rx arg");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg(&package_ref)
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "irrustbin");
    assert_stdout_contains(&out, "irrustbin-arch");

    let out = Command::new(launcher_path(&bin_dir, "irrustbin"))
        .args(["install", "arg"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.location=bin");
    assert_stdout_contains(&out, "tool.args=install arg");

    let out = Command::new(launcher_path(&bin_dir, "irrustbin-arch"))
        .args(["arch", "arg"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.location=bin/");
    assert_stdout_contains(&out, "tool.args=arch arg");
}

#[cfg(unix)]
#[test]
fn tool_run_and_install_treat_bin_executables_opaquely() {
    let cache_dir = temp_dir("ir-tool-bin-opaque-cache");
    let bin_dir = temp_dir("ir-tool-bin-opaque-install-bin");
    let library = temp_dir("ir-tool-bin-opaque-library");
    let rscript_dir = temp_dir("ir-tool-bin-opaque-rscript");
    let package = library.join("iropaquebin");
    let package_bin_dir = package.join("bin");
    fs::create_dir_all(&package_bin_dir).unwrap();
    write_executable(
        &package_bin_dir.join("native.exe"),
        "#!/bin/sh\nprintf 'tool.fixture=exe\\n'\nprintf 'tool.args=%s\\n' \"$*\"\n",
    );
    write_executable(
        &package_bin_dir.join("script.R"),
        "#!/bin/sh\nprintf 'tool.fixture=script.R\\n'\nprintf 'tool.args=%s\\n' \"$*\"\n",
    );
    let rscript = write_fake_tool_resolver(&rscript_dir, "iropaquebin", "x64");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "run", "--rscript"])
        .arg(&rscript)
        .args(["--from", "iropaquebin", "native.exe", "run", "arg"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.fixture=exe");
    assert_stdout_contains(&out, "tool.args=run arg");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "run", "--rscript"])
        .arg(&rscript)
        .args(["--from", "iropaquebin", "script.R", "run", "arg"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.fixture=script.R");
    assert_stdout_contains(&out, "tool.args=run arg");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "run", "--rscript"])
        .arg(&rscript)
        .args(["--from", "iropaquebin", "native"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", output_text(&out));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "install", "--rscript"])
        .arg(&rscript)
        .args(["--bin-dir"])
        .arg(&bin_dir)
        .arg("iropaquebin")
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "native.exe");
    assert_stdout_contains(&out, "script.R");
    assert!(launcher_path(&bin_dir, "native.exe").exists());
    assert!(launcher_path(&bin_dir, "script.R").exists());
    assert!(
        fs::symlink_metadata(launcher_path(&bin_dir, "native.exe"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "installed bin executable should be a symlink"
    );
    assert!(!launcher_path(&bin_dir, "native").exists());

    let out = Command::new(launcher_path(&bin_dir, "native.exe"))
        .args(["install", "arg"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.fixture=exe");
    assert_stdout_contains(&out, "tool.args=install arg");
}

#[cfg(unix)]
#[test]
fn tool_run_and_install_use_selected_bin_architecture() {
    let cache_dir = temp_dir("ir-tool-bin-arch-cache");
    let bin_dir = temp_dir("ir-tool-bin-arch-install-bin");
    let library = temp_dir("ir-tool-bin-arch-library");
    let rscript_dir = temp_dir("ir-tool-bin-arch-rscript");
    let package = library.join("irarchtool");
    let exec_dir = package.join("exec");
    let x64_bin_dir = package.join("bin").join("x64");
    let i386_bin_dir = package.join("bin").join("i386");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::create_dir_all(&x64_bin_dir).unwrap();
    fs::create_dir_all(&i386_bin_dir).unwrap();
    write_executable(
        &x64_bin_dir.join("archtool"),
        "#!/bin/sh\nprintf 'tool.arch=x64\\n'\n",
    );
    write_executable(
        &i386_bin_dir.join("archtool"),
        "#!/bin/sh\nprintf 'tool.arch=i386\\n'\n",
    );
    write_executable(&x64_bin_dir.join("helper"), "#!/bin/sh\nprintf 'x64\\n'\n");
    write_executable(
        &i386_bin_dir.join("helper"),
        "#!/bin/sh\nprintf 'i386\\n'\n",
    );
    write_executable(
        &exec_dir.join("path-probe"),
        "#!/bin/sh\nprintf 'helper.arch='\nhelper\n",
    );
    let rscript = write_fake_tool_resolver(&rscript_dir, "irarchtool", "x64");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "run", "--rscript"])
        .arg(&rscript)
        .args(["--from", "irarchtool", "archtool"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.arch=x64");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "install", "--rscript"])
        .arg(&rscript)
        .args(["--bin-dir"])
        .arg(&bin_dir)
        .arg("irarchtool")
        .output()
        .unwrap();
    assert_success(&out);

    let out = Command::new(launcher_path(&bin_dir, "path-probe"))
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "helper.arch=x64");
}

#[cfg(unix)]
#[test]
fn tool_run_queries_architecture_with_forwarded_rscript_args() {
    let cache_dir = temp_dir("ir-tool-rscript-arch-args-cache");
    let library = temp_dir("ir-tool-rscript-arch-args-library");
    let rscript_dir = temp_dir("ir-tool-rscript-arch-args-rscript");
    let package = library.join("irarchargs");
    let i386_bin_dir = package.join("bin").join("i386");
    fs::create_dir_all(&i386_bin_dir).unwrap();
    write_executable(
        &i386_bin_dir.join("archtool"),
        "#!/bin/sh\nprintf 'tool.arch=i386\\n'\n",
    );
    let rscript = write_fake_tool_resolver_with_forwarded_arch(&rscript_dir, "irarchargs", "x64");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "run", "--rscript"])
        .arg(&rscript)
        .args(["--arch=i386", "--from", "irarchargs", "archtool"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.arch=i386");
}

#[cfg(unix)]
#[test]
fn tool_run_prefers_selected_arch_bin_on_path() {
    let cache_dir = temp_dir("ir-tool-bin-path-arch-cache");
    let library = temp_dir("ir-tool-bin-path-arch-library");
    let rscript_dir = temp_dir("ir-tool-bin-path-arch-rscript");
    let package = library.join("irpatharch");
    let exec_dir = package.join("exec");
    let bin_dir = package.join("bin");
    let x64_bin_dir = bin_dir.join("x64");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::create_dir_all(&x64_bin_dir).unwrap();
    write_executable(
        &bin_dir.join("helper"),
        "#!/bin/sh\nprintf 'helper.arch=generic\\n'\n",
    );
    write_executable(
        &x64_bin_dir.join("helper"),
        "#!/bin/sh\nprintf 'helper.arch=x64\\n'\n",
    );
    write_executable(&exec_dir.join("path-probe"), "#!/bin/sh\nhelper\n");
    let rscript = write_fake_tool_resolver(&rscript_dir, "irpatharch", "x64");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "run", "--rscript"])
        .arg(&rscript)
        .args(["--from", "irpatharch", "path-probe"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "helper.arch=x64");
}

#[cfg(unix)]
#[test]
fn tool_run_does_not_load_default_packages_when_querying_architecture() {
    let cache_dir = temp_dir("ir-tool-rscript-default-packages-cache");
    let library = temp_dir("ir-tool-rscript-default-packages-library");
    let rscript_dir = temp_dir("ir-tool-rscript-default-packages-rscript");
    let package = library.join("irprobeenv");
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("probetool.R"),
        "#!/usr/bin/env Rscript\ncat('not reached\\n')\n",
    )
    .unwrap();
    let rscript = write_fake_tool_resolver_rejecting_default_packages_in_arch_query(
        &rscript_dir,
        "irprobeenv",
        "x64",
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "run", "--rscript"])
        .arg(&rscript)
        .args([
            "--default-packages=cli",
            "--from",
            "irprobeenv",
            "probetool",
        ])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.ran=true");
    assert_stdout_contains(&out, "tool.default.packages=true");
}

#[cfg(unix)]
#[test]
fn tool_run_and_install_prefer_selected_arch_bin_over_generic_bin() {
    let cache_dir = temp_dir("ir-tool-bin-arch-shadow-cache");
    let bin_dir = temp_dir("ir-tool-bin-arch-shadow-install-bin");
    let library = temp_dir("ir-tool-bin-arch-shadow-library");
    let rscript_dir = temp_dir("ir-tool-bin-arch-shadow-rscript");
    let package = library.join("irshadowarch");
    let package_bin_dir = package.join("bin");
    let x64_bin_dir = package_bin_dir.join("x64");
    fs::create_dir_all(&x64_bin_dir).unwrap();
    write_executable(
        &package_bin_dir.join("shadowtool"),
        "#!/bin/sh\nprintf 'tool.arch=generic\\n'\n",
    );
    write_executable(
        &x64_bin_dir.join("shadowtool"),
        "#!/bin/sh\nprintf 'tool.arch=x64\\n'\n",
    );
    let rscript = write_fake_tool_resolver(&rscript_dir, "irshadowarch", "x64");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "run", "--rscript"])
        .arg(&rscript)
        .args(["--from", "irshadowarch", "shadowtool"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.arch=x64");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "install", "--rscript"])
        .arg(&rscript)
        .args(["--bin-dir"])
        .arg(&bin_dir)
        .arg("irshadowarch")
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "shadowtool");

    let out = Command::new(launcher_path(&bin_dir, "shadowtool"))
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.arch=x64");
}

#[cfg(unix)]
#[test]
fn tool_run_and_install_prefer_exec_over_same_named_bin_helper() {
    let cache_dir = temp_dir("ir-tool-exec-bin-shadow-cache");
    let bin_dir = temp_dir("ir-tool-exec-bin-shadow-install-bin");
    let library = temp_dir("ir-tool-exec-bin-shadow-library");
    let rscript_dir = temp_dir("ir-tool-exec-bin-shadow-rscript");
    let package = library.join("irexecshadow");
    let exec_dir = package.join("exec");
    let package_bin_dir = package.join("bin");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::create_dir_all(&package_bin_dir).unwrap();
    write_executable(
        &exec_dir.join("shadowed"),
        "#!/bin/sh\nprintf 'tool.location=exec\\n'\n",
    );
    write_executable(
        &package_bin_dir.join("shadowed"),
        "#!/bin/sh\nprintf 'tool.location=bin\\n'\n",
    );
    let rscript = write_fake_tool_resolver(&rscript_dir, "irexecshadow", "x64");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "run", "--rscript"])
        .arg(&rscript)
        .args(["--from", "irexecshadow", "shadowed"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.location=exec");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "install", "--rscript"])
        .arg(&rscript)
        .args(["--bin-dir"])
        .arg(&bin_dir)
        .arg("irexecshadow")
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "shadowed");

    let out = Command::new(launcher_path(&bin_dir, "shadowed"))
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.location=exec");
}

#[cfg(unix)]
#[test]
fn tool_install_launcher_preserves_selected_r_arch_env() {
    let cache_dir = temp_dir("ir-tool-install-r-arch-cache");
    let bin_dir = temp_dir("ir-tool-install-r-arch-bin");
    let library = temp_dir("ir-tool-install-r-arch-library");
    let rscript_dir = temp_dir("ir-tool-install-r-arch-rscript");
    let package = library.join("irinstallarch");
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("archtool.R"),
        "#!/usr/bin/env Rscript\ncat('not reached\\n')\n",
    )
    .unwrap();
    let rscript = write_fake_tool_resolver_with_env_arch(&rscript_dir, "irinstallarch", "x64");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env("R_ARCH", "/i386")
        .env_remove("IR_RSCRIPT")
        .args(["tool", "install", "--rscript"])
        .arg(&rscript)
        .args(["--bin-dir"])
        .arg(&bin_dir)
        .arg("irinstallarch")
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "archtool");

    let out = Command::new(launcher_path(&bin_dir, "archtool"))
        .env_remove("R_ARCH")
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.r_arch=/i386");
}

#[cfg(target_os = "linux")]
#[test]
fn tool_install_does_not_slurp_large_native_bin_executable() {
    let cache_dir = temp_dir("ir-tool-large-native-cache");
    let bin_dir = temp_dir("ir-tool-large-native-install-bin");
    let library = temp_dir("ir-tool-large-native-library");
    let rscript_dir = temp_dir("ir-tool-large-native-rscript");
    let package = library.join("irlargebin");
    let package_bin_dir = package.join("bin");
    fs::create_dir_all(&package_bin_dir).unwrap();
    let large_bin = package_bin_dir.join("large-native");
    let file = fs::File::create(&large_bin).unwrap();
    file.set_len(512 * 1024 * 1024).unwrap();
    make_executable(&large_bin);
    let rscript = write_fake_tool_resolver(&rscript_dir, "irlargebin", "x64");

    let out = Command::new("sh")
        .arg("-c")
        .arg("ulimit -v 262144; exec \"$1\" tool install --rscript \"$2\" --bin-dir \"$3\" irlargebin")
        .arg("sh")
        .arg(env!("CARGO_BIN_EXE_ir"))
        .arg(&rscript)
        .arg(&bin_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .output()
        .unwrap();
    assert_success(&out);
    assert!(launcher_path(&bin_dir, "large-native").exists());
}

#[cfg(unix)]
#[test]
fn tool_run_resolves_package_with_forwarded_architecture() {
    let cache_dir = temp_dir("ir-tool-resolve-arch-cache");
    let library = temp_dir("ir-tool-resolve-arch-library");
    let rscript_dir = temp_dir("ir-tool-resolve-arch-rscript");
    let rscript =
        write_fake_tool_resolver_with_arch_sensitive_resolution(&rscript_dir, "irresolvearch");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "run", "--rscript"])
        .arg(&rscript)
        .args(["--arch=i386", "--from", "irresolvearch", "archtool"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "tool.ran=true");
    assert_stdout_contains(&out, "tool.arch=i386");
}

#[cfg(target_os = "macos")]
#[test]
fn tool_install_adds_default_macos_bin_dir_to_zprofile_once() {
    let cache_dir = temp_dir("ir-tool-install-macos-path-cache");
    let home = temp_dir("ir-tool-install-macos-path-home");
    let default_bin_dir = home.join(".local").join("bin");
    fs::create_dir_all(&default_bin_dir).unwrap();
    let package_dir = temp_dir("ir-tool-install-macos-path-packages");
    let package = write_r_source_package(&package_dir, "irmacpath", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("hello.R"),
        r#"#!/usr/bin/env Rscript
cat("mac.path.fixture=TRUE\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("ZDOTDIR")
        .env_remove("IR_TOOL_BIN_DIR")
        .env_remove("RAPP_BIN_DIR")
        .env_remove("XDG_BIN_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("IR_NO_MODIFY_PATH")
        .args(["tool", "install"])
        .arg(&package_ref)
        .output()
        .unwrap();

    assert_success(&out);
    let first_stderr = stderr(&out);
    assert!(
        first_stderr.contains("Added ~/.local/bin to PATH in ~/.zprofile"),
        "{}",
        output_text(&out)
    );
    assert_stdout_contains(&out, "Installed");
    assert!(launcher_path(&default_bin_dir, "hello").exists());
    assert!(
        !tree_contains_dir_named(&cache_dir, "Rapp"),
        "PATH setup should not add a hidden Rapp dependency"
    );

    let zprofile = fs::read_to_string(home.join(".zprofile")).unwrap();
    assert_eq!(
        zprofile,
        concat!(
            "\n",
            "case \":$PATH:\" in\n",
            "  *:\"$HOME/.local/bin\":*) ;;\n",
            "  *) export PATH=\"$HOME/.local/bin:$PATH\" ;;\n",
            "esac\n"
        )
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("ZDOTDIR")
        .env_remove("IR_TOOL_BIN_DIR")
        .env_remove("RAPP_BIN_DIR")
        .env_remove("XDG_BIN_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("IR_NO_MODIFY_PATH")
        .args(["tool", "install", "--force"])
        .arg(&package_ref)
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "Installed");
    let second_stderr = stderr(&out);
    assert!(
        !second_stderr.contains("PATH"),
        "reinstall should not rerun PATH setup:\n{second_stderr}"
    );
    assert_eq!(
        fs::read_to_string(home.join(".zprofile")).unwrap(),
        zprofile
    );
}

#[cfg(target_os = "macos")]
#[test]
fn tool_install_custom_bin_dir_skips_default_macos_path_setup() {
    let cache_dir = temp_dir("ir-tool-install-custom-path-cache");
    let home = temp_dir("ir-tool-install-custom-path-home");
    let bin_dir = temp_path("ir-tool-install-custom-path-bin", "");
    let package_dir = temp_dir("ir-tool-install-custom-path-packages");
    let package = write_r_source_package(&package_dir, "irmaccustompath", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("hello.R"),
        r#"#!/usr/bin/env Rscript
cat("mac.custom.path.fixture=TRUE\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("ZDOTDIR")
        .env_remove("IR_TOOL_BIN_DIR")
        .env_remove("RAPP_BIN_DIR")
        .env_remove("XDG_BIN_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("IR_NO_MODIFY_PATH")
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg(&package_ref)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Installed");
    assert!(launcher_path(&bin_dir, "hello").exists());
    assert!(!home.join(".local").join("bin").exists());
    assert!(!home.join(".zprofile").exists());
    assert!(!stderr(&out).contains("PATH"));
}

#[cfg(target_os = "macos")]
#[test]
fn tool_install_existing_launcher_does_not_modify_zprofile() {
    let cache_dir = temp_dir("ir-tool-install-collision-path-cache");
    let home = temp_dir("ir-tool-install-collision-path-home");
    let default_bin_dir = home.join(".local").join("bin");
    fs::create_dir_all(&default_bin_dir).unwrap();
    fs::write(
        launcher_path(&default_bin_dir, "hello"),
        "existing launcher\n",
    )
    .unwrap();
    let package_dir = temp_dir("ir-tool-install-collision-path-packages");
    let package = write_r_source_package(&package_dir, "irmacpathcollision", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("hello.R"),
        r#"#!/usr/bin/env Rscript
cat("mac.path.collision.fixture=TRUE\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("ZDOTDIR")
        .env_remove("IR_TOOL_BIN_DIR")
        .env_remove("RAPP_BIN_DIR")
        .env_remove("XDG_BIN_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("IR_NO_MODIFY_PATH")
        .args(["tool", "install"])
        .arg(&package_ref)
        .output()
        .unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    let text = output_text(&out);
    assert!(
        text.contains("already exists; pass --force to overwrite it"),
        "{text}"
    );
    assert!(
        !home.join(".zprofile").exists(),
        "failed install should not write .zprofile\n{text}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn tool_install_write_failure_does_not_modify_zprofile() {
    use std::os::unix::fs::PermissionsExt as _;

    let cache_dir = temp_dir("ir-tool-install-write-failure-cache");
    let home = temp_dir("ir-tool-install-write-failure-home");
    let default_bin_dir = home.join(".local").join("bin");
    fs::create_dir_all(&default_bin_dir).unwrap();
    let original_permissions = fs::metadata(&default_bin_dir).unwrap().permissions();
    fs::set_permissions(&default_bin_dir, fs::Permissions::from_mode(0o555)).unwrap();

    let package_dir = temp_dir("ir-tool-install-write-failure-packages");
    let package = write_r_source_package(&package_dir, "irmacpathwritefailure", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("hello.R"),
        r#"#!/usr/bin/env Rscript
cat("mac.path.write.failure.fixture=TRUE\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", rscript())
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("ZDOTDIR")
        .env_remove("IR_TOOL_BIN_DIR")
        .env_remove("RAPP_BIN_DIR")
        .env_remove("XDG_BIN_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("IR_NO_MODIFY_PATH")
        .args(["tool", "install"])
        .arg(&package_ref)
        .output()
        .unwrap();

    fs::set_permissions(&default_bin_dir, original_permissions).unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    let text = output_text(&out);
    assert!(text.contains("failed to write launcher"), "{text}");
    assert!(
        !home.join(".zprofile").exists(),
        "failed install should not write .zprofile\n{text}"
    );
}

#[test]
fn tool_run_and_install_rapp_package_frontend() {
    let cache_dir = temp_dir("ir-rapp-frontend-cache");
    let bin_dir = temp_dir("ir-rapp-frontend-bin");
    let app = temp_path("ir-rapp-frontend-app", "R");
    fs::write(
        &app,
        "#!/usr/bin/env Rapp\ncat(\"ir.fixture=rapp-frontend\\n\")\n",
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("R_PROFILE_USER")
        .args(["tool", "run", "--from", "Rapp", "Rapp"])
        .arg(&app)
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=rapp-frontend");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("R_PROFILE_USER")
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg("Rapp")
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "Rapp");

    let out = Command::new(launcher_path(&bin_dir, "Rapp"))
        .arg(&app)
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=rapp-frontend");
}

#[test]
fn tool_run_and_install_use_launcher_metadata() {
    let cache_dir = temp_dir("ir-tool-launcher-metadata-cache");
    let bin_dir = temp_dir("ir-tool-launcher-metadata-bin");
    let package_dir = temp_dir("ir-tool-launcher-metadata-packages");
    let package = write_r_source_package(&package_dir, "irtoolmeta", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("default-name.R"),
        r#"#!/usr/bin/env Rscript
#| name: ignored-top-level
#| launcher:
#|   name: custom-tool
cat("launcher.name=", Sys.getenv("RAPP_LAUNCHER_NAME"), "\n", sep = "")
cat("utils.attached=", tolower("package:utils" %in% search()), "\n", sep = "")
cat("package.function.exists=", tolower(exists("ok")), "\n", sep = "")
"#,
    )
    .unwrap();
    fs::write(
        exec_dir.join("old-name.R"),
        r#"#!/usr/bin/env Rscript
#| launcher:
#|   name: new-name
cat("launcher.name=", Sys.getenv("RAPP_LAUNCHER_NAME"), "\n", sep = "")
cat("selected=renamed\n")
"#,
    )
    .unwrap();
    fs::write(
        exec_dir.join("actual-old.R"),
        r#"#!/usr/bin/env Rscript
#| launcher:
#|   name: old-name
cat("launcher.name=", Sys.getenv("RAPP_LAUNCHER_NAME"), "\n", sep = "")
cat("selected=actual\n")
"#,
    )
    .unwrap();
    fs::write(
        exec_dir.join("top-level.R"),
        r#"#!/usr/bin/env Rscript
#| name: top-level-tool
cat("launcher.name=", Sys.getenv("RAPP_LAUNCHER_NAME"), "\n", sep = "")
cat("selected=top-level\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "custom-tool"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "launcher.name=custom-tool");
    assert_stdout_contains(&out, "utils.attached=true");
    assert_stdout_contains(&out, "package.function.exists=false");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "old-name"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "launcher.name=old-name");
    assert_stdout_contains(&out, "selected=actual");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "top-level-tool"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "launcher.name=top-level-tool");
    assert_stdout_contains(&out, "selected=top-level");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg(&package_ref)
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "custom-tool");
    assert_stdout_contains(&out, "new-name");
    assert_stdout_contains(&out, "old-name");
    assert_stdout_contains(&out, "top-level-tool");
    assert!(
        !launcher_path(&bin_dir, "default-name").exists(),
        "launcher should use package launcher metadata"
    );

    let out = Command::new(launcher_path(&bin_dir, "custom-tool"))
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "launcher.name=custom-tool");
    assert_stdout_contains(&out, "utils.attached=true");
    assert_stdout_contains(&out, "package.function.exists=false");

    let out = Command::new(launcher_path(&bin_dir, "top-level-tool"))
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "launcher.name=top-level-tool");
    assert_stdout_contains(&out, "selected=top-level");
}

#[test]
fn tool_run_rejects_duplicate_launcher_metadata_names() {
    let cache_dir = temp_dir("ir-tool-duplicate-launcher-cache");
    let package_dir = temp_dir("ir-tool-duplicate-launcher-packages");
    let package = write_r_source_package(&package_dir, "irtooldupe", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("foo.R"),
        r#"#!/usr/bin/env Rscript
cat("selected=basename\n")
"#,
    )
    .unwrap();
    fs::write(
        exec_dir.join("renamed.R"),
        r#"#!/usr/bin/env Rscript
#| launcher:
#|   name: foo
cat("selected=metadata\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "foo"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "duplicate launchers should fail\n{}",
        output_text(&out)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("multiple package executables map to launcher `foo`"),
        "{}",
        output_text(&out)
    );
}

#[test]
fn tool_run_ignores_non_r_direct_file_for_metadata_name() {
    let cache_dir = temp_dir("ir-tool-non-r-direct-cache");
    let package_dir = temp_dir("ir-tool-non-r-direct-packages");
    let package = write_r_source_package(&package_dir, "irtoolnonr", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(exec_dir.join("picked"), "not an R launcher\n").unwrap();
    fs::write(
        exec_dir.join("metadata.R"),
        r#"#!/usr/bin/env Rscript
#| launcher:
#|   name: picked
cat("selected=metadata\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "picked"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "selected=metadata");
}

#[cfg(unix)]
#[test]
fn tool_run_and_install_support_direct_package_scripts() {
    let cache_dir = temp_dir("ir-tool-direct-script-cache");
    let bin_dir = temp_dir("ir-tool-direct-script-bin");
    let package_dir = temp_dir("ir-tool-direct-script-packages");
    let package = write_r_source_package(&package_dir, "irtooldirect", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    write_executable(
        &exec_dir.join("direct-sh"),
        "#!/bin/sh\nprintf 'tool.fixture=sh\\n'\nprintf 'tool.args=%s\\n' \"$*\"\n",
    );
    write_executable(
        &exec_dir.join("direct-python"),
        "#!/usr/bin/env python3\nimport sys\nprint('tool.fixture=python')\nprint('tool.args=' + '|'.join(sys.argv[1:]))\n",
    );
    write_executable(&exec_dir.join("native-tool"), "not a script\n");
    let package_ref = format!("local::{}", renviron_path(&package));

    for executable in [
        ("direct-sh", &["run", "sh"][..], "tool.fixture=sh"),
        ("direct-python", &["run", "python"], "tool.fixture=python"),
    ] {
        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .args(["tool", "run", "--from", &package_ref])
            .arg(executable.0)
            .args(executable.1)
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, executable.2);
    }

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg(&package_ref)
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "Installed 2 executables");

    for executable in [
        ("direct-sh", &["install", "sh"][..], "tool.fixture=sh"),
        (
            "direct-python",
            &["install", "python"],
            "tool.fixture=python",
        ),
    ] {
        let out = Command::new(launcher_path(&bin_dir, executable.0))
            .args(executable.1)
            .output()
            .unwrap();

        assert_success(&out);
        assert_stdout_contains(&out, executable.2);
    }
}

#[test]
fn tool_run_skips_binary_exec_files() {
    let cache_dir = temp_dir("ir-tool-binary-exec-cache");
    let package_dir = temp_dir("ir-tool-binary-exec-packages");
    let package = write_r_source_package(&package_dir, "irtoolbinary", &[]);
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(exec_dir.join("helper.bin"), [0xff, 0xfe, b'\n']).unwrap();
    fs::write(
        exec_dir.join("valid-tool.R"),
        r#"#!/usr/bin/env Rscript
cat("selected=valid\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "valid-tool"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "selected=valid");
}

#[test]
fn tool_install_rejects_invalid_metadata_launcher_names() {
    for (package_name, launcher_name) in [
        ("irtoolbadname", "bad?name"),
        ("irtoolpercentname", "foo%PATH%"),
        ("irtooldotname", "."),
        ("irtooldotdotname", ".."),
    ] {
        let cache_dir = temp_dir("ir-tool-invalid-launcher-cache");
        let bin_dir = temp_dir("ir-tool-invalid-launcher-bin");
        let package_dir = temp_dir("ir-tool-invalid-launcher-packages");
        let package = write_r_source_package(&package_dir, package_name, &[]);
        let exec_dir = package.join("exec");
        fs::create_dir_all(&exec_dir).unwrap();
        fs::write(
            exec_dir.join("invalid.R"),
            format!(
                r#"#!/usr/bin/env Rscript
#| launcher:
#|   name: {launcher_name}
cat("not reached\n")
"#
            ),
        )
        .unwrap();
        let package_ref = format!("local::{}", renviron_path(&package));

        let out = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .args(["tool", "install", "--bin-dir"])
            .arg(&bin_dir)
            .arg(&package_ref)
            .output()
            .unwrap();
        assert!(
            !out.status.success(),
            "invalid launcher names should fail\n{}",
            output_text(&out)
        );
        assert!(
            String::from_utf8_lossy(&out.stderr)
                .contains(&format!("unsupported launcher name `{launcher_name}`")),
            "{}",
            output_text(&out)
        );
    }
}

#[test]
fn tool_run_limits_metadata_lookup_to_primary_package() {
    let cache_dir = temp_dir("ir-tool-primary-package-cache");
    let package_dir = temp_dir("ir-tool-primary-package-packages");
    let dep = write_r_source_package(&package_dir, "irtooldep", &[]);
    let dep_exec_dir = dep.join("exec");
    fs::create_dir_all(&dep_exec_dir).unwrap();
    fs::write(
        dep_exec_dir.join("dep-tool.R"),
        r#"#!/usr/bin/env Rscript
#| launcher:
#|   name: picked
cat("selected=dependency\n")
"#,
    )
    .unwrap();

    let package = write_r_source_package(
        &package_dir,
        "irtoolprimary",
        &[
            "Imports: irtooldep".to_string(),
            format!("Remotes: irtooldep=local::{}", renviron_path(&dep)),
        ],
    );
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("picked.R"),
        r#"#!/usr/bin/env Rscript
cat("selected=primary\n")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "picked"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "selected=primary");
}

#[test]
fn tool_run_and_install_apply_package_default_packages() {
    let cache_dir = temp_dir("ir-tool-default-packages-cache");
    let bin_dir = temp_dir("ir-tool-default-packages-bin");
    let package_dir = temp_dir("ir-tool-default-packages-packages");
    let package = write_r_source_package(
        &package_dir,
        "irtooldefaults",
        &["Imports: Rapp".to_string()],
    );
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("no-launcher.R"),
        r#"#!/usr/bin/env Rapp
#| name: no-launcher-app

cat("package.function=", ok(), "\n", sep = "")
"#,
    )
    .unwrap();
    fs::write(
        exec_dir.join("null-default.R"),
        r#"#!/usr/bin/env Rscript
#| launcher:
#|   default-packages: null
cat("base.attached=", tolower("package:base" %in% search()), "\n", sep = "")
cat("stats.attached=", tolower("package:stats" %in% search()), "\n", sep = "")
"#,
    )
    .unwrap();
    let package_ref = format!("local::{}", renviron_path(&package));

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "no-launcher-app"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "package.function=TRUE");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "run", "--from", &package_ref, "null-default"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "base.attached=true");
    assert_stdout_contains(&out, "stats.attached=false");

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg(&package_ref)
        .output()
        .unwrap();
    assert_success(&out);

    let out = Command::new(launcher_path(&bin_dir, "no-launcher-app"))
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "package.function=TRUE");

    let out = Command::new(launcher_path(&bin_dir, "null-default"))
        .output()
        .unwrap();
    assert_success(&out);
    assert_stdout_contains(&out, "base.attached=true");
    assert_stdout_contains(&out, "stats.attached=false");
}

#[cfg(unix)]
#[test]
fn tool_install_warm_resolution_cache_skips_resolver_rscript() {
    let cache_dir = temp_dir("ir-warm-tool-install-cache");
    let bin_dir = temp_dir("ir-warm-tool-install-bin");
    let rscript = rscript();
    let profile = temp_path("ir-rprofile-fail", "R");
    fs::write(
        &profile,
        "stop('resolver Rscript should not be launched')\n",
    )
    .unwrap();

    let warm = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", &rscript)
        .env_remove("R_PROFILE_USER")
        .args([
            "tool",
            "install",
            "--with",
            "docopt,pkgsearch,prettyunits",
            "--bin-dir",
        ])
        .arg(&bin_dir)
        .arg("cli")
        .output()
        .unwrap();
    assert_success(&warm);

    let cached = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", &rscript)
        .env("R_PROFILE_USER", &profile)
        .args([
            "tool",
            "install",
            "--force",
            "--with",
            "docopt,pkgsearch,prettyunits",
            "--bin-dir",
        ])
        .arg(&bin_dir)
        .arg("cli")
        .output()
        .unwrap();

    assert_success(&cached);
    assert_stdout_contains(&cached, "Installed");
}

#[test]
fn tool_install_ignores_ir_exclude_newer_env() {
    let cache_dir = temp_dir("ir-tool-install-ignores-exclude-newer-cache");
    let bin_dir = temp_dir("ir-tool-install-ignores-exclude-newer-bin");
    let library = temp_dir("ir-tool-install-ignores-exclude-newer-library");
    let profile = temp_path("ir-tool-install-ignores-exclude-newer-profile", "R");
    let package = library.join("irfake");
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(
        exec_dir.join("hello.R"),
        "#!/usr/bin/env Rscript\ncat('hello\\n')\n",
    )
    .unwrap();
    fs::write(
        &profile,
        r#"
if (nzchar(Sys.getenv("IR_RESOLVE_RESULT_FILE"))) {
  if (nzchar(Sys.getenv("IR_EXCLUDE_NEWER"))) {
    stop("IR_EXCLUDE_NEWER leaked into tool resolver", call. = FALSE)
  }
  writeLines(Sys.getenv("IR_TEST_LIBRARY"), Sys.getenv("IR_RESOLVE_RESULT_FILE"))
  writeLines("irfake", Sys.getenv("IR_RESOLVE_PACKAGE_RESULT_FILE"))
  q(save = "no", status = 0)
}
"#,
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_EXCLUDE_NEWER", "2024-06-01")
        .env("IR_RSCRIPT", rscript())
        .env("IR_TEST_LIBRARY", &library)
        .env("R_PROFILE_USER", &profile)
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg("irfake")
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Installed");
}

#[cfg(unix)]
fn fake_tool_package_with_rscript(
    prefix: &str,
    rscript_name: &str,
    label: &str,
) -> (TempPath, TempPath, PathBuf) {
    let library = temp_dir(&format!("{prefix}-library"));
    let rscript_dir = temp_dir(&format!("{prefix}-r"));
    let exec_dir = library.join("irfake").join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    write_executable(&exec_dir.join("hello.R"), "#!/usr/bin/env Rscript\n");

    let rscript = rscript_dir.join(rscript_name);
    write_executable(
        &rscript,
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n",
                "  cat >/dev/null\n",
                "  printf '%s\\n' \"$IR_TEST_LIBRARY\" > \"$IR_RESOLVE_RESULT_FILE\"\n",
                "  printf '%s\\n' irfake > \"$IR_RESOLVE_PACKAGE_RESULT_FILE\"\n",
                "  exit 0\n",
                "fi\n",
                "echo {}\n",
            ),
            label
        ),
    );

    (library, rscript_dir, rscript)
}

#[cfg(unix)]
#[test]
fn tool_run_accepts_cli_rscript() {
    let cache_dir = temp_dir("ir-tool-run-cli-rscript-cache");
    let (library, _rscript_dir, rscript) = fake_tool_package_with_rscript(
        "ir-tool-run-cli-rscript",
        "Rscript",
        "selected=tool-rscript",
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "run", "--rscript"])
        .arg(&rscript)
        .args(["--from", "irfake", "hello"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "selected=tool-rscript");
}

#[cfg(unix)]
#[test]
fn tool_install_accepts_cli_rscript_and_records_recovery_command() {
    let cache_dir = temp_dir("ir-tool-install-cli-rscript-cache");
    let bin_dir = temp_dir("ir-tool-install-cli-rscript-bin");
    let rscript_name = "Rscript-ir-tool-install-cli-rscript";
    let (library, rscript_dir, rscript) = fake_tool_package_with_rscript(
        "ir-tool-install-cli-rscript",
        rscript_name,
        "selected=tool-rscript",
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env("PATH", path_with_bin_dir(&rscript_dir))
        .env_remove("IR_RSCRIPT")
        .args(["tool", "install", "--rscript", rscript_name])
        .args(["--bin-dir"])
        .arg(&bin_dir)
        .arg("irfake")
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Installed");
    let launcher = fs::read_to_string(launcher_path(&bin_dir, "hello")).unwrap();
    let selected = fs::canonicalize(&rscript).unwrap();
    let reinstall = format!(
        "ir tool install --force --rscript {} irfake",
        selected.to_string_lossy()
    );
    assert!(
        launcher.contains(&selected.to_string_lossy().into_owned()),
        "{launcher}"
    );
    assert!(launcher.contains(&reinstall), "{launcher}");
}

#[cfg(unix)]
#[test]
fn tool_install_records_env_selected_rscript_in_recovery_command() {
    let cache_dir = temp_dir("ir-tool-install-env-rscript-cache");
    let bin_dir = temp_dir("ir-tool-install-env-rscript-bin");
    let (library, _rscript_dir, rscript) = fake_tool_package_with_rscript(
        "ir-tool-install-env-rscript",
        "Rscript",
        "selected=env-rscript",
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env("IR_RSCRIPT", &rscript)
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg("irfake")
        .output()
        .unwrap();

    assert_success(&out);
    let launcher = fs::read_to_string(launcher_path(&bin_dir, "hello")).unwrap();
    let selected = std::path::absolute(&rscript).unwrap();
    let reinstall = format!(
        "ir tool install --force --rscript {} irfake",
        selected.to_string_lossy()
    );
    assert!(launcher.contains(&reinstall), "{launcher}");
}

#[cfg(windows)]
#[test]
fn tool_install_quotes_windows_recovery_rscript() {
    let cache_dir = temp_dir("ir-tool-install-windows-rscript-cache");
    let bin_dir = temp_dir("ir-tool-install-windows-rscript-bin");
    let library = temp_dir("ir-tool-install-windows-rscript-library");
    let rscript_dir = temp_dir("ir tool install windows rscript");
    let package = library.join("irfake");
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    fs::write(exec_dir.join("hello.R"), "#!/usr/bin/env Rscript\n").unwrap();

    let rscript = rscript_dir.join("Rscript.cmd");
    fs::write(
        &rscript,
        concat!(
            "@echo off\r\n",
            "if not \"%IR_RESOLVE_RESULT_FILE%\" == \"\" (\r\n",
            "  more > nul\r\n",
            "  echo %IR_TEST_LIBRARY%> \"%IR_RESOLVE_RESULT_FILE%\"\r\n",
            "  echo irfake> \"%IR_RESOLVE_PACKAGE_RESULT_FILE%\"\r\n",
            "  exit /b 0\r\n",
            ")\r\n",
            "echo selected=windows-rscript\r\n",
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "install", "--rscript"])
        .arg(&rscript)
        .args(["--bin-dir"])
        .arg(&bin_dir)
        .arg("irfake")
        .output()
        .unwrap();

    assert_success(&out);
    let launcher = fs::read_to_string(launcher_path(&bin_dir, "hello")).unwrap();
    let selected = std::path::absolute(&rscript).unwrap();
    let reinstall = format!(
        "ir tool install --force --rscript \"{}\" irfake",
        selected.to_string_lossy().replace('"', "\"\"")
    );
    assert!(launcher.contains(&reinstall), "{launcher}");
    assert!(
        !launcher.contains("--rscript '"),
        "Windows recovery command should not use POSIX quoting:\n{launcher}"
    );
}

#[cfg(unix)]
#[test]
fn tool_install_with_path_rscript_symlink_records_target() {
    let cache_dir = temp_dir("ir-tool-install-rscript-link-cache");
    let bin_dir = temp_dir("ir-tool-install-rscript-link-bin");
    let link_dir = temp_dir("ir-tool-install-rscript-link-path");
    let (library, _target_dir, target_rscript) = fake_tool_package_with_rscript(
        "ir-tool-install-rscript-link-target",
        "Rscript",
        "target-rscript",
    );
    let link_rscript = link_dir.join("Rscript");
    std::os::unix::fs::symlink(&target_rscript, &link_rscript).unwrap();

    let path = std::env::join_paths(
        std::iter::once(link_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_TEST_LIBRARY", &library)
        .env("PATH", path)
        .env_remove("IR_RSCRIPT")
        .args(["tool", "install", "--bin-dir"])
        .arg(&bin_dir)
        .arg("irfake")
        .output()
        .unwrap();

    assert_success(&out);
    let launcher = fs::read_to_string(launcher_path(&bin_dir, "hello")).unwrap();
    let target = fs::canonicalize(&target_rscript).unwrap();
    assert!(
        launcher.contains(&target.to_string_lossy().into_owned()),
        "{launcher}"
    );
    assert!(
        !launcher.contains(&link_rscript.to_string_lossy().into_owned()),
        "{launcher}"
    );
}

#[cfg(unix)]
#[test]
fn tool_install_with_rscript_wrapper_records_primary_package_marker() {
    let cache_dir = temp_dir("ir-wrapper-tool-install-cache");
    let bin_dir = temp_dir("ir-wrapper-tool-install-bin");
    let wrapper = temp_path("ir-rscript-wrapper", "sh");
    fs::write(
        &wrapper,
        "#!/bin/sh\nexec \"$IR_TEST_RSCRIPT_TARGET\" \"$@\"\n",
    )
    .unwrap();
    make_executable(&wrapper);

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_RSCRIPT", &wrapper)
        .env("IR_TEST_RSCRIPT_TARGET", rscript())
        .args([
            "tool",
            "install",
            "--with",
            "docopt,pkgsearch,prettyunits",
            "--bin-dir",
        ])
        .arg(&bin_dir)
        .arg("cli")
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "Installed");
}

fn launcher_path(bin_dir: &Path, name: &str) -> PathBuf {
    #[cfg(unix)]
    {
        bin_dir.join(name)
    }

    #[cfg(not(unix))]
    {
        bin_dir.join(format!("{name}.cmd"))
    }
}

#[cfg(unix)]
fn write_fake_tool_resolver(rscript_dir: &Path, package: &str, arch: &str) -> PathBuf {
    let rscript = rscript_dir.join("Rscript");
    write_executable(
        &rscript,
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n",
                "  cat >/dev/null\n",
                "  printf '%s\\n' \"$IR_TEST_LIBRARY\" > \"$IR_RESOLVE_RESULT_FILE\"\n",
                "  printf '%s\\n' {} > \"$IR_RESOLVE_PACKAGE_RESULT_FILE\"\n",
                "  exit 0\n",
                "fi\n",
                "printf '%s\\n' {}\n",
            ),
            package, arch
        ),
    );
    rscript
}

#[cfg(unix)]
fn write_fake_tool_resolver_with_forwarded_arch(
    rscript_dir: &Path,
    package: &str,
    default_arch: &str,
) -> PathBuf {
    let rscript = rscript_dir.join("Rscript");
    write_executable(
        &rscript,
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n",
                "  cat >/dev/null\n",
                "  printf '%s\\n' \"$IR_TEST_LIBRARY\" > \"$IR_RESOLVE_RESULT_FILE\"\n",
                "  printf '%s\\n' {} > \"$IR_RESOLVE_PACKAGE_RESULT_FILE\"\n",
                "  exit 0\n",
                "fi\n",
                "case \" $* \" in\n",
                "  *' -e '*) is_arch_query=true ;;\n",
                "  *) is_arch_query=false ;;\n",
                "esac\n",
                "if [ \"$is_arch_query\" = true ]; then\n",
                "  case \" $* \" in\n",
                "    *' --arch=i386 '*) printf '%s\\n' i386 ;;\n",
                "    *) printf '%s\\n' {} ;;\n",
                "  esac\n",
                "  exit 0\n",
                "fi\n",
                "printf 'tool.ran=true\\n'\n",
                "case \" $* \" in\n",
                "  *' --arch=i386 '*) printf 'tool.arch.arg=true\\n' ;;\n",
                "  *) printf 'tool.arch.arg=false\\n' ;;\n",
                "esac\n",
            ),
            package, default_arch
        ),
    );
    rscript
}

#[cfg(unix)]
fn write_fake_tool_resolver_with_env_arch(
    rscript_dir: &Path,
    package: &str,
    default_arch: &str,
) -> PathBuf {
    let rscript = rscript_dir.join("Rscript");
    write_executable(
        &rscript,
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n",
                "  cat >/dev/null\n",
                "  printf '%s\\n' \"$IR_TEST_LIBRARY\" > \"$IR_RESOLVE_RESULT_FILE\"\n",
                "  printf '%s\\n' {} > \"$IR_RESOLVE_PACKAGE_RESULT_FILE\"\n",
                "  exit 0\n",
                "fi\n",
                "arch=${{R_ARCH#/}}\n",
                "if [ -z \"$arch\" ]; then arch={}; fi\n",
                "case \" $* \" in\n",
                "  *' -e '*) printf '%s\\n' \"$arch\"; exit 0 ;;\n",
                "esac\n",
                "printf 'tool.r_arch=%s\\n' \"${{R_ARCH:-<unset>}}\"\n",
            ),
            package, default_arch
        ),
    );
    rscript
}

#[cfg(unix)]
fn write_fake_tool_resolver_rejecting_default_packages_in_arch_query(
    rscript_dir: &Path,
    package: &str,
    arch: &str,
) -> PathBuf {
    let rscript = rscript_dir.join("Rscript");
    write_executable(
        &rscript,
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n",
                "  cat >/dev/null\n",
                "  printf '%s\\n' \"$IR_TEST_LIBRARY\" > \"$IR_RESOLVE_RESULT_FILE\"\n",
                "  printf '%s\\n' {} > \"$IR_RESOLVE_PACKAGE_RESULT_FILE\"\n",
                "  exit 0\n",
                "fi\n",
                "case \" $* \" in\n",
                "  *' -e '*) is_arch_query=true ;;\n",
                "  *) is_arch_query=false ;;\n",
                "esac\n",
                "if [ \"$is_arch_query\" = true ]; then\n",
                "  case \" $* \" in\n",
                "    *' --default-packages=cli '*) printf '%s\\n' 'default package loaded during arch query' >&2; exit 43 ;;\n",
                "  esac\n",
                "  printf '%s\\n' {}\n",
                "  exit 0\n",
                "fi\n",
                "printf 'tool.ran=true\\n'\n",
                "case \" $* \" in\n",
                "  *' --default-packages=cli '*) printf 'tool.default.packages=true\\n' ;;\n",
                "  *) printf 'tool.default.packages=false\\n' ;;\n",
                "esac\n",
            ),
            package, arch
        ),
    );
    rscript
}

#[cfg(unix)]
fn write_fake_tool_resolver_with_arch_sensitive_resolution(
    rscript_dir: &Path,
    package: &str,
) -> PathBuf {
    let rscript = rscript_dir.join("Rscript");
    write_executable(
        &rscript,
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -n \"${{IR_RESOLVE_RESULT_FILE:-}}\" ]; then\n",
                "  cat >/dev/null\n",
                "  case \" $* \" in\n",
                "    *' --arch=i386 '*) arch=i386 ;;\n",
                "    *) arch=x64 ;;\n",
                "  esac\n",
                "  mkdir -p \"$IR_TEST_LIBRARY\"/{}/bin/$arch\n",
                "  cat >\"$IR_TEST_LIBRARY\"/{}/bin/$arch/archtool <<EOF\n",
                "#!/bin/sh\n",
                "printf 'tool.ran=true\\n'\n",
                "printf 'tool.arch=$arch\\n'\n",
                "EOF\n",
                "  chmod +x \"$IR_TEST_LIBRARY\"/{}/bin/$arch/archtool\n",
                "  printf '%s\\n' \"$IR_TEST_LIBRARY\" > \"$IR_RESOLVE_RESULT_FILE\"\n",
                "  printf '%s\\n' {} > \"$IR_RESOLVE_PACKAGE_RESULT_FILE\"\n",
                "  exit 0\n",
                "fi\n",
                "case \" $* \" in\n",
                "  *' -e '*)\n",
                "    case \" $* \" in\n",
                "      *' --arch=i386 '*) printf '%s\\n' i386 ;;\n",
                "      *) printf '%s\\n' x64 ;;\n",
                "    esac\n",
                "    exit 0\n",
                "    ;;\n",
                "esac\n",
                "printf 'tool.ran=true\\n'\n",
                "case \" $* \" in\n",
                "  *' --arch=i386 '*) printf 'tool.arch.arg=true\\n' ;;\n",
                "  *) printf 'tool.arch.arg=false\\n' ;;\n",
                "esac\n",
            ),
            package, package, package, package
        ),
    );
    rscript
}

#[cfg(unix)]
fn path_with_bin_dir(bin_dir: &Path) -> std::ffi::OsString {
    std::env::join_paths(
        std::iter::once(bin_dir.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap()
}
