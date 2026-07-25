use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, serde::Deserialize)]
pub(crate) struct AvailableR {
    pub(crate) name: String,
    pub(crate) version: String,
}

#[derive(Debug, serde::Deserialize)]
struct ResolvedR {
    version: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct InstalledR {
    pub(crate) name: String,
    pub(crate) version: String,
    #[serde(default, rename = "default")]
    pub(crate) is_default: bool,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    #[serde(default)]
    pub(crate) path: Option<PathBuf>,
    binary: PathBuf,
}

pub(crate) struct UserInstallation {
    r_root: PathBuf,
    binary_dir: PathBuf,
    home_dir: PathBuf,
    temp_dir: PathBuf,
    resolution_dir: PathBuf,
    path: OsString,
}

impl UserInstallation {
    pub(crate) fn new(cache_dir: &Path) -> Result<Self, Box<dyn Error>> {
        let root = std::path::absolute(cache_dir)?.join("rig");
        let r_root = root.join("r");
        let binary_dir = root.join("bin");
        let home_dir = root.join("home");
        let temp_dir = root.join("tmp");
        let resolution_dir = root.join("resolutions");
        for path in [&r_root, &binary_dir, &home_dir, &temp_dir, &resolution_dir] {
            path.to_str().ok_or_else(|| {
                format!(
                    "rig requires the private cache path `{}` to be valid UTF-8",
                    path.display()
                )
            })?;
            fs::create_dir_all(path)
                .map_err(|e| format!("failed to create `{}`: {e}", path.display()))?;
        }
        let path = std::env::join_paths(std::iter::once(binary_dir.clone()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .map_err(|e| {
            format!(
                "failed to add private rig binary directory `{}` to PATH: {e}",
                binary_dir.display()
            )
        })?;
        Ok(Self {
            r_root,
            binary_dir,
            home_dir,
            temp_dir,
            resolution_dir,
            path,
        })
    }
}

pub(crate) fn cached_resolution(
    installation: &UserInstallation,
    selector: &str,
) -> Result<Option<String>, Box<dyn Error>> {
    let path = resolution_path(installation, selector);
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "failed to read cached rig resolution `{}`: {error}",
                path.display()
            )
            .into());
        }
    };
    Ok(Some(contents.trim().to_string()))
}

pub(crate) fn cache_resolution(
    installation: &UserInstallation,
    selector: &str,
    version: &str,
) -> Result<(), Box<dyn Error>> {
    let path = resolution_path(installation, selector);
    fs::write(&path, format!("{version}\n")).map_err(|error| {
        format!(
            "failed to cache rig resolution `{}`: {error}",
            path.display()
        )
        .into()
    })
}

fn resolution_path(installation: &UserInstallation, selector: &str) -> PathBuf {
    debug_assert!(selector
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'/'));
    installation.resolution_dir.join(selector.replace('/', "-"))
}

pub(crate) fn list() -> Result<Vec<InstalledR>, Box<dyn Error>> {
    rig_json(&["list", "--json"])
}

pub(crate) fn list_user(
    installation: &UserInstallation,
) -> Result<Vec<InstalledR>, Box<dyn Error>> {
    user_rig_json(installation, &["list", "--json"])
}

pub(crate) fn available_user(
    installation: &UserInstallation,
) -> Result<Vec<AvailableR>, Box<dyn Error>> {
    user_rig_json(installation, &["available", "--all", "--json"])
}

pub(crate) fn resolve_user(
    installation: &UserInstallation,
    selector: &str,
) -> Result<String, Box<dyn Error>> {
    let resolved: Vec<ResolvedR> =
        user_rig_json(installation, &["resolve", "--json", "--", selector])?;
    let [resolved] = resolved.as_slice() else {
        return Err(format!(
            "`rig --user resolve {selector}` returned {} results; expected one",
            resolved.len()
        )
        .into());
    };
    resolved.version.clone().ok_or_else(|| {
        format!("`rig --user resolve {selector}` did not return an R version").into()
    })
}

pub(crate) fn require_user_mode(installation: &UserInstallation) -> Result<(), Box<dyn Error>> {
    let output = user_command(installation)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(launch_error)?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "ir requires the development version of rig for private R installations. Install it with `cargo install --git https://github.com/r-lib/rig --locked --force rig`, then place Cargo's bin directory before any existing rig on PATH.\nrig error: {}",
        stderr.trim()
    )
    .into())
}

pub(crate) fn install_user(
    installation: &UserInstallation,
    version: &str,
) -> Result<(), Box<dyn Error>> {
    let args = ["add", "--without-pak", "--without-repos", "--", version];
    let status = user_command(installation)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .map_err(launch_error)?;
    if !status.success() {
        return Err(format!("`rig --user {}` failed with {status}", args.join(" ")).into());
    }
    Ok(())
}

pub(crate) fn output(args: &[&str]) -> Result<Vec<u8>, Box<dyn Error>> {
    let output = Command::new("rig")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(launch_error)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`rig {}` failed: {stderr}", args.join(" ")).into());
    }

    Ok(output.stdout)
}

fn rig_json<T: serde::de::DeserializeOwned>(args: &[&str]) -> Result<T, Box<dyn Error>> {
    let output = output(args)?;

    serde_json::from_slice(&output)
        .map_err(|e| format!("failed to parse `rig {}` JSON: {e}", args.join(" ")).into())
}

fn user_rig_json<T: serde::de::DeserializeOwned>(
    installation: &UserInstallation,
    args: &[&str],
) -> Result<T, Box<dyn Error>> {
    let output = user_command(installation)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(launch_error)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`rig --user {}` failed: {stderr}", args.join(" ")).into());
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("failed to parse `rig --user {}` JSON: {e}", args.join(" ")).into())
}

fn user_command(installation: &UserInstallation) -> Command {
    let mut command = Command::new("rig");
    command
        .arg("--user")
        .env("RIG_MODE", "user")
        .env("RIG_R_INSTALL_DIR", &installation.r_root)
        .env("RIG_BINARY_DIR", &installation.binary_dir)
        .env("TMPDIR", &installation.temp_dir)
        .env("TMP", &installation.temp_dir)
        .env("TEMP", &installation.temp_dir)
        .env("PATH", &installation.path)
        .env("HOME", &installation.home_dir)
        .env_remove("RIG_PLATFORM")
        .env_remove("RIG_RTOOLS_INSTALL_DIR")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CACHE_HOME");
    command
}

fn launch_error(error: io::Error) -> String {
    if error.kind() == io::ErrorKind::NotFound {
        "could not find `rig` on PATH. Install the development version with `cargo install --git https://github.com/r-lib/rig --locked --force rig`, then place Cargo's bin directory before any existing rig on PATH."
            .to_string()
    } else {
        format!("failed to launch `rig`: {error}")
    }
}

impl InstalledR {
    pub(crate) fn rscript(&self) -> Result<OsString, Box<dyn Error>> {
        let rscript = rscript_from_r_binary(&self.binary);
        if !rscript.exists() {
            return Err(format!(
                "rig reported R {} at `{}`, but `{}` does not exist",
                self.version,
                self.binary.display(),
                rscript.display()
            )
            .into());
        }

        Ok(rscript.into_os_string())
    }
}

fn rscript_from_r_binary(binary: &Path) -> PathBuf {
    binary.with_file_name(if cfg!(windows) {
        "Rscript.exe"
    } else {
        "Rscript"
    })
}
