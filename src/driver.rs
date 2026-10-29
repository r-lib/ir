use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const INIT_FILE: &str = env!("IR_INIT_DRIVER_FILE");
pub(crate) const RESOLVE_FILE: &str = env!("IR_RESOLVE_DRIVER_FILE");

pub(crate) fn cached_path(
    cache_dir: &Path,
    file_name: &str,
    contents: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let dir = cache_dir.join("drivers");
    let path = dir.join(file_name);
    if path.exists() {
        return Ok(path);
    }

    write_path(&path, contents)
}

fn write_path(path: &Path, contents: &str) -> Result<PathBuf, Box<dyn Error>> {
    fs::create_dir_all(path.parent().ok_or("driver cache path has no parent")?)?;
    let tmp = temporary_path(path)?;
    fs::write(&tmp, contents)?;
    set_cached_driver_permissions(&tmp)?;
    fs::rename(&tmp, path).map_err(|e| {
        format!(
            "failed to install cached driver `{}` from `{}`: {e}",
            path.display(),
            tmp.display()
        )
    })?;
    Ok(path.to_path_buf())
}

fn temporary_path(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .ok_or("driver cache path has no file name")?;
    let mut tmp_name = OsString::from(".");
    tmp_name.push(file_name);
    tmp_name.push(format!(".{}-{nanos}.tmp", std::process::id()));
    Ok(path.with_file_name(tmp_name))
}

#[cfg(not(windows))]
fn set_cached_driver_permissions(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(windows)]
fn set_cached_driver_permissions(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}
