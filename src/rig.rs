use std::error::Error;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, serde::Deserialize)]
struct AvailableR {
    name: String,
    version: String,
    date: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct InstalledR {
    name: String,
    version: String,
    #[serde(default)]
    aliases: Vec<String>,
    binary: PathBuf,
}

#[derive(Debug)]
enum VersionRequirement {
    Bare(String),
    Comparison {
        op: VersionOp,
        version: Vec<u64>,
        raw: String,
    },
}

#[derive(Debug)]
enum VersionOp {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
}

pub fn resolve_rscript(req: &str, exclude_newer: Option<&str>) -> Result<OsString, Box<dyn Error>> {
    let exclude_newer = exclude_newer
        .map(|value| parse_iso_date_field("exclude-newer", value))
        .transpose()?;
    let requirement = parse_version_requirement(req)?;
    let available = rig_available()?;
    let installed = rig_list()?;

    let required = available
        .iter()
        .filter(|version| released_before_or_on(version, exclude_newer.as_deref()))
        .filter(|version| requirement.matches(version))
        .max_by(|a, b| compare_versions(&a.version, &b.version))
        .ok_or_else(|| {
            let suffix = exclude_newer
                .as_deref()
                .map(|date| format!(" before or on {date}"))
                .unwrap_or_default();
            format!("could not resolve R version `{req}` with `rig available --json`{suffix}")
        })?;

    let installed = installed
        .iter()
        .find(|version| version.matches_available(required))
        .ok_or_else(|| {
            format!(
                "R {} is required but is not installed. Run `rig install {}`.",
                required.version, required.name
            )
        })?;

    let rscript = rscript_from_r_binary(&installed.binary);
    if !rscript.exists() {
        return Err(format!(
            "rig reported R {} at `{}`, but `{}` does not exist",
            installed.version,
            installed.binary.display(),
            rscript.display()
        )
        .into());
    }

    Ok(rscript.into_os_string())
}

fn parse_iso_date_field(key: &str, value: &str) -> Result<String, Box<dyn Error>> {
    let value = value.trim();
    if !is_iso_date(value) {
        return Err(format!("`{key}` must be a date string in YYYY-MM-DD format").into());
    }
    Ok(value.to_string())
}

fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2].is_ascii_digit()
        && bytes[3].is_ascii_digit()
        && bytes[4] == b'-'
        && bytes[5].is_ascii_digit()
        && bytes[6].is_ascii_digit()
        && bytes[7] == b'-'
        && bytes[8].is_ascii_digit()
        && bytes[9].is_ascii_digit()
}

fn rig_available() -> Result<Vec<AvailableR>, Box<dyn Error>> {
    rig_json(&["available", "--json"])
}

fn rig_list() -> Result<Vec<InstalledR>, Box<dyn Error>> {
    rig_json(&["list", "--json"])
}

fn rig_json<T: serde::de::DeserializeOwned>(args: &[&str]) -> Result<T, Box<dyn Error>> {
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

    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("failed to parse `rig {}` JSON: {e}", args.join(" ")).into())
}

fn released_before_or_on(version: &AvailableR, exclude_newer: Option<&str>) -> bool {
    let Some(exclude_newer) = exclude_newer else {
        return true;
    };
    let Some(date) = version.date.as_deref().and_then(iso_date_prefix) else {
        return false;
    };
    date <= exclude_newer
}

fn iso_date_prefix(value: &str) -> Option<&str> {
    let date = value.get(..10)?;
    if is_iso_date(date) {
        Some(date)
    } else {
        None
    }
}

fn parse_version_requirement(req: &str) -> Result<VersionRequirement, Box<dyn Error>> {
    let req = req.trim();
    for (prefix, op) in [
        (">=", VersionOp::Gte),
        ("<=", VersionOp::Lte),
        ("==", VersionOp::Eq),
        (">", VersionOp::Gt),
        ("<", VersionOp::Lt),
    ] {
        if let Some(version) = req.strip_prefix(prefix) {
            let raw = version.trim().to_string();
            let version = parse_version(&raw)
                .ok_or_else(|| format!("`r-version` has an unsupported version spec `{req}`"))?;
            return Ok(VersionRequirement::Comparison { op, version, raw });
        }
    }

    if req.is_empty() {
        return Err("`r-version` must not be empty".into());
    }
    Ok(VersionRequirement::Bare(req.to_string()))
}

impl VersionRequirement {
    fn matches(&self, available: &AvailableR) -> bool {
        match self {
            VersionRequirement::Bare(req) => {
                available.name == *req
                    || available.version == *req
                    || parse_version(req)
                        .map(|_| available.version.starts_with(&format!("{req}.")))
                        .unwrap_or(false)
            }
            VersionRequirement::Comparison { op, version, raw } => {
                let Some(candidate) = parse_version(&available.version) else {
                    return false;
                };
                if matches!(op, VersionOp::Eq) && available.name == *raw {
                    return true;
                }
                match op {
                    VersionOp::Gt => compare_version_parts(&candidate, version).is_gt(),
                    VersionOp::Gte => compare_version_parts(&candidate, version).is_ge(),
                    VersionOp::Lt => compare_version_parts(&candidate, version).is_lt(),
                    VersionOp::Lte => compare_version_parts(&candidate, version).is_le(),
                    VersionOp::Eq => compare_version_parts(&candidate, version).is_eq(),
                }
            }
        }
    }
}

impl InstalledR {
    fn matches_available(&self, available: &AvailableR) -> bool {
        self.version == available.version
            || self.name == available.name
            || self.aliases.iter().any(|alias| alias == &available.name)
    }
}

fn parse_version(value: &str) -> Option<Vec<u64>> {
    let mut parts = Vec::new();
    for part in value.split('.') {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        parts.push(part.parse().ok()?);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts)
    }
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    match (parse_version(a), parse_version(b)) {
        (Some(a), Some(b)) => compare_version_parts(&a, &b),
        _ => a.cmp(b),
    }
}

fn compare_version_parts(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
    let len = a.len().max(b.len());
    for idx in 0..len {
        let left = a.get(idx).copied().unwrap_or(0);
        let right = b.get(idx).copied().unwrap_or(0);
        match left.cmp(&right) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

fn rscript_from_r_binary(binary: &Path) -> PathBuf {
    binary.with_file_name(if cfg!(windows) {
        "Rscript.exe"
    } else {
        "Rscript"
    })
}
