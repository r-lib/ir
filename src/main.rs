//! `ir` — a uv-style front-end to R.
//!
//! Runs a self-contained R script whose dependencies are declared in a YAML
//! comment block at the top of the file:
//!
//! ```r
//! #!/usr/bin/env -S ir run
//! # dependencies:
//! #   - dplyr>=1.0
//! #   - tidyr
//! # R: ">= 4.0"
//! # exclude after: "2024-01-15"
//!
//! library(dplyr)
//! 1 + 1
//! ```
//!
//! The pipeline has two phases:
//!
//!   1. A private R session (`driver/resolve.R`) parses the frontmatter,
//!      resolves the dependencies with pak, hashes the resolved set into a
//!      content-addressed library path under the cache directory, and
//!      materialises that path as a light-weight library of symlinks into
//!      renv's package cache. The path is reported back to us.
//!
//!   2. We launch the user's script in a fresh, isolated R session whose
//!      library path is exactly that library plus base R.

use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// The R resolution driver, embedded at compile time so `ir` ships as one
/// self-contained binary while the source stays editable as real R.
const RESOLVE_DRIVER: &str = include_str!("../driver/resolve.R");

fn main() {
    if let Err(err) = try_main() {
        eprintln!("ir: {err}");
        std::process::exit(1);
    }
}

fn try_main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("run") => {
            let script = args
                .next()
                .ok_or("`ir run` requires a script path (try `ir run script.R`)")?;
            let script_args: Vec<String> = args.collect();
            cmd_run(&script, &script_args)
        }
        Some("--version" | "-V") => {
            println!("ir {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--help" | "-h") | None => {
            print_help();
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}` (try `ir run script.R`)").into()),
    }
}

fn print_help() {
    println!(
        "ir {} — a uv-style front-end to R\n\
         \n\
         USAGE:\n    \
             ir run <script.R> [args...]\n\
         \n\
         `ir run` reads the YAML frontmatter from <script.R>, resolves its\n\
         dependencies, builds a dedicated package library, and runs the script\n\
         against it. Any trailing args are passed through to the script.\n\
         \n\
         ENVIRONMENT:\n    \
             IR_CACHE_DIR   override the cache dir (default: tools::R_user_dir(\"ir\", \"cache\"))\n    \
             IR_RSCRIPT     path to the Rscript executable (default: Rscript on PATH)",
        env!("CARGO_PKG_VERSION")
    );
}

/// Resolve dependencies for `script`, then run it against the resulting library.
/// Exits the process with the script's own exit code.
fn cmd_run(script: &str, script_args: &[String]) -> Result<(), Box<dyn Error>> {
    let script_path =
        fs::canonicalize(script).map_err(|e| format!("cannot read script `{script}`: {e}"))?;

    let rscript = rscript_command();

    // Phase 1: private R session resolves deps and materialises the library.
    // It owns the cache location (tools::R_user_dir), so we pass only paths.
    let library = resolve_library(&rscript, &script_path)?;

    // Phase 2: run the user's script in an isolated R session.
    let code = run_script(&rscript, library.as_deref(), &script_path, script_args)?;
    std::process::exit(code);
}

/// Phase 1 — run the embedded driver in a private R session and return the
/// path to the materialised library.
fn resolve_library(rscript: &OsStr, script: &Path) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let tmp = env::temp_dir();
    let driver = unique_path(&tmp, "ir-resolve", "R");
    let out = unique_path(&tmp, "ir-libpath", "txt");
    fs::write(&driver, RESOLVE_DRIVER)?;

    let status = Command::new(rscript)
        .arg("--vanilla")
        .arg(&driver)
        .arg(script)
        .arg(&out)
        .stdin(Stdio::null()) // resolution never reads stdin
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        // pak suppresses progress in noninteractive Rscript unless this is set.
        // Resolution cache hits return before pak, so this adds no cache-hit pak output.
        .env("R_PKG_SHOW_PROGRESS", "true")
        .status()
        .map_err(|e| spawn_error(rscript, e))?;

    let _ = fs::remove_file(&driver);
    let result = fs::read_to_string(&out).unwrap_or_default();
    let _ = fs::remove_file(&out);

    if !status.success() {
        return Err("dependency resolution failed".into());
    }

    let path = result.trim();
    Ok(if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    })
}

/// Phase 2 — run `script` in a vanilla R session pointed at `library`.
///
/// The script runs as an ordinary `Rscript script.R` — its `.Renviron`,
/// `.Rprofile` and site files are read, so it sees the user's normal R
/// environment. The resolved library is injected via `R_LIBS`, which is
/// *prepended* to `.libPaths()`: resolved dependencies take precedence, while
/// the user's other libraries remain available. (`R_LIBS` is used rather than
/// `R_LIBS_USER`, since a user `.Renviron` setting `R_LIBS_USER` would override
/// the latter.) Returns the script's exit code.
fn run_script(
    rscript: &OsStr,
    library: Option<&Path>,
    script: &Path,
    script_args: &[String],
) -> Result<i32, Box<dyn Error>> {
    let mut cmd = Command::new(rscript);
    cmd.arg(script).args(script_args);

    if let Some(lib) = library {
        cmd.env("R_LIBS", lib);
    }

    let status = cmd.status().map_err(|e| spawn_error(rscript, e))?;
    Ok(status.code().unwrap_or(1))
}

/// The Rscript executable to use: `$IR_RSCRIPT` if set, otherwise `Rscript`
/// resolved via `PATH`.
fn rscript_command() -> std::ffi::OsString {
    env::var_os("IR_RSCRIPT").unwrap_or_else(|| "Rscript".into())
}

/// A unique path in `dir` for this process, e.g. `ir-resolve-1234-987.R`.
fn unique_path(dir: &Path, prefix: &str, ext: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dir.join(format!("{prefix}-{}-{nanos}.{ext}", std::process::id()))
}

/// Turn a failure to launch Rscript into an actionable message.
fn spawn_error(rscript: &OsStr, err: io::Error) -> String {
    if err.kind() == io::ErrorKind::NotFound {
        format!(
            "could not find `{}` on PATH. Install R, or set IR_RSCRIPT to its path.",
            rscript.to_string_lossy()
        )
    } else {
        format!("failed to launch `{}`: {err}", rscript.to_string_lossy())
    }
}
