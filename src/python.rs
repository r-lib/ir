use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::spec::PythonSpec;

const DEFAULT_LATEST_MAX_AGE_SECONDS: u64 = 24 * 60 * 60;
const LATEST_MAX_AGE_SECONDS_ENV: &str = "IR_LATEST_RESOLUTION_MAX_AGE_SECONDS";

pub(crate) struct EnvRequest {
    pub(crate) packages: Vec<String>,
    pub(crate) python_version: Option<String>,
    pub(crate) exclude_newer: Option<String>,
    marker: Option<PathBuf>,
    source: String,
    latest_max_age_seconds: Option<u64>,
}

pub(crate) fn request(
    cache_dir: &Path,
    python: Option<&PythonSpec>,
    include_jupyter: bool,
) -> Result<Option<EnvRequest>, Box<dyn Error>> {
    let Some(python) = python else {
        return Ok(None);
    };

    let packages = python_packages(python, include_jupyter);
    let latest_max_age_seconds = if python.exclude_newer.is_none() {
        Some(latest_max_age_seconds()?)
    } else {
        None
    };
    let source = cache_source(python.exclude_newer.as_deref())?;
    let uv_config_parts = uv_config_cache_key_parts();
    let marker = if packages
        .iter()
        .all(|package| python_package_spec_cacheable(package))
        && python_resolver_env_cacheable()
    {
        uv_config_parts.map(|parts| {
            cache_dir.join("python").join(cache_key(
                &packages,
                python.python_version.as_deref(),
                python.exclude_newer.as_deref(),
                &parts,
            ))
        })
    } else {
        None
    };

    Ok(Some(EnvRequest {
        packages,
        python_version: python.python_version.clone(),
        exclude_newer: python.exclude_newer.clone(),
        marker,
        source,
        latest_max_age_seconds,
    }))
}

pub(crate) fn read_cache(request: Option<&EnvRequest>) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let Some(request) = request else {
        return Ok(None);
    };

    let Some(marker_path) = &request.marker else {
        return Ok(None);
    };

    if !marker_path.exists() {
        return Ok(None);
    }

    let marker = fs::read_to_string(marker_path)
        .map_err(|e| format!("failed to read `{}`: {e}", marker_path.display()))?;
    let mut lines = marker.lines();
    let source = lines.next().unwrap_or_default();
    if !source_is_current(source, request)? {
        return Ok(None);
    }

    let python = lines.next().unwrap_or_default().trim();
    if python.is_empty() || !Path::new(python).exists() {
        return Ok(None);
    }

    Ok(Some(PathBuf::from(python)))
}

pub(crate) fn write_cache(request: &EnvRequest, python: &Path) -> Result<(), Box<dyn Error>> {
    let Some(marker_path) = &request.marker else {
        return Ok(());
    };

    let parent = marker_path
        .parent()
        .ok_or("Python cache marker path has no parent")?;
    fs::create_dir_all(parent)?;
    fs::write(
        marker_path,
        format!("{}\n{}\n", request.source, python.display()),
    )
    .map_err(|e| format!("failed to write `{}`: {e}", marker_path.display()))?;
    Ok(())
}

fn python_packages(python: &PythonSpec, include_jupyter: bool) -> Vec<String> {
    let mut packages = python.packages.clone();
    if include_jupyter
        && !packages
            .iter()
            .any(|package| python_package_name(package) == "jupyter")
    {
        packages.push("jupyter".to_string());
    }
    packages
}

fn python_package_name(package: &str) -> &str {
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

fn python_package_spec_cacheable(package: &str) -> bool {
    let package = package.trim();
    if package.is_empty() {
        return false;
    }

    let lower = package.to_ascii_lowercase();
    if package.starts_with(['.', '/', '~', '\\'])
        || package.contains(['/', '\\'])
        || package.contains("://")
        || lower.starts_with("-e ")
        || lower == "-e"
        || lower.starts_with("--editable")
        || lower.starts_with("file:")
        || lower.starts_with("git+")
        || lower.starts_with("hg+")
        || lower.starts_with("svn+")
        || lower.starts_with("bzr+")
    {
        return false;
    }

    python_distribution_name(python_package_name(package))
}

fn python_distribution_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        && name
            .chars()
            .last()
            .is_some_and(|ch| ch.is_ascii_alphanumeric())
}

fn python_resolver_env_cacheable() -> bool {
    env::vars_os()
        .all(|(name, value)| value.is_empty() || !python_resolver_env_var(name.as_os_str()))
}

fn python_resolver_env_var(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| name.starts_with("UV_") || name == "RETICULATE_UV")
}

