use std::error::Error;

use super::rig_client::{AvailableR, InstalledR};

#[derive(Debug)]
pub(crate) enum VersionRequirement {
    Bare(String),
    Comparison {
        op: VersionOp,
        version: Vec<u64>,
        raw: String,
    },
}

#[derive(Debug)]
pub(crate) enum VersionOp {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
}

pub(crate) enum InstallRequest<'a> {
    Direct(&'a str),
    Resolve(&'a str),
    Available,
}

pub(crate) fn parse_iso_date_field(key: &str, value: &str) -> Result<String, Box<dyn Error>> {
    let value = value.trim();
    if !is_iso_date(value) {
        return Err(format!("`{key}` must be a date string in YYYY-MM-DD format").into());
    }
    Ok(value.to_string())
}

pub(crate) fn parse_version_requirement(req: &str) -> Result<VersionRequirement, Box<dyn Error>> {
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

pub(crate) fn select_installed_r<'a>(
    requirement: &VersionRequirement,
    installed: &'a [InstalledR],
) -> Option<&'a InstalledR> {
    installed
        .iter()
        .filter(|version| requirement.matches_installed(version))
        .max_by(|a, b| compare_installed_r(a, b))
}

pub(crate) fn select_available_r<'a>(
    req: &str,
    requirement: &VersionRequirement,
    available: &'a [AvailableR],
) -> Result<&'a AvailableR, Box<dyn Error>> {
    available
        .iter()
        .filter(|candidate| !matches!(candidate.name.as_str(), "devel" | "next"))
        .filter(|candidate| requirement.matches_candidate(&candidate.name, &candidate.version, &[]))
        .max_by(|a, b| compare_versions(&a.version, &b.version))
        .ok_or_else(|| format!("no R release available through rig matches `{req}`").into())
}

pub(crate) fn install_request(
    requirement: &VersionRequirement,
) -> Result<InstallRequest<'_>, Box<dyn Error>> {
    match requirement {
        VersionRequirement::Bare(req)
            if parse_version(req).is_some_and(|version| version.len() == 1) =>
        {
            Ok(InstallRequest::Available)
        }
        VersionRequirement::Bare(req) if parse_version(req).is_some() => {
            Ok(InstallRequest::Direct(req))
        }
        VersionRequirement::Bare(req) if matches!(req.as_str(), "release" | "oldrel") => {
            Ok(InstallRequest::Resolve(req))
        }
        VersionRequirement::Bare(req) if is_numbered_oldrel(req) => {
            Ok(InstallRequest::Resolve(req))
        }
        VersionRequirement::Bare(req) if matches!(req.as_str(), "devel" | "next") => {
            Ok(InstallRequest::Direct(req))
        }
        VersionRequirement::Bare(req) => Err(format!(
            "cannot automatically install unsupported `r-version` selector `{req}`; use a numeric version, `release`, `oldrel`, `oldrel/N`, `devel`, or `next`"
        )
        .into()),
        VersionRequirement::Comparison {
            op: VersionOp::Eq,
            version,
            raw,
            ..
        } if version.len() > 1 => Ok(InstallRequest::Direct(raw)),
        VersionRequirement::Comparison { .. } => Ok(InstallRequest::Available),
    }
}

pub(crate) fn is_numeric_version(value: &str) -> bool {
    parse_version(value).is_some()
}

impl VersionRequirement {
    fn matches_installed(&self, installed: &InstalledR) -> bool {
        self.matches_candidate(&installed.name, &installed.version, &installed.aliases)
    }

    fn matches_candidate(&self, name: &str, candidate_version: &str, aliases: &[String]) -> bool {
        match self {
            VersionRequirement::Bare(req) => {
                name == req
                    || candidate_version == req
                    || aliases.iter().any(|alias| alias == req)
                    || parse_version(req)
                        .map(|_| candidate_version.starts_with(&format!("{req}.")))
                        .unwrap_or(false)
            }
            VersionRequirement::Comparison {
                op,
                version: required_version,
                raw,
            } => {
                let Some(candidate) = parse_version(candidate_version) else {
                    return false;
                };
                if matches!(op, VersionOp::Eq)
                    && (name == raw || aliases.iter().any(|alias| alias == raw))
                {
                    return true;
                }
                match op {
                    VersionOp::Gt => compare_version_parts(&candidate, required_version).is_gt(),
                    VersionOp::Gte => compare_version_parts(&candidate, required_version).is_ge(),
                    VersionOp::Lt => compare_version_parts(&candidate, required_version).is_lt(),
                    VersionOp::Lte => compare_version_parts(&candidate, required_version).is_le(),
                    VersionOp::Eq if required_version.len() < 3 => {
                        candidate.starts_with(required_version)
                    }
                    VersionOp::Eq => compare_version_parts(&candidate, required_version).is_eq(),
                }
            }
        }
    }
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

fn is_numbered_oldrel(value: &str) -> bool {
    value
        .strip_prefix("oldrel/")
        .and_then(|number| number.parse::<u64>().ok())
        .is_some_and(|number| number > 0)
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    match (parse_version(a), parse_version(b)) {
        (Some(a), Some(b)) => compare_version_parts(&a, &b),
        _ => a.cmp(b),
    }
}

fn compare_installed_r(a: &InstalledR, b: &InstalledR) -> std::cmp::Ordering {
    compare_versions(&a.version, &b.version)
        .then_with(|| a.is_default.cmp(&b.is_default))
        .then_with(|| native_macos_r_preference(a).cmp(&native_macos_r_preference(b)))
}

fn native_macos_r_preference(installed: &InstalledR) -> u8 {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return 0;
    }
    if rig_metadata_contains(installed, "-arm64") {
        return 2;
    }
    if rig_metadata_contains(installed, "-x86_64") {
        return 0;
    }
    1
}

fn rig_metadata_contains(installed: &InstalledR, needle: &str) -> bool {
    installed.name.contains(needle)
        || installed
            .path
            .as_deref()
            .map(|path| path.to_string_lossy().contains(needle))
            .unwrap_or(false)
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
