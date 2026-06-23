use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use time::macros::format_description;
use time::{Date, OffsetDateTime};

use crate::driver;
use crate::lock::{resolver_lock_path, FileLock};
use crate::python;
use crate::quarto::{self, RenderSource};
use crate::resolve_cache;
use crate::rig;
use crate::script::RunSource;
use crate::spec::RuntimeSpec;

/// The R resolution driver, embedded at compile time so `ir` ships as one
/// self-contained binary while the source stays editable as real R.
const RESOLVE_DRIVER: &str = concat!(
    include_str!("../driver/tooling.R"),
    "\n",
    include_str!("../driver/resolve.R")
);
const TOOLING_RESTART_STATUS: i32 = 86;

/// Resolve dependencies for `source`, then run it against the resulting
/// library. Exits the process with the program's own exit code.
pub(crate) fn cmd_run(
    source: &RunSource,
    rscript_args: &[String],
    with_deps: &[String],
    r_selection: RSelectionArgs<'_>,
    exclude_newer: Option<&str>,
    script_args: &[String],
    isolated: bool,
) -> Result<(), Box<dyn Error>> {
    let mut spec = source.script_spec()?;
    apply_exclude_newer_override(&mut spec, exclude_newer)?;
    spec.dependencies.extend(with_deps.iter().cloned());
    let isolated = isolated || spec.isolated;
    let rscript = rscript_for_spec(&spec, r_selection)?;

    // Reuse a warm resolution marker, or launch the private resolver R session
    // to resolve deps and materialise the library.
    let library = resolve_library(&rscript, &spec)?;

    // Render the document, or run the user's program, in an isolated R session.
    let code = run_user_code(
        source,
        &rscript,
        library.as_deref(),
        rscript_args,
        script_args,
        isolated,
    )?;
    std::process::exit(code);
}

/// Resolve dependencies for `source`, then render it with Quarto. Exits the
/// process with Quarto's own exit code.
pub(crate) fn cmd_render(
    source: &RenderSource,
    with_deps: &[String],
    r_selection: RSelectionArgs<'_>,
    exclude_newer: Option<&str>,
    render_args: &[String],
    isolated: bool,
    vanilla: bool,
) -> Result<(), Box<dyn Error>> {
    let mut spec = source.script_spec()?;
    apply_exclude_newer_override(&mut spec, exclude_newer)?;
    spec.dependencies.extend(with_deps.iter().cloned());
    spec.quarto_render = true;
    let isolated = isolated || spec.isolated;
    let rscript = rscript_for_spec(&spec, r_selection)?;

    let library = resolve_library(&rscript, &spec)?;
    let cache_dir = ir_cache_dir()?;
    let python = python::resolve_env(&rscript, &cache_dir, spec.python.as_ref())?;
    let code = quarto::run(
        &rscript,
        library.as_deref(),
        python.as_deref(),
        source.path(),
        render_args,
        isolated,
        vanilla,
    )?;
    std::process::exit(code);
}

enum RSelection {
    Version(String),
    Rscript(OsString),
}

pub(crate) struct RSelectionArgs<'a> {
    pub(crate) r_requirement: Option<&'a str>,
    pub(crate) rscript: Option<&'a str>,
}

pub(crate) fn rscript_for_spec(
    spec: &RuntimeSpec,
    cli: RSelectionArgs<'_>,
) -> Result<OsString, Box<dyn Error>> {
    if let Some(selection) = cli_r_selection(cli.r_requirement, cli.rscript)? {
        return resolve_r_selection(selection, spec.exclude_newer.as_deref());
    }
    if let Some(selection) = env_r_selection()? {
        return resolve_r_selection(selection, spec.exclude_newer.as_deref());
    }
    if let Some(selection) = frontmatter_r_selection(spec)? {
        return resolve_r_selection(selection, spec.exclude_newer.as_deref());
    }
    if let Some(exclude_newer) = &spec.exclude_newer {
        return rig::resolve_rscript_for_exclude_newer(exclude_newer);
    }

    Ok(rscript_command())
}

fn cli_r_selection(
    r_requirement: Option<&str>,
    rscript: Option<&str>,
) -> Result<Option<RSelection>, Box<dyn Error>> {
    match (r_requirement, rscript) {
        (Some(_), Some(_)) => Err("cannot set both `--r-version` and `--rscript`".into()),
        (Some(req), None) => Ok(Some(RSelection::Version(nonempty_cli_value(
            "--r-version",
            req,
        )?))),
        (None, Some(rscript)) => Ok(Some(RSelection::Rscript(OsString::from(
            nonempty_cli_value("--rscript", rscript)?,
        )))),
        (None, None) => Ok(None),
    }
}

