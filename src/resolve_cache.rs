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
    pub(crate) advisory_marker: PathBuf,
}

pub(crate) struct CachedResolution {
    pub(crate) library: PathBuf,
    pub(crate) primary_package: Option<String>,
}

pub(crate) fn paths(
    rscript: &OsStr,
    dependencies: &[String],
    exclude_newer: Option<&str>,
    quarto: bool,
) -> Result<Option<Paths>, Box<dyn Error>> {
    let Some(rscript_identity) = rscript_identity(rscript) else {
        return Ok(None);
    };

    let cache_dir = crate::ir_cache_dir()?;
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
    let advisory_marker = marker.with_file_name(format!("{marker_name}-advisory"));

    Ok(Some(Paths {
        marker,
        package_marker,
        advisory_marker,
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

    let result = fs::read_to_string(&cache.marker)
        .map_err(|e| format!("failed to read `{}`: {e}", cache.marker.display()))?;
    let library = result.lines().next().unwrap_or_default().trim();
    if library.is_empty() || !Path::new(library).is_dir() {
        return Ok(None);
    }
    replay_advisory(cache)?;

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

fn replay_advisory(cache: &Paths) -> Result<(), Box<dyn Error>> {
    if !cache.advisory_marker.exists() {
        return Ok(());
    }

    let advisory = fs::read_to_string(&cache.advisory_marker)
        .map_err(|e| format!("failed to read `{}`: {e}", cache.advisory_marker.display()))?;
    let advisory = advisory.trim();
    if !advisory.is_empty() {
        eprintln!("{advisory}");
    }

    Ok(())
}

fn resolution_cache_key(
    dependencies: &[String],
    exclude_newer: Option<&str>,
    quarto: bool,
    rscript_identity: &str,
) -> String {
    let source_key = exclude_newer
        .map(|date| format!("exclude-newer: {date}"))
        .unwrap_or_else(current_utc_date);
    let mut parts = dependencies.to_vec();
    parts.sort();
    parts.push(source_key);
    if quarto {
        parts.push("quarto".to_string());
    }
    parts.push("ir-resolution-v2".to_string());
    parts.push(format!("rscript: {rscript_identity}"));

    sha256_fields(&parts)
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

    Some(identity)
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
