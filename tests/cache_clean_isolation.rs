//! Isolation checks for destructive public CLI fixtures.

mod support;

use support::*;

use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn cache_clean_tests_ignore_ambient_r_startup() {
    const CHILD_ROOT: &str = "IR_TEST_CACHE_CLEAN_STARTUP_CHILD";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = Path::new(&root);
        let out = ir_with_private_r_caches()
            .env("IR_CACHE_DIR", root.join("ir-cache"))
            .env("R_USER_CACHE_DIR", root.join("r-user"))
            .env("RENV_PATHS_CACHE", root.join("private-cache"))
            .env("R_PROFILE_USER", root.join("profile.R"))
            .env("RETICULATE_UV", "managed")
            .args(["cache", "clean", "--all"])
            .output()
            .unwrap();
        assert_success(&out);
        assert_stdout_contains(&out, "ir.fixture=cache-clean-startup-profile");
        return;
    }

    // A child test process supplies inherited settings without changing the
    // environment of other tests running in this process.
    for source in ["site-profile", "user", "site", "working-directory", "home"] {
        let root = temp_dir("ir-cache-clean-startup");
        let workdir = root.join("workdir");
        let home = root.join("home");
        let alternate = root.join("alternate-cache");
        let private = root.join("private-cache");
        for directory in [&workdir, &home, &alternate, &private] {
            fs::create_dir_all(directory).unwrap();
        }
        fs::write(alternate.join("sentinel"), "preserve").unwrap();
        fs::write(private.join("package"), "cached").unwrap();
        fs::write(
            root.join("profile.R"),
            r#"
requireNamespace <- function(package, ...) {
  if (identical(package, "rappdirs")) return(FALSE)
  base::requireNamespace(package, ...)
}
cat("ir.fixture=cache-clean-startup-profile\n")
"#,
        )
        .unwrap();
        let startup_file = match source {
            "working-directory" => workdir.join(".Renviron"),
            "home" => home.join(".Renviron"),
            "user" | "site" => root.join("inherited.Renviron"),
            "site-profile" => root.join("inherited.Rprofile"),
            _ => unreachable!(),
        };
        let alternate_value = serde_json::to_string(&renviron_path(&alternate)).unwrap();
        let startup = if source == "site-profile" {
            format!("Sys.setenv(RENV_PATHS_CACHE = {alternate_value})\n")
        } else {
            format!("RENV_PATHS_CACHE={alternate_value}\n")
        };
        fs::write(&startup_file, startup).unwrap();

        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "cache_clean_tests_ignore_ambient_r_startup",
                "--nocapture",
            ])
            .current_dir(&workdir)
            .env(CHILD_ROOT, &root)
            .env("HOME", &home)
            .env("R_USER", &home)
            .env("R_ENVIRON", "")
            .env_remove("R_ENVIRON_USER")
            .env("R_PROFILE", "");
        match source {
            "user" => {
                child.env("R_ENVIRON_USER", &startup_file);
            }
            "site" => {
                child.env("R_ENVIRON", &startup_file);
            }
            "site-profile" => {
                child.env("R_PROFILE", &startup_file);
            }
            _ => {}
        }
        let output = child.output().unwrap();
        assert_success(&output);
        assert!(
            alternate.join("sentinel").exists(),
            "{source} startup file redirected cache cleanup outside the fixture\n{}",
            output_text(&output)
        );
        assert!(
            !private.exists(),
            "private cache was not removed for {source}"
        );
    }
}