fn env_r_selection() -> Result<Option<RSelection>, Box<dyn Error>> {
    let rscript = nonempty_env("IR_RSCRIPT");
    let r_version = env_optional_trimmed_string("IR_R_VERSION")?;
    match (r_version, rscript) {
        (Some(_), Some(_)) => Err("cannot set both `IR_R_VERSION` and `IR_RSCRIPT`".into()),
        (Some(req), None) => Ok(Some(RSelection::Version(req))),
        (None, Some(rscript)) => Ok(Some(RSelection::Rscript(rscript))),
        (None, None) => Ok(None),
    }
}

fn frontmatter_r_selection(spec: &RuntimeSpec) -> Result<Option<RSelection>, Box<dyn Error>> {
    match (&spec.r_requirement, &spec.rscript) {
        (Some(_), Some(_)) => Err("frontmatter cannot set both `r-version` and `rscript`".into()),
        (Some(req), None) => Ok(Some(RSelection::Version(req.clone()))),
        (None, Some(rscript)) => Ok(Some(RSelection::Rscript(OsString::from(rscript)))),
        (None, None) => Ok(None),
    }
}

fn resolve_r_selection(
    selection: RSelection,
    exclude_newer: Option<&str>,
) -> Result<OsString, Box<dyn Error>> {
    match selection {
        RSelection::Version(req) => rig::resolve_rscript(&req, exclude_newer),
        RSelection::Rscript(rscript) => Ok(resolve_rscript_command(&rscript)),
    }
}

fn nonempty_cli_value(name: &str, value: &str) -> Result<String, Box<dyn Error>> {
    if value.is_empty() {
        return Err(format!("`{name}` must not be empty").into());
    }
    Ok(value.to_string())
}

fn env_optional_trimmed_string(name: &str) -> Result<Option<String>, Box<dyn Error>> {
    let Some(value) = env::var_os(name) else {
        return Ok(None);
    };
    let value = env_string(name, value)?;
    let value = value.trim();
    Ok((!value.is_empty()).then(|| value.to_string()))
}

fn apply_exclude_newer_override(
    spec: &mut RuntimeSpec,
    cli_exclude_newer: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    if spec.python.is_some() {
        apply_python_exclude_newer_override(spec, cli_exclude_newer)?;
        spec.exclude_newer = None;
        return Ok(());
    }

    if let Some(exclude_newer) = cli_exclude_newer {
        spec.exclude_newer = normalize_exclude_newer_override(exclude_newer)?;
        return Ok(());
    }

    if let Some(exclude_newer) = env::var_os("IR_EXCLUDE_NEWER") {
        let exclude_newer = env_string("IR_EXCLUDE_NEWER", exclude_newer)?;
        spec.exclude_newer = normalize_exclude_newer_override(&exclude_newer)?;
        return Ok(());
    }

    if let Some(exclude_newer) = spec.exclude_newer.take() {
        spec.exclude_newer = normalize_exclude_newer_override(&exclude_newer)?;
    }

    Ok(())
}

fn apply_python_exclude_newer_override(
    spec: &mut RuntimeSpec,
    cli_exclude_newer: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let Some(python) = spec.python.as_mut() else {
        return Ok(());
    };

    if let Some(exclude_newer) = cli_exclude_newer {
        python.exclude_newer = Some(exclude_newer.to_string());
        return Ok(());
    }

    if let Some(exclude_newer) = env::var_os("IR_EXCLUDE_NEWER") {
        let exclude_newer = env_string("IR_EXCLUDE_NEWER", exclude_newer)?;
        python.exclude_newer = Some(exclude_newer);
        return Ok(());
    }

    Ok(())
}

fn normalize_exclude_newer_override(value: &str) -> Result<Option<String>, Box<dyn Error>> {
    let value = value.trim();
    if value.is_empty() || is_future_iso_date(value) {
        return Ok(None);
    }
    Ok(Some(value.to_string()))
}

fn is_future_iso_date(value: &str) -> bool {
    let format = format_description!("[year]-[month]-[day]");
    let Ok(date) = Date::parse(value, &format) else {
        return false;
    };
    date > OffsetDateTime::now_utc().date()
}

/// Return a cached materialised library path, or run the embedded driver in a
/// private R session to resolve and materialise it. Shorthand version specs in
/// `spec` are normalized before cache keying and resolver input; other package
/// refs are passed through.
pub(crate) fn resolve_library(
    rscript: &OsStr,
    spec: &RuntimeSpec,
) -> Result<Option<PathBuf>, Box<dyn Error>> {
    Ok(resolve_library_inner(rscript, spec, false)?.library)
}