fn uv_config_cache_key_parts() -> Option<Vec<String>> {
    let mut parts = Vec::new();
    for (scope, path) in uv_config_files() {
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => continue,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(_) => return None,
        }

        let contents = fs::read(&path).ok()?;
        parts.push(format!("uv-config-{scope}-path: {}", path.display()));
        parts.push(format!(
            "uv-config-{scope}-sha256: {}",
            sha256_bytes(&contents)
        ));
    }
    Some(parts)
}

fn uv_config_files() -> Vec<(&'static str, PathBuf)> {
    let mut files = Vec::new();
    if let Some(path) = user_uv_config_file() {
        files.push(("user", path));
    }
    if let Some(path) = system_uv_config_file() {
        files.push(("system", path));
    }
    files
}

fn env_os_nonempty(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

#[cfg(not(windows))]
fn user_uv_config_file() -> Option<PathBuf> {
    env_os_nonempty("XDG_CONFIG_HOME")
        .map(|config_home| PathBuf::from(config_home).join("uv").join("uv.toml"))
        .or_else(|| {
            env_os_nonempty("HOME").map(|home| {
                PathBuf::from(home)
                    .join(".config")
                    .join("uv")
                    .join("uv.toml")
            })
        })
}

#[cfg(windows)]
fn user_uv_config_file() -> Option<PathBuf> {
    env_os_nonempty("APPDATA").map(|appdata| PathBuf::from(appdata).join("uv").join("uv.toml"))
}

#[cfg(not(windows))]
fn system_uv_config_file() -> Option<PathBuf> {
    let dirs = env_os_nonempty("XDG_CONFIG_DIRS").unwrap_or_else(|| OsString::from("/etc/xdg"));
    for dir in env::split_paths(&dirs) {
        let path = dir.join("uv").join("uv.toml");
        if path.is_file() {
            return Some(path);
        }
    }

    let path = PathBuf::from("/etc/uv/uv.toml");
    path.is_file().then_some(path)
}

#[cfg(windows)]
fn system_uv_config_file() -> Option<PathBuf> {
    env_os_nonempty("PROGRAMDATA")
        .map(|program_data| PathBuf::from(program_data).join("uv").join("uv.toml"))
}

fn cache_key(
    packages: &[String],
    python_version: Option<&str>,
    exclude_newer: Option<&str>,
    uv_config_parts: &[String],
) -> String {
    let mut parts = packages.to_vec();
    parts.sort();
    parts.push(
        python_version
            .map(|version| format!("python-version: {version}"))
            .unwrap_or_else(|| "python-version: default".to_string()),
    );
    parts.push(
        exclude_newer
            .map(|date| format!("exclude-newer: {date}"))
            .unwrap_or_else(|| "latest".to_string()),
    );
    parts.extend(uv_config_parts.iter().cloned());
    sha256_fields(&parts)
}

fn cache_source(exclude_newer: Option<&str>) -> Result<String, Box<dyn Error>> {
    Ok(match exclude_newer {
        Some(date) => format!("exclude-newer: {date}"),
        None => format!("latest: {}", current_utc_seconds()?),
    })
}

fn source_is_current(source: &str, request: &EnvRequest) -> Result<bool, Box<dyn Error>> {
    let Some(max_age_seconds) = request.latest_max_age_seconds else {
        return Ok(source == request.source.as_str());
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
    Ok(now - created_at <= max_age_seconds)
}

fn latest_max_age_seconds() -> Result<u64, Box<dyn Error>> {
    let Some(value) = std::env::var_os(LATEST_MAX_AGE_SECONDS_ENV) else {
        return Ok(DEFAULT_LATEST_MAX_AGE_SECONDS);
    };
    let value = value
        .into_string()
        .map_err(|_| format!("{LATEST_MAX_AGE_SECONDS_ENV} must be valid UTF-8"))?;
    let value = value.trim();
    if value.is_empty() {
        return Ok(DEFAULT_LATEST_MAX_AGE_SECONDS);
    }
    value
        .parse::<u64>()
        .map_err(|_| format!("{LATEST_MAX_AGE_SECONDS_ENV} must be an integer").into())
}

fn current_utc_seconds() -> Result<u64, Box<dyn Error>> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("system clock is before UNIX epoch: {e}"))?
        .as_secs())
}

fn sha256_fields(parts: &[String]) -> String {
    let mut input = String::new();
    for part in parts {
        writeln!(&mut input, "{part}").expect("writing to a String cannot fail");
    }
    sha256_bytes(input.as_bytes())
}

fn sha256_bytes(input: &[u8]) -> String {
    let hash = Sha256::digest(input);
    let mut output = String::with_capacity(hash.len() * 2);
    for byte in hash {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}
