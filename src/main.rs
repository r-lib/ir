//! `ir` — self-describing R scripts.
//!
//! Runs a standalone R script whose dependencies are declared in YAML
//! frontmatter at the top of the file:
//!
//! ```r
//! #!/usr/bin/env -S ir run
//! #| dependencies:
//! #|   - dplyr>=1.0
//! #|   - tidyr
//! #| R: ">= 4.0"
//! #| exclude after: "2024-01-15"
//!
//! library(dplyr)
//! 1 + 1
//! ```
//!
//! The pipeline has two phases:
//!
//!   1. Rust extracts and parses the leading `#| ` YAML frontmatter block. A
//!      private R session (`driver/resolve.R`) receives the dependency specs on
//!      stdin, resolves them with pak, hashes the resolved set into a
//!      content-addressed library path under the cache directory, and
//!      materialises that path as a light-weight library of symlinks into renv's
//!      package cache. The path is reported back to us.
//!
//!   2. We launch the user's script in a fresh, isolated R session whose
//!      library path is exactly that library plus base R.

use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use saphyr::{LoadableYamlNode, Yaml};

/// The R resolution driver, embedded at compile time so `ir` ships as one
/// self-contained binary while the source stays editable as real R.
const RESOLVE_DRIVER: &str = include_str!("../driver/resolve.R");

#[derive(Debug, Default)]
struct ScriptSpec {
    dependencies: Vec<String>,
    exclude_after: Option<String>,
    r_requirement: Option<String>,
}

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
            if script == "--help" || script == "-h" {
                print_run_help();
                return Ok(());
            }
            let script_args: Vec<String> = args.collect();
            cmd_run(&script, &script_args)
        }
        Some("cache") => cmd_cache(args.collect()),
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

fn cmd_cache(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    match args.first().map(String::as_str) {
        Some("clean") => cmd_cache_clean(&args[1..]),
        Some("dir") => cmd_cache_dir(&args[1..]),
        Some("--help" | "-h") => {
            print_cache_help();
            Ok(())
        }
        None => {
            print_cache_help();
            Err("`ir cache` requires a subcommand".into())
        }
        Some(other) => Err(format!("unrecognized subcommand `{other}`").into()),
    }
}

fn cmd_cache_clean(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_cache_clean_help();
        return Ok(());
    }

    for arg in args {
        match arg.as_str() {
            "--force" => {}
            other => return Err(format!("unexpected argument `{other}`").into()),
        }
    }

    let cache_dir = ir_cache_dir()?;
    if !cache_dir.exists() {
        println!("No cache found at: {}", cache_dir.display());
        return Ok(());
    }

    let files = count_files(&cache_dir)?;
    println!("Clearing cache at: {}", cache_dir.display());
    fs::remove_dir_all(&cache_dir)
        .map_err(|e| format!("failed to remove cache `{}`: {e}", cache_dir.display()))?;
    println!(
        "Removed {files} {}",
        if files == 1 { "file" } else { "files" }
    );
    Ok(())
}

fn cmd_cache_dir(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_cache_dir_help();
        return Ok(());
    }
    if let Some(arg) = args.first() {
        return Err(format!("unexpected argument `{arg}`").into());
    }

    println!("{}", ir_cache_dir()?.display());
    Ok(())
}

fn print_help() {
    println!(
        concat!(
            "ir {} — self-describing R scripts\n",
            "\n",
            "USAGE:\n",
            "    ir run <script.R> [args...]\n",
            "    ir cache <command>\n",
            "\n",
            "`ir run` reads the YAML frontmatter from <script.R>, resolves its\n",
            "dependencies, builds a dedicated package library, and runs the script\n",
            "against it. Any trailing args are passed through to the script.\n",
            "`ir cache` manages the dependency resolution and materialised library\n",
            "cache.\n",
            "\n",
            "ENVIRONMENT:\n",
            "    IR_CACHE_DIR   override the cache dir (default: tools::R_user_dir(\"ir\", \"cache\"))\n",
            "    IR_RSCRIPT     path to the Rscript executable (default: Rscript on PATH)"
        ),
        env!("CARGO_PKG_VERSION")
    );
}