pub(crate) fn resolve_library_and_primary_package(
    rscript: &OsStr,
    spec: &RuntimeSpec,
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
    spec: &RuntimeSpec,
    primary_package: bool,
) -> Result<ResolvedLibrary, Box<dyn Error>> {
    let dependencies = normalized_dependencies(&spec.dependencies);
    let cache_dir = ir_cache_dir()?;
    let resolution_cache_paths = resolve_cache::paths(
        &cache_dir,
        rscript,
        &dependencies,
        spec.exclude_newer.as_deref(),
        spec.quarto_render,
    )?;
    if let Some(resolved) = resolve_cache::read(resolution_cache_paths.as_ref(), primary_package)? {
        return Ok(ResolvedLibrary {
            library: Some(resolved.library),
            primary_package: resolved.primary_package,
        });
    }

    let _resolver_lock = FileLock::acquire(&resolver_lock_path(&cache_dir))?;
    if let Some(resolved) = resolve_cache::read(resolution_cache_paths.as_ref(), primary_package)? {
        return Ok(ResolvedLibrary {
            library: Some(resolved.library),
            primary_package: resolved.primary_package,
        });
    }

    let driver = driver::cached_path(&cache_dir, driver::RESOLVE_FILE, RESOLVE_DRIVER)?;
    let tmp = env::temp_dir();
    let result_file = unique_path(&tmp, "ir-libpath", "txt");
    let package_result_file = primary_package.then(|| unique_path(&tmp, "ir-package", "txt"));
    let restart_file = unique_path(&tmp, "ir-tooling-restart", "txt");

    let mut status = None;
    for attempt in 0..=1 {
        let _ = fs::remove_file(&result_file);
        let _ = fs::remove_file(&restart_file);
        if let Some(package_result_file) = &package_result_file {
            let _ = fs::remove_file(package_result_file);
        }

        let mut cmd = Command::new(rscript);
        cmd.arg(&driver)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .env("IR_RESOLVE_RESULT_FILE", &result_file)
            .env("IR_TOOLING_RESTART_FILE", &restart_file)
            .env("IR_CACHE_DIR", &cache_dir)
            // pak suppresses progress in noninteractive Rscript unless this is set.
            // Resolution cache hits return before pak, so this adds no cache-hit pak output.
            .env("R_PKG_SHOW_PROGRESS", "true")
            // The RuntimeSpec owns snapshot selection. Do not let unsupported
            // commands accidentally reach the resolver through ambient process env.
            .env_remove("IR_EXCLUDE_NEWER");
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
        if spec.quarto_render {
            // Distinct from IR_QUARTO (the quarto executable, read in quarto.rs):
            // this flag tells the resolver a Quarto render needs rmarkdown.
            cmd.env("IR_QUARTO_RENDER", "1");
        }

        let mut child = cmd.spawn().map_err(|e| spawn_error(rscript, e))?;
        let stdin_result = (|| -> Result<(), Box<dyn Error>> {
            let mut stdin = child.stdin.take().ok_or("failed to open resolver stdin")?;
            for dependency in &dependencies {
                writeln!(stdin, "{dependency}")?;
            }
            Ok(())
        })();
        let current_status = child
            .wait()
            .map_err(|e| format!("failed to wait for dependency resolver: {e}"))?;

        if tooling_restart_requested(&current_status, &restart_file) {
            if attempt == 0 {
                continue;
            }
            return Err(
                repeated_tooling_restart_error("dependency resolver", &restart_file).into(),
            );
        }

        stdin_result?;
        status = Some(current_status);
        break;
    }
    let status = status.ok_or("dependency resolver did not run")?;

    let result = fs::read_to_string(&result_file).unwrap_or_default();
    let _ = fs::remove_file(&result_file);
    let _ = fs::remove_file(&restart_file);
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

fn tooling_restart_requested(status: &ExitStatus, restart_file: &Path) -> bool {
    status.code() == Some(TOOLING_RESTART_STATUS) && restart_file.exists()
}

fn repeated_tooling_restart_error(context: &str, restart_file: &Path) -> String {
    let packages = fs::read_to_string(restart_file).unwrap_or_default();
    let packages = packages.trim();
    if packages.is_empty() {
        format!("{context} repeatedly requested a tooling restart")
    } else {
        format!("{context} repeatedly requested a tooling restart for {packages}")
    }
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

enum RscriptSource<'a> {
    Script(&'a Path),
    Expressions(&'a [String]),
    Stdin,
}

fn run_user_code(
    source: &RunSource,
    rscript: &OsStr,
    library: Option<&Path>,
    rscript_args: &[String],
    script_args: &[String],
    isolated: bool,
) -> Result<i32, Box<dyn Error>> {
    match source {
        RunSource::Script(script) => run_script(
            rscript,
            library,
            RscriptSource::Script(script),
            rscript_args,
            script_args,
            isolated,
        ),
        RunSource::Expressions(expressions) => run_script(
            rscript,
            library,
            RscriptSource::Expressions(expressions),
            rscript_args,
            script_args,
            isolated,
        ),
        RunSource::Stdin => run_script(
            rscript,
            library,
            RscriptSource::Stdin,
            rscript_args,
            script_args,
            isolated,
        ),
    }
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

/// The default Rscript executable to use when R is not selected explicitly.
pub(crate) fn rscript_command() -> OsString {
    resolve_rscript_command(OsStr::new("Rscript"))
}

pub(crate) fn resolve_rscript_command(command: &OsStr) -> OsString {
    resolve_command_path(command).unwrap_or_else(|| command.to_os_string())
}

fn resolve_command_path(command: &OsStr) -> Option<OsString> {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.is_file().then(|| absolute_path(path).into_os_string());
    }

    find_on_path(command).map(PathBuf::into_os_string)
}

fn find_on_path(command: &OsStr) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(command);
        if is_runnable_file(&candidate) {
            return Some(selected_command_path(&candidate));
        }

        #[cfg(windows)]
        if Path::new(command).extension().is_none() {
            let pathext = env::var_os("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
            let command = command.to_string_lossy();
            for ext in pathext.to_string_lossy().split(';') {
                let candidate = dir.join(format!("{command}{ext}"));
                if is_runnable_file(&candidate) {
                    return Some(selected_command_path(&candidate));
                }
            }
        }
    }

    None
}

#[cfg(unix)]
fn is_runnable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_runnable_file(path: &Path) -> bool {
    path.is_file()
        && path.extension().and_then(OsStr::to_str).is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "com" | "exe" | "bat" | "cmd"
            )
        })
}

