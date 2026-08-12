//! Integration tests for `ir init --script`.

mod support;

use support::*;

use std::fs;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

fn init(script: &std::path::Path) -> std::process::Output {
    ir().args(["init", "--script"])
        .arg(script)
        .output()
        .unwrap()
}

fn assert_stderr_contains(output: &std::process::Output, expected: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(expected),
        "stderr did not contain {expected:?}\n{}",
        output_text(output)
    );
}

#[test]
fn init_script_adds_frontmatter_and_preserves_body() {
    let script = temp_path("ir-init-script", "R");
    let body = b"library(dplyr)\njsonlite::toJSON(airquality)\n";
    fs::write(&script, body).unwrap();

    let out = init(&script);

    assert_success(&out);
    let initialized = fs::read(&script).unwrap();
    assert!(initialized.starts_with(
        b"#!/usr/bin/env -S ir run\n\
#| packages:\n\
#|   - dplyr\n\
#|   - jsonlite\n\
#| r-version: \""
    ));
    assert!(
        String::from_utf8_lossy(&initialized).contains("#| r-version: \">= "),
        "{}",
        String::from_utf8_lossy(&initialized)
    );
    assert!(initialized
        .ends_with(b"\"\n#| isolated: true\n\nlibrary(dplyr)\njsonlite::toJSON(airquality)\n"));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Initialized script"));
}

#[test]
fn init_script_omits_packages_supplied_by_r() {
    let script = temp_path("ir-init-r-supplied", "R");
    fs::write(&script, "library(MASS)\nstats::lm(mpg ~ cyl, mtcars)\n").unwrap();

    let out = init(&script);

    assert_success(&out);
    let initialized = fs::read_to_string(&script).unwrap();
    assert!(initialized.contains("#| packages: []\n"), "{initialized}");
    assert!(!initialized.contains("#|   - MASS"), "{initialized}");
    assert!(!initialized.contains("#|   - stats"), "{initialized}");
}

#[test]
fn init_script_includes_only_statically_referenced_packages() {
    let script = temp_path("ir-init-explicit-packages", "R");
    fs::write(&script, "library(dplyr)\nDBI::dbConnect\n").unwrap();

    let out = init(&script);

    assert_success(&out);
    let initialized = fs::read_to_string(&script).unwrap();
    assert!(initialized.contains("#|   - DBI\n"), "{initialized}");
    assert!(initialized.contains("#|   - dplyr\n"), "{initialized}");
    assert!(!initialized.contains("dbplyr"), "{initialized}");
}

#[test]
fn init_script_uses_locked_direct_dependencies_from_nearest_renv_project() {
    let project = temp_dir("ir-init-renv-project");
    let nested = project.join("reports");
    fs::create_dir_all(&nested).unwrap();
    let script = nested.join("analysis.R");
    fs::write(&script, "library(dplyr)\nglue::glue('ok')\n").unwrap();
    fs::write(
        project.join("renv.lock"),
        r#"{
  "R": {"Version": "4.3.2"},
  "Packages": {
    "dplyr": {
      "Package": "dplyr",
      "Version": "1.1.4",
      "Source": "Repository",
      "Repository": "CRAN"
    },
    "glue": {
      "Package": "glue",
      "Version": "1.8.0",
      "Source": "GitHub",
      "RemoteType": "github",
      "RemoteUsername": "tidyverse",
      "RemoteRepo": "glue",
      "RemoteSha": "0123456789abcdef"
    },
    "rlang": {
      "Package": "rlang",
      "Version": "1.1.6",
      "Source": "Repository",
      "Repository": "CRAN"
    }
  }
}
"#,
    )
    .unwrap();

    let out = init(&script);

    assert_success(&out);
    let initialized = fs::read_to_string(&script).unwrap();
    assert!(
        initialized.contains("#|   - dplyr==1.1.4\n"),
        "{initialized}"
    );
    assert!(
        initialized.contains("#|   - github::tidyverse/glue@0123456789abcdef\n"),
        "{initialized}"
    );
    assert!(!initialized.contains("rlang"), "{initialized}");
    assert!(
        initialized.contains("#| r-version: \">= 4.3\"\n"),
        "{initialized}"
    );
}

