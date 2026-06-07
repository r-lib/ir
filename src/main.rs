//! `ir` — self-describing R scripts.
//!
//! Runs a standalone R script whose dependencies are declared in YAML
//! frontmatter at the top of the file:
//!
//! ```r
//! #!/usr/bin/env -S ir run
//! #| packages:
//! #|   - dplyr>=1.0
//! #|   - tidyr
//! #| r-version: ">= 4.0"
//! #| isolated: true
//! #| exclude-newer: "2024-01-15"
//!
//! library(dplyr)
//! 1 + 1
//! ```
//!
//! The pipeline has two phases:
//!
//!   1. Rust extracts and parses the leading `#| ` YAML frontmatter block. If
//!      the resolution cache is warm, Rust reuses the cached library path
//!      directly. Otherwise, a private R session (`driver/resolve.R`) receives
//!      the normalized pak refs on stdin, resolves them with pak, hashes the
//!      resolved set into a content-addressed library path under the cache
//!      directory, and materialises that path as a light-weight library of
//!      symlinks into renv's package cache. The path is reported back to us.
//!
//!   2. We launch the user's script in a fresh R session with that library
//!      prepended to `.libPaths()`. With `--isolated`, the user library is
//!      dropped.

use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Arg, ArgAction, ArgMatches, Command as ClapCommand};
use saphyr::{Yaml, YamlLoader};
use saphyr_parser::Parser;

mod quarto;
mod resolve_cache;
mod rig;

/// The R resolution driver, embedded at compile time so `ir` ships as one
/// self-contained binary while the source stays editable as real R.
const RESOLVE_DRIVER: &str = include_str!("../driver/resolve.R");

#[derive(Debug, Default)]
struct ScriptSpec {
    dependencies: Vec<String>,
    exclude_newer: Option<String>,
    isolated: bool,
    r_requirement: Option<String>,
    // A Quarto source: the resolver injects rmarkdown for the knitr engine.
    quarto: bool,
}

fn main() {
    if let Err(err) = try_main() {
        match err.downcast::<clap::Error>() {
            Ok(err) => err.exit(),
            Err(err) => {
                eprintln!("ir: {err}");
                std::process::exit(1);
            }
        }
    }
}

fn try_main() -> Result<(), Box<dyn Error>> {
    let argv: Vec<String> = env::args().collect();
    let matches = cli().try_get_matches_from(argv.clone())?;
    match matches.subcommand() {
        Some(("run", _)) => {
            let run = parse_run_args(argv[2..].to_vec())?;
            cmd_run(
                &run.source,
                &run.rscript_args,
                &run.with_deps,
                run.r_requirement.as_deref(),
                &run.script_args,
                run.isolated,
            )
        }
        Some(("tool", matches)) => cmd_tool(matches, &argv),
        Some(("cache", matches)) => cmd_cache(matches),
        _ => Ok(()),
    }
}

fn cli() -> ClapCommand {
    ClapCommand::new("ir")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Run self-describing R scripts")
        .arg_required_else_help(true)
        .subcommand(run_command())
        .subcommand(tool_command())
        .subcommand(cache_command())
}

fn run_command() -> ClapCommand {
    ClapCommand::new("run")
        .about("Run a script or inline R expression")
        .arg(
            Arg::new("expr")
                .short('e')
                .long("expr")
                .value_name("EXPR")
                .num_args(1)
                .action(ArgAction::Append)
                .help("Evaluate an inline R expression instead of a script file"),
        )
        .arg(
            Arg::new("with")
                .long("with")
                .value_name("PKG")
                .num_args(1)
                .action(ArgAction::Append)
                .help("Add a dependency for this run; may be repeated"),
        )
        .arg(
            Arg::new("r-version")
                .long("r-version")
                .value_name("SPEC")
                .num_args(1)
                .help("Select the R version for this run with rig"),
        )
        .arg(
            Arg::new("isolated")
                .long("isolated")
                .action(ArgAction::SetTrue)
                .help("Disable the user library for this run"),
        )
        .arg(raw_args_arg(
            "Rscript options, script path, and script arguments",
        ))
}

fn tool_command() -> ClapCommand {
    ClapCommand::new("tool")
        .about("Run package executables")
        .arg_required_else_help(true)
        .subcommand(tool_run_command())
        .subcommand(tool_rx_command())
        .subcommand(tool_install_command())
}

fn tool_run_command() -> ClapCommand {
    tool_run_args(
        ClapCommand::new("run")
            .about("Resolve a package and run an executable from its exec directory"),
    )
}

fn tool_rx_command() -> ClapCommand {
    tool_run_args(
        ClapCommand::new("rx")
            .hide(true)
            .display_name("rx")
            .override_usage("rx [OPTIONS] [ARGS]...")
            .about("Run a package executable")
            .version(env!("CARGO_PKG_VERSION"))
            .after_help("Use `ir tool run` for more details."),
    )
}

fn tool_run_args(command: ClapCommand) -> ClapCommand {
    command
        .arg(
            Arg::new("from")
                .long("from")
                .value_name("PKG_REF")
                .num_args(1)
                .help("Resolve a package ref and run <command> from its exec/ directory"),
        )
        .arg(
            Arg::new("with")
                .short('w')
                .long("with")
                .value_name("PKG")
                .num_args(1)
                .action(ArgAction::Append)
                .help("Add a dependency for this tool run; may be repeated"),
        )
        .arg(
            Arg::new("r-version")
                .long("r-version")
                .value_name("SPEC")
                .num_args(1)
                .help("Select the R version for this tool run with rig"),
        )
        .arg(
            Arg::new("isolated")
                .long("isolated")
                .action(ArgAction::SetTrue)
                .hide(true),
        )
        .arg(raw_args_arg(
            "Rscript options, package ref or command, and tool arguments",
        ))
}

