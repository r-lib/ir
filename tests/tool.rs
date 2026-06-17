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
    let cache_dir = test_cache("ir-tool-run-real-cache");
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
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn rx_executes_real_package_entrypoint() {
    let cache_dir = test_cache("ir-rx-real-cache");
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
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn tool_install_installs_real_package_entrypoint() {
    let cache_dir = test_cache("ir-tool-install-real-cache");
    let bin_dir = unique_dir("ir-e2e-tool-install-bin");

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

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[cfg(target_os = "macos")]
#[test]
fn tool_install_adds_default_macos_bin_dir_to_zprofile_once() {
    let cache_dir = unique_dir("ir-tool-install-macos-path-cache");
    let home = unique_dir("ir-tool-install-macos-path-home");
    let default_bin_dir = home.join(".local").join("bin");
    fs::create_dir_all(&default_bin_dir).unwrap();
    let package_dir = unique_dir("ir-tool-install-macos-path-packages");
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

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&package_dir);
}

#[cfg(target_os = "macos")]
#[test]
fn tool_install_custom_bin_dir_skips_default_macos_path_setup() {
    let cache_dir = unique_dir("ir-tool-install-custom-path-cache");
    let home = unique_dir("ir-tool-install-custom-path-home");
    let bin_dir = unique_path("ir-tool-install-custom-path-bin", "");
    let package_dir = unique_dir("ir-tool-install-custom-path-packages");
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

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[cfg(target_os = "macos")]
#[test]
fn tool_install_existing_launcher_does_not_modify_zprofile() {
    let cache_dir = unique_dir("ir-tool-install-collision-path-cache");
    let home = unique_dir("ir-tool-install-collision-path-home");
    let default_bin_dir = home.join(".local").join("bin");
    fs::create_dir_all(&default_bin_dir).unwrap();
    fs::write(
        launcher_path(&default_bin_dir, "hello"),
        "existing launcher\n",
    )
    .unwrap();
    let package_dir = unique_dir("ir-tool-install-collision-path-packages");
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

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&package_dir);
}

#[cfg(target_os = "macos")]
#[test]
fn tool_install_write_failure_does_not_modify_zprofile() {
    use std::os::unix::fs::PermissionsExt as _;

    let cache_dir = unique_dir("ir-tool-install-write-failure-cache");
    let home = unique_dir("ir-tool-install-write-failure-home");
    let default_bin_dir = home.join(".local").join("bin");
    fs::create_dir_all(&default_bin_dir).unwrap();
    let original_permissions = fs::metadata(&default_bin_dir).unwrap().permissions();
    fs::set_permissions(&default_bin_dir, fs::Permissions::from_mode(0o555)).unwrap();

    let package_dir = unique_dir("ir-tool-install-write-failure-packages");
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

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&package_dir);
}

