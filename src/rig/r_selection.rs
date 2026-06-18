use std::error::Error;

use super::rig_client::InstalledR;

#[derive(Clone, Copy, Debug)]
pub(crate) struct AvailableCandidate<'a> {
    pub(crate) name: &'a str,
    pub(crate) version: &'a str,
    pub(crate) date: Option<&'a str>,
}

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
        .max_by(|a, b| compare_versions(&a.version, &b.version))
}

pub(crate) fn select_available_candidate<'a>(
    req: &str,
    requirement: &VersionRequirement,
    exclude_newer: Option<&str>,
    candidates: impl IntoIterator<Item = AvailableCandidate<'a>>,
) -> Result<AvailableCandidate<'a>, Box<dyn Error>> {
    candidates
        .into_iter()
        .filter(|version| released_before_or_on(version, exclude_newer))
        .filter(|version| requirement.matches_candidate(version.name, version.version, &[]))
        .max_by(|a, b| compare_versions(a.version, b.version))
        .ok_or_else(|| {
            let suffix = exclude_newer
                .map(|date| format!(" before or on {date}"))
                .unwrap_or_default();
            format!("could not resolve R version `{req}` with available R versions{suffix}").into()
        })
}

pub(crate) fn rig_install_hint(requirement: &VersionRequirement) -> Option<&str> {
    match requirement {
        VersionRequirement::Bare(req) => Some(req),
        VersionRequirement::Comparison {
            op: VersionOp::Eq,
            raw,
            ..
        } => Some(raw),
        VersionRequirement::Comparison { .. } => None,
    }
}

pub(crate) fn iso_date_prefix(value: &str) -> Option<&str> {
    let date = value.get(..10)?;
    if is_iso_date(date) {
        Some(date)
    } else {
        None
    }
}

fn released_before_or_on(version: &AvailableCandidate<'_>, exclude_newer: Option<&str>) -> bool {
    let Some(exclude_newer) = exclude_newer else {
        return true;
    };
    let Some(date) = version.date.and_then(iso_date_prefix) else {
        return false;
    };
    date <= exclude_newer
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
