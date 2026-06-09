use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::{is_package_executable_name, ToolInstallArgs, ToolRunArgs};
use crate::runtime::{
    resolve_library, resolve_library_and_primary_package, rscript_for_spec, spawn_error,
};
use crate::spec::RuntimeSpec;

pub(crate) fn cmd_tool_run(run: &ToolRunArgs) -> Result<(), Box<dyn Error>> {
    let mut deps = vec![run.target.package_ref.clone()];
    deps.extend(run.with_deps.iter().cloned());
    let mut spec = RuntimeSpec {
        dependencies: deps,
        ..RuntimeSpec::default()
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

pub(crate) fn cmd_tool_install(install: &ToolInstallArgs) -> Result<(), Box<dyn Error>> {
    let mut spec = RuntimeSpec {
        dependencies: vec![install.package_ref.clone()],
        ..RuntimeSpec::default()
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