fn print_run_help() {
    println!(concat!(
        "Run an R script\n",
        "\n",
        "USAGE:\n",
        "    ir run <script.R> [args...]\n",
        "\n",
        "`ir run` reads the YAML frontmatter from <script.R>, resolves its\n",
        "dependencies, builds a dedicated package library, and runs the script\n",
        "against it. Any trailing args are passed through to the script.\n",
        "\n",
        "ENVIRONMENT:\n",
        "    IR_CACHE_DIR   override the cache dir (default: tools::R_user_dir(\"ir\", \"cache\"))\n",
        "    IR_RSCRIPT     path to the Rscript executable (default: Rscript on PATH)"
    ));
}

fn print_cache_help() {
    println!(concat!(
        "Manage ir's cache\n",
        "\n",
        "USAGE:\n",
        "    ir cache <COMMAND>\n",
        "\n",
        "COMMANDS:\n",
        "    clean  Clear the cache, removing all entries\n",
        "    dir    Show the cache directory\n",
        "\n",
        "ENVIRONMENT:\n",
        "    IR_CACHE_DIR   override the cache dir (default: tools::R_user_dir(\"ir\", \"cache\"))"
    ));
}

fn print_cache_clean_help() {
    println!(concat!(
        "Clear the cache, removing all entries\n",
        "\n",
        "USAGE:\n",
        "    ir cache clean [OPTIONS]\n",
        "\n",
        "OPTIONS:\n",
        "    --force  Force removal of the cache"
    ));
}

fn print_cache_dir_help() {
    println!(concat!(
        "Show the cache directory\n",
        "\n",
        "USAGE:\n",
        "    ir cache dir"
    ));
}

/// Resolve dependencies for `script`, then run it against the resulting library.
/// Exits the process with the script's own exit code.
fn cmd_run(script: &str, script_args: &[String]) -> Result<(), Box<dyn Error>> {
    let script_path =
        fs::canonicalize(script).map_err(|e| format!("cannot read script `{script}`: {e}"))?;

    let rscript = rscript_command();

    // Phase 1: private R session resolves deps and materialises the library.
    // Rust parses the YAML frontmatter and sends dependency specs on stdin.
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
    let spec = read_script_spec(script)?;
    fs::write(&driver, RESOLVE_DRIVER)?;

    let mut cmd = Command::new(rscript);
    cmd.arg(&driver)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("IR_RESOLVE_OUT_FILE", &out)
        // pak suppresses progress in noninteractive Rscript unless this is set.
        // Resolution cache hits return before pak, so this adds no cache-hit pak output.
        .env("R_PKG_SHOW_PROGRESS", "true");
    if let Some(exclude_after) = &spec.exclude_after {
        cmd.env("IR_EXCLUDE_AFTER", exclude_after);
    }
    if let Some(r_requirement) = &spec.r_requirement {
        cmd.env("IR_R_REQUIREMENT", r_requirement);
    }

    let mut child = cmd.spawn().map_err(|e| spawn_error(rscript, e))?;
    {
        let mut stdin = child.stdin.take().ok_or("failed to open resolver stdin")?;
        for dependency in &spec.dependencies {
            writeln!(stdin, "{dependency}")?;
        }
    }
    let status = child
        .wait()
        .map_err(|e| format!("failed to wait for dependency resolver: {e}"))?;

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

fn read_script_spec(script: &Path) -> Result<ScriptSpec, Box<dyn Error>> {
    parse_frontmatter(&read_op_frontmatter_to_string(script)?)
}

fn parse_frontmatter(frontmatter: &str) -> Result<ScriptSpec, Box<dyn Error>> {
    if frontmatter.trim().is_empty() {
        return Ok(ScriptSpec::default());
    }

    let docs = Yaml::load_from_str(frontmatter)
        .map_err(|e| format!("could not parse script frontmatter as YAML: {e}"))?;
    if docs.len() != 1 {
        return Err("script frontmatter must contain exactly one YAML document".into());
    }
    if docs[0].is_null() {
        return Ok(ScriptSpec::default());
    }

    let doc = &docs[0];
    if !doc.is_mapping() {
        return Err("script frontmatter must be a YAML mapping".into());
    }

    Ok(ScriptSpec {
        dependencies: frontmatter_dependencies(doc)?,
        exclude_after: frontmatter_optional_string(doc, "exclude after")?,
        r_requirement: frontmatter_optional_string(doc, "R")?,
    })
}

fn frontmatter_dependencies(doc: &Yaml<'_>) -> Result<Vec<String>, Box<dyn Error>> {
    let Some(value) = doc.as_mapping_get("dependencies") else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }

    let mut dependencies = Vec::new();
    if let Some(seq) = value.as_vec() {
        for item in seq {
            push_dependency_words(&mut dependencies, item)?;
        }
    } else {
        push_dependency_words(&mut dependencies, value)?;
    }
    Ok(dependencies)
}

