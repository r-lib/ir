use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};
use time::OffsetDateTime;

pub(crate) struct Paths {
    pub(crate) marker: PathBuf,
    pub(crate) package_marker: Option<PathBuf>,
    pub(crate) source: String,
}

pub(crate) struct CachedResolution {
    pub(crate) library: PathBuf,
    pub(crate) primary_package: Option<String>,
}

pub(crate) fn paths(
    cache_dir: &Path,
    rscript: &OsStr,
    dependencies: &[String],
    exclude_newer: Option<&str>,
    quarto: bool,
) -> Result<Option<Paths>, Box<dyn Error>> {
    let Some(rscript_identity) = rscript_identity(rscript) else {
        return Ok(None);
    };

    let source = resolution_cache_source(exclude_newer);
    let marker = cache_dir.join("resolutions").join(resolution_cache_key(
        dependencies,
        exclude_newer,
        quarto,
        &rscript_identity,
    ));
    let marker_name = marker
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or("resolution cache marker path is not valid UTF-8")?;
    let package_marker = dependencies.first().map(|primary_ref| {
        marker.with_file_name(format!("{marker_name}-primary-{}", sha256_hex(primary_ref)))
    });

    Ok(Some(Paths {
        marker,
        package_marker,
        source,
    }))
}

pub(crate) fn read(
    cache: Option<&Paths>,
    primary_package: bool,
) -> Result<Option<CachedResolution>, Box<dyn Error>> {
    let Some(cache) = cache else {
        return Ok(None);
    };

    if !cache.marker.exists() {
        return Ok(None);
    }

    let marker = fs::read_to_string(&cache.marker)
        .map_err(|e| format!("failed to read `{}`: {e}", cache.marker.display()))?;
    let mut lines = marker.lines();
    if lines.next() != Some(cache.source.as_str()) {
        return Ok(None);
    }
    let library = lines.next().unwrap_or_default().trim();
    if library.is_empty() || !Path::new(library).is_dir() {
        return Ok(None);
    }

    let primary_package = if primary_package {
        let Some(package_marker) = &cache.package_marker else {
            return Ok(None);
        };
        if !package_marker.exists() {
            return Ok(None);
        }
        let package = fs::read_to_string(package_marker)
            .map_err(|e| format!("failed to read `{}`: {e}", package_marker.display()))?;
        let package = package.lines().next().unwrap_or_default().trim();
        if package.is_empty() {
            return Ok(None);
        }
        Some(package.to_string())
    } else {
        None
    };

    Ok(Some(CachedResolution {
        library: PathBuf::from(library),
        primary_package,
    }))
}

fn resolution_cache_key(
    dependencies: &[String],
    exclude_newer: Option<&str>,
    quarto: bool,
    rscript_identity: &str,
) -> String {
    let source_key = exclude_newer
        .map(|date| format!("exclude-newer: {date}"))
        .unwrap_or_else(|| "latest".to_string());
    let mut parts = dependencies.to_vec();
    parts.sort();
    parts.push(source_key);
    if quarto {
        parts.push("quarto".to_string());
    }
    parts.push(format!("rscript: {rscript_identity}"));

    sha256_fields(&parts)
}

fn resolution_cache_source(exclude_newer: Option<&str>) -> String {
    exclude_newer
        .map(|date| format!("exclude-newer: {date}"))
        .unwrap_or_else(|| format!("latest: {}", current_utc_date()))
}

fn rscript_identity(rscript: &OsStr) -> Option<String> {
    let command = rscript_command_path(rscript);
    let path = fs::canonicalize(&command).ok()?;
    if !is_rscript_executable(&path) {
        return None;
    }

    let metadata = fs::metadata(&path).ok()?;
    let mut identity = path.to_string_lossy().into_owned();

    identity.push_str(&format!(";len={}", metadata.len()));
    if let Ok(modified) = metadata.modified() {
        let nanos = modified
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        identity.push_str(&format!(";mtime={nanos}"));
    }
    append_runtime_env(&mut identity, "R_ARCH");
    append_runtime_env(&mut identity, "R_HOME");

    Some(identity)
}

