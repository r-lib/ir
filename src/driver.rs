use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

const DRIVER_CACHE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) fn cached_path(
    cache_dir: &Path,
    file_name: &str,
    contents: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let dir = cache_dir.join("drivers");
    let path = dir.join(file_name);
    let version_path = dir.join(format!("{file_name}.version"));
    if path.exists()
        && fs::read_to_string(&version_path).ok().as_deref() == Some(DRIVER_CACHE_VERSION)
    {
        return Ok(path);
    }

    fs::create_dir_all(&dir)?;
    if path.exists() {
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions)?;
    }
    fs::write(&path, contents)?;
    let mut permissions = fs::metadata(&path)?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions)?;
    fs::write(version_path, DRIVER_CACHE_VERSION)?;
    Ok(path)
}
