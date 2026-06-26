use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

const DEFAULT_LATEST_MAX_AGE_SECONDS: u64 = 24 * 60 * 60;
const LATEST_MAX_AGE_SECONDS_ENV: &str = "IR_LATEST_RESOLUTION_MAX_AGE_SECONDS";

pub(crate) struct Paths {
    pub(crate) marker: PathBuf,
    pub(crate) package_marker: Option<PathBuf>,
    source: String,
    latest_max_age_seconds: Option<u64>,
}

pub(crate) struct CachedResolution {
    pub(crate) library: PathBuf,
    pub(crate) primary_package: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct QuartoCacheFlags {
    pub(crate) render: bool,
    pub(crate) reticulate: bool,
}

pub(crate) fn paths(
    cache_dir: &Path,
    rscript: &OsStr,
    rscript_args: &[String],
    dependencies: &[String],
    exclude_newer: Option<&str>,
    quarto: QuartoCacheFlags,
    library_root: Option<&Path>,
) -> Result<Option<Paths>, Box<dyn Error>> {
    if !dependencies
        .iter()
        .all(|dependency| is_standard_ref(dependency))
    {
        return Ok(None);
    }

    let Some(rscript_identity) = rscript_identity(rscript) else {
        return Ok(None);
    };

    let latest_max_age_seconds = if exclude_newer.is_none() {
        Some(latest_max_age_seconds()?)
    } else {
        None
    };
    let source = resolution_cache_source(exclude_newer)?;
    let marker = cache_dir.join("resolutions").join(resolution_cache_key(
        dependencies,
        exclude_newer,
        quarto,
        &rscript_identity,
        rscript_args,
        library_root,
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
        latest_max_age_seconds,
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
    let source = lines.next().unwrap_or_default();
    if !source_is_current(source, cache)? {
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
    quarto: QuartoCacheFlags,
    rscript_identity: &str,
    rscript_args: &[String],
    library_root: Option<&Path>,
) -> String {
    let source_key = exclude_newer
        .map(|date| format!("exclude-newer: {date}"))
        .unwrap_or_else(|| "latest".to_string());
    let mut parts = dependencies.to_vec();
    parts.sort();
    parts.push(source_key);
    if quarto.render {
        parts.push("quarto".to_string());
    }
    if quarto.reticulate {
        parts.push("quarto-reticulate".to_string());
    }
    parts.push(format!("rscript: {rscript_identity}"));
    for arg in rscript_args {
        parts.push(format!("rscript-arg: {arg}"));
    }
    if let Some(library_root) = library_root {
        parts.push(format!("library-root: {}", library_root.display()));
    }

    sha256_fields(&parts)
}

fn is_standard_ref(dependency: &str) -> bool {
    let dependency = dependency.trim();

    let Some((package, version)) = dependency.split_once('@') else {
        return is_package_name(dependency);
    };

    is_package_name(package) && is_standard_version(version)
}

fn is_standard_version(version: &str) -> bool {
    let version = version.strip_prefix(">=").unwrap_or(version);
    if matches!(version, "current" | "last") {
        return true;
    }

    let mut part_count = 0;
    for part in version.split(['.', '-']) {
        if part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit()) {
            return false;
        }
        part_count += 1;
    }

    part_count >= 2
}

fn is_package_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '.')
        && name
            .chars()
            .last()
            .is_some_and(|ch| ch.is_ascii_alphanumeric())
}

fn resolution_cache_source(exclude_newer: Option<&str>) -> Result<String, Box<dyn Error>> {
    Ok(match exclude_newer {
        Some(date) => format!("exclude-newer: {date}"),
        None => format!("latest: {}", current_utc_seconds()?),
    })
}

fn source_is_current(source: &str, cache: &Paths) -> Result<bool, Box<dyn Error>> {
    let Some(max_age_seconds) = cache.latest_max_age_seconds else {
        return Ok(source == cache.source.as_str());
    };

    let Some(created_at) = source.strip_prefix("latest: ") else {
        return Ok(false);
    };
    let Ok(created_at) = created_at.parse::<u64>() else {
        return Ok(false);
    };
    let now = current_utc_seconds()?;
    if created_at > now {
        return Ok(false);
    }
    let age_seconds = now - created_at;
    Ok(age_seconds <= max_age_seconds)
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
    matches!(
        name.to_ascii_lowercase().as_str(),
        "rscript" | "rscript.exe"
    ) && !is_script_launcher(path)
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

fn latest_max_age_seconds() -> Result<u64, Box<dyn Error>> {
    let Some(value) = env::var_os(LATEST_MAX_AGE_SECONDS_ENV) else {
        return Ok(DEFAULT_LATEST_MAX_AGE_SECONDS);
    };
    let value = value.to_string_lossy();
    if value.is_empty() {
        return Ok(DEFAULT_LATEST_MAX_AGE_SECONDS);
    }
    let max_age_seconds = value
        .parse::<u64>()
        .map_err(|e| format!("{LATEST_MAX_AGE_SECONDS_ENV} must be an integer: {e}"))?;
    Ok(max_age_seconds)
}

fn current_utc_seconds() -> Result<u64, Box<dyn Error>> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("system clock is before the Unix epoch: {e}"))?
        .as_secs())
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

    fn default_quarto_flags() -> QuartoCacheFlags {
        QuartoCacheFlags {
            render: false,
            reticulate: false,
        }
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
        let x64_marker = paths(
            &cache_dir,
            rscript.as_os_str(),
            &[],
            &dependencies,
            None,
            default_quarto_flags(),
            None,
        )
        .unwrap()
        .unwrap()
        .marker;

        env::set_var("R_ARCH", "i386");
        let i386_marker = paths(
            &cache_dir,
            rscript.as_os_str(),
            &[],
            &dependencies,
            None,
            default_quarto_flags(),
            None,
        )
        .unwrap()
        .unwrap()
        .marker;

        env::set_var("R_ARCH", "x64");
        env::set_var("R_HOME", dir.join("R-home"));
        let r_home_marker = paths(
            &cache_dir,
            rscript.as_os_str(),
            &[],
            &dependencies,
            None,
            default_quarto_flags(),
            None,
        )
        .unwrap()
        .unwrap()
        .marker;

        assert_ne!(x64_marker, i386_marker);
        assert_ne!(x64_marker, r_home_marker);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_root_changes_resolution_marker() {
        let dir = unique_dir("ir-resolve-cache-library-root-unit");
        let cache_dir = dir.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let rscript = dummy_rscript(&dir);
        let dependencies = vec!["cli".to_string()];

        let cache_marker = paths(
            &cache_dir,
            rscript.as_os_str(),
            &[],
            &dependencies,
            Some("2026-06-01"),
            default_quarto_flags(),
            None,
        )
        .unwrap()
        .unwrap()
        .marker;
        let store_marker = paths(
            &cache_dir,
            rscript.as_os_str(),
            &[],
            &dependencies,
            Some("2026-06-01"),
            default_quarto_flags(),
            Some(&dir.join("tool-store")),
        )
        .unwrap()
        .unwrap()
        .marker;

        assert_ne!(cache_marker, store_marker);
        assert!(store_marker.starts_with(cache_dir.join("resolutions")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nonstandard_refs_skip_resolution_markers() {
        let dir = unique_dir("ir-nonstandard-ref-cache-unit");
        let cache_dir = dir.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let rscript = dummy_rscript(&dir);

        for dependency in [
            "github::owner/repo@branch",
            "owner/repo/subdir@main",
            "pkg=owner/repo/subdir@main",
            "gitlab::group/project",
            "https://example.com/pkg.tar.gz",
            "\\work\\pkg",
            "cli@3",
        ] {
            let dependencies = vec![dependency.to_string()];
            assert!(
                paths(
                    &cache_dir,
                    rscript.as_os_str(),
                    &[],
                    &dependencies,
                    Some("2026-06-01"),
                    default_quarto_flags(),
                    None
                )
                .unwrap()
                .is_none(),
                "{dependency} should not use a warm resolution marker",
            );
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn standard_refs_use_resolution_markers() {
        let dir = unique_dir("ir-standard-ref-cache-unit");
        let cache_dir = dir.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let rscript = dummy_rscript(&dir);

        for dependency in ["cli", "cli@3.6.6", "cli@>=3.6.6"] {
            let dependencies = vec![dependency.to_string()];
            assert!(
                paths(
                    &cache_dir,
                    rscript.as_os_str(),
                    &[],
                    &dependencies,
                    Some("2026-06-01"),
                    default_quarto_flags(),
                    None
                )
                .unwrap()
                .is_some(),
                "{dependency} should use a warm resolution marker",
            );
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
