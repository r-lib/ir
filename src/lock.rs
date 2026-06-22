use std::env;
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

pub(crate) struct FileLock {
    _file: fs::File,
}

impl FileLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, Box<dyn Error>> {
        fs::create_dir_all(path.parent().ok_or("resolver lock path has no parent")?)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| format!("failed to open resolver lock `{}`: {e}", path.display()))?;
        file.lock()
            .map_err(|e| format!("failed to lock resolver cache `{}`: {e}", path.display()))?;

        Ok(Self { _file: file })
    }
}

pub(crate) fn resolver_lock_path(root: &Path) -> PathBuf {
    root.join("locks").join("resolver.lock")
}

pub(crate) fn package_cache_lock_path(root: &Path) -> PathBuf {
    env::temp_dir()
        .join("ir-resolver-locks")
        .join(format!("r-user-cache-{}.lock", stable_hash(root)))
}

fn stable_hash(path: &Path) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
