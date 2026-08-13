//! Integration tests for `ir init --file`.

mod support;

use support::*;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use time::OffsetDateTime;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

fn init_cache() -> &'static Path {
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    CACHE.get_or_init(|| {
        // Nextest gives every process in one run the same ID, so public init
        // tests share the resolver lock without touching the user's cache.
        let run = std::env::var("NEXTEST_RUN_ID")
            .unwrap_or_else(|_| format!("process-{}", std::process::id()));
        let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("ir-init-cache-{run}"));
        fs::create_dir_all(&path).unwrap();
        path
    })
}

fn init_command() -> Command {
    let mut command = ir();
    command.env("IR_CACHE_DIR", init_cache());
    command
}

fn init(script: &Path) -> Output {
    init_command()
        .args(["init", "--file"])
        .arg(script)
        .output()
        .unwrap()
}

fn assert_stderr_contains(output: &Output, expected: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(expected),
        "stderr did not contain {expected:?}\n{}",
        output_text(output)
    );
}

fn active_r_requirement() -> String {
    let expression = concat!(
        "status <- R.version$status; ",
        "if (identical(status, 'Under development (unstable)')) cat('devel') ",
        "else if (status %in% c('alpha', 'beta', 'RC')) cat('next') ",
        "else cat(paste('>=', paste(R.version$major, ",
        "strsplit(R.version$minor, '.', fixed = TRUE)[[1L]][[1L]], sep = '.')))"
    );
    let output = Command::new(rscript())
        .args(["--vanilla", "-e", expression])
        .output()
        .unwrap();
    assert_success(&output);
    String::from_utf8(output.stdout).unwrap()
}

fn active_r_minor_requirement() -> String {
    let output = Command::new(rscript())
        .args([
            "--vanilla",
            "-e",
            "cat(paste('>=', paste(R.version$major, strsplit(R.version$minor, '.', fixed = TRUE)[[1L]][[1L]], sep = '.')))",
        ])
        .output()
        .unwrap();
    assert_success(&output);
    String::from_utf8(output.stdout).unwrap()
}

fn utc_date() -> String {
    OffsetDateTime::now_utc().date().to_string()
}

#[cfg(unix)]
fn wait_for_marker(mut child: std::process::Child, marker: &Path) -> std::process::Child {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !marker.exists() && std::time::Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            let output = child.wait_with_output().unwrap();
            panic!(
                "initializer exited before reaching the test checkpoint\n{}",
                output_text(&output)
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        marker.exists(),
        "initializer should reach the test checkpoint"
    );
    child
}

#[test]
fn init_script_adds_bare_frontmatter_and_preserves_body() {
    let script = temp_path("ir-init-script", "R");
    let body = b"library(dplyr)\njsonlite::toJSON(airquality)\n";
    fs::write(&script, body).unwrap();

    let date_before = utc_date();
    let output = init(&script);
    let date_after = utc_date();

    assert_success(&output);
    let initialized = fs::read(&script).unwrap();
    let expected_prefix = format!(
        "#!/usr/bin/env -S ir run\n\
#| packages:\n\
#|   - \"dplyr\"\n\
#|   - \"jsonlite\"\n\
#| r-version: \"{}\"\n\
#| isolated: true\n",
        active_r_requirement()
    );
    assert!(
        initialized.starts_with(expected_prefix.as_bytes()),
        "{}",
        String::from_utf8_lossy(&initialized)
    );
    let initialized_text = String::from_utf8_lossy(&initialized);
    assert!(
        [date_before, date_after]
            .iter()
            .any(|date| initialized_text.contains(&format!("#| exclude-newer: \"{date}\"\n"))),
        "{initialized_text}"
    );
    assert!(initialized.ends_with(body));
    assert_stdout_contains(&output, "Initialized script");
}

#[test]
fn init_script_ignores_nearby_renv_lockfile() {
    let project = temp_dir("ir-init-renv-ignored");
    let script = project.join("analysis.R");
    fs::write(&script, "library(dplyr)\n").unwrap();
    fs::write(
        project.join("renv.lock"),
        r#"{
  "R": {"Version": "9.9.9"},
  "Packages": {
    "dplyr": {
      "Package": "dplyr",
      "Version": "0.0.1",
      "Source": "Repository",
      "Repository": "CRAN"
    }
  }
}"#,
    )
    .unwrap();

    let output = init(&script);

    assert_success(&output);
    let initialized = fs::read_to_string(&script).unwrap();
    assert!(initialized.contains("#|   - \"dplyr\"\n"), "{initialized}");
    assert!(!initialized.contains("dplyr=="), "{initialized}");
    assert!(!initialized.contains("dplyr@"), "{initialized}");
}