fn append_runtime_env(identity: &mut String, name: &str) {
    if let Some(value) = env::var_os(name) {
        identity.push(';');
        identity.push_str(name);
        identity.push('=');
        identity.push_str(&value.to_string_lossy());
    }
}

fn is_rscript_executable(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    if !matches!(
        name.to_ascii_lowercase().as_str(),
        "rscript" | "rscript.exe"
    ) {
        return false;
    }
    !is_script_launcher(path)
}

fn is_script_launcher(path: &Path) -> bool {
    if path.extension().and_then(OsStr::to_str).is_some_and(|ext| {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "bat" | "cmd" | "ps1" | "sh"
        )
    }) {
        return true;
    }

    let Ok(mut file) = File::open(path) else {
        return true;
    };
    let mut magic = [0; 2];
    matches!(file.read(&mut magic), Ok(2)) && magic == *b"#!"
}

fn rscript_command_path(rscript: &OsStr) -> PathBuf {
    let path = Path::new(rscript);
    if path.components().count() > 1 {
        return path.to_path_buf();
    }

    find_on_path(rscript).unwrap_or_else(|| path.to_path_buf())
}

fn find_on_path(command: &OsStr) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }

        #[cfg(windows)]
        {
            let pathext = env::var_os("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
            let command = command.to_string_lossy();
            for ext in pathext.to_string_lossy().split(';') {
                let candidate = dir.join(format!("{command}{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

fn current_utc_date() -> String {
    OffsetDateTime::now_utc().date().to_string()
}

fn sha256_hex(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").unwrap();
    }
    hex
}

fn sha256_fields(fields: &[String]) -> String {
    let mut encoded = String::new();
    for field in fields {
        write!(&mut encoded, "{}:", field.len()).unwrap();
        encoded.push_str(field);
        encoded.push('\n');
    }
    sha256_hex(&encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        name: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn capture(name: &'static str) -> Self {
            Self {
                name,
                previous: env::var_os(name),
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                env::set_var(self.name, previous);
            } else {
                env::remove_var(self.name);
            }
        }
    }

    fn unique_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dir = env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn dummy_rscript(dir: &Path) -> PathBuf {
        let name = if cfg!(windows) {
            "Rscript.exe"
        } else {
            "Rscript"
        };
        let path = dir.join(name);
        fs::write(&path, "not a script launcher").unwrap();
        path
    }

    #[test]
    fn runtime_selection_env_changes_resolution_marker() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _r_arch = EnvVarGuard::capture("R_ARCH");
        let _r_home = EnvVarGuard::capture("R_HOME");
        let dir = unique_dir("ir-resolve-cache-unit");
        let cache_dir = dir.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let rscript = dummy_rscript(&dir);
        let dependencies = vec!["cli".to_string()];

        env::set_var("R_ARCH", "x64");
        env::remove_var("R_HOME");
        let x64_marker = paths(&cache_dir, rscript.as_os_str(), &dependencies, None, false)
            .unwrap()
            .unwrap()
            .marker;

        env::set_var("R_ARCH", "i386");
        let i386_marker = paths(&cache_dir, rscript.as_os_str(), &dependencies, None, false)
            .unwrap()
            .unwrap()
            .marker;

        env::set_var("R_ARCH", "x64");
        env::set_var("R_HOME", dir.join("R-home"));
        let r_home_marker = paths(&cache_dir, rscript.as_os_str(), &dependencies, None, false)
            .unwrap()
            .unwrap()
            .marker;

        assert_ne!(x64_marker, i386_marker);
        assert_ne!(x64_marker, r_home_marker);

        let _ = fs::remove_dir_all(&dir);
    }
}