#[test]
fn tool_run_and_install_rapp_package_frontend() {
    let cache_dir = unique_dir("ir-rapp-frontend-cache");
    let bin_dir = unique_dir("ir-rapp-frontend-bin");
    let app = unique_path("ir-rapp-frontend-app", "R");
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

    let _ = fs::remove_file(&app);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[test]
fn tool_run_and_install_use_launcher_metadata() {
    let cache_dir = unique_dir("ir-tool-launcher-metadata-cache");
    let bin_dir = unique_dir("ir-tool-launcher-metadata-bin");
    let package_dir = unique_dir("ir-tool-launcher-metadata-packages");
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

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[test]
fn tool_run_rejects_duplicate_launcher_metadata_names() {
    let cache_dir = unique_dir("ir-tool-duplicate-launcher-cache");
    let package_dir = unique_dir("ir-tool-duplicate-launcher-packages");
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

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[test]
fn tool_run_ignores_non_r_direct_file_for_metadata_name() {
    let cache_dir = unique_dir("ir-tool-non-r-direct-cache");
    let package_dir = unique_dir("ir-tool-non-r-direct-packages");
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

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[cfg(unix)]
#[test]
fn tool_run_and_install_support_direct_package_scripts() {
    let cache_dir = unique_dir("ir-tool-direct-script-cache");
    let bin_dir = unique_dir("ir-tool-direct-script-bin");
    let package_dir = unique_dir("ir-tool-direct-script-packages");
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

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[test]
fn tool_run_skips_binary_exec_files() {
    let cache_dir = unique_dir("ir-tool-binary-exec-cache");
    let package_dir = unique_dir("ir-tool-binary-exec-packages");
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

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[test]
fn tool_install_rejects_invalid_metadata_launcher_names() {
    for (package_name, launcher_name) in [
        ("irtoolbadname", "bad?name"),
        ("irtoolpercentname", "foo%PATH%"),
        ("irtooldotname", "."),
        ("irtooldotdotname", ".."),
    ] {
        let cache_dir = unique_dir("ir-tool-invalid-launcher-cache");
        let bin_dir = unique_dir("ir-tool-invalid-launcher-bin");
        let package_dir = unique_dir("ir-tool-invalid-launcher-packages");
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

        let _ = fs::remove_dir_all(&bin_dir);
        let _ = fs::remove_dir_all(&cache_dir);
        let _ = fs::remove_dir_all(&package_dir);
    }
}

#[test]
fn tool_run_limits_metadata_lookup_to_primary_package() {
    let cache_dir = unique_dir("ir-tool-primary-package-cache");
    let package_dir = unique_dir("ir-tool-primary-package-packages");
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

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[test]
fn tool_run_and_install_apply_package_default_packages() {
    let cache_dir = unique_dir("ir-tool-default-packages-cache");
    let bin_dir = unique_dir("ir-tool-default-packages-bin");
    let package_dir = unique_dir("ir-tool-default-packages-packages");
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

    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_dir_all(&package_dir);
}

#[cfg(unix)]
#[test]
fn resolver_tooling_uses_compatible_user_library_packages() {
    let cache_dir = unique_dir("ir-compatible-tooling-cache");
    let user_library = unique_dir("ir-compatible-tooling-user-library");
    let fake_load_marker = unique_path("ir-compatible-secretbase-loaded", "txt");
    let profile = unique_path("ir-compatible-tooling-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
ir_test_write_pkg <- function(lib, pkg, namespace, code,
                              built = as.character(getRversion())) {{
  path <- file.path(lib, pkg)
  dir.create(file.path(path, "R"), recursive = TRUE, showWarnings = FALSE)
  dir.create(file.path(path, "Meta"), recursive = TRUE, showWarnings = FALSE)

  built_field <- paste0(
    "R ", built, "; ; 2026-01-01 00:00:00 UTC; ", .Platform$OS.type
  )
  description <- c(
    Package = pkg,
    Version = "0.0.1",
    Title = pkg,
    Description = paste0(pkg, "."),
    License = "MIT",
    Built = built_field
  )

  writeLines(paste(names(description), description, sep = ": "),
             file.path(path, "DESCRIPTION"))
  writeLines(namespace, file.path(path, "NAMESPACE"))
  writeLines(code, file.path(path, "R", pkg))
  saveRDS(
    list(
      DESCRIPTION = description,
      Built = list(
        R = package_version(built),
        Platform = "",
        Date = "2026-01-01 00:00:00 UTC",
        OStype = .Platform$OS.type
      ),
      Depends = NULL,
      Imports = NULL,
      LinkingTo = NULL,
      Suggests = NULL
    ),
    file.path(path, "Meta", "package.rds")
  )
}}

ir_test_write_pkg(
  Sys.getenv("R_LIBS_USER"),
  "secretbase",
  "export(sha256)",
  paste(
    paste0(".onLoad <- function(...) writeLines('loaded', ", deparse({}), ")"),
    "sha256 <- function(x) 'ambienthash'",
    sep = "\n"
  )
)
ir_test_write_pkg(
  Sys.getenv("R_LIBS_USER"),
  "pak",
  "export(pkg_deps)",
  paste(
    "pkg_deps <- function(refs, dependencies = NA, upgrade = TRUE) {{",
    "  refs <- as.character(refs)",
    "  data.frame(",
    "    status = rep('OK', length(refs)),",
    "    ref = refs,",
    "    package = sub('@.*$', '', refs),",
    "    version = rep('0.0.1', length(refs)),",
    "    type = rep('standard', length(refs)),",
    "    priority = NA_character_,",
    "    direct = TRUE,",
    "    stringsAsFactors = FALSE",
    "  )",
    "}}",
    sep = "\n"
  )
)
ir_test_write_pkg(
  Sys.getenv("R_LIBS_USER"),
  "renv",
  "export(use)",
  paste(
    "use <- function(..., library, repos, attach, sandbox, isolate, verbose) {{",
    "  specs <- unlist(list(...), use.names = FALSE)",
    "  for (spec in specs) {{",
    "    pkg <- sub('@.*$', '', spec)",
    "    dir.create(file.path(library, pkg), recursive = TRUE, showWarnings = FALSE)",
    "  }}",
    "  invisible(TRUE)",
    "}}",
    sep = "\n"
  )
)

utils::assignInNamespace("install.packages", function(...) {{
  stop("resolver should use compatible R_LIBS_USER tooling", call. = FALSE)
}}, ns = "utils")
"#,
            r_string(&fake_load_marker)
        ),
    )
    .unwrap();

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_LIBS_USER", &user_library)
        .env("R_PROFILE_USER", &profile)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            "cat('ir.fixture=compatible-tooling\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=compatible-tooling");
    assert!(
        fake_load_marker.exists(),
        "resolver should load compatible secretbase from R_LIBS_USER"
    );

    let _ = fs::remove_file(&profile);
    let _ = fs::remove_file(&fake_load_marker);
    let _ = fs::remove_dir_all(&user_library);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[cfg(unix)]
#[test]
fn resolver_tooling_ignores_wrong_r_minor_user_library_package() {
    let cache_dir = unique_dir("ir-ambient-tooling-cache");
    let ambient_library = unique_dir("ir-ambient-tooling-user-library");
    let fake_secretbase_load_marker = unique_path("ir-ambient-secretbase-loaded", "txt");
    let fake_pillar_load_marker = unique_path("ir-ambient-pillar-loaded", "txt");
    let profile = unique_path("ir-tooling-install-profile", "R");

    fs::write(
        &profile,
        format!(
            r#"
ir_test_write_pkg <- function(lib, pkg, namespace, code, built = NULL) {{
  path <- file.path(lib, pkg)
  dir.create(file.path(path, "R"), recursive = TRUE, showWarnings = FALSE)

  description <- c(
    Package = pkg,
    Version = "0.0.1",
    Title = pkg,
    Description = paste0(pkg, "."),
    License = "MIT"
  )
  if (!is.null(built)) {{
    built_field <- paste0(
      "R ", built, "; ; 2026-01-01 00:00:00 UTC; ", .Platform$OS.type
    )
    description <- c(description, Built = built_field)
  }}

  writeLines(paste(names(description), description, sep = ": "),
             file.path(path, "DESCRIPTION"))
  writeLines(namespace, file.path(path, "NAMESPACE"))
  writeLines(code, file.path(path, "R", pkg))

  if (!is.null(built)) {{
    dir.create(file.path(path, "Meta"), recursive = TRUE, showWarnings = FALSE)
    saveRDS(
      list(
        DESCRIPTION = description,
        Built = list(
          R = package_version(built),
          Platform = "",
          Date = "2026-01-01 00:00:00 UTC",
          OStype = .Platform$OS.type
        ),
        Depends = NULL,
        Imports = NULL,
        LinkingTo = NULL,
        Suggests = NULL
      ),
      file.path(path, "Meta", "package.rds")
    )
  }}
}}

ir_test_private_lib <- file.path(
  Sys.getenv("IR_CACHE_DIR"),
  "tooling",
  paste0(getRversion(), "-", R.version$platform)
)
ir_test_r_parts <- strsplit(as.character(getRversion()), ".", fixed = TRUE)[[1]]
ir_test_wrong_minor <- if (identical(ir_test_r_parts[[2]], "0")) "1" else "0"
ir_test_wrong_r <- paste(ir_test_r_parts[[1]], ir_test_wrong_minor, "0", sep = ".")

ir_test_write_pkg(
  Sys.getenv("R_LIBS_USER"),
  "secretbase",
  "export(sha256)",
  paste(
    paste0(".onLoad <- function(...) writeLines('loaded', ", deparse({}), ")"),
    "sha256 <- function(x) 'ambienthash'",
    sep = "\n"
  ),
  built = ir_test_wrong_r
)
ir_test_write_pkg(
  Sys.getenv("R_LIBS_USER"),
  "pillar",
  "export(pillar_shaft)",
  paste(
    paste0(".onLoad <- function(...) writeLines('loaded', ", deparse({}), ")"),
    "pillar_shaft <- function(x, ...) x",
    sep = "\n"
  ),
  built = ir_test_wrong_r
)
ir_test_write_pkg(
  ir_test_private_lib,
  "pak",
  "export(pkg_deps)",
  paste(
    "pkg_deps <- function(refs, dependencies = NA, upgrade = TRUE) {{",
    "  invisible(requireNamespace('pillar', quietly = TRUE))",
    "  refs <- as.character(refs)",
    "  data.frame(",
    "    status = rep('OK', length(refs)),",
    "    ref = refs,",
    "    package = sub('@.*$', '', refs),",
    "    version = rep('0.0.1', length(refs)),",
    "    type = rep('standard', length(refs)),",
    "    priority = NA_character_,",
    "    direct = TRUE,",
    "    stringsAsFactors = FALSE",
    "  )",
    "}}",
    sep = "\n"
  )
)
ir_test_write_pkg(
  ir_test_private_lib,
  "renv",
  "export(use)",
  paste(
    "use <- function(..., library, repos, attach, sandbox, isolate, verbose) {{",
    "  specs <- unlist(list(...), use.names = FALSE)",
    "  for (spec in specs) {{",
    "    pkg <- sub('@.*$', '', spec)",
    "    dir.create(file.path(library, pkg), recursive = TRUE, showWarnings = FALSE)",
    "  }}",
    "  invisible(TRUE)",
    "}}",
    sep = "\n"
  )
)

utils::assignInNamespace("install.packages", function(pkgs, lib, repos, ...) {{
  dir.create(lib, recursive = TRUE, showWarnings = FALSE)
  for (pkg in pkgs) {{
    if (!identical(pkg, "secretbase"))
      stop("unexpected resolver tooling package: ", pkg, call. = FALSE)
    ir_test_write_pkg(
      lib,
      "secretbase",
      "export(sha256)",
      "sha256 <- function(x) 'privatehash'"
    )
  }}
}}, ns = "utils")
"#,
            r_string(&fake_secretbase_load_marker),
            r_string(&fake_pillar_load_marker)
        ),
    )
    .unwrap();

    let first = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_LIBS_USER", &ambient_library)
        .env("R_PROFILE_USER", &profile)
        .args([
            "run",
            "--isolated",
            "--with",
            "cli",
            "--vanilla",
            "-e",
            "cat('ir.fixture=ambient-tooling\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&first);
    assert_stdout_contains(&first, "ir.fixture=ambient-tooling");
    assert!(
        !fake_secretbase_load_marker.exists(),
        "resolver should not load secretbase from ambient R_LIBS_USER"
    );
    assert!(
        !fake_pillar_load_marker.exists(),
        "resolver should remove wrong-R-minor R_LIBS_USER before pak loads auxiliary packages"
    );

    let second = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("R_LIBS_USER", &ambient_library)
        .env("R_PROFILE_USER", &profile)
        .args([
            "run",
            "--isolated",
            "--with",
            "glue",
            "--vanilla",
            "-e",
            "cat('ir.fixture=ambient-tooling-warm\\n')",
        ])
        .output()
        .unwrap();

    assert_success(&second);
    assert_stdout_contains(&second, "ir.fixture=ambient-tooling-warm");
    assert!(
        !fake_pillar_load_marker.exists(),
        "resolver should prune wrong-R-minor R_LIBS_USER even when private tooling is warm"
    );

    let _ = fs::remove_file(&profile);
    let _ = fs::remove_file(&fake_secretbase_load_marker);
    let _ = fs::remove_file(&fake_pillar_load_marker);
    let _ = fs::remove_dir_all(&ambient_library);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[cfg(unix)]
#[test]
fn tool_install_warm_resolution_cache_skips_resolver_rscript() {
    let cache_dir = unique_dir("ir-warm-tool-install-cache");
    let bin_dir = unique_dir("ir-warm-tool-install-bin");
    let rscript = rscript();
    let profile = unique_path("ir-rprofile-fail", "R");
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

    let _ = fs::remove_file(&profile);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[cfg(unix)]
#[test]
fn tool_install_with_path_rscript_symlink_records_target() {
    let cache_dir = unique_dir("ir-tool-install-rscript-link-cache");
    let bin_dir = unique_dir("ir-tool-install-rscript-link-bin");
    let link_dir = unique_dir("ir-tool-install-rscript-link-path");
    let target_dir = unique_dir("ir-tool-install-rscript-link-target");
    let library = unique_dir("ir-tool-install-rscript-link-library");
    let package = library.join("irfake");
    let exec_dir = package.join("exec");
    fs::create_dir_all(&exec_dir).unwrap();
    write_executable(
        &exec_dir.join("hello.R"),
        "#!/usr/bin/env Rscript\ncat('hello\\n')\n",
    );

    let target_rscript = target_dir.join("Rscript");
    write_executable(
        &target_rscript,
        concat!(
            "#!/bin/sh\n",
            "if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n",
            "  cat >/dev/null\n",
            "  printf '%s\\n' \"$IR_TEST_LIBRARY\" > \"$IR_RESOLVE_RESULT_FILE\"\n",
            "  printf '%s\\n' irfake > \"$IR_RESOLVE_PACKAGE_RESULT_FILE\"\n",
            "  exit 0\n",
            "fi\n",
            "echo target-rscript\n",
        ),
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

    let _ = fs::remove_dir_all(&library);
    let _ = fs::remove_dir_all(&target_dir);
    let _ = fs::remove_dir_all(&link_dir);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
}

#[cfg(unix)]
#[test]
fn tool_install_with_rscript_wrapper_records_primary_package_marker() {
    let cache_dir = unique_dir("ir-wrapper-tool-install-cache");
    let bin_dir = unique_dir("ir-wrapper-tool-install-bin");
    let wrapper = unique_path("ir-rscript-wrapper", "sh");
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

    let _ = fs::remove_file(&wrapper);
    let _ = fs::remove_dir_all(&bin_dir);
    let _ = fs::remove_dir_all(&cache_dir);
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