#[test]
fn init_script_omits_r_supplied_and_implied_packages() {
    let script = temp_path("ir-init-direct-packages", "R");
    fs::write(
        &script,
        "library(MASS)\nstats::lm(mpg ~ cyl, mtcars)\nlibrary(dplyr)\nDBI::dbConnect\n",
    )
    .unwrap();

    let output = init(&script);

    assert_success(&output);
    let initialized = fs::read_to_string(&script).unwrap();
    assert!(initialized.contains("#|   - \"DBI\"\n"), "{initialized}");
    assert!(initialized.contains("#|   - \"dplyr\"\n"), "{initialized}");
    assert!(!initialized.contains("#|   - \"MASS\""), "{initialized}");
    assert!(!initialized.contains("#|   - \"stats\""), "{initialized}");
    assert!(!initialized.contains("dbplyr"), "{initialized}");
}

#[test]
fn init_script_emits_an_empty_package_list() {
    let script = temp_path("ir-init-empty-packages", "R");
    fs::write(&script, "cat('ok')\n").unwrap();

    let output = init(&script);

    assert_success(&output);
    let initialized = fs::read_to_string(&script).unwrap();
    assert!(initialized.contains("#| packages: []\n"), "{initialized}");
}

#[cfg(unix)]
#[test]
fn init_script_records_r_release_channels() {
    let wrapper = temp_path("ir-init-unreleased-rscript", "sh");
    write_executable(
        &wrapper,
        "#!/bin/sh\ndriver=\nfor argument in \"$@\"; do\n  driver=$argument\ndone\nexport IR_TEST_DRIVER=\"$driver\"\nexec \"$IR_TEST_REAL_RSCRIPT\" --vanilla -e 'R.version$status <- Sys.getenv(\"IR_TEST_R_STATUS\"); sys.source(Sys.getenv(\"IR_TEST_DRIVER\"), envir = .GlobalEnv); ir_init_main()'\n",
    );

    let patched = active_r_minor_requirement();
    for (status, expected) in [
        ("Under development (unstable)", "devel"),
        ("beta", "next"),
        ("Patched", patched.as_str()),
    ] {
        let script = temp_path(&format!("ir-init-{expected}-r"), "R");
        fs::write(&script, "cat('ok')\n").unwrap();

        let output = init_command()
            .env("IR_RSCRIPT", &*wrapper)
            .env("IR_TEST_REAL_RSCRIPT", rscript())
            .env("IR_TEST_R_STATUS", status)
            .args(["init", "--file"])
            .arg(&*script)
            .output()
            .unwrap();

        assert_success(&output);
        let initialized = fs::read_to_string(&script).unwrap();
        assert!(
            initialized.contains(&format!("#| r-version: \"{expected}\"\n")),
            "{initialized}"
        );
    }
}

#[test]
fn init_script_replaces_existing_shebang_with_canonical_shebang() {
    for (name, shebang) in [
        ("rscript", "#!/usr/bin/Rscript"),
        ("unsplit-env", "#!/usr/bin/env ir run"),
        ("other-source", "#!/usr/bin/env -S ir run other.R"),
    ] {
        let script = temp_path(&format!("ir-init-{name}-shebang"), "R");
        fs::write(&script, format!("{shebang}\ncat('ok')\n")).unwrap();

        let output = init(&script);

        assert_success(&output);
        let initialized = fs::read_to_string(&script).unwrap();
        assert!(
            initialized.starts_with("#!/usr/bin/env -S ir run\n#| packages: []\n"),
            "{initialized}"
        );
        assert!(!initialized.contains(shebang), "{initialized}");
        assert!(initialized.ends_with("cat('ok')\n"), "{initialized}");
    }
}

#[test]
fn init_script_preserves_crlf_body_and_metadata_style() {
    let script = temp_path("ir-init-crlf", "R");
    let original = b"library(glue)\r\ncat('ok')\r\n";
    fs::write(&script, original).unwrap();

    let output = init(&script);

    assert_success(&output);
    let initialized = fs::read(&script).unwrap();
    assert!(initialized.starts_with(b"#!/usr/bin/env -S ir run\r\n#| packages:\r\n"));
    assert!(initialized.ends_with(original));
}

#[test]
fn init_script_handles_shebang_without_a_newline() {
    let script = temp_path("ir-init-shebang-no-newline", "R");
    fs::write(&script, "#!/usr/bin/Rscript").unwrap();

    let output = init(&script);

    assert_success(&output);
    let initialized = fs::read_to_string(&script).unwrap();
    assert!(
        initialized.starts_with("#!/usr/bin/env -S ir run\n#| packages: []\n"),
        "{initialized}"
    );
    assert!(!initialized.contains("/usr/bin/Rscript"), "{initialized}");
}