#[cfg(not(any(unix, windows)))]
fn is_runnable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(unix)]
fn selected_command_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| absolute_path(path))
}

#[cfg(windows)]
fn selected_command_path(path: &Path) -> PathBuf {
    resolved_windows_rscript_batch_target(path).unwrap_or_else(|| absolute_path(path))
}

#[cfg(not(any(unix, windows)))]
fn selected_command_path(path: &Path) -> PathBuf {
    absolute_path(path)
}

#[cfg(windows)]
fn resolved_windows_rscript_batch_target(path: &Path) -> Option<PathBuf> {
    let ext = path.extension().and_then(OsStr::to_str)?;
    if !matches!(ext.to_ascii_lowercase().as_str(), "bat" | "cmd") {
        return None;
    }

    let contents = fs::read_to_string(path).ok()?;
    for line in contents.lines() {
        let line = line.trim_start();
        if line.is_empty()
            || line.starts_with("::")
            || line.to_ascii_lowercase().starts_with("rem ")
        {
            continue;
        }
        let line = line.strip_prefix('@').unwrap_or(line).trim_start();
        let Some(rest) = line.strip_prefix('"') else {
            continue;
        };
        let (target, _) = rest.split_once('"')?;
        return windows_rscript_target(Path::new(target));
    }
    None
}

#[cfg(windows)]
fn windows_rscript_target(target: &Path) -> Option<PathBuf> {
    if target.is_file() && is_windows_rscript_target(target) {
        return Some(absolute_path(target));
    }
    let exe = target.with_extension("exe");
    (exe.is_file() && is_windows_rscript_target(&exe)).then(|| absolute_path(&exe))
}

#[cfg(windows)]
fn is_windows_rscript_target(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                "rscript" | "rscript.exe"
            )
        })
}

fn absolute_path(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The Rust-owned `ir` cache root. `IR_CACHE_DIR` overrides it; otherwise it
/// follows R's per-package cache layout from the process environment and
/// platform defaults.
pub(crate) fn ir_cache_dir() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = nonempty_env("IR_CACHE_DIR") {
        return Ok(PathBuf::from(path));
    }

    Ok(r_user_cache_dir()?.join("R").join("ir"))
}

pub(crate) fn nonempty_env(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

fn env_string(name: &str, value: OsString) -> Result<String, Box<dyn Error>> {
    value
        .into_string()
        .map_err(|_| format!("`{name}` must be valid UTF-8").into())
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

pub(crate) fn count_files(path: &Path) -> io::Result<u64> {
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
pub(crate) fn spawn_error(rscript: &OsStr, err: io::Error) -> String {
    if err.kind() == io::ErrorKind::NotFound {
        format!(
            "could not find `{}` on PATH. Install R, set IR_RSCRIPT, or pass --rscript.",
            rscript.to_string_lossy()
        )
    } else {
        format!("failed to launch `{}`: {err}", rscript.to_string_lossy())
    }
}
