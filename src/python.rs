use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::driver;
use crate::spec::UvSpec;

/// The Python resolution driver is embedded for the same reason as the R
/// package resolver: ir ships as one self-contained binary.
const PYTHON_RESOLVE_DRIVER: &str = concat!(
    include_str!("../driver/tooling.R"),
    "\n",
    include_str!("../driver/resolve_python.R")
);

pub(crate) fn resolve_env(
    rscript: &OsStr,
    cache_dir: &Path,
    uv: Option<&UvSpec>,
) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let Some(uv) = uv else {
        return Ok(None);
    };

    let driver = driver::cached_path(cache_dir, "resolve-python.R", PYTHON_RESOLVE_DRIVER)?;
    let tmp = std::env::temp_dir();
    let result_file = unique_path(&tmp, "ir-python", "txt");

    let mut cmd = Command::new(rscript);
    cmd.arg(&driver)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("IR_PYTHON_RESULT_FILE", &result_file)
        .env("IR_CACHE_DIR", cache_dir)
        .env_remove("IR_EXCLUDE_NEWER")
        .env_remove("IR_UV_PYTHON_VERSION")
        .env_remove("IR_UV_EXCLUDE_NEWER");
    if let Some(python_version) = &uv.python_version {
        cmd.env("IR_UV_PYTHON_VERSION", python_version);
    }
    if let Some(exclude_newer) = &uv.exclude_newer {
        cmd.env("IR_UV_EXCLUDE_NEWER", exclude_newer);
    }

    let mut child = cmd.spawn().map_err(|e| spawn_error(rscript, e))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or("failed to open Python resolver stdin")?;
        for package in uv_packages(uv) {
            writeln!(stdin, "{package}")?;
        }
    }
    let status = child
        .wait()
        .map_err(|e| format!("failed to wait for Python resolver: {e}"))?;

    let result = fs::read_to_string(&result_file).unwrap_or_default();
    let _ = fs::remove_file(&result_file);

    if !status.success() {
        return Err("Python environment resolution failed".into());
    }

    let path = result.trim();
    if path.is_empty() {
        return Err("Python environment resolver did not return a Python path".into());
    }

    Ok(Some(PathBuf::from(path)))
}

fn uv_packages(uv: &UvSpec) -> Vec<String> {
    let mut packages = uv.packages.clone();
    if !packages
        .iter()
        .any(|package| uv_package_name(package) == "jupyter")
    {
        packages.push("jupyter".to_string());
    }
    packages
}

fn uv_package_name(package: &str) -> &str {
    let package = package.trim();
    let end = package
        .find(|ch: char| {
            matches!(
                ch,
                '<' | '>' | '=' | '~' | '!' | '[' | '@' | ';' | ' ' | '\t'
            )
        })
        .unwrap_or(package.len());
    &package[..end]
}

fn unique_path(dir: &Path, prefix: &str, ext: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut path = dir.join(format!("{prefix}-{}-{nanos}", std::process::id()));
    if !ext.is_empty() {
        path.set_extension(ext);
    }
    path
}

fn spawn_error(rscript: &OsStr, err: io::Error) -> String {
    if err.kind() == io::ErrorKind::NotFound {
        format!(
            "could not find Rscript `{}` while resolving Python environment",
            rscript.to_string_lossy()
        )
    } else {
        format!(
            "failed to launch Python resolver with `{}`: {err}",
            rscript.to_string_lossy()
        )
    }
}