#[test]
fn init_script_refuses_existing_metadata_without_modifying_file() {
    for (name, original) in [
        (
            "frontmatter",
            b"#!/usr/bin/env -S ir run\n#| packages: []\n\ncat('ok')\n".as_slice(),
        ),
        ("marker", b"#|packages: []\ncat('ok')\n".as_slice()),
        ("bom", b"\xef\xbb\xbfcat('ok')\n".as_slice()),
    ] {
        let script = temp_path(&format!("ir-init-existing-{name}"), "R");
        fs::write(&script, original).unwrap();

        let output = init(&script);

        assert!(!output.status.success(), "{}", output_text(&output));
        assert_eq!(fs::read(&script).unwrap(), original);
    }
}

#[test]
fn init_script_parse_failure_does_not_modify_file() {
    let script = temp_path("ir-init-parse-failure", "R");
    let original = b"library(dplyr\n";
    fs::write(&script, original).unwrap();

    let output = init(&script);

    assert!(!output.status.success(), "{}", output_text(&output));
    assert_stderr_contains(&output, "unexpected end of input");
    assert_eq!(fs::read(&script).unwrap(), original);
}

#[test]
fn init_script_validates_the_target_before_discovery() {
    let missing = temp_path("ir-init-missing", "R");
    let missing_output = init(&missing);
    assert!(
        !missing_output.status.success(),
        "{}",
        output_text(&missing_output)
    );
    assert_stderr_contains(&missing_output, "cannot read script");
    assert!(!missing.exists());

    let wrong_extension = temp_path("ir-init-wrong-extension", "qmd");
    fs::write(&wrong_extension, "cat('ok')\n").unwrap();
    let extension_output = init(&wrong_extension);
    assert!(
        !extension_output.status.success(),
        "{}",
        output_text(&extension_output)
    );
    assert_stderr_contains(&extension_output, "must have a `.R` extension");
    assert_eq!(fs::read_to_string(&wrong_extension).unwrap(), "cat('ok')\n");

    let directory = temp_dir("ir-init-directory.R");
    let directory_output = init(&directory);
    assert!(
        !directory_output.status.success(),
        "{}",
        output_text(&directory_output)
    );
    assert_stderr_contains(&directory_output, "not a regular file");
}