fn push_dependency_words(
    dependencies: &mut Vec<String>,
    value: &Yaml<'_>,
) -> Result<(), Box<dyn Error>> {
    let Some(value) = value.as_str() else {
        return Err("frontmatter `dependencies` entries must be strings".into());
    };
    dependencies.extend(value.split_whitespace().map(str::to_owned));
    Ok(())
}

fn frontmatter_optional_string(
    doc: &Yaml<'_>,
    key: &str,
) -> Result<Option<String>, Box<dyn Error>> {
    let Some(value) = doc.as_mapping_get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    let Some(value) = value.as_str() else {
        return Err(format!("frontmatter `{key}` must be a string").into());
    };
    let value = value.trim();
    Ok(if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    })
}

/// Phase 2 — run `script` in an ordinary R session pointed at `library`.
///
/// The script runs as an ordinary `Rscript script.R` — its `.Renviron`,
/// `.Rprofile` and site files are read, so it sees the user's normal R
/// environment. The resolved library is injected via `R_LIBS`, which is
/// *prepended* to `.libPaths()`: resolved dependencies take precedence, while
/// the user's other libraries remain available. (`R_LIBS` is used rather than
/// `R_LIBS_USER`, since a user `.Renviron` setting `R_LIBS_USER` would override
/// the latter.)
///
/// As `ir`'s final step, on Unix we `exec` into Rscript so R takes over this
/// process — inheriting our PID, stdio and signals, and propagating its exit
/// status (signal deaths included) verbatim. `exec` returns only on launch
/// failure. Without `exec` (Windows), R runs as a child and we return its code.
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

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Replace ir with R; returns only if the exec fails.
        Err(spawn_error(rscript, cmd.exec()).into())
    }

    #[cfg(not(unix))]
    {
        let status = cmd.status().map_err(|e| spawn_error(rscript, e))?;
        Ok(status.code().unwrap_or(1))
    }
}

fn read_op_frontmatter_to_string(script: &Path) -> Result<String, Box<dyn Error>> {
    let file = File::open(script)?;
    let mut reader = BufReader::new(file);
    let mut frontmatter = String::new();
    let mut line = String::new();

    let mut read_next_line = |line: &mut String| {
        line.clear();
        reader.read_line(line)
    };

    read_next_line(&mut line)?;

    if line.starts_with("#!") {
        read_next_line(&mut line)?;
    }

    while let Some(rest) = line.strip_prefix("#| ") {
        frontmatter.push_str(rest);

        if read_next_line(&mut line)? == 0 {
            break;
        }
    }

    Ok(frontmatter)
}

/// The Rscript executable to use: `$IR_RSCRIPT` if set, otherwise `Rscript`
/// resolved via `PATH`.
fn rscript_command() -> std::ffi::OsString {
    env::var_os("IR_RSCRIPT").unwrap_or_else(|| "Rscript".into())
}

/// The `ir` cache root, matching `tools::R_user_dir("ir", "cache")` unless
/// `IR_CACHE_DIR` overrides it.
fn ir_cache_dir() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = nonempty_env("IR_CACHE_DIR") {
        return Ok(PathBuf::from(path));
    }

    let rscript = rscript_command();
    let output = Command::new(&rscript)
        .arg("-e")
        .arg("writeLines(tools::R_user_dir(\"ir\", \"cache\"))")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| spawn_error(&rscript, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("failed to resolve cache dir with tools::R_user_dir: {stderr}").into());
    }

    let stdout = String::from_utf8(output.stdout)?;
    let path = stdout.trim();
    if path.is_empty() {
        return Err("tools::R_user_dir returned an empty cache dir".into());
    }

    Ok(PathBuf::from(path))
}

fn nonempty_env(name: &str) -> Option<std::ffi::OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

fn count_files(path: &Path) -> io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return Ok(1);
    }

    let mut files = 0;
    for entry in fs::read_dir(path)? {
        files += count_files(&entry?.path())?;
    }
    Ok(files)
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