#[test]
fn init_script_preserves_bioconductor_lockfile_source() {
    let project = temp_dir("ir-init-bioc-project");
    let script = project.join("analysis.R");
    fs::write(&script, "BiocGenerics::combine(1, 2)\n").unwrap();
    fs::write(
        project.join("renv.lock"),
        r#"{
  "R": {"Version": "4.4.3"},
  "Packages": {
    "BiocGenerics": {
      "Package": "BiocGenerics",
      "Version": "0.52.0",
      "Source": "Bioconductor"
    }
  }
}"#,
    )
    .unwrap();

    let out = init(&script);

    assert_success(&out);
    let initialized = fs::read_to_string(&script).unwrap();
    assert!(
        initialized.contains("#|   - bioc::BiocGenerics@0.52.0\n"),
        "{initialized}"
    );
}

#[test]
fn init_script_uses_lock_repository_when_record_omits_repository() {
    let project = temp_dir("ir-init-lock-repository-project");
    let script = project.join("analysis.R");
    fs::write(&script, "jsonlite::toJSON(airquality)\n").unwrap();
    fs::write(
        project.join("renv.lock"),
        r#"{
  "R": {
    "Version": "4.4.3",
    "Repositories": [
      {
        "Name": "PPM",
        "URL": "https://packagemanager.posit.co/cran/2025-01-01"
      }
    ]
  },
  "Packages": {
    "jsonlite": {
      "Package": "jsonlite",
      "Version": "1.9.1",
      "Source": "Repository"
    }
  }
}"#,
    )
    .unwrap();

    let out = init(&script);

    assert_success(&out);
    let initialized = fs::read_to_string(&script).unwrap();
    assert!(
        initialized.contains("#|   - jsonlite==1.9.1\n"),
        "{initialized}"
    );
}

#[test]
fn init_script_preserves_supported_git_lockfile_sources() {
    let project = temp_dir("ir-init-git-project");
    let script = project.join("analysis.R");
    fs::write(
        &script,
        "gitlabpkg::run()\nbitbucketpkg::run()\ngitpkg::run()\n",
    )
    .unwrap();
    fs::write(
        project.join("renv.lock"),
        r#"{
  "R": {"Version": "4.4.3"},
  "Packages": {
    "gitlabpkg": {
      "Package": "gitlabpkg",
      "Version": "1.0.0",
      "Source": "GitLab",
      "RemoteType": "gitlab",
      "RemoteHost": "gitlab.com",
      "RemoteUsername": "group/subgroup",
      "RemoteRepo": "project",
      "RemoteSubdir": "r/pkg",
      "RemoteSha": "1111111111111111"
    },
    "bitbucketpkg": {
      "Package": "bitbucketpkg",
      "Version": "2.0.0",
      "Source": "Bitbucket",
      "RemoteType": "bitbucket",
      "RemoteHost": "api.bitbucket.org/2.0",
      "RemoteUsername": "owner",
      "RemoteRepo": "project",
      "RemoteSha": "2222222222222222"
    },
    "gitpkg": {
      "Package": "gitpkg",
      "Version": "3.0.0",
      "Source": "git",
      "RemoteType": "git",
      "RemoteUrl": "https://example.com/owner/project.git",
      "RemoteSha": "3333333333333333"
    }
  }
}"#,
    )
    .unwrap();

    let out = init(&script);

    assert_success(&out);
    let initialized = fs::read_to_string(&script).unwrap();
    assert!(
        initialized.contains(
            "#|   - bitbucketpkg=git::https://bitbucket.org/owner/project.git@2222222222222222\n"
        ),
        "{initialized}"
    );
    assert!(
        initialized.contains(
            "#|   - gitlabpkg=gitlab::https://gitlab.com/group/subgroup/project/-/r/pkg@1111111111111111\n"
        ),
        "{initialized}"
    );
    assert!(
        initialized.contains(
            "#|   - gitpkg=git::https://example.com/owner/project.git@3333333333333333\n"
        ),
        "{initialized}"
    );
}

#[test]
fn init_script_no_project_ignores_nearest_renv_lockfile() {
    let project = temp_dir("ir-init-no-project");
    let script = project.join("analysis.R");
    fs::write(&script, "library(dplyr)\n").unwrap();
    fs::write(
        project.join("renv.lock"),
        r#"{"R":{"Version":"4.2.3"},"Packages":{"dplyr":{"Package":"dplyr","Version":"1.0.10","Source":"Repository"}}}"#,
    )
    .unwrap();

    let out = ir()
        .args(["init", "--script"])
        .arg(&script)
        .arg("--no-project")
        .output()
        .unwrap();

    assert_success(&out);
    let initialized = fs::read_to_string(&script).unwrap();
    assert!(initialized.contains("#|   - dplyr\n"), "{initialized}");
    assert!(!initialized.contains("dplyr=="), "{initialized}");
}