#[test]
fn init_script_reports_missing_rscript_without_modifying_file() {
    let script = temp_path("ir-init-missing-rscript", "R");
    let missing_rscript = temp_path("ir-init-nonexistent-rscript", "exe");
    let original = b"cat('ok')\n";
    fs::write(&script, original).unwrap();

    let output = init_command()
        .env("IR_RSCRIPT", &*missing_rscript)
        .args(["init", "--file"])
        .arg(&*script)
        .output()
        .unwrap();

    assert!(!output.status.success(), "{}", output_text(&output));
    assert_stderr_contains(&output, "Install R or set IR_RSCRIPT");
    assert_eq!(fs::read(&script).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn init_script_honors_ir_rscript() {
    let script = temp_path("ir-init-rscript", "R");
    let wrapper = temp_path("ir-init-rscript-wrapper", "sh");
    let marker = temp_path("ir-init-rscript-marker", "txt");
    fs::write(&script, "cat('ok')\n").unwrap();
    write_executable(
        &wrapper,
        "#!/bin/sh\nprintf 'invoked\n' > \"$IR_TEST_MARKER\"\nexec \"$IR_TEST_REAL_RSCRIPT\" \"$@\"\n",
    );

    let output = init_command()
        .env("IR_RSCRIPT", &*wrapper)
        .env("IR_TEST_MARKER", &*marker)
        .env("IR_TEST_REAL_RSCRIPT", rscript())
        .args(["init", "--file"])
        .arg(&*script)
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(fs::read_to_string(&marker).unwrap(), "invoked\n");
}

#[test]
fn initialized_script_runs_through_public_cli() {
    let script = temp_path("ir-init-runs", "R");
    fs::write(&script, "cat('ir.fixture=initialized-script\\n')\n").unwrap();
    let initialized = init(&script);
    assert_success(&initialized);

    let output = init_command()
        .env("IR_RSCRIPT", rscript())
        .args(["run", "--vanilla"])
        .arg(&*script)
        .output()
        .unwrap();

    assert_success(&output);
    assert_stdout_contains(&output, "ir.fixture=initialized-script");
}

#[cfg(unix)]
#[test]
fn initialized_script_is_executable_and_preserves_existing_mode_bits() {
    for (name, initial, expected) in [("plain", 0o644, 0o755), ("executable", 0o751, 0o751)] {
        let script = temp_path(&format!("ir-init-mode-{name}"), "R");
        fs::write(&script, "cat('ok')\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(initial)).unwrap();

        let output = init(&script);

        assert_success(&output);
        let mode = fs::metadata(&script).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, expected);
    }
}

#[cfg(unix)]
#[test]
fn initialized_script_runs_directly() {
    let script = temp_path("ir-init-direct-execution", "R");
    fs::write(&script, "cat('ir.fixture=direct-execution\\n')\n").unwrap();
    let initialized = init(&script);
    assert_success(&initialized);

    let ir_dir = Path::new(env!("CARGO_BIN_EXE_ir")).parent().unwrap();
    let path = std::env::join_paths(std::iter::once(ir_dir.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let output = Command::new(&*script)
        .env("PATH", path)
        .env("IR_CACHE_DIR", init_cache())
        .env("IR_RSCRIPT", rscript())
        .output()
        .unwrap();

    assert_success(&output);
    assert_stdout_contains(&output, "ir.fixture=direct-execution");
}

#[cfg(unix)]
#[test]
fn init_script_does_not_overwrite_concurrent_edits() {
    use std::process::Stdio;

    let script = temp_path("ir-init-concurrent-edit", "R");
    let wrapper = temp_path("ir-init-concurrent-edit-rscript", "sh");
    let entered = temp_path("ir-init-concurrent-edit-entered", "txt");
    let release = temp_path("ir-init-concurrent-edit-release", "txt");
    let edited = b"library(glue)\n";
    fs::write(&script, "library(dplyr)\n").unwrap();
    write_executable(
        &wrapper,
        "#!/bin/sh\nprintf 'entered\n' > \"$IR_TEST_ENTERED\"\nwhile [ ! -e \"$IR_TEST_RELEASE\" ]; do\n  sleep 0.02\ndone\nexec \"$IR_TEST_REAL_RSCRIPT\" \"$@\"\n",
    );

    let mut command = init_command();
    command
        .env("IR_RSCRIPT", &*wrapper)
        .env("IR_TEST_ENTERED", &*entered)
        .env("IR_TEST_RELEASE", &*release)
        .env("IR_TEST_REAL_RSCRIPT", rscript())
        .args(["init", "--file"])
        .arg(&*script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = wait_for_marker(command.spawn().unwrap(), &entered);

    fs::write(&script, edited).unwrap();
    fs::write(&release, "continue\n").unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(!output.status.success(), "{}", output_text(&output));
    assert_stderr_contains(&output, "changed during initialization");
    assert_eq!(fs::read(&script).unwrap(), edited);
}

#[cfg(unix)]
#[test]
fn init_script_uses_permissions_rechecked_after_discovery() {
    use std::process::Stdio;

    let script = temp_path("ir-init-concurrent-mode", "R");
    let wrapper = temp_path("ir-init-concurrent-mode-rscript", "sh");
    let entered = temp_path("ir-init-concurrent-mode-entered", "txt");
    let release = temp_path("ir-init-concurrent-mode-release", "txt");
    fs::write(&script, "cat('ok')\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).unwrap();
    write_executable(
        &wrapper,
        "#!/bin/sh\nprintf 'entered\n' > \"$IR_TEST_ENTERED\"\nwhile [ ! -e \"$IR_TEST_RELEASE\" ]; do\n  sleep 0.02\ndone\nexec \"$IR_TEST_REAL_RSCRIPT\" \"$@\"\n",
    );

    let mut command = init_command();
    command
        .env("IR_RSCRIPT", &*wrapper)
        .env("IR_TEST_ENTERED", &*entered)
        .env("IR_TEST_RELEASE", &*release)
        .env("IR_TEST_REAL_RSCRIPT", rscript())
        .args(["init", "--file"])
        .arg(&*script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = wait_for_marker(command.spawn().unwrap(), &entered);

    fs::set_permissions(&script, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&release, "continue\n").unwrap();
    let output = child.wait_with_output().unwrap();

    assert_success(&output);
    assert_eq!(
        fs::metadata(&script).unwrap().permissions().mode() & 0o777,
        0o711
    );
}

#[cfg(unix)]
#[test]
fn init_script_refuses_symlinks_without_modifying_target() {
    use std::os::unix::fs::symlink;

    let target = temp_path("ir-init-symlink-target", "R");
    let link = temp_path("ir-init-symlink", "R");
    fs::write(&target, "cat('ok')\n").unwrap();
    symlink(&*target, &*link).unwrap();

    let output = init(&link);

    assert!(!output.status.success(), "{}", output_text(&output));
    assert_stderr_contains(&output, "symbolic link");
    assert_eq!(fs::read_to_string(&target).unwrap(), "cat('ok')\n");
    assert!(fs::symlink_metadata(&*link)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn init_without_file_target_is_reserved_for_projects() {
    let script = temp_path("ir-init-reserved", "R");
    fs::write(&script, "cat('ok')\n").unwrap();

    let output = init_command().arg("init").arg(&*script).output().unwrap();

    assert!(!output.status.success(), "{}", output_text(&output));
    assert_stderr_contains(&output, "--file");
    assert_eq!(fs::read_to_string(&script).unwrap(), "cat('ok')\n");
}
