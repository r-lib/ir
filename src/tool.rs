use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

use saphyr::Yaml;

use crate::cli::{is_package_executable_name, ToolInstallArgs, ToolRunArgs};
#[cfg(any(target_os = "macos", not(unix)))]
use crate::runtime::nonempty_env;
use crate::runtime::{
    resolve_library_and_primary_package, rscript_for_spec, spawn_error, RSelectionArgs,
};
use crate::spec::{load_first_yaml_document, RuntimeSpec};

pub(crate) fn cmd_tool_run(run: &ToolRunArgs) -> Result<(), Box<dyn Error>> {
    let mut deps = vec![run.target.package_ref.clone()];
    deps.extend(run.with_deps.iter().cloned());
    let spec = RuntimeSpec {
        dependencies: deps,
        ..RuntimeSpec::default()
    };

    let rscript = rscript_for_spec(
        &spec,
        RSelectionArgs {
            r_requirement: run.r_requirement.as_deref(),
            rscript: run.rscript.as_deref(),
        },
    )?;
    let (library, package_name) = resolve_library_and_primary_package(&rscript, &spec)?;
    let executable = find_package_executable(&library, &package_name, &run.target.executable)?;
    let code = run_package_executable(
        &rscript,
        &library,
        &executable,
        &run.rscript_args,
        &run.tool_args,
    )?;
    std::process::exit(code);
}

