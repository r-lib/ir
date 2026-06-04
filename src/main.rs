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
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use saphyr::{LoadableYamlNode, Yaml};

mod quarto;
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
                run.isolated,
            )
        }
        Some("tool") => cmd_tool(args.collect()),
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

struct PackageExecTarget {
    package_ref: String,
    package_name: Option<String>,
    executable: String,
}

struct RunArgs {
    rscript_args: Vec<String>,
    with_deps: Vec<String>,
    r_requirement: Option<String>,
    source: RunSource,
    script_args: Vec<String>,
    isolated: bool,
}

struct ToolRunArgs {
    rscript_args: Vec<String>,
    with_deps: Vec<String>,
    r_requirement: Option<String>,
    target: PackageExecTarget,
    tool_args: Vec<String>,
}

/// Split the leading region of `ir run`'s arguments into Rscript options,
/// `--with` dependency specs, an optional `--r-version` spec, and the program
/// source, with everything after the source treated as program args.
///
/// `-e <expr>`, `--with <spec>`, `--r-version <spec>` and `--isolated` are
/// `ir`-level flags handled here. Any other `-...` argument is an Rscript
/// option, forwarded verbatim to the user-code phase. Scanning stops at the
/// first non-option, which is the script path unless `-e` was given (in which
/// case it, and everything after, are program args, as with Rscript).
fn parse_run_args(args: Vec<String>) -> Result<RunArgs, Box<dyn Error>> {
    let mut rscript_args = Vec::new();
    let mut with_deps = Vec::new();
    let mut r_requirement = None;
    let mut expressions = Vec::new();
    let mut isolated = false;
    let mut iter = args.into_iter();
    let mut positional = None;

    while let Some(arg) = iter.next() {
        if arg == "-e" {
            let expr = iter
                .next()
                .ok_or("`-e` requires an expression (try `ir run -e '1 + 1'`)")?;
            expressions.push(expr);
        } else if arg == "--from" || arg.starts_with("--from=") {
            return Err("`--from` is only supported by `ir tool run`".into());
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
        } else if arg == "--isolated" {
            isolated = true;
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
        isolated,
    })
}

fn infer_self_named_executable(package_ref: &str) -> Option<String> {
    let end = package_ref
        .find(['@', '<', '>', '=', '!', ' '])
        .unwrap_or(package_ref.len());
    let name = &package_ref[..end];
    if is_r_package_name(name) {
        Some(name.to_string())
    } else {
        None
    }
}

fn is_r_package_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && !name.ends_with('.')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '.')
}

fn is_package_executable_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains(':')
        && !name.chars().any(char::is_whitespace)
}