#[test]
fn init_script_fails_when_locked_dependency_is_missing() {
    let project = temp_dir("ir-init-stale-lockfile");
    let script = project.join("analysis.R");
    let original = b"library(dplyr)\n";
    fs::write(&script, original).unwrap();
    fs::write(
        project.join("renv.lock"),
        r#"{"R":{"Version":"4.4.2"},"Packages":{}}"#,
    )
    .unwrap();

    let out = init(&script);

    assert!(!out.status.success(), "{}", output_text(&out));
    assert_stderr_contains(&out, "not recorded in");
    assert_eq!(fs::read(&script).unwrap(), original);
}

#[test]
fn init_script_fails_for_nonportable_locked_source() {
    let project = temp_dir("ir-init-local-lockfile");
    let script = project.join("analysis.R");
    let original = b"localpkg::run()\n";
    fs::write(&script, original).unwrap();
    fs::write(
        project.join("renv.lock"),
        r#"{
  "R": {"Version": "4.4.2"},
  "Packages": {
    "localpkg": {
      "Package": "localpkg",
      "Version": "1.0.0",
      "Source": "Local",
      "RemoteType": "local",
      "RemoteUrl": "../localpkg"
    }
  }
}"#,
    )
    .unwrap();

    let out = init(&script);

    assert!(!out.status.success(), "{}", output_text(&out));
    assert_stderr_contains(&out, "local source that is not portable");
    assert_eq!(fs::read(&script).unwrap(), original);
}

#[test]
fn init_script_fails_for_unrepresentable_git_url() {
    let project = temp_dir("ir-init-git-url-project");
    let script = project.join("analysis.R");
    let original = b"gitpkg::run()\n";
    fs::write(&script, original).unwrap();
    fs::write(
        project.join("renv.lock"),
        r#"{
  "R": {"Version": "4.4.2"},
  "Packages": {
    "gitpkg": {
      "Package": "gitpkg",
      "Version": "1.0.0",
      "Source": "git",
      "RemoteType": "git",
      "RemoteUrl": "git@example.com:owner/project.git",
      "RemoteSha": "3333333333333333"
    }
  }
}"#,
    )
    .unwrap();

    let out = init(&script);

    assert!(!out.status.success(), "{}", output_text(&out));
    assert_stderr_contains(&out, "cannot be represented as a portable ir package ref");
    assert_eq!(fs::read(&script).unwrap(), original);
}

#[test]
fn init_script_preserves_existing_shebang_and_body() {
    let script = temp_path("ir-init-shebang", "R");
    let original = b"#!/usr/bin/Rscript\n\ncat('ok')";
    fs::write(&script, original).unwrap();

    let out = init(&script);

    assert_success(&out);
    let initialized = fs::read(&script).unwrap();
    assert!(initialized.starts_with(b"#!/usr/bin/Rscript\n#| packages: []\n"));
    assert!(initialized.ends_with(b"\n\ncat('ok')"));
    assert_stderr_contains(&out, "existing shebang bypasses ir metadata");
}

#[test]
fn init_script_terminates_shebang_without_newline() {
    let script = temp_path("ir-init-shebang-without-newline", "R");
    fs::write(&script, "#!/usr/bin/Rscript").unwrap();

    let out = init(&script);

    assert_success(&out);
    let initialized = fs::read(&script).unwrap();
    assert!(initialized.starts_with(b"#!/usr/bin/Rscript\n#| packages: []\n"));
}

#[test]
fn init_script_preserves_crlf_body() {
    let script = temp_path("ir-init-crlf", "R");
    let original = b"library(glue)\r\ncat('ok')\r\n";
    fs::write(&script, original).unwrap();

    let out = init(&script);

    assert_success(&out);
    let initialized = fs::read(&script).unwrap();
    assert!(initialized.starts_with(b"#!/usr/bin/env -S ir run\r\n#| packages:\r\n"));
    assert!(initialized.ends_with(original));
}

#[test]
fn init_script_uses_first_line_ending_for_generated_metadata() {
    let script = temp_path("ir-init-mixed-line-endings", "R");
    let original = b"#!/usr/bin/env -S ir run\ncat('ok')\r\n";
    fs::write(&script, original).unwrap();

    let out = init(&script);

    assert_success(&out);
    let initialized = fs::read(&script).unwrap();
    assert!(initialized.starts_with(b"#!/usr/bin/env -S ir run\n#| packages: []\n#| r-version: \""));
    assert!(initialized.ends_with(b"\n\ncat('ok')\r\n"));
}

