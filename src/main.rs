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
//! #| r-version: ">= 4.0"
//! #| exclude-newer: "2024-01-15"
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

mod rig;

/// The R resolution driver, embedded at compile time so `ir` ships as one
/// self-contained binary while the source stays editable as real R.
const RESOLVE_DRIVER: &str = include_str!("../driver/resolve.R");

#[derive(Debug, Default)]
struct ScriptSpec {
    dependencies: Vec<String>,
    exclude_newer: Option<String>,
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
            let args: Vec<String> = args.collect();
            if matches!(args.as_slice(), [arg] if arg == "--help" || arg == "-h") {
                print_run_help();
                return Ok(());
            }

            let run = parse_run_args(args)?;
            cmd_run(
                &run.source,
                &run.rscript_args,
                &run.with_deps,
                run.r_requirement.as_deref(),
                &run.script_args,
            )
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

/// Where the user's program comes from: a script file, or one or more inline
/// `-e` expressions evaluated in its place (mirroring `Rscript -e`).
enum RunSource {
    Script(String),
    Expressions(Vec<String>),
}

struct RunArgs {
    rscript_args: Vec<String>,
    with_deps: Vec<String>,
    r_requirement: Option<String>,
    source: RunSource,
    script_args: Vec<String>,
}

/// Split the leading region of `ir run`'s arguments into Rscript options,
/// `--with` dependency specs, an optional `--r-version` spec, and the program
/// source (a script path or `-e` expressions), with everything after the source
/// treated as program args.
///
/// `-e <expr>`, `--with <spec>`, and `--r-version <spec>` are `ir`-level flags
/// handled here: `-e` supplies inline R to run instead of a file, `--with`
/// declares extra dependencies, and `--r-version` chooses the R version via
/// rig. Any other `-…` argument is an Rscript option, forwarded verbatim to the
/// user-code phase. Scanning stops at the first non-option, which is the script
/// path unless `-e` was given (in which case it, and everything after, are
/// program args — as with Rscript).
fn parse_run_args(args: Vec<String>) -> Result<RunArgs, Box<dyn Error>> {
    let mut rscript_args = Vec::new();
    let mut with_deps = Vec::new();
    let mut r_requirement = None;
    let mut expressions = Vec::new();
    let mut iter = args.into_iter();
    let mut positional = None;

    while let Some(arg) = iter.next() {
        if arg == "-e" {
            let expr = iter
                .next()
                .ok_or("`-e` requires an expression (try `ir run -e '1 + 1'`)")?;
            expressions.push(expr);
        } else if arg == "--with" {
            let value = iter
                .next()
                .ok_or("`--with` requires a package (try `ir run --with dplyr script.R`)")?;
            push_with_deps(&mut with_deps, &value);
        } else if let Some(value) = arg.strip_prefix("--with=") {
            push_with_deps(&mut with_deps, value);
        } else if arg == "--r-version" {
            let value = iter.next().ok_or(
                "`--r-version` requires a version spec (try `ir run --r-version 4.5 script.R`)",
            )?;
            r_requirement = Some(value);
        } else if let Some(value) = arg.strip_prefix("--r-version=") {
            r_requirement = Some(value.to_string());
        } else if arg.starts_with('-') {
            rscript_args.push(arg);
        } else {
            positional = Some(arg);
            break;
        }
    }

    let script_args: Vec<String> = iter.collect();

    let (source, script_args) = if expressions.is_empty() {
        let script = positional
            .ok_or("`ir run` requires a script path or -e expression (try `ir run script.R`)")?;
        (RunSource::Script(script), script_args)
    } else {
        // With `-e`, there is no script file; the first non-option and anything
        // after it are program args (commandArgs), matching Rscript.
        let mut program_args = Vec::new();
        program_args.extend(positional);
        program_args.extend(script_args);
        (RunSource::Expressions(expressions), program_args)
    };

    Ok(RunArgs {
        rscript_args,
        with_deps,
        r_requirement,
        source,
        script_args,
    })
}

/// Append the dependency specs in a `--with` value, which may be a single spec
/// or a comma-separated list, to `with_deps`. Blank entries are ignored.
fn push_with_deps(with_deps: &mut Vec<String>, value: &str) {
    for dep in value.split(',') {
        let dep = dep.trim();
        if !dep.is_empty() {
            with_deps.push(dep.to_string());
        }
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
            "    ir run [Rscript-options...] [--with <pkg>]... [--r-version <spec>] <script.R> [args...]\n",
            "    ir run [Rscript-options...] [--with <pkg>]... [--r-version <spec>] -e <expr> [args...]\n",
            "    ir cache <command>\n",
            "\n",
            "`ir run` reads the YAML frontmatter from <script.R>, resolves its\n",
            "dependencies, builds a dedicated package library, and runs the script\n",
            "against it. With -e it evaluates inline R expressions instead of a file,\n",
            "--with adds dependencies on the command line, and --r-version selects\n",
            "the R version with rig. Leading Rscript options are passed to Rscript for\n",
            "the user-code phase; trailing args are passed through to the program.\n",
            "`ir cache` manages the dependency resolution and materialised library cache.\n",
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
        "    ir run [Rscript-options...] [--with <pkg>]... [--r-version <spec>] <script.R> [args...]\n",
        "    ir run [Rscript-options...] [--with <pkg>]... [--r-version <spec>] -e <expr> [-e <expr>]... [args...]\n",
        "\n",
        "`ir run` reads the YAML frontmatter from <script.R>, resolves its\n",
        "dependencies, builds a dedicated package library, and runs the script\n",
        "against it. With -e it instead evaluates inline R expressions (mirroring\n",
        "Rscript) against the same isolated library. --r-version selects the R\n",
        "version with rig and overrides script frontmatter. Leading Rscript options\n",
        "are passed to Rscript for the user-code phase; trailing args are passed\n",
        "through to the program.\n",
        "\n",
        "OPTIONS:\n",
        "    -e <expr>     Evaluate an inline R expression instead of a script file.\n",
        "                  May be repeated; runs in place of <script.R>.\n",
        "    --with <pkg>  Add a dependency for this run, merged with any declared\n",
        "                  in the script frontmatter. May be repeated and accepts a\n",
        "                  comma-separated list (e.g. --with dplyr,tidyr). Uses the\n",
        "                  same spec format as `dependencies:` (e.g. cli==3.6.6).\n",
        "    --r-version <spec>\n",
        "                  Select the R version for this run with rig. Overrides\n",
        "                  `r-version:` in script frontmatter.\n",
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

/// Resolve dependencies for `source`, then run it against the resulting
/// library. Exits the process with the program's own exit code.
fn cmd_run(
    source: &RunSource,
    rscript_args: &[String],
    with_deps: &[String],
    r_requirement: Option<&str>,
    script_args: &[String],
) -> Result<(), Box<dyn Error>> {
    // A script file declares its dependencies, `exclude-newer`, and `r-version` in
    // YAML frontmatter and is canonicalised so the run is independent of the
    // working directory. An inline `-e` expression has no frontmatter; its deps
    // come solely from `--with`.
    let (script_path, mut spec) = match source {
        RunSource::Script(script) => {
            let path = fs::canonicalize(script)
                .map_err(|e| format!("cannot read script `{script}`: {e}"))?;
            let spec = read_script_spec(&path)?;
            (Some(path), spec)
        }
        RunSource::Expressions(_) => (None, ScriptSpec::default()),
    };
    spec.dependencies.extend(with_deps.iter().cloned());
    if let Some(req) = r_requirement {
        spec.r_requirement = Some(req.to_string());
    }
    let rscript = rscript_for_spec(&spec)?;

    // Phase 1: private R session resolves deps and materialises the library.
    // Rust parses the frontmatter and sends the dependency specs on stdin.
    let library = resolve_library(&rscript, &spec)?;

    // Phase 2: run the user's program in an isolated R session.
    let expressions: &[String] = match source {
        RunSource::Expressions(exprs) => exprs,
        RunSource::Script(_) => &[],
    };
    let code = run_script(
        &rscript,
        library.as_deref(),
        script_path.as_deref(),
        expressions,
        rscript_args,
        script_args,
    )?;
    std::process::exit(code);
}

/// Phase 1 — run the embedded driver in a private R session and return the
/// path to the materialised library. The dependency specs in `spec` (the
/// script's frontmatter plus any `--with` packages) are streamed on stdin.
fn resolve_library(rscript: &OsStr, spec: &ScriptSpec) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let tmp = env::temp_dir();
    let driver = unique_path(&tmp, "ir-resolve", "R");
    let result_file = unique_path(&tmp, "ir-libpath", "txt");
    fs::write(&driver, RESOLVE_DRIVER)?;

    let mut cmd = Command::new(rscript);
    cmd.arg(&driver)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("IR_RESOLVE_RESULT_FILE", &result_file)
        // pak suppresses progress in noninteractive Rscript unless this is set.
        // Resolution cache hits return before pak, so this adds no cache-hit pak output.
        .env("R_PKG_SHOW_PROGRESS", "true");
    if let Some(exclude_newer) = &spec.exclude_newer {
        cmd.env("IR_EXCLUDE_NEWER", exclude_newer);
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
    let result = fs::read_to_string(&result_file).unwrap_or_default();
    let _ = fs::remove_file(&result_file);

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
    parse_frontmatter(&read_op_frontmatter_to_string(script)?, false)
}

fn parse_frontmatter(frontmatter: &str, nested: bool) -> Result<ScriptSpec, Box<dyn Error>> {
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

    // For Quarto documents the dependency spec lives under the `ir:` key,
    // alongside ordinary quarto metadata; for scripts it is the document itself.
    let spec_node = if nested {
        match doc.as_mapping_get("ir") {
            None => return Ok(ScriptSpec::default()),
            Some(node) if node.is_null() => return Ok(ScriptSpec::default()),
            Some(node) => node,
        }
    } else {
        doc
    };

    if nested && !spec_node.is_mapping() {
        return Err("frontmatter `ir` must be a YAML mapping".into());
    }

    Ok(ScriptSpec {
        dependencies: frontmatter_dependencies(spec_node)?,
        exclude_newer: frontmatter_optional_string(spec_node, "exclude-newer")?,
        r_requirement: frontmatter_optional_string(spec_node, "r-version")?,
    })
}

fn rscript_for_spec(spec: &ScriptSpec) -> Result<std::ffi::OsString, Box<dyn Error>> {
    let Some(req) = &spec.r_requirement else {
        return Ok(rscript_command());
    };

    rig::resolve_rscript(req, spec.exclude_newer.as_deref())
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

/// Phase 2 — run the user's program in an ordinary R session pointed at
/// `library`. The program is either a script file (`script`) or, when that is
/// `None`, the inline `expressions` evaluated via `Rscript -e` in its place.
///
/// It runs as an ordinary `Rscript [Rscript-options...] (script.R | -e expr…)` —
/// its `.Renviron`, `.Rprofile` and site files are read unless the forwarded
/// Rscript options disable them. The resolved library is injected via `R_LIBS`,
/// which is *prepended* to `.libPaths()`: resolved dependencies take precedence,
/// while the user's other libraries remain available. (`R_LIBS` is used rather
/// than `R_LIBS_USER`, since a user `.Renviron` setting `R_LIBS_USER` would
/// override the latter.)
///
/// As `ir`'s final step, on Unix we `exec` into Rscript so R takes over this
/// process — inheriting our PID, stdio and signals, and propagating its exit
/// status (signal deaths included) verbatim. `exec` returns only on launch
/// failure. Without `exec` (Windows), R runs as a child and we return its code.
fn run_script(
    rscript: &OsStr,
    library: Option<&Path>,
    script: Option<&Path>,
    expressions: &[String],
    rscript_args: &[String],
    script_args: &[String],
) -> Result<i32, Box<dyn Error>> {
    let mut cmd = Command::new(rscript);
    cmd.args(rscript_args);
    match script {
        Some(script) => {
            cmd.arg(script);
        }
        None => {
            for expr in expressions {
                cmd.arg("-e").arg(expr);
            }
        }
    }
    cmd.args(script_args);

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

/// Read the leading YAML metadata block from a Quarto document.
// Wired into read_script_spec in the quarto dispatch task (Task 4).
#[allow(dead_code)]
fn read_yaml_block_to_string(script: &Path) -> Result<String, Box<dyn Error>> {
    let content = fs::read_to_string(script)?;
    Ok(extract_yaml_block(&content))
}

/// Extract the leading YAML metadata block delimited by `---` fences, returning
/// the inner text. `str::lines` strips a trailing `\r`, so CRLF input is handled.
/// Returns an empty string when there is no opening fence on the first line or
/// no closing `---`/`...` line — both mean "no frontmatter", never an error.
fn extract_yaml_block(content: &str) -> String {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = content.lines();

    match lines.next() {
        Some(first) if first.trim_end() == "---" => {}
        _ => return String::new(),
    }

    let mut block = String::new();
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            return block;
        }
        block.push_str(line);
        block.push('\n');
    }

    // No closing fence: treat as no frontmatter.
    String::new()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter_reads_top_level_for_scripts() {
        let spec = parse_frontmatter(
            "dependencies:\n  - dplyr>=1.0\n  - tidyr\nr-version: \">= 4.0\"\n",
            false,
        )
        .unwrap();
        assert_eq!(spec.dependencies, vec!["dplyr>=1.0", "tidyr"]);
        assert_eq!(spec.r_requirement.as_deref(), Some(">= 4.0"));
    }

    #[test]
    fn parse_frontmatter_descends_into_ir_for_quarto() {
        let yaml = "title: Demo\nir:\n  dependencies:\n    - gt@1.0\n  exclude-newer: \"2024-01-15\"\n";
        let spec = parse_frontmatter(yaml, true).unwrap();
        assert_eq!(spec.dependencies, vec!["gt@1.0"]);
        assert_eq!(spec.exclude_newer.as_deref(), Some("2024-01-15"));
    }

    #[test]
    fn parse_frontmatter_quarto_without_ir_key_is_empty() {
        let spec = parse_frontmatter("title: Demo\nformat: html\n", true).unwrap();
        assert!(spec.dependencies.is_empty());
        assert!(spec.exclude_newer.is_none());
        assert!(spec.r_requirement.is_none());
    }

    #[test]
    fn parse_frontmatter_quarto_null_ir_key_is_empty() {
        let spec = parse_frontmatter("ir:\n", true).unwrap();
        assert!(spec.dependencies.is_empty());
    }

    #[test]
    fn parse_frontmatter_quarto_non_mapping_ir_errors() {
        let err = parse_frontmatter("ir: nope\n", true).unwrap_err().to_string();
        assert!(err.contains("`ir`"), "{err}");
    }

    #[test]
    fn extract_yaml_block_reads_fenced_block() {
        let doc = "---\ntitle: Demo\nir:\n  dependencies:\n    - gt\n---\n\nbody\n";
        assert_eq!(
            extract_yaml_block(doc),
            "title: Demo\nir:\n  dependencies:\n    - gt\n"
        );
    }

    #[test]
    fn extract_yaml_block_handles_crlf_and_dot_terminator() {
        let doc = "---\r\ntitle: Demo\r\n...\r\nbody\r\n";
        assert_eq!(extract_yaml_block(doc), "title: Demo\n");
    }

    #[test]
    fn extract_yaml_block_strips_optional_bom() {
        let doc = "\u{feff}---\ntitle: Demo\n---\n";
        assert_eq!(extract_yaml_block(doc), "title: Demo\n");
    }

    #[test]
    fn extract_yaml_block_without_opening_fence_is_empty() {
        assert_eq!(extract_yaml_block("title: Demo\nbody\n"), "");
        assert_eq!(extract_yaml_block(""), "");
    }

    #[test]
    fn extract_yaml_block_without_closing_fence_is_empty() {
        assert_eq!(extract_yaml_block("---\ntitle: Demo\nbody\n"), "");
    }
}