pub(crate) fn cmd_tool_install(install: &ToolInstallArgs) -> Result<(), Box<dyn Error>> {
    let mut spec = RuntimeSpec {
        dependencies: vec![install.package_ref.clone()],
        ..RuntimeSpec::default()
    };
    spec.dependencies.extend(install.with_deps.iter().cloned());

    let rscript = rscript_for_spec(
        &spec,
        RSelectionArgs {
            r_requirement: install.r_requirement.as_deref(),
            rscript: install.rscript.as_deref(),
        },
    )?;
    let (library, package_name) = resolve_library_and_primary_package(&rscript, &spec)?;
    let executables = discover_package_executables(&library, &package_name)?;
    if executables.is_empty() {
        return Err(format!(
            "package `{}` does not expose supported executables in `{}`",
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
    if install.setup_bin_dir_on_path {
        ensure_launcher_dir_on_path(&install.bin_dir)?;
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
    package: &str,
    executable: &str,
) -> Result<PackageExecutable, Box<dyn Error>> {
    let exec_dir = library.join(package).join("exec");
    find_package_executable_in_dir(&exec_dir, executable)?.ok_or_else(|| {
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
    })
}

fn find_package_executable_in_dir(
    exec_dir: &Path,
    executable: &str,
) -> Result<Option<PackageExecutable>, Box<dyn Error>> {
    if !exec_dir.is_dir() {
        return Ok(None);
    }

    let mut matches: Vec<_> = package_executables_in_dir(exec_dir)?
        .into_iter()
        .filter(|candidate| candidate.name == executable)
        .collect();

    matches.sort_by(|a, b| a.path.cmp(&b.path));
    match matches.len() {
        0 => Ok(None),
        1 => Ok(Some(matches.remove(0))),
        _ => Err(format!(
            "multiple package executables map to launcher `{executable}` in `{}`",
            exec_dir.display()
        )
        .into()),
    }
}

struct PackageExecutable {
    name: String,
    path: PathBuf,
    launcher: PackageLauncher,
    rscript_args: Vec<String>,
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

    let mut executables = package_executables_in_dir(&exec_dir)?;
    reject_duplicate_launcher_names(&executables, &exec_dir)?;

    executables.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(executables)
}

fn package_executables_in_dir(exec_dir: &Path) -> Result<Vec<PackageExecutable>, Box<dyn Error>> {
    let mut executables = Vec::new();
    let rapp_frontend = if exec_dir
        .parent()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
        == Some("Rapp")
    {
        let path = exec_dir.join("Rapp");
        path.is_file().then_some(path)
    } else {
        None
    };
    if let Some(path) = &rapp_frontend {
        executables.push(rapp_frontend_executable(path.to_path_buf()));
    }

    for entry in fs::read_dir(exec_dir)
        .map_err(|e| format!("cannot read exec directory `{}`: {e}", exec_dir.display()))?
    {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        if rapp_frontend.is_some() && is_rapp_frontend_alias(&path) {
            continue;
        }
        let Some(executable) = package_executable_from_discovered_path(&path)? else {
            continue;
        };
        executables.push(executable);
    }
    Ok(executables)
}

fn is_rapp_frontend_alias(path: &Path) -> bool {
    let name = if path
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(is_package_executable_launcher_suffix)
    {
        path.file_stem()
    } else {
        path.file_name()
    };
    name.and_then(OsStr::to_str) == Some("Rapp")
}

fn rapp_frontend_executable(path: PathBuf) -> PackageExecutable {
    PackageExecutable {
        name: "Rapp".to_string(),
        path,
        launcher: PackageLauncher::RappFrontend,
        rscript_args: Vec::new(),
    }
}

fn reject_duplicate_launcher_names(
    executables: &[PackageExecutable],
    exec_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    for (index, executable) in executables.iter().enumerate() {
        if executables[..index]
            .iter()
            .any(|known| known.name == executable.name)
        {
            return Err(format!(
                "multiple package executables map to launcher `{}` in `{}`",
                executable.name,
                exec_dir.display()
            )
            .into());
        }
    }
    Ok(())
}

fn package_executable_from_discovered_path(
    path: &Path,
) -> Result<Option<PackageExecutable>, Box<dyn Error>> {
    let Some(launcher) = package_executable_launcher_kind(path)? else {
        return Ok(None);
    };
    package_executable_from_path_and_launcher(path, launcher).map(Some)
}

fn package_executable_from_path_and_launcher(
    path: &Path,
    launcher: PackageLauncher,
) -> Result<PackageExecutable, Box<dyn Error>> {
    let package = package_executable_package(path)?;
    let metadata = package_launcher_metadata(path, &package, launcher)?;
    let name = package_executable_launcher_name(path, metadata.name)?;
    Ok(PackageExecutable {
        name,
        path: path.to_path_buf(),
        launcher,
        rscript_args: metadata.rscript_args,
    })
}

fn package_executable_package(path: &Path) -> Result<String, Box<dyn Error>> {
    let package = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
        .ok_or_else(|| {
            format!(
                "package executable `{}` is not under a package exec directory",
                path.display()
            )
        })?;
    Ok(package.to_string())
}

fn package_executable_launcher_name(
    path: &Path,
    metadata_name: Option<String>,
) -> Result<String, Box<dyn Error>> {
    let name = if let Some(name) = metadata_name {
        name
    } else if path
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(is_package_executable_launcher_suffix)
    {
        path.file_stem()
            .and_then(OsStr::to_str)
            .ok_or_else(|| format!("package executable `{}` is not valid UTF-8", path.display()))?
            .to_string()
    } else {
        path.file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| format!("package executable `{}` is not valid UTF-8", path.display()))?
            .to_string()
    };

    if !is_package_executable_name(&name) {
        return Err(format!(
            "package executable `{}` maps to unsupported launcher name `{name}`",
            path.display()
        )
        .into());
    }

    Ok(name)
}

#[cfg(unix)]
fn is_package_executable_launcher_suffix(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("R")
}

#[cfg(not(unix))]
fn is_package_executable_launcher_suffix(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("R")
        || ext.eq_ignore_ascii_case("cmd")
        || ext.eq_ignore_ascii_case("bat")
}

struct PackageLauncherMetadata {
    name: Option<String>,
    rscript_args: Vec<String>,
}

fn package_launcher_metadata(
    path: &Path,
    package: &str,
    package_launcher: PackageLauncher,
) -> Result<PackageLauncherMetadata, Box<dyn Error>> {
    if matches!(package_launcher, PackageLauncher::Direct) {
        return Ok(PackageLauncherMetadata {
            name: None,
            rscript_args: Vec::new(),
        });
    }

    let frontmatter = read_rapp_frontmatter_to_string(path)?;
    if frontmatter.trim().is_empty() {
        return package_launcher_metadata_from_mapping(None, None, path, package, package_launcher);
    }

    let Some(doc) = load_first_yaml_document(&frontmatter, "launcher frontmatter")? else {
        return package_launcher_metadata_from_mapping(None, None, path, package, package_launcher);
    };
    if doc.is_null() {
        return package_launcher_metadata_from_mapping(None, None, path, package, package_launcher);
    }
    if !doc.is_mapping() {
        return Err(format!(
            "launcher frontmatter in `{}` must be a YAML mapping",
            path.display()
        )
        .into());
    }

    let launcher = launcher_frontmatter_mapping(&doc, path)?;
    let top_level_name = launcher_optional_string(&doc, "name", path)?;
    package_launcher_metadata_from_mapping(
        launcher,
        top_level_name,
        path,
        package,
        package_launcher,
    )
}

fn package_launcher_metadata_from_mapping(
    launcher: Option<&Yaml<'_>>,
    top_level_name: Option<String>,
    path: &Path,
    package: &str,
    package_launcher: PackageLauncher,
) -> Result<PackageLauncherMetadata, Box<dyn Error>> {
    let name = match launcher {
        Some(launcher) => launcher_optional_string(launcher, "name", path)?.or(top_level_name),
        None => top_level_name,
    };
    let rscript_args = match package_launcher {
        PackageLauncher::Rapp => launcher_rscript_args(launcher, path, package, true)?,
        PackageLauncher::Rscript if launcher.is_some() => {
            launcher_rscript_args(launcher, path, package, false)?
        }
        PackageLauncher::Direct | PackageLauncher::Rscript | PackageLauncher::RappFrontend => {
            Vec::new()
        }
    };

    Ok(PackageLauncherMetadata { name, rscript_args })
}

fn launcher_frontmatter_mapping<'a, 'input>(
    doc: &'a Yaml<'input>,
    path: &Path,
) -> Result<Option<&'a Yaml<'input>>, Box<dyn Error>> {
    let Some(launcher) = doc.as_mapping_get("launcher") else {
        return Ok(None);
    };
    if launcher.is_null() {
        return Ok(None);
    }
    if !launcher.is_mapping() {
        return Err(format!(
            "launcher frontmatter `launcher` in `{}` must be a YAML mapping",
            path.display()
        )
        .into());
    }
    Ok(Some(launcher))
}

fn read_rapp_frontmatter_to_string(path: &Path) -> Result<String, Box<dyn Error>> {
    let file = File::open(path)
        .map_err(|e| format!("cannot read package executable `{}`: {e}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut frontmatter = String::new();
    let mut line = String::new();

    reader.read_line(&mut line)?;
    if line.starts_with("#!") {
        line.clear();
        reader.read_line(&mut line)?;
    }

    while let Some(rest) = rapp_hashpipe_content(&line) {
        frontmatter.push_str(rest);
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
    }

    Ok(frontmatter)
}

fn rapp_hashpipe_content(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("#| ")
}

fn launcher_rscript_args(
    launcher: Option<&Yaml<'_>>,
    path: &Path,
    package: &str,
    use_package_default_packages: bool,
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut args = Vec::new();
    if let Some(launcher) = launcher {
        for (key, arg) in [
            ("vanilla", "--vanilla"),
            ("no-environ", "--no-environ"),
            ("no-site-file", "--no-site-file"),
            ("no-init-file", "--no-init-file"),
            ("restore", "--restore"),
            ("save", "--save"),
            ("verbose", "--verbose"),
        ] {
            if launcher_optional_bool(launcher, key, path)? {
                args.push(arg.to_string());
            }
        }
    }

    if let Some(default_packages) =
        launcher_default_packages(launcher, path, package, use_package_default_packages)?
    {
        args.push(format!("--default-packages={default_packages}"));
    }

    Ok(args)
}

fn launcher_optional_string(
    mapping: &Yaml<'_>,
    key: &str,
    path: &Path,
) -> Result<Option<String>, Box<dyn Error>> {
    let Some(value) = launcher_mapping_get(mapping, key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    let Some(value) = value.as_str() else {
        return Err(format!(
            "launcher frontmatter `{key}` in `{}` must be a string",
            path.display()
        )
        .into());
    };
    Ok(Some(value.trim().to_string()))
}

fn launcher_optional_bool(
    mapping: &Yaml<'_>,
    key: &str,
    path: &Path,
) -> Result<bool, Box<dyn Error>> {
    let Some(value) = launcher_mapping_get(mapping, key) else {
        return Ok(false);
    };
    if value.is_null() {
        return Ok(false);
    }

    value.as_bool().ok_or_else(|| {
        format!(
            "launcher frontmatter `{key}` in `{}` must be a boolean",
            path.display()
        )
        .into()
    })
}

fn launcher_default_packages(
    mapping: Option<&Yaml<'_>>,
    path: &Path,
    package: &str,
    use_package_default_packages: bool,
) -> Result<Option<String>, Box<dyn Error>> {
    let Some(mapping) = mapping else {
        return Ok(use_package_default_packages.then(|| format!("base,{package}")));
    };
    let Some(value) = launcher_mapping_get(mapping, "default-packages") else {
        return Ok(use_package_default_packages.then(|| format!("base,{package}")));
    };
    if value.is_null() {
        return Ok(Some("NULL".to_string()));
    }
    if let Some(value) = value.as_str() {
        return nonempty_launcher_string(value, "default-packages", path).map(Some);
    }

    let Some(values) = value.as_vec() else {
        return Err(format!(
            "launcher frontmatter `default-packages` in `{}` must be a string or sequence",
            path.display()
        )
        .into());
    };

    let mut packages = Vec::new();
    for value in values {
        let Some(value) = value.as_str() else {
            return Err(format!(
                "launcher frontmatter `default-packages` entries in `{}` must be strings",
                path.display()
            )
            .into());
        };
        packages.push(nonempty_launcher_string(value, "default-packages", path)?);
    }
    Ok(Some(packages.join(",")))
}

fn nonempty_launcher_string(value: &str, key: &str, path: &Path) -> Result<String, Box<dyn Error>> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!(
            "launcher frontmatter `{key}` in `{}` must not contain empty strings",
            path.display()
        )
        .into());
    }
    Ok(value.to_string())
}

fn launcher_mapping_get<'a, 'input>(
    mapping: &'a Yaml<'input>,
    key: &str,
) -> Option<&'a Yaml<'input>> {
    if let Some(value) = mapping.as_mapping_get(key) {
        return Some(value);
    }
    let normalized = key.replace('-', "_");
    if normalized == key {
        None
    } else {
        mapping.as_mapping_get(normalized.as_str())
    }
}

#[cfg(any(target_os = "macos", not(unix)))]
fn tool_install_path_setup_enabled() -> bool {
    nonempty_env("IR_NO_MODIFY_PATH").is_none() && nonempty_env("RAPP_NO_MODIFY_PATH").is_none()
}

#[cfg(target_os = "macos")]
fn ensure_launcher_dir_on_path(bin_dir: &Path) -> Result<(), Box<dyn Error>> {
    if !tool_install_path_setup_enabled() {
        return Ok(());
    }

    let default_bin_dir = macos_default_launcher_dir()?;
    if !same_existing_path(bin_dir, &default_bin_dir) {
        return Ok(());
    }

    let bin_dir = fs::canonicalize(bin_dir).map_err(|e| {
        format!(
            "cannot resolve launcher directory `{}`: {e}",
            bin_dir.display()
        )
    })?;
    if path_has_dir(&bin_dir) {
        return Ok(());
    }

    let zprofile = macos_zprofile()?;
    let display = macos_zprofile_display(&zprofile);
    let lines = macos_path_lines();
    if profile_has_lines(&zprofile, lines)? {
        return Ok(());
    }

    if let Err(e) = append_macos_path_lines(&zprofile, lines) {
        eprintln!("Could not add ~/.local/bin to PATH in {display}: {e}");
        return Ok(());
    }

    eprintln!("Added ~/.local/bin to PATH in {display}.");
    eprintln!("Restart your shell, or run:\n\n  source {display}");
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_default_launcher_dir() -> Result<PathBuf, Box<dyn Error>> {
    let home = nonempty_env("HOME").ok_or("cannot determine home directory for PATH setup")?;
    Ok(PathBuf::from(home).join(".local").join("bin"))
}

#[cfg(target_os = "macos")]
fn same_existing_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(target_os = "macos")]
fn macos_zprofile() -> Result<PathBuf, Box<dyn Error>> {
    let dir = nonempty_env("ZDOTDIR")
        .or_else(|| nonempty_env("HOME"))
        .ok_or("cannot determine zsh profile path for PATH setup")?;
    Ok(PathBuf::from(dir).join(".zprofile"))
}

#[cfg(target_os = "macos")]
fn macos_zprofile_display(zprofile: &Path) -> String {
    if let Some(home) = nonempty_env("HOME") {
        if zprofile == PathBuf::from(home).join(".zprofile") {
            return "~/.zprofile".to_string();
        }
    }
    zprofile.display().to_string()
}

#[cfg(target_os = "macos")]
fn macos_path_lines() -> &'static str {
    concat!(
        "case \":$PATH:\" in\n",
        "  *:\"$HOME/.local/bin\":*) ;;\n",
        "  *) export PATH=\"$HOME/.local/bin:$PATH\" ;;\n",
        "esac\n"
    )
}

#[cfg(target_os = "macos")]
fn profile_has_lines(profile: &Path, lines: &str) -> Result<bool, Box<dyn Error>> {
    if !profile.exists() {
        return Ok(false);
    }
    let profile = fs::read_to_string(profile)?;
    Ok(profile.contains(lines))
}

#[cfg(target_os = "macos")]
fn append_macos_path_lines(profile: &Path, lines: &str) -> Result<(), Box<dyn Error>> {
    use std::io::Write as _;

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(profile)?;
    file.write_all(b"\n")?;
    file.write_all(lines.as_bytes())?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn path_has_dir(dir: &Path) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|entry| {
        entry == dir
            || fs::canonicalize(&entry)
                .map(|entry| entry == dir)
                .unwrap_or(false)
    })
}

#[cfg(not(unix))]
fn ensure_launcher_dir_on_path(bin_dir: &Path) -> Result<(), Box<dyn Error>> {
    if !tool_install_path_setup_enabled() {
        return Ok(());
    }

    let bin_dir = windows_path_entry_str(bin_dir)?;
    let script = r#"
$ErrorActionPreference = "Stop"
$InstallDir = $env:IR_NEW_PATH_ENTRY
function Normalize-PathEntry([string]$PathEntry) {
  $PathEntry = [Environment]::ExpandEnvironmentVariables($PathEntry)
  try { $PathEntry = (Resolve-Path -LiteralPath $PathEntry -ErrorAction Stop).ProviderPath } catch {}
  try { $PathEntry = [System.IO.Path]::GetFullPath($PathEntry) } catch {}
  return $PathEntry.TrimEnd('\').ToLowerInvariant()
}
function Get-ShortPathEntry([string]$PathEntry) {
  try {
    $FileSystem = New-Object -ComObject Scripting.FileSystemObject
    $ShortPath = $FileSystem.GetFolder($PathEntry).ShortPath
    if ($ShortPath -and $ShortPath.Length -lt $PathEntry.Length) { return $ShortPath }
  } catch {}
  return $PathEntry
}
$PathEntry = Get-ShortPathEntry $InstallDir
$RegistryPath = 'registry::HKEY_CURRENT_USER\Environment'
$PathEntries = (Get-Item -LiteralPath $RegistryPath).GetValue(
  'Path', '', 'DoNotExpandEnvironmentNames') -split ';' -ne ''
$PathEntryNorm = Normalize-PathEntry $PathEntry
$InstallDirNorm = Normalize-PathEntry $InstallDir
$PathEntryNorms = $PathEntries | ForEach-Object { Normalize-PathEntry $_ }
if (($PathEntryNorm -in $PathEntryNorms) -or ($InstallDirNorm -in $PathEntryNorms)) { exit 0 }
$NewPath = (,$PathEntry + $PathEntries) -join ';'
if ($NewPath.Length -gt 32767) {
  Write-Error "Adding $PathEntry would make your user Path $($NewPath.Length) characters, exceeding the Windows environment variable limit of 32767."
  exit 3
}
Set-ItemProperty -Type ExpandString -LiteralPath $RegistryPath Path -Value $NewPath
$DummyName = 'ir-' + [guid]::NewGuid().ToString()
[Environment]::SetEnvironmentVariable($DummyName, 'ir-dummy', 'User')
[Environment]::SetEnvironmentVariable($DummyName, $null, 'User')
Write-Output "Added $PathEntry to your user Path"
"#;
    match Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("IR_NEW_PATH_ENTRY", bin_dir)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!(
            "Could not add launcher directory to the user Path; powershell exited with status {status}."
        ),
        Err(e) => eprintln!("Could not add launcher directory to the user Path: {e}"),
    }

    Ok(())
}

#[cfg(not(unix))]
fn windows_path_entry_str(path: &Path) -> Result<String, Box<dyn Error>> {
    let path = launcher_path_str(path)?;
    Ok(non_verbatim_windows_path(path))
}

#[cfg(not(unix))]
fn non_verbatim_windows_path(path: String) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if path.starts_with(r"\\?\") {
        let bytes = path.as_bytes();
        if bytes.len() > 6 && bytes[5] == b':' && bytes[4].is_ascii_alphabetic() {
            return path[4..].to_string();
        }
    }
    path
}