fn tool_install_command() -> ClapCommand {
    ClapCommand::new("install")
        .about("Install package executable launchers")
        .arg(
            Arg::new("with")
                .long("with")
                .value_name("PKG")
                .num_args(1)
                .action(ArgAction::Append)
                .help("Add a dependency for installed launchers; may be repeated"),
        )
        .arg(
            Arg::new("r-version")
                .long("r-version")
                .value_name("SPEC")
                .num_args(1)
                .help("Select the R version for installed launchers with rig"),
        )
        .arg(
            Arg::new("bin-dir")
                .long("bin-dir")
                .value_name("DIR")
                .num_args(1)
                .help("Directory where launchers are written"),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Overwrite an existing launcher path"),
        )
        .arg(
            Arg::new("package-ref")
                .value_name("PKG_REF")
                .required(true)
                .help("Package ref that resolves to the package exposing exec/ launchers"),
        )
}

fn cache_command() -> ClapCommand {
    ClapCommand::new("cache")
        .about("Manage ir's cache")
        .arg_required_else_help(true)
        .subcommand(
            ClapCommand::new("clean")
                .about("Clear the cache, removing all entries")
                .arg(
                    Arg::new("force")
                        .long("force")
                        .action(ArgAction::SetTrue)
                        .help("Accepted for compatibility; same as `ir cache clean`"),
                ),
        )
        .subcommand(ClapCommand::new("dir").about("Show the cache directory"))
}

fn raw_args_arg(help: &'static str) -> Arg {
    Arg::new("args")
        .value_name("ARGS")
        .num_args(0..)
        .allow_hyphen_values(true)
        .trailing_var_arg(true)
        .help(help)
}

/// Where the user's program comes from.
enum RunSource {
    Script(PathBuf),
    Quarto(PathBuf),
    Expressions(Vec<String>),
    Stdin,
}