#[test]
fn init_script_refuses_existing_frontmatter_without_modifying_file() {
    let script = temp_path("ir-init-existing-frontmatter", "R");
    let original = b"#!/usr/bin/env -S ir run\n#| packages: []\n\ncat('ok')\n";
    fs::write(&script, original).unwrap();

    let out = init(&script);

    assert!(!out.status.success(), "{}", output_text(&out));
    assert_stderr_contains(&out, "already contains ir frontmatter");
    assert_eq!(fs::read(&script).unwrap(), original);
}

#[test]
fn init_script_refuses_malformed_frontmatter_marker_without_modifying_file() {
    let script = temp_path("ir-init-malformed-frontmatter", "R");
    let original = b"#|packages: []\ncat('ok')\n";
    fs::write(&script, original).unwrap();

    let out = init(&script);

    assert!(!out.status.success(), "{}", output_text(&out));
    assert_stderr_contains(&out, "already starts with a #| metadata marker");
    assert_eq!(fs::read(&script).unwrap(), original);
}

#[test]
fn init_script_parse_failure_does_not_modify_file() {
    let script = temp_path("ir-init-parse-failure", "R");
    let original = b"library(dplyr\n";
    fs::write(&script, original).unwrap();

    let out = init(&script);

    assert!(!out.status.success(), "{}", output_text(&out));
    assert_stderr_contains(&out, "unexpected end of input");
    assert_eq!(fs::read(&script).unwrap(), original);
}

#[test]
fn init_script_refuses_utf8_bom_without_modifying_file() {
    let script = temp_path("ir-init-bom", "R");
    let original = b"\xef\xbb\xbfcat('ok')\n";
    fs::write(&script, original).unwrap();

    let out = init(&script);

    assert!(!out.status.success(), "{}", output_text(&out));
    assert_stderr_contains(&out, "UTF-8 byte order mark");
    assert_eq!(fs::read(&script).unwrap(), original);
}

#[test]
fn initialized_script_runs_through_public_cli() {
    let script = temp_path("ir-init-runs", "R");
    let cache = temp_cache("ir-init-runs-cache");
    fs::write(&script, "cat('ir.fixture=initialized-script\\n')\n").unwrap();

    let initialized = init(&script);
    assert_success(&initialized);

    let out = ir()
        .env("IR_CACHE_DIR", &*cache)
        .env("IR_RSCRIPT", rscript())
        .args(["run", "--vanilla"])
        .arg(&*script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_stdout_contains(&out, "ir.fixture=initialized-script");
}

#[test]
fn init_script_missing_file_does_not_create_it() {
    let script = temp_path("ir-init-missing", "R");

    let out = init(&script);

    assert!(!out.status.success(), "{}", output_text(&out));
    assert_stderr_contains(&out, "cannot read script");
    assert!(!script.exists());
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
        "#!/bin/sh\nprintf 'invoked\\n' > \"$IR_TEST_MARKER\"\nexec \"$IR_TEST_REAL_RSCRIPT\" \"$@\"\n",
    );

    let out = ir()
        .env("IR_RSCRIPT", &*wrapper)
        .env("IR_TEST_MARKER", &*marker)
        .env("IR_TEST_REAL_RSCRIPT", rscript())
        .args(["init", "--script"])
        .arg(&*script)
        .output()
        .unwrap();

    assert_success(&out);
    assert_eq!(fs::read_to_string(&marker).unwrap(), "invoked\n");
}

#[test]
fn init_without_script_mode_is_reserved_for_projects() {
    let script = temp_path("ir-init-reserved", "R");
    fs::write(&script, "cat('ok')\n").unwrap();

    let out = ir().arg("init").arg(&*script).output().unwrap();

    assert!(!out.status.success(), "{}", output_text(&out));
    assert_stderr_contains(&out, "--script");
    assert_eq!(fs::read_to_string(&script).unwrap(), "cat('ok')\n");
}

#[cfg(unix)]
#[test]
fn init_script_preserves_unix_file_mode() {
    let script = temp_path("ir-init-mode", "R");
    fs::write(&script, "cat('ok')\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o751)).unwrap();

    let out = init(&script);

    assert_success(&out);
    assert_eq!(
        fs::metadata(&script).unwrap().permissions().mode() & 0o777,
        0o751
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

    let out = init(&link);

    assert!(!out.status.success(), "{}", output_text(&out));
    assert_stderr_contains(&out, "symbolic link");
    assert_eq!(fs::read_to_string(&target).unwrap(), "cat('ok')\n");
    assert!(fs::symlink_metadata(&*link)
        .unwrap()
        .file_type()
        .is_symlink());
}
