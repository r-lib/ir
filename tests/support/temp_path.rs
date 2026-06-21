use std::ffi::OsStr;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static UNIQUE_ID: AtomicU64 = AtomicU64::new(0);

fn unique_path(prefix: &str, ext: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let id = UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    let mut path =
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}-{id}", std::process::id()));
    if !ext.is_empty() {
        path.set_extension(ext);
    }
    path
}

fn unique_dir(prefix: &str) -> PathBuf {
    let dir = unique_path(prefix, "");
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[derive(Debug)]
pub(crate) struct TempPath {
    path: PathBuf,
}

impl TempPath {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let Ok(metadata) = fs::symlink_metadata(&self.path) else {
            return;
        };
        let result = if metadata.is_dir() {
            fs::remove_dir_all(&self.path)
        } else {
            fs::remove_file(&self.path)
        };
        let _ = result;
    }
}

impl Deref for TempPath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl AsRef<Path> for TempPath {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<OsStr> for TempPath {
    fn as_ref(&self) -> &OsStr {
        self.path.as_os_str()
    }
}

pub(crate) fn temp_path(prefix: &str, ext: &str) -> TempPath {
    TempPath::new(unique_path(prefix, ext))
}

pub(crate) fn temp_dir(prefix: &str) -> TempPath {
    TempPath::new(unique_dir(prefix))
}

pub(crate) fn temp_cache(prefix: &str) -> TempPath {
    temp_dir(prefix)
}