enum RscriptSource<'a> {
    Script(&'a Path),
    Expressions(&'a [String]),
    Stdin,
}

impl RunSource {
    fn from_script_arg(script: String) -> Result<Self, Box<dyn Error>> {
        if script == "-" {
            return Ok(Self::Stdin);
        }

        // The path is passed through untouched: R and quarto both inherit `ir`'s
        // working directory, so a relative path resolves exactly as the user
        // typed it. (`fs::canonicalize` was avoided because on Windows it returns
        // a `\\?\C:\...` verbatim path that quarto's Deno `expandGlobSync` cannot
        // stat — `os error 123`.) Verify existence here for a clear error.
        let path = PathBuf::from(&script);
        fs::metadata(&path).map_err(|e| format!("cannot read script `{script}`: {e}"))?;
        if quarto::is_quarto(&path) {
            Ok(Self::Quarto(path))
        } else {
            Ok(Self::Script(path))
        }
    }

    fn script_spec(&self) -> Result<ScriptSpec, Box<dyn Error>> {
        match self {
            Self::Script(script) => read_script_spec(script, false),
            Self::Quarto(doc) => read_script_spec(doc, true),
            Self::Expressions(_) | Self::Stdin => Ok(ScriptSpec::default()),
        }
    }

    /// True for Quarto documents, which are rendered with `quarto render` and
    /// whose knitr engine needs `rmarkdown` injected into the resolved library.
    fn is_quarto(&self) -> bool {
        matches!(self, Self::Quarto(_))
    }

    fn reject_unsupported_rscript_args(
        &self,
        rscript_args: &[String],
    ) -> Result<(), Box<dyn Error>> {
        match self {
            Self::Quarto(_) => quarto::reject_comma_rscript_args(rscript_args),
            Self::Script(_) | Self::Expressions(_) | Self::Stdin => Ok(()),
        }
    }

    fn run_user_code(
        &self,
        rscript: &OsStr,
        library: Option<&Path>,
        rscript_args: &[String],
        script_args: &[String],
        isolated: bool,
    ) -> Result<i32, Box<dyn Error>> {
        match self {
            Self::Quarto(doc) => {
                quarto::run(rscript, library, doc, rscript_args, script_args, isolated)
            }
            Self::Script(script) => run_script(
                rscript,
                library,
                RscriptSource::Script(script),
                rscript_args,
                script_args,
                isolated,
            ),
            Self::Expressions(expressions) => run_script(
                rscript,
                library,
                RscriptSource::Expressions(expressions),
                rscript_args,
                script_args,
                isolated,
            ),
            Self::Stdin => run_script(
                rscript,
                library,
                RscriptSource::Stdin,
                rscript_args,
                script_args,
                isolated,
            ),
        }
    }
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

struct ToolInstallArgs {
    package_ref: String,
    with_deps: Vec<String>,
    r_requirement: Option<String>,
    bin_dir: PathBuf,
    force: bool,
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
        if arg == "-e" || arg == "--expr" {
            let expr = iter
                .next()
                .ok_or("`-e` requires an expression (try `ir run -e '1 + 1'`)")?;
            expressions.push(expr);
        } else if let Some(expr) = arg.strip_prefix("--expr=") {
            expressions.push(expr.to_string());
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
        } else if arg == "-" {
            positional = Some(arg);
            break;
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
        (RunSource::from_script_arg(script)?, script_args)
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

#[derive(Clone, Copy)]
enum ToolRunInvocation {
    ToolRun,
    Rx,
}

impl ToolRunInvocation {
    fn command(self) -> &'static str {
        match self {
            Self::ToolRun => "ir tool run",
            Self::Rx => "rx",
        }
    }
}

/// Parse `ir tool run`, which resolves a provider package and runs a command
/// from that package's `exec/` directory. This is intentionally separate from
/// `ir run`: script and expression runs are source-oriented, tool runs are
/// package-oriented and isolated by default.
fn parse_tool_run_args(
    args: Vec<String>,
    invocation: ToolRunInvocation,
) -> Result<ToolRunArgs, Box<dyn Error>> {
    let mut rscript_args = Vec::new();
    let mut with_deps = Vec::new();
    let mut r_requirement = None;
    let mut from = None;
    let mut iter = args.into_iter();
    let mut positional = None;

    while let Some(arg) = iter.next() {
        if arg == "--from" {
            let value = iter.next().ok_or_else(|| {
                format!(
                    "`--from` requires a package ref (try `{} --from cli cli`)",
                    invocation.command()
                )
            })?;
            from = Some(value);
        } else if let Some(value) = arg.strip_prefix("--from=") {
            if value.is_empty() {
                return Err("`--from` requires a package ref".into());
            }
            from = Some(value.to_string());
        } else if arg == "--with" || arg == "-w" {
            let value = iter.next().ok_or_else(|| {
                format!(
                    "`{arg}` requires a package (try `{} {arg} dplyr btw`)",
                    invocation.command()
                )
            })?;
            push_with_deps(&mut with_deps, &value);
        } else if let Some(value) = arg.strip_prefix("--with=") {
            push_with_deps(&mut with_deps, value);
        } else if arg == "--r-version" {
            let value = iter.next().ok_or_else(|| {
                format!(
                    "`--r-version` requires a version spec (try `{} --r-version 4.5 btw`)",
                    invocation.command()
                )
            })?;
            r_requirement = Some(value);
        } else if let Some(value) = arg.strip_prefix("--r-version=") {
            r_requirement = Some(value.to_string());
        } else if arg == "--isolated" {
            // `ir tool run` is always isolated; accept this for symmetry with
            // `ir run` without changing behavior.
        } else if arg == "-e" {
            return Err(format!("`-e` is not supported by `{}`", invocation.command()).into());
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
        let package_ref = positional.ok_or_else(|| {
            format!(
                "`{}` requires a package ref or `--from <pkg-ref> <command>`",
                invocation.command()
            )
        })?;
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

fn parse_tool_install_args(args: Vec<String>) -> Result<ToolInstallArgs, Box<dyn Error>> {
    let mut with_deps = Vec::new();
    let mut r_requirement = None;
    let mut bin_dir = None;
    let mut force = false;
    let mut iter = args.into_iter();
    let mut positional = None;

    while let Some(arg) = iter.next() {
        if arg == "--with" {
            let value = iter
                .next()
                .ok_or("`--with` requires a package (try `ir tool install --with cli btw`)")?;
            push_with_deps(&mut with_deps, &value);
        } else if let Some(value) = arg.strip_prefix("--with=") {
            push_with_deps(&mut with_deps, value);
        } else if arg == "--r-version" {
            let value = iter.next().ok_or(
                "`--r-version` requires a version spec (try `ir tool install --r-version 4.5 btw`)",
            )?;
            r_requirement = Some(value);
        } else if let Some(value) = arg.strip_prefix("--r-version=") {
            r_requirement = Some(value.to_string());
        } else if arg == "--bin-dir" {
            let value = iter
                .next()
                .ok_or("`--bin-dir` requires a directory (try `ir tool install --bin-dir ~/.local/bin btw`)")?;
            bin_dir = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--bin-dir=") {
            if value.is_empty() {
                return Err("`--bin-dir` requires a directory".into());
            }
            bin_dir = Some(PathBuf::from(value));
        } else if arg == "--force" {
            force = true;
        } else if arg == "-e" {
            return Err("`-e` is not supported by `ir tool install`".into());
        } else if arg.starts_with('-') {
            return Err(format!("unexpected option `{arg}` for `ir tool install`").into());
        } else {
            positional = Some(arg);
            break;
        }
    }

    let package_arg =
        positional.ok_or("`ir tool install` requires a package ref (try `ir tool install btw`)")?;
    if let Some(extra) = iter.next() {
        return Err(
            format!("unexpected argument `{extra}` after package ref `{package_arg}`").into(),
        );
    }

    Ok(ToolInstallArgs {
        package_ref: package_arg,
        with_deps,
        r_requirement,
        bin_dir: bin_dir.unwrap_or(tool_install_bin_dir()?),
        force,
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

fn cmd_tool(matches: &ArgMatches, argv: &[String]) -> Result<(), Box<dyn Error>> {
    match matches.subcommand() {
        Some(("run", _)) => {
            let run = parse_tool_run_args(argv[3..].to_vec(), ToolRunInvocation::ToolRun)?;
            cmd_tool_run(&run)
        }
        Some(("rx", _)) => {
            let run = parse_tool_run_args(argv[3..].to_vec(), ToolRunInvocation::Rx)?;
            cmd_tool_run(&run)
        }
        Some(("install", _)) => {
            let install_args = argv[3..].to_vec();
            let install = parse_tool_install_args(install_args)?;
            cmd_tool_install(&install)
        }
        _ => unreachable!("clap requires a tool subcommand"),
    }
}

fn cmd_cache(matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    match matches.subcommand() {
        Some(("clean", matches)) => cmd_cache_clean(matches.get_flag("force")),
        Some(("dir", _)) => cmd_cache_dir(),
        _ => unreachable!("clap requires a cache subcommand"),
    }
}

fn cmd_cache_clean(_force: bool) -> Result<(), Box<dyn Error>> {
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

fn cmd_cache_dir() -> Result<(), Box<dyn Error>> {
    println!("{}", ir_cache_dir()?.display());
    Ok(())
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
    let mut spec = source.script_spec()?;
    spec.dependencies.extend(with_deps.iter().cloned());
    spec.quarto = source.is_quarto();
    if let Some(req) = r_requirement {
        spec.r_requirement = Some(req.to_string());
    }
    let isolated = isolated || spec.isolated;
    let rscript = rscript_for_spec(&spec)?;

    // Reject comma-bearing Rscript options before resolving, so a run that could
    // never be launched fails fast instead of after dependency resolution. quarto
    // forwards them via comma-separated QUARTO_KNITR_RSCRIPT_ARGS, which has no
    // escaping.
    source.reject_unsupported_rscript_args(rscript_args)?;

    // Reuse a warm resolution marker, or launch the private resolver R session
    // to resolve deps and materialise the library.
    let library = resolve_library(&rscript, &spec)?;

    // Render the document, or run the user's program, in an isolated R session.
    let code = source.run_user_code(
        &rscript,
        library.as_deref(),
        rscript_args,
        script_args,
        isolated,
    )?;
    std::process::exit(code);
}

fn cmd_tool_run(run: &ToolRunArgs) -> Result<(), Box<dyn Error>> {
    let mut deps = vec![run.target.package_ref.clone()];
    deps.extend(run.with_deps.iter().cloned());
    let mut spec = ScriptSpec {
        dependencies: deps,
        ..ScriptSpec::default()
    };
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

fn cmd_tool_install(install: &ToolInstallArgs) -> Result<(), Box<dyn Error>> {
    let mut spec = ScriptSpec {
        dependencies: vec![install.package_ref.clone()],
        ..ScriptSpec::default()
    };
    spec.dependencies.extend(install.with_deps.iter().cloned());
    if let Some(req) = &install.r_requirement {
        spec.r_requirement = Some(req.clone());
    }

    let rscript = rscript_for_spec(&spec)?;
    let (library, package_name) = resolve_library_and_primary_package(&rscript, &spec)?;
    let executables = discover_package_executables(&library, &package_name)?;
    if executables.is_empty() {
        return Err(format!(
            "package `{}` does not expose Rscript or Rapp executables in `{}`",
            package_name,
            library.join(&package_name).join("exec").display()
        )
        .into());
    }

    fs::create_dir_all(&install.bin_dir).map_err(|e| {
        format!(
            "failed to create launcher directory `{}`: {e}",
            install.bin_dir.display()
        )
    })?;

    let path_prefix = resolved_runtime_path_prefix(&library, &rscript)?;
    let reinstall_command = tool_install_recovery_command(install);
    for executable in &executables {
        let target = launcher_target_path(&install.bin_dir, &executable.name);
        if target.exists() && !install.force {
            return Err(format!(
                "launcher `{}` already exists; pass --force to overwrite it",
                target.display()
            )
            .into());
        }
    }

    let mut installed = Vec::new();
    for executable in executables {
        let target = launcher_target_path(&install.bin_dir, &executable.name);
        let contents = installed_launcher_contents(
            &rscript,
            &library,
            &executable,
            &path_prefix,
            &reinstall_command,
        )?;
        fs::write(&target, contents)
            .map_err(|e| format!("failed to write launcher `{}`: {e}", target.display()))?;
        make_executable(&target)?;
        installed.push(executable.name);
    }

    println!(
        "Installed {} executable{}: {}",
        installed.len(),
        if installed.len() == 1 { "" } else { "s" },
        installed.join(", ")
    );
    Ok(())
}

/// Return a cached materialised library path, or run the embedded driver in a
/// private R session to resolve and materialise it. The dependency specs in
/// `spec` (the script's frontmatter plus any `--with` packages) are normalized
/// into pak refs before cache keying and resolver input.
fn resolve_library(rscript: &OsStr, spec: &ScriptSpec) -> Result<Option<PathBuf>, Box<dyn Error>> {
    Ok(resolve_library_inner(rscript, spec, false)?.library)
}

fn resolve_library_and_primary_package(
    rscript: &OsStr,
    spec: &ScriptSpec,
) -> Result<(PathBuf, String), Box<dyn Error>> {
    let resolved = resolve_library_inner(rscript, spec, true)?;
    let library = resolved
        .library
        .ok_or("dependency resolver did not return a library path")?;
    let package = resolved
        .primary_package
        .ok_or("dependency resolver did not return a package name")?;
    Ok((library, package))
}

struct ResolvedLibrary {
    library: Option<PathBuf>,
    primary_package: Option<String>,
}

fn resolve_library_inner(
    rscript: &OsStr,
    spec: &ScriptSpec,
    primary_package: bool,
) -> Result<ResolvedLibrary, Box<dyn Error>> {
    let dependencies = normalized_dependencies(&spec.dependencies);
    let cache_dir = ir_cache_dir()?;
    let resolution_cache_paths = resolve_cache::paths(
        &cache_dir,
        rscript,
        &dependencies,
        spec.exclude_newer.as_deref(),
        spec.quarto,
    )?;
    if let Some(resolved) = resolve_cache::read(resolution_cache_paths.as_ref(), primary_package)? {
        return Ok(ResolvedLibrary {
            library: Some(resolved.library),
            primary_package: resolved.primary_package,
        });
    }

    let tmp = env::temp_dir();
    let driver = unique_path(&tmp, "ir-resolve", "R");
    let result_file = unique_path(&tmp, "ir-libpath", "txt");
    let package_result_file = primary_package.then(|| unique_path(&tmp, "ir-package", "txt"));
    fs::write(&driver, RESOLVE_DRIVER)?;

    let mut cmd = Command::new(rscript);
    cmd.arg(&driver)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("IR_RESOLVE_RESULT_FILE", &result_file)
        .env("IR_CACHE_DIR", &cache_dir)
        // pak suppresses progress in noninteractive Rscript unless this is set.
        // Resolution cache hits return before pak, so this adds no cache-hit pak output.
        .env("R_PKG_SHOW_PROGRESS", "true");
    if let Some(paths) = &resolution_cache_paths {
        cmd.env("IR_RESOLUTION_MARKER", &paths.marker);
    }
    if let Some(package_result_file) = &package_result_file {
        cmd.env("IR_RESOLVE_PACKAGE_RESULT_FILE", package_result_file);
        if let Some(package_marker) = resolution_cache_paths
            .as_ref()
            .and_then(|paths| paths.package_marker.as_ref())
        {
            cmd.env("IR_PRIMARY_PACKAGE_MARKER", package_marker);
        }
    }
    if let Some(exclude_newer) = &spec.exclude_newer {
        cmd.env("IR_EXCLUDE_NEWER", exclude_newer);
    }
    if spec.quarto {
        // Distinct from IR_QUARTO (the quarto executable, read in quarto.rs):
        // this flag tells the resolver a Quarto render needs rmarkdown.
        cmd.env("IR_QUARTO_RENDER", "1");
    }

    let mut child = cmd.spawn().map_err(|e| spawn_error(rscript, e))?;
    {
        let mut stdin = child.stdin.take().ok_or("failed to open resolver stdin")?;
        for dependency in dependencies {
            writeln!(stdin, "{dependency}")?;
        }
    }
    let status = child
        .wait()
        .map_err(|e| format!("failed to wait for dependency resolver: {e}"))?;

    let _ = fs::remove_file(&driver);
    let result = fs::read_to_string(&result_file).unwrap_or_default();
    let _ = fs::remove_file(&result_file);
    let package_result = package_result_file
        .as_ref()
        .map(|path| {
            let result = fs::read_to_string(path).unwrap_or_default();
            let _ = fs::remove_file(path);
            result
        })
        .unwrap_or_default();

    if !status.success() {
        return Err("dependency resolution failed".into());
    }

    let path = result.trim();
    let library = if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    };
    let package = package_result.trim();
    let primary_package = if package.is_empty() {
        None
    } else {
        Some(package.to_string())
    };

    Ok(ResolvedLibrary {
        library,
        primary_package,
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

struct PackageExecutable {
    name: String,
    path: PathBuf,
    launcher: PackageLauncher,
}

fn discover_package_executables(
    library: &Path,
    package: &str,
) -> Result<Vec<PackageExecutable>, Box<dyn Error>> {
    let exec_dir = library.join(package).join("exec");
    if !exec_dir.is_dir() {
        return Err(format!(
            "package `{package}` does not have an exec directory in `{}`",
            library.display()
        )
        .into());
    }

    let mut executables = Vec::new();
    for entry in fs::read_dir(&exec_dir)
        .map_err(|e| format!("cannot read exec directory `{}`: {e}", exec_dir.display()))?
    {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Some(launcher) = package_executable_launcher_kind(&path)? else {
            continue;
        };
        let name = package_executable_launcher_name(&path)?;
        if executables
            .iter()
            .any(|executable: &PackageExecutable| executable.name == name)
        {
            return Err(format!(
                "multiple package executables map to launcher `{name}` in `{}`",
                exec_dir.display()
            )
            .into());
        }
        executables.push(PackageExecutable {
            name,
            path,
            launcher,
        });
    }

    executables.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(executables)
}

fn package_executable_launcher_name(path: &Path) -> Result<String, Box<dyn Error>> {
    let name = if path
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("R"))
    {
        path.file_stem()
    } else {
        path.file_name()
    }
    .and_then(OsStr::to_str)
    .ok_or_else(|| format!("package executable `{}` is not valid UTF-8", path.display()))?;

    if !is_package_executable_name(name) {
        return Err(format!(
            "package executable `{}` maps to unsupported launcher name `{name}`",
            path.display()
        )
        .into());
    }

    Ok(name.to_string())
}

fn resolved_runtime_path_prefix(
    library: &Path,
    rscript: &OsStr,
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
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

    Ok(entries)
}

fn resolved_runtime_path(library: &Path, rscript: &OsStr) -> Result<OsString, Box<dyn Error>> {
    let mut entries = resolved_runtime_path_prefix(library, rscript)?;
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

#[derive(Clone, Copy)]
enum PackageLauncher {
    Rscript,
    Rapp,
}

fn package_executable_launcher(executable: &Path) -> Result<PackageLauncher, Box<dyn Error>> {
    package_executable_launcher_kind(executable)?.ok_or_else(|| {
        format!(
            "package executable `{}` must use a Rscript or Rapp shebang",
            executable.display()
        )
        .into()
    })
}

fn package_executable_launcher_kind(
    executable: &Path,
) -> Result<Option<PackageLauncher>, Box<dyn Error>> {
    let file = File::open(executable)
        .map_err(|e| format!("cannot read executable `{}`: {e}", executable.display()))?;
    let mut reader = BufReader::new(file);
    let mut shebang = String::new();
    reader.read_line(&mut shebang)?;

    if !shebang.starts_with("#!") {
        return Ok(None);
    }

    if shebang_mentions(&shebang, "Rapp") {
        Ok(Some(PackageLauncher::Rapp))
    } else if shebang_mentions(&shebang, "Rscript") {
        Ok(Some(PackageLauncher::Rscript))
    } else {
        Ok(None)
    }
}

fn shebang_mentions(shebang: &str, name: &str) -> bool {
    shebang
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| word == name)
}

fn read_script_spec(script: &Path, quarto: bool) -> Result<ScriptSpec, Box<dyn Error>> {
    if quarto {
        parse_quarto_frontmatter(&quarto::read_to_string(script)?)
    } else {
        parse_r_script_frontmatter(&read_r_script_frontmatter_to_string(script)?)
    }
}

fn parse_r_script_frontmatter(frontmatter: &str) -> Result<ScriptSpec, Box<dyn Error>> {
    if frontmatter.trim().is_empty() {
        return Ok(ScriptSpec::default());
    }

    let Some(doc) = load_first_yaml_document(frontmatter)? else {
        return Ok(ScriptSpec::default());
    };

    script_spec_from_yaml_mapping(&doc)
}

fn parse_quarto_frontmatter(document: &str) -> Result<ScriptSpec, Box<dyn Error>> {
    if document.trim().is_empty() {
        return Ok(ScriptSpec::default());
    }

    let Some(doc) = load_first_yaml_document(document)? else {
        return Ok(ScriptSpec::default());
    };
    if doc.is_null() {
        return Ok(ScriptSpec::default());
    }
    if !doc.is_mapping() {
        return Err("script frontmatter must be a YAML mapping".into());
    }

    let Some(spec_node) = doc.as_mapping_get("ir") else {
        return Ok(ScriptSpec::default());
    };
    if spec_node.is_null() {
        return Ok(ScriptSpec::default());
    }
    if !spec_node.is_mapping() {
        return Err("frontmatter `ir` must be a YAML mapping".into());
    }

    script_spec_from_yaml_mapping(spec_node)
}

fn script_spec_from_yaml_mapping(doc: &Yaml<'_>) -> Result<ScriptSpec, Box<dyn Error>> {
    if doc.is_null() {
        return Ok(ScriptSpec::default());
    }
    if !doc.is_mapping() {
        return Err("script frontmatter must be a YAML mapping".into());
    }

    Ok(ScriptSpec {
        dependencies: frontmatter_dependencies(doc)?,
        exclude_newer: frontmatter_optional_string(doc, "exclude-newer")?,
        isolated: frontmatter_optional_bool(doc, "isolated")?.unwrap_or(false),
        r_requirement: frontmatter_optional_string(doc, "r-version")?,
        // Quarto-ness is a property of the source, not its frontmatter; cmd_run
        // sets it from RunSource::is_quarto after parsing.
        ..ScriptSpec::default()
    })
}

fn load_first_yaml_document(source: &str) -> Result<Option<Yaml<'_>>, Box<dyn Error>> {
    let mut parser = Parser::new_from_str(source);
    let mut loader = YamlLoader::default();
    parser
        .load(&mut loader, false)
        .map_err(|e| format!("could not parse script frontmatter as YAML: {e}"))?;
    Ok(loader.into_documents().into_iter().next())
}

fn rscript_for_spec(spec: &ScriptSpec) -> Result<OsString, Box<dyn Error>> {
    let Some(req) = &spec.r_requirement else {
        return Ok(rscript_command());
    };

    rig::resolve_rscript(req, spec.exclude_newer.as_deref())
}

fn frontmatter_dependencies(doc: &Yaml<'_>) -> Result<Vec<String>, Box<dyn Error>> {
    let Some(value) = doc.as_mapping_get("packages") else {
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
        return Err("frontmatter `packages` entries must be strings".into());
    };
    dependencies.extend(value.split_whitespace().map(str::to_owned));
    Ok(())
}

fn frontmatter_optional_bool(doc: &Yaml<'_>, key: &str) -> Result<Option<bool>, Box<dyn Error>> {
    let Some(value) = doc.as_mapping_get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| format!("frontmatter `{key}` must be a boolean").into())
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

fn normalized_dependencies(dependencies: &[String]) -> Vec<String> {
    dependencies
        .iter()
        .map(|dependency| dependency_to_ref(dependency))
        .collect()
}

fn dependency_to_ref(dependency: &str) -> String {
    let dependency = dependency.trim();
    let Some((package, operator, version)) = parse_simple_version_ref(dependency) else {
        return dependency.to_string();
    };

    match operator {
        ">=" => format!("{package}@>={version}"),
        "==" => format!("{package}@{version}"),
        _ => unreachable!("parse_simple_version_ref only returns supported operators"),
    }
}

fn parse_simple_version_ref(dependency: &str) -> Option<(&str, &str, &str)> {
    let mut name_chars = dependency.char_indices();
    let (_, first) = name_chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }

    let mut name_len = 1;
    let mut name_end = first.len_utf8();
    let mut last_name_char = first;
    for (index, ch) in name_chars {
        if !(ch.is_ascii_alphanumeric() || ch == '.') {
            break;
        }

        name_len += 1;
        name_end = index + ch.len_utf8();
        last_name_char = ch;
    }
    if name_len < 2 || !last_name_char.is_ascii_alphanumeric() {
        return None;
    }

    let rest = &dependency[name_end..];
    let operator_start = name_end + rest.len() - rest.trim_start().len();

    let operator = if dependency[operator_start..].starts_with(">=") {
        ">="
    } else if dependency[operator_start..].starts_with("==") {
        "=="
    } else {
        return None;
    };

    let version_rest = &dependency[(operator_start + operator.len())..];
    let version_start =
        operator_start + operator.len() + version_rest.len() - version_rest.trim_start().len();

    let version = &dependency[version_start..];
    let mut version_chars = version.char_indices();
    let (_, first_version_char) = version_chars.next()?;
    if !first_version_char.is_ascii_digit() {
        return None;
    }

    let mut version_end = version_start + first_version_char.len_utf8();
    for (index, ch) in version_chars {
        if !(ch.is_ascii_digit() || ch == '.' || ch == '-') {
            return None;
        }

        version_end = version_start + index + ch.len_utf8();
    }

    Some((
        &dependency[..name_end],
        operator,
        &dependency[version_start..version_end],
    ))
}

/// Run the user's program in an ordinary R session pointed at `library`. The
/// program is a script file, `-` stdin source, or one or more inline expressions
/// evaluated via `Rscript -e`.
///
/// It runs as an ordinary `Rscript [Rscript-options...] (script.R | - | -e
/// expr...)` - its `.Renviron`, `.Rprofile` and site files are read unless the
/// forwarded Rscript options disable them. The resolved library is injected via
/// `R_LIBS`, which is *prepended* to `.libPaths()`: resolved dependencies take
/// precedence, while the user's other libraries remain available. (`R_LIBS` is
/// used rather than `R_LIBS_USER`, since a user `.Renviron` setting
/// `R_LIBS_USER` would override the latter.)
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
    source: RscriptSource<'_>,
    rscript_args: &[String],
    script_args: &[String],
    isolated: bool,
) -> Result<i32, Box<dyn Error>> {
    let mut cmd = Command::new(rscript);
    cmd.args(rscript_args);
    match source {
        RscriptSource::Script(script) => {
            cmd.arg(script);
        }
        RscriptSource::Expressions(expressions) => {
            for expr in expressions {
                cmd.arg("-e").arg(expr);
            }
        }
        RscriptSource::Stdin => {
            cmd.arg("-");
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

fn read_r_script_frontmatter_to_string(script: &Path) -> Result<String, Box<dyn Error>> {
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

/// The Rscript executable to use when no `r-version` is requested: `$IR_RSCRIPT`
/// if set, else rig's default R install, else bare `Rscript` resolved via `PATH`.
///
/// The rig step matters on Windows: `rig system make-links` puts only
/// `Rscript.bat` on `PATH`, which `std::process::Command` won't spawn. Resolving
/// the default install's real `Rscript.exe` from `rig list --json` avoids the
/// shim — the same mechanism the `--r-version` path already uses.
fn rscript_command() -> OsString {
    if let Some(rscript) = env::var_os("IR_RSCRIPT") {
        return rscript;
    }
    rig::default_rscript().unwrap_or_else(|| "Rscript".into())
}

/// The Rust-owned `ir` cache root. `IR_CACHE_DIR` overrides it; otherwise it
/// follows R's per-package cache layout from the process environment and
/// platform defaults.
fn ir_cache_dir() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = nonempty_env("IR_CACHE_DIR") {
        return Ok(PathBuf::from(path));
    }

    Ok(r_user_cache_dir()?.join("R").join("ir"))
}

fn nonempty_env(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

fn r_user_cache_dir() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = nonempty_env("R_USER_CACHE_DIR") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = nonempty_env("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path));
    }

    #[cfg(windows)]
    {
        if let Some(path) = nonempty_env("LOCALAPPDATA") {
            return Ok(PathBuf::from(path).join("R").join("cache"));
        }
        if let Some(path) = nonempty_env("USERPROFILE") {
            return Ok(PathBuf::from(path)
                .join("AppData")
                .join("Local")
                .join("R")
                .join("cache"));
        }
        Err(
            "cannot determine Windows cache directory; set IR_CACHE_DIR, R_USER_CACHE_DIR, XDG_CACHE_HOME, LOCALAPPDATA, or USERPROFILE"
                .into(),
        )
    }

    #[cfg(target_os = "macos")]
    {
        return Ok(home_dir()?
            .join("Library")
            .join("Caches")
            .join("org.R-project.R"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Ok(home_dir()?.join(".cache"))
    }
}

#[cfg(unix)]
fn home_dir() -> Result<PathBuf, Box<dyn Error>> {
    let home = nonempty_env("HOME").ok_or("cannot determine home directory")?;
    let home = PathBuf::from(home);
    Ok(fs::canonicalize(&home).unwrap_or(home))
}

fn tool_install_bin_dir() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = nonempty_env("IR_TOOL_BIN_DIR") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = nonempty_env("RAPP_BIN_DIR") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = nonempty_env("XDG_BIN_HOME") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = nonempty_env("XDG_DATA_HOME") {
        let data_home = PathBuf::from(path);
        return Ok(data_home
            .parent()
            .ok_or("XDG_DATA_HOME must have a parent directory")?
            .join("bin"));
    }

    #[cfg(unix)]
    {
        let home = nonempty_env("HOME")
            .ok_or("cannot determine launcher directory; set --bin-dir or IR_TOOL_BIN_DIR")?;
        Ok(PathBuf::from(home).join(".local").join("bin"))
    }

    #[cfg(not(unix))]
    {
        if let Some(path) = nonempty_env("LOCALAPPDATA") {
            return Ok(PathBuf::from(path)
                .join("Programs")
                .join("R")
                .join("ir")
                .join("bin"));
        }
        let home = nonempty_env("USERPROFILE")
            .ok_or("cannot determine launcher directory; set --bin-dir or IR_TOOL_BIN_DIR")?;
        Ok(PathBuf::from(home)
            .join("AppData")
            .join("Local")
            .join("Programs")
            .join("R")
            .join("ir")
            .join("bin"))
    }
}

fn launcher_target_path(bin_dir: &Path, name: &str) -> PathBuf {
    #[cfg(unix)]
    {
        bin_dir.join(name)
    }

    #[cfg(not(unix))]
    {
        bin_dir.join(format!("{name}.cmd"))
    }
}

fn tool_install_recovery_command(install: &ToolInstallArgs) -> String {
    let mut words = vec![
        "ir".to_string(),
        "tool".to_string(),
        "install".to_string(),
        "--force".to_string(),
    ];
    for dep in &install.with_deps {
        words.push("--with".to_string());
        words.push(command_word(dep));
    }
    if let Some(req) = &install.r_requirement {
        words.push("--r-version".to_string());
        words.push(command_word(req));
    }
    words.push(command_word(&install.package_ref));
    words.join(" ")
}

fn command_word(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '@'))
    {
        value.to_string()
    } else {
        sh_quote_str(value)
    }
}

#[cfg(unix)]
fn installed_launcher_contents(
    rscript: &OsStr,
    library: &Path,
    executable: &PackageExecutable,
    path_prefix: &[PathBuf],
    recovery_command: &str,
) -> Result<String, Box<dyn Error>> {
    let mut lines = vec![
        "#!/bin/sh".to_string(),
        "# Generated by `ir tool install`. Do not edit by hand.".to_string(),
        format!("IR_LIBRARY={}", sh_quote_path(library)?),
        "if [ ! -d \"$IR_LIBRARY\" ]; then".to_string(),
        "  printf '%s\\n' \"ir: missing ir cache library: $IR_LIBRARY\" >&2".to_string(),
        format!(
            "  printf '%s\\n' {} >&2",
            sh_quote_str(&format!(
                "ir: run `{recovery_command}` to recreate this launcher after `ir cache clean`."
            ))
        ),
        "  exit 1".to_string(),
        "fi".to_string(),
        "export R_LIBS=\"$IR_LIBRARY\"".to_string(),
        "export R_LIBS_USER=NULL".to_string(),
        format!(
            "export RAPP_LAUNCHER_NAME={}",
            sh_quote_str(&executable.name)
        ),
    ];

    if !path_prefix.is_empty() {
        let prefix = path_prefix
            .iter()
            .map(|path| sh_quote_path(path))
            .collect::<Result<Vec<_>, _>>()?
            .join(":");
        lines.push(format!("export PATH={prefix}${{PATH:+:$PATH}}"));
    }

    let mut cmd = vec!["exec".to_string(), sh_quote_os(rscript)];
    match executable.launcher {
        PackageLauncher::Rscript => {
            cmd.push(sh_quote_path(&executable.path)?);
        }
        PackageLauncher::Rapp => {
            cmd.push("-e".to_string());
            cmd.push(sh_quote_str("Rapp::run()"));
            cmd.push(sh_quote_path(&executable.path)?);
        }
    }
    cmd.push("\"$@\"".to_string());
    lines.push(cmd.join(" "));
    lines.push(String::new());
    Ok(lines.join("\n"))
}

#[cfg(not(unix))]
fn installed_launcher_contents(
    rscript: &OsStr,
    library: &Path,
    executable: &PackageExecutable,
    _path_prefix: &[PathBuf],
    recovery_command: &str,
) -> Result<String, Box<dyn Error>> {
    let mut cmd = vec![cmd_quote_os(rscript)];
    match executable.launcher {
        PackageLauncher::Rscript => {
            cmd.push(cmd_quote_path(&executable.path)?);
        }
        PackageLauncher::Rapp => {
            cmd.push("-e".to_string());
            cmd.push("Rapp::run()".to_string());
            cmd.push(cmd_quote_path(&executable.path)?);
        }
    }
    cmd.push("%*".to_string());
    let library = launcher_path_str(library)?;

    Ok(format!(
        "@echo off\r\n\
         :: Generated by `ir tool install`. Do not edit by hand.\r\n\
         setlocal\r\n\
         set \"IR_LIBRARY={}\"\r\n\
         if not exist \"%IR_LIBRARY%\" (\r\n\
         echo ir: missing ir cache library: %IR_LIBRARY% 1>&2\r\n\
         echo ir: run `{}` to recreate this launcher after `ir cache clean`. 1>&2\r\n\
         exit /b 1\r\n\
         )\r\n\
         set \"R_LIBS=%IR_LIBRARY%\"\r\n\
         set \"R_LIBS_USER=NULL\"\r\n\
         set \"RAPP_LAUNCHER_NAME={}\"\r\n\
         {}\r\n",
        library,
        recovery_command,
        executable.name,
        cmd.join(" ")
    ))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|e| {
        format!(
            "failed to mark launcher `{}` executable: {e}",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}

#[cfg(unix)]
fn sh_quote_path(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(sh_quote_str(&launcher_path_str(path)?))
}

#[cfg(unix)]
fn sh_quote_os(value: &OsStr) -> String {
    sh_quote_str(&value.to_string_lossy())
}

fn sh_quote_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(not(unix))]
fn cmd_quote_path(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(cmd_quote_str(&launcher_path_str(path)?))
}

fn launcher_path_str(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(std::path::absolute(path)
        .map_err(|e| {
            format!(
                "failed to normalize `{}` as an absolute path: {e}",
                path.display()
            )
        })?
        .to_string_lossy()
        .into_owned())
}

#[cfg(not(unix))]
fn cmd_quote_os(value: &OsStr) -> String {
    cmd_quote_str(&value.to_string_lossy())
}

#[cfg(not(unix))]
fn cmd_quote_str(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
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