/// Parse `ir tool run`, which resolves a provider package and runs a command
/// from that package's `exec/` directory. This is intentionally separate from
/// `ir run`: script and expression runs are source-oriented, tool runs are
/// package-oriented and isolated by default.
fn parse_tool_run_args(args: Vec<String>) -> Result<ToolRunArgs, Box<dyn Error>> {
    let mut rscript_args = Vec::new();
    let mut with_deps = Vec::new();
    let mut r_requirement = None;
    let mut from = None;
    let mut iter = args.into_iter();
    let mut positional = None;

    while let Some(arg) = iter.next() {
        if arg == "--from" {
            let value = iter
                .next()
                .ok_or("`--from` requires a package ref (try `ir tool run --from cli cli`)")?;
            from = Some(value);
        } else if let Some(value) = arg.strip_prefix("--from=") {
            if value.is_empty() {
                return Err("`--from` requires a package ref".into());
            }
            from = Some(value.to_string());
        } else if arg == "--with" {
            let value = iter
                .next()
                .ok_or("`--with` requires a package (try `ir tool run --with dplyr btw`)")?;
            push_with_deps(&mut with_deps, &value);
        } else if let Some(value) = arg.strip_prefix("--with=") {
            push_with_deps(&mut with_deps, value);
        } else if arg == "--r-version" {
            let value = iter.next().ok_or(
                "`--r-version` requires a version spec (try `ir tool run --r-version 4.5 btw`)",
            )?;
            r_requirement = Some(value);
        } else if let Some(value) = arg.strip_prefix("--r-version=") {
            r_requirement = Some(value.to_string());
        } else if arg == "--isolated" {
            // `ir tool run` is always isolated; accept this for symmetry with
            // `ir run` without changing behavior.
        } else if arg == "-e" {
            return Err("`-e` is not supported by `ir tool run`".into());
        } else if arg.starts_with('-') {
            rscript_args.push(arg);
        } else {
            positional = Some(arg);
            break;
        }
    }

    let tool_args: Vec<String> = iter.collect();
    let target = if let Some(package_ref) = from {
        let executable = positional.ok_or("`--from` requires a command to run")?;
        if !is_package_executable_name(&executable) {
            return Err("`--from` requires a command name, not a path".into());
        }
        PackageExecTarget {
            package_name: infer_self_named_executable(&package_ref),
            package_ref,
            executable,
        }
    } else {
        let package_ref = positional
            .ok_or("`ir tool run` requires a package ref or `--from <pkg-ref> <command>`")?;
        let executable = infer_self_named_executable(&package_ref)
            .ok_or("self-named package tools require an inferable package name; use `--from <pkg-ref> <command>`")?;
        PackageExecTarget {
            package_ref,
            package_name: Some(executable.clone()),
            executable,
        }
    };

    Ok(ToolRunArgs {
        rscript_args,
        with_deps,
        r_requirement,
        target,
        tool_args,
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

fn cmd_tool(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    match args.first().map(String::as_str) {
        Some("run") => {
            let run_args = args[1..].to_vec();
            if matches!(run_args.as_slice(), [arg] if arg == "--help" || arg == "-h") {
                print_tool_run_help();
                return Ok(());
            }
            let run = parse_tool_run_args(run_args)?;
            cmd_tool_run(&run)
        }
        Some("--help" | "-h") => {
            print_tool_help();
            Ok(())
        }
        None => {
            print_tool_help();
            Err("`ir tool` requires a subcommand".into())
        }
        Some(other) => Err(format!("unrecognized tool subcommand `{other}`").into()),
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
            "    ir run [Rscript-options...] [--isolated] [--with <pkg>]... [--r-version <spec>] <script.R> [args...]\n",
            "    ir run [Rscript-options...] [--isolated] [--with <pkg>]... [--r-version <spec>] -e <expr> [args...]\n",
            "    ir tool run [Rscript-options...] [--with <pkg>]... [--r-version <spec>] --from <pkg-ref> <command> [args...]\n",
            "    ir tool run [Rscript-options...] [--with <pkg>]... [--r-version <spec>] <pkg-ref> [args...]\n",
            "    ir cache <command>\n",
            "\n",
            "`ir run` reads the YAML frontmatter from <script.R>, resolves its\n",
            "dependencies, builds a dedicated package library, and runs the script\n",
            "against it. With -e it evaluates inline R expressions instead of a file.\n",
            "`ir tool run` resolves a package ref and runs an executable from that\n",
            "package's exec directory. --with adds dependencies on the command line,\n",
            "and --r-version selects the R version with rig. Leading Rscript options\n",
            "are passed to Rscript for script and tool targets; trailing args are\n",
            "passed through to the program.\n",
            "`ir cache` manages the dependency resolution and materialised library cache.\n",
            "\n",
            "Quarto documents (.qmd, .Rmd) are also supported: declare\n",
            "dependencies under an `ir:` key in the document's YAML frontmatter\n",
            "and ir renders them with `quarto render`.\n",
            "\n",
            "ENVIRONMENT:\n",
            "    IR_CACHE_DIR   override the cache dir (default: tools::R_user_dir(\"ir\", \"cache\"))\n",
            "    IR_RSCRIPT     path to the Rscript executable (default: Rscript on PATH)\n",
            "    IR_QUARTO      path to the quarto executable (default: quarto on PATH)"
        ),
        env!("CARGO_PKG_VERSION")
    );
}

fn print_run_help() {
    println!(concat!(
        "Run an R script\n",
        "\n",
        "USAGE:\n",
        "    ir run [Rscript-options...] [--isolated] [--with <pkg>]... [--r-version <spec>] <script.R> [args...]\n",
        "    ir run [Rscript-options...] [--isolated] [--with <pkg>]... [--r-version <spec>] -e <expr> [-e <expr>]... [args...]\n",
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
        "    --isolated    Disable the user library (R_LIBS_USER) so the run cannot\n",
        "                  borrow undeclared packages from it.\n",
        "\n",
        "Quarto documents (.qmd, .Rmd) are also supported: declare\n",
        "dependencies under an `ir:` key in the document's YAML frontmatter\n",
        "and ir renders them with `quarto render`.\n",
        "\n",
        "ENVIRONMENT:\n",
        "    IR_CACHE_DIR   override the cache dir (default: tools::R_user_dir(\"ir\", \"cache\"))\n",
        "    IR_RSCRIPT     path to the Rscript executable (default: Rscript on PATH)\n",
        "    IR_QUARTO      path to the quarto executable (default: quarto on PATH)"
    ));
}

fn print_tool_help() {
    println!(concat!(
        "Run package executables\n",
        "\n",
        "USAGE:\n",
        "    ir tool run [Rscript-options...] [--with <pkg>]... [--r-version <spec>] --from <pkg-ref> <command> [args...]\n",
        "    ir tool run [Rscript-options...] [--with <pkg>]... [--r-version <spec>] <pkg-ref> [args...]\n",
        "\n",
        "COMMANDS:\n",
        "    run  Resolve a package and run an executable from its exec directory\n",
        "\n",
        "ENVIRONMENT:\n",
        "    IR_CACHE_DIR   override the cache dir (default: tools::R_user_dir(\"ir\", \"cache\"))\n",
        "    IR_RSCRIPT     path to the Rscript executable (default: Rscript on PATH)"
    ));
}

fn print_tool_run_help() {
    println!(concat!(
        "Run a package executable\n",
        "\n",
        "USAGE:\n",
        "    ir tool run [Rscript-options...] [--with <pkg>]... [--r-version <spec>] --from <pkg-ref> <command> [args...]\n",
        "    ir tool run [Rscript-options...] [--with <pkg>]... [--r-version <spec>] <pkg-ref> [args...]\n",
        "\n",
        "`ir tool run` resolves <pkg-ref>, finds <library>/<package>/exec/<command>\n",
        "or <command>.R, and launches it with the selected Rscript. A bare package\n",
        "ref such as `btw` is treated as `--from btw btw`. Tool runs are isolated:\n",
        "R_LIBS_USER is set to NULL so undeclared user-library packages are not\n",
        "borrowed.\n",
        "\n",
        "OPTIONS:\n",
        "    --from <pkg-ref>\n",
        "                  Resolve a package ref and run <command> from its exec/\n",
        "                  directory. Omit for self-named commands such as `btw`.\n",
        "    --with <pkg>  Add a dependency for this run, resolved alongside the\n",
        "                  provider package. May be repeated and accepts a\n",
        "                  comma-separated list (e.g. --with cli,jsonlite).\n",
        "    --r-version <spec>\n",
        "                  Select the R version for this tool run with rig.\n",
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
    isolated: bool,
) -> Result<(), Box<dyn Error>> {
    // A script file declares its dependencies, `exclude-newer`, and `r-version` in
    // YAML frontmatter and is canonicalised so the run is independent of the
    // working directory. Quarto documents (.qmd/.Rmd) declare them under an
    // `ir:` key in that frontmatter. An inline `-e` expression has no
    // frontmatter and is never a Quarto document; its deps come solely from
    // `--with`.
    let (script_path, mut spec, quarto) = match source {
        RunSource::Script(script) => {
            let path = fs::canonicalize(script)
                .map_err(|e| format!("cannot read script `{script}`: {e}"))?;
            let quarto = quarto::is_quarto(&path);
            let spec = read_script_spec(&path, quarto)?;
            (Some(path), spec, quarto)
        }
        RunSource::Expressions(_) => (None, ScriptSpec::default(), false),
    };
    spec.dependencies.extend(with_deps.iter().cloned());
    if let Some(req) = r_requirement {
        spec.r_requirement = Some(req.to_string());
    }
    let rscript = rscript_for_spec(&spec)?;

    // Reject comma-bearing Rscript options before resolving, so a run that could
    // never be launched fails fast instead of after phase-1 resolution. quarto
    // forwards them via comma-separated QUARTO_KNITR_RSCRIPT_ARGS, which has no
    // escaping.
    if quarto {
        quarto::reject_comma_rscript_args(rscript_args)?;
    }

    // Phase 1: private R session resolves deps and materialises the library.
    // Rust parses the frontmatter and sends the dependency specs on stdin.
    let library = resolve_library(&rscript, &spec)?;

    // Phase 2: render the document, or run the user's program, in an isolated
    // R session.
    let code = if quarto {
        let doc = script_path
            .as_deref()
            .expect("is_quarto is only true for a RunSource::Script path");
        quarto::run(
            &rscript,
            library.as_deref(),
            doc,
            rscript_args,
            script_args,
            isolated,
        )?
    } else {
        let expressions: &[String] = match source {
            RunSource::Expressions(exprs) => exprs,
            RunSource::Script(_) => &[],
        };
        run_script(
            &rscript,
            library.as_deref(),
            script_path.as_deref(),
            expressions,
            rscript_args,
            script_args,
            isolated,
        )?
    };
    std::process::exit(code);
}

fn cmd_tool_run(run: &ToolRunArgs) -> Result<(), Box<dyn Error>> {
    let mut spec = ScriptSpec {
        dependencies: vec![run.target.package_ref.clone()],
        ..ScriptSpec::default()
    };
    spec.dependencies.extend(run.with_deps.iter().cloned());
    if let Some(req) = &run.r_requirement {
        spec.r_requirement = Some(req.clone());
    }

    let rscript = rscript_for_spec(&spec)?;
    let library = resolve_library(&rscript, &spec)?
        .ok_or("dependency resolver did not return a library path")?;
    let executable = find_package_executable(
        &library,
        run.target.package_name.as_deref(),
        &run.target.executable,
    )?;
    let code = run_package_executable(
        &rscript,
        &library,
        &executable,
        &run.rscript_args,
        &run.tool_args,
        &run.target.executable,
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

fn find_package_executable(
    library: &Path,
    package: Option<&str>,
    executable: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    if let Some(package) = package {
        let exec_dir = library.join(package).join("exec");
        return find_package_executable_in_dir(&exec_dir, executable).ok_or_else(|| {
            if !exec_dir.is_dir() {
                format!(
                    "package `{package}` does not have an exec directory in `{}`",
                    library.display()
                )
                .into()
            } else {
                format!(
                    "could not find executable `{executable}` or `{executable}.R` in `{}`",
                    exec_dir.display()
                )
                .into()
            }
        });
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(library)
        .map_err(|e| format!("cannot read resolved library `{}`: {e}", library.display()))?
    {
        let exec_dir = entry?.path().join("exec");
        if let Some(path) = find_package_executable_in_dir(&exec_dir, executable) {
            candidates.push(path);
        }
    }
    candidates.sort();

    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        0 => Err(format!(
            "could not find executable `{executable}` or `{executable}.R` in any package under `{}`",
            library.display()
        )
        .into()),
        _ => Err(format!(
            "found multiple executables named `{executable}` in `{}`; use a package ref whose installed package name can be inferred",
            library.display()
        )
        .into()),
    }
}

fn find_package_executable_in_dir(exec_dir: &Path, executable: &str) -> Option<PathBuf> {
    let candidates = [
        exec_dir.join(executable),
        exec_dir.join(format!("{executable}.R")),
    ];

    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn resolved_runtime_path(library: &Path, rscript: &OsStr) -> Result<OsString, Box<dyn Error>> {
    let mut entries = Vec::new();

    let rscript_path = Path::new(rscript);
    if let Some(parent) = rscript_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        entries.push(parent.to_path_buf());
    }

    for entry in fs::read_dir(library)
        .map_err(|e| format!("cannot read resolved library `{}`: {e}", library.display()))?
    {
        let exec = entry?.path().join("exec");
        if exec.is_dir() {
            entries.push(exec);
        }
    }

    let current_path = env::var_os("PATH").unwrap_or_default();
    entries.extend(env::split_paths(&current_path));
    Ok(env::join_paths(entries)?)
}

fn run_package_executable(
    rscript: &OsStr,
    library: &Path,
    executable: &Path,
    rscript_args: &[String],
    args: &[String],
    launcher_name: &str,
) -> Result<i32, Box<dyn Error>> {
    let launcher = package_executable_launcher(executable)?;
    let mut cmd = Command::new(rscript);
    cmd.args(rscript_args);
    match launcher {
        PackageLauncher::Rscript => {
            cmd.arg(executable);
        }
        PackageLauncher::Rapp => {
            cmd.arg("-e").arg("Rapp::run()").arg(executable);
        }
    }
    cmd.args(args)
        .env("R_LIBS", library)
        .env("R_LIBS_USER", "NULL")
        .env("RAPP_LAUNCHER_NAME", launcher_name)
        .env("PATH", resolved_runtime_path(library, rscript)?);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(spawn_error(rscript, cmd.exec()).into())
    }

    #[cfg(not(unix))]
    {
        let status = cmd.status().map_err(|e| spawn_error(rscript, e))?;
        Ok(status.code().unwrap_or(1))
    }
}

enum PackageLauncher {
    Rscript,
    Rapp,
}

fn package_executable_launcher(executable: &Path) -> Result<PackageLauncher, Box<dyn Error>> {
    let file = File::open(executable)
        .map_err(|e| format!("cannot read executable `{}`: {e}", executable.display()))?;
    let mut reader = BufReader::new(file);
    let mut shebang = String::new();
    reader.read_line(&mut shebang)?;

    if !shebang.starts_with("#!") {
        return Err(format!(
            "package executable `{}` must start with a Rscript or Rapp shebang",
            executable.display()
        )
        .into());
    }

    if shebang_mentions(&shebang, "Rapp") {
        Ok(PackageLauncher::Rapp)
    } else if shebang_mentions(&shebang, "Rscript") {
        Ok(PackageLauncher::Rscript)
    } else {
        Err(format!(
            "package executable `{}` must use a Rscript or Rapp shebang",
            executable.display()
        )
        .into())
    }
}

fn shebang_mentions(shebang: &str, name: &str) -> bool {
    shebang
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| word == name)
}

fn read_script_spec(script: &Path, quarto: bool) -> Result<ScriptSpec, Box<dyn Error>> {
    let frontmatter = if quarto {
        quarto::read_yaml_block_to_string(script)?
    } else {
        read_op_frontmatter_to_string(script)?
    };
    parse_frontmatter(&frontmatter, quarto)
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

fn rscript_for_spec(spec: &ScriptSpec) -> Result<OsString, Box<dyn Error>> {
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
/// It runs as an ordinary `Rscript [Rscript-options...] (script.R | -e expr...)` -
/// its `.Renviron`, `.Rprofile` and site files are read unless the forwarded
/// Rscript options disable them. The resolved library is injected via `R_LIBS`,
/// which is *prepended* to `.libPaths()`: resolved dependencies take precedence,
/// while the user's other libraries remain available. (`R_LIBS` is used rather
/// than `R_LIBS_USER`, since a user `.Renviron` setting `R_LIBS_USER` would
/// override the latter.)
///
/// When `isolated` is set, the user library is dropped too: `R_LIBS_USER=NULL`
/// is R's documented way to disable it, so `.libPaths()` is the resolved library
/// (via `R_LIBS`) plus the site and base/system libraries. The system library
/// stays available, so base and recommended packages keep working.
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
    isolated: bool,
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

    if isolated {
        // Drop the user library so the run can't borrow undeclared packages from
        // it. "NULL" is R's special value that disables the user library; an
        // empty value or unset would instead fall back to the default location.
        // The site and base/system libraries stay on the path.
        cmd.env("R_LIBS_USER", "NULL");
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
fn rscript_command() -> OsString {
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

fn nonempty_env(name: &str) -> Option<OsString> {
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