#[cfg(all(unix, not(target_os = "macos")))]
fn ensure_launcher_dir_on_path(_bin_dir: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
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
    executable: &PackageExecutable,
    rscript_args: &[String],
    args: &[String],
) -> Result<i32, Box<dyn Error>> {
    let launch_program: &OsStr;
    let mut cmd;
    match executable.launcher {
        PackageLauncher::Direct => {
            if !executable.rscript_args.is_empty() || !rscript_args.is_empty() {
                return Err(
                    "Rscript options are only supported for Rscript and Rapp package executables"
                        .into(),
                );
            }
            launch_program = executable.path.as_os_str();
            cmd = Command::new(&executable.path);
        }
        PackageLauncher::Rscript => {
            launch_program = rscript;
            cmd = Command::new(rscript);
            cmd.args(&executable.rscript_args);
            cmd.args(rscript_args);
            cmd.arg(&executable.path);
        }
        PackageLauncher::Rapp => {
            launch_program = rscript;
            cmd = Command::new(rscript);
            cmd.args(&executable.rscript_args);
            cmd.args(rscript_args);
            cmd.arg("-e").arg("Rapp::run()").arg(&executable.path);
        }
        PackageLauncher::RappFrontend => {
            launch_program = rscript;
            cmd = Command::new(rscript);
            cmd.args(&executable.rscript_args);
            cmd.args(rscript_args);
            cmd.arg("-e").arg("Rapp::run()");
        }
    }
    cmd.args(args)
        .env("R_LIBS", library)
        .env("R_LIBS_USER", "NULL")
        .env("RAPP_LAUNCHER_NAME", &executable.name)
        .env("PATH", resolved_runtime_path(library, rscript)?);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        let message = match executable.launcher {
            PackageLauncher::Direct => executable_spawn_error(launch_program, err),
            PackageLauncher::Rscript | PackageLauncher::Rapp | PackageLauncher::RappFrontend => {
                spawn_error(rscript, err)
            }
        };
        Err(message.into())
    }

    #[cfg(not(unix))]
    {
        let status = cmd.status().map_err(|e| match executable.launcher {
            PackageLauncher::Direct => executable_spawn_error(launch_program, e),
            PackageLauncher::Rscript | PackageLauncher::Rapp | PackageLauncher::RappFrontend => {
                spawn_error(rscript, e)
            }
        })?;
        Ok(status.code().unwrap_or(1))
    }
}

