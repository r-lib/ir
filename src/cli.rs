use std::error::Error;
use std::path::PathBuf;

use clap::{Arg, ArgAction, Command as ClapCommand};

use crate::quarto::RenderSource;
use crate::runtime::nonempty_env;
use crate::script::RunSource;

pub(crate) fn root() -> ClapCommand {
    ClapCommand::new("ir")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Run self-describing R scripts")
        .arg_required_else_help(true)
        .subcommand(run_command())
        .subcommand(render_command())
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

fn render_command() -> ClapCommand {
    ClapCommand::new("render")
        .about("Render a Quarto document or script")
        .arg(
            Arg::new("with")
                .long("with")
                .value_name("PKG")
                .num_args(1)
                .action(ArgAction::Append)
                .help("Add a dependency for this render; may be repeated"),
        )
        .arg(
            Arg::new("r-version")
                .long("r-version")
                .value_name("SPEC")
                .num_args(1)
                .help("Select the R version for this render with rig"),
        )
        .arg(
            Arg::new("isolated")
                .long("isolated")
                .action(ArgAction::SetTrue)
                .help("Disable the user library for this render"),
        )
        .arg(
            Arg::new("vanilla")
                .long("vanilla")
                .action(ArgAction::SetTrue)
                .help("Run Quarto's knitr R with --vanilla"),
        )
        .arg(
            Arg::new("source")
                .value_name("SOURCE")
                .required(true)
                .help("Quarto document or script to render"),
        )
        .arg(
            Arg::new("quarto-args")
                .value_name("QUARTO_ARGS")
                .num_args(0..)
                .allow_hyphen_values(true)
                .trailing_var_arg(true)
                .help("Arguments passed to `quarto render`"),
        )
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

pub(crate) struct PackageExecTarget {
    pub(crate) package_ref: String,
    pub(crate) package_name: Option<String>,
    pub(crate) executable: String,
}

pub(crate) struct RunArgs {
    pub(crate) rscript_args: Vec<String>,
    pub(crate) with_deps: Vec<String>,
    pub(crate) r_requirement: Option<String>,
    pub(crate) source: RunSource,
    pub(crate) script_args: Vec<String>,
    pub(crate) isolated: bool,
}

pub(crate) struct RenderArgs {
    pub(crate) with_deps: Vec<String>,
    pub(crate) r_requirement: Option<String>,
    pub(crate) source: RenderSource,
    pub(crate) render_args: Vec<String>,
    pub(crate) isolated: bool,
    pub(crate) vanilla: bool,
}

pub(crate) struct ToolRunArgs {
    pub(crate) rscript_args: Vec<String>,
    pub(crate) with_deps: Vec<String>,
    pub(crate) r_requirement: Option<String>,
    pub(crate) target: PackageExecTarget,
    pub(crate) tool_args: Vec<String>,
}

pub(crate) struct ToolInstallArgs {
    pub(crate) package_ref: String,
    pub(crate) with_deps: Vec<String>,
    pub(crate) r_requirement: Option<String>,
    pub(crate) bin_dir: PathBuf,
    pub(crate) force: bool,
}

/// Split the leading region of `ir run`'s arguments into Rscript options,
/// `--with` dependency specs, an optional `--r-version` spec, and the program
/// source, with everything after the source treated as program args.
///
/// `-e <expr>`, `--with <spec>`, `--r-version <spec>` and `--isolated` are
/// `ir`-level flags handled here. Any other `-...` argument is an Rscript
/// option, forwarded verbatim to the user-code phase. Scanning stops at the
/// script path unless `-e` was given, in which case scanning stops after the
/// last `-e <expr>` pair. Everything after the source boundary is passed to
/// user code as program args.
pub(crate) fn parse_run_args(args: Vec<String>) -> Result<RunArgs, Box<dyn Error>> {
    let mut rscript_args = Vec::new();
    let mut with_deps = Vec::new();
    let mut r_requirement = None;
    let mut expressions = Vec::new();
    let mut isolated = false;
    let mut iter = args.into_iter();
    let mut positional = None;

    while let Some(arg) = iter.next() {
        if !expressions.is_empty() && arg != "-e" && arg != "--expr" && !arg.starts_with("--expr=")
        {
            positional = Some(arg);
            break;
        } else if arg == "-e" || arg == "--expr" {
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

/// Parse `ir render`, which resolves metadata for a Quarto source and then
/// forwards the source plus trailing args to `quarto render`.
pub(crate) fn parse_render_args(args: Vec<String>) -> Result<RenderArgs, Box<dyn Error>> {
    let mut with_deps = Vec::new();
    let mut r_requirement = None;
    let mut isolated = false;
    let mut vanilla = false;
    let mut iter = args.into_iter();
    let mut positional = None;

    while let Some(arg) = iter.next() {
        if arg == "-e" || arg == "--expr" || arg.starts_with("--expr=") {
            return Err("`-e` is only supported by `ir run`".into());
        } else if arg == "--from" || arg.starts_with("--from=") {
            return Err("`--from` is only supported by `ir tool run`".into());
        } else if arg == "--with" {
            let value = iter
                .next()
                .ok_or("`--with` requires a package (try `ir render --with dplyr report.qmd`)")?;
            push_with_deps(&mut with_deps, &value);
        } else if let Some(value) = arg.strip_prefix("--with=") {
            push_with_deps(&mut with_deps, value);
        } else if arg == "--r-version" {
            let value = iter.next().ok_or(
                "`--r-version` requires a version spec (try `ir render --r-version 4.5 report.qmd`)",
            )?;
            r_requirement = Some(value);
        } else if let Some(value) = arg.strip_prefix("--r-version=") {
            r_requirement = Some(value.to_string());
        } else if arg == "--isolated" {
            isolated = true;
        } else if arg == "--vanilla" {
            vanilla = true;
        } else if arg == "-" {
            return Err("`ir render` requires a source path, not stdin".into());
        } else if arg.starts_with('-') {
            return Err(format!("unexpected option `{arg}` before render source").into());
        } else {
            positional = Some(arg);
            break;
        }
    }

    let render_args: Vec<String> = iter.collect();
    let source =
        positional.ok_or("`ir render` requires a source path (try `ir render report.qmd`)")?;

    Ok(RenderArgs {
        with_deps,
        r_requirement,
        source: RenderSource::from_source_arg(source)?,
        render_args,
        isolated,
        vanilla,
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

pub(crate) fn is_package_executable_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains(':')
        && !name.chars().any(char::is_whitespace)
}

#[derive(Clone, Copy)]
pub(crate) enum ToolRunInvocation {
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
pub(crate) fn parse_tool_run_args(
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

pub(crate) fn parse_tool_install_args(
    args: Vec<String>,
) -> Result<ToolInstallArgs, Box<dyn Error>> {
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
