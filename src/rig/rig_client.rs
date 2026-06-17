use std::error::Error;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, serde::Deserialize)]
pub(crate) struct AvailableR {
    #[serde(default)]
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) date: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct InstalledR {
    pub(crate) name: String,
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    binary: PathBuf,
}

pub(crate) fn available() -> Result<Vec<AvailableR>, Box<dyn Error>> {
    rig_json(&["available", "--json"])
}

pub(crate) fn list() -> Result<Vec<InstalledR>, Box<dyn Error>> {
    rig_json(&["list", "--json"])
}

pub(crate) fn output(args: &[&str]) -> Result<Vec<u8>, Box<dyn Error>> {
    let output = Command::new("rig")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                "could not find `rig` on PATH. Install rig to use `r-version`.".to_string()
            } else {
                format!("failed to launch `rig`: {e}")
            }
        })?;

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