#[derive(Clone, Copy)]
enum PackageLauncher {
    Direct,
    Rscript,
    Rapp,
    RappFrontend,
}

fn package_executable_launcher_kind(
    executable: &Path,
) -> Result<Option<PackageLauncher>, Box<dyn Error>> {
    let file = File::open(executable)
        .map_err(|e| format!("cannot read executable `{}`: {e}", executable.display()))?;
    let mut reader = BufReader::new(file);
    let mut shebang = Vec::new();
    reader.read_until(b'\n', &mut shebang)?;

    if !shebang.starts_with(b"#!") {
        return if is_direct_package_script_without_shebang(executable)? {
            Ok(Some(PackageLauncher::Direct))
        } else {
            Ok(None)
        };
    }

    if shebang_mentions(&shebang, b"Rapp") {
        Ok(Some(PackageLauncher::Rapp))
    } else if shebang_mentions(&shebang, b"Rscript") {
        Ok(Some(PackageLauncher::Rscript))
    } else if is_direct_package_script(executable)? {
        Ok(Some(PackageLauncher::Direct))
    } else {
        Ok(None)
    }
}

#[cfg(unix)]
fn is_direct_package_script_without_shebang(_path: &Path) -> Result<bool, Box<dyn Error>> {
    Ok(false)
}

#[cfg(not(unix))]
fn is_direct_package_script_without_shebang(path: &Path) -> Result<bool, Box<dyn Error>> {
    is_direct_package_script(path)
}

#[cfg(unix)]
fn is_direct_package_script(path: &Path) -> Result<bool, Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    Ok(fs::metadata(path)?.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_direct_package_script(path: &Path) -> Result<bool, Box<dyn Error>> {
    let Some(ext) = path.extension().and_then(OsStr::to_str) else {
        return Ok(false);
    };
    Ok(ext.eq_ignore_ascii_case("bat") || ext.eq_ignore_ascii_case("cmd"))
}

fn shebang_mentions(shebang: &[u8], name: &[u8]) -> bool {
    shebang
        .split(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_'))
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
    if let Some(rscript) = &install.rscript {
        words.push("--rscript".to_string());
        words.push(command_word(rscript));
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
        PackageLauncher::Direct => {
            cmd = vec!["exec".to_string(), sh_quote_path(&executable.path)?];
        }
        PackageLauncher::Rscript => {
            cmd.extend(executable.rscript_args.iter().map(|arg| sh_quote_str(arg)));
            cmd.push(sh_quote_path(&executable.path)?);
        }
        PackageLauncher::Rapp => {
            cmd.extend(executable.rscript_args.iter().map(|arg| sh_quote_str(arg)));
            cmd.push("-e".to_string());
            cmd.push(sh_quote_str("Rapp::run()"));
            cmd.push(sh_quote_path(&executable.path)?);
        }
        PackageLauncher::RappFrontend => {
            cmd.extend(executable.rscript_args.iter().map(|arg| sh_quote_str(arg)));
            cmd.push("-e".to_string());
            cmd.push(sh_quote_str("Rapp::run()"));
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
    path_prefix: &[PathBuf],
    recovery_command: &str,
) -> Result<String, Box<dyn Error>> {
    let mut cmd = Vec::new();
    match executable.launcher {
        PackageLauncher::Direct => {
            cmd.push(cmd_quote_path(&executable.path)?);
        }
        PackageLauncher::Rscript => {
            cmd.push(cmd_quote_os(rscript));
            cmd.extend(executable.rscript_args.iter().map(|arg| cmd_quote_str(arg)));
            cmd.push(cmd_quote_path(&executable.path)?);
        }
        PackageLauncher::Rapp => {
            cmd.push(cmd_quote_os(rscript));
            cmd.extend(executable.rscript_args.iter().map(|arg| cmd_quote_str(arg)));
            cmd.push("-e".to_string());
            cmd.push("Rapp::run()".to_string());
            cmd.push(cmd_quote_path(&executable.path)?);
        }
        PackageLauncher::RappFrontend => {
            cmd.push(cmd_quote_os(rscript));
            cmd.extend(executable.rscript_args.iter().map(|arg| cmd_quote_str(arg)));
            cmd.push("-e".to_string());
            cmd.push("Rapp::run()".to_string());
        }
    }
    cmd.push("%*".to_string());
    let library = launcher_path_str(library)?;
    let mut env_lines = vec![
        r#"set "R_LIBS=%IR_LIBRARY%""#.to_string(),
        r#"set "R_LIBS_USER=NULL""#.to_string(),
        format!(r#"set "RAPP_LAUNCHER_NAME={}""#, executable.name),
    ];
    if let Some(path_assignment) = cmd_path_prefix_assignment(path_prefix)? {
        env_lines.push(path_assignment);
    }

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
         {}\r\n\
         {}\r\n",
        library,
        recovery_command,
        env_lines.join("\r\n"),
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

#[cfg(not(unix))]
fn cmd_path_prefix_assignment(path_prefix: &[PathBuf]) -> Result<Option<String>, Box<dyn Error>> {
    let paths = path_prefix
        .iter()
        .map(|path| launcher_path_str(path))
        .collect::<Result<Vec<_>, _>>()?;
    if paths.is_empty() {
        Ok(None)
    } else {
        Ok(Some(format!(r#"set "PATH={};%PATH%""#, paths.join(";"))))
    }
}

fn executable_spawn_error(program: &OsStr, err: io::Error) -> String {
    if err.kind() == io::ErrorKind::NotFound {
        format!("could not find executable `{}`", program.to_string_lossy())
    } else {
        format!(
            "failed to launch executable `{}`: {err}",
            program.to_string_lossy()
        )
    }
}
