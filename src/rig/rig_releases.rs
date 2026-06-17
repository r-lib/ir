use std::error::Error;
use std::fs;

use super::r_selection::{self, AvailableCandidate, VersionRequirement};
use super::release_metadata::{parse_release_metadata_json, ReleaseMetadata};
use super::rig_client::{self, AvailableR};

include!(concat!(env!("OUT_DIR"), "/r_version_releases.rs"));

pub(crate) fn required_available_version(
    req: &str,
    requirement: &VersionRequirement,
    exclude_newer: Option<&str>,
) -> Result<AvailableR, Box<dyn Error>> {
    if let Some(exclude_newer) = exclude_newer {
        if exclude_newer <= EMBEDDED_AVAILABLE_METADATA_DATE {
            let embedded = required_available_version_from_candidates(
                req,
                requirement,
                Some(exclude_newer),
                EMBEDDED_R_RELEASES.iter().map(AvailableCandidate::from),
            );
            if embedded.is_ok() || requirement_uses_symbolic_name(requirement) {
                return required_available_version_from_host(req, requirement, Some(exclude_newer));
            }
            return embedded;
        }

        return required_available_version_from_host(req, requirement, Some(exclude_newer));
    }

    let available = rig_client::available()?;
    required_available_version_from_candidates(
        req,
        requirement,
        None,
        available.iter().map(AvailableCandidate::from),
    )
}

fn required_available_version_from_candidates<'a>(
    req: &str,
    requirement: &VersionRequirement,
    exclude_newer: Option<&str>,
    candidates: impl IntoIterator<Item = AvailableCandidate<'a>>,
) -> Result<AvailableR, Box<dyn Error>> {
    r_selection::select_available_candidate(req, requirement, exclude_newer, candidates)
        .map(AvailableR::from)
}

fn required_available_version_from_host(
    req: &str,
    requirement: &VersionRequirement,
    exclude_newer: Option<&str>,
) -> Result<AvailableR, Box<dyn Error>> {
    if requirement_uses_symbolic_name(requirement) {
        let available = rig_client::available()?;
        return required_available_version_from_candidates(
            req,
            requirement,
            exclude_newer,
            available.iter().map(AvailableCandidate::from),
        );
    }

    let available = cached_release_metadata()?;
    required_available_version_from_candidates(
        req,
        requirement,
        exclude_newer,
        available.iter().map(AvailableCandidate::from),
    )
}

fn cached_release_metadata() -> Result<Vec<ReleaseMetadata<'static>>, Box<dyn Error>> {
    let path = crate::runtime::ir_cache_dir()?
        .join("r-versions")
        .join("available.json");
    if path.exists() {
        let json = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read `{}`: {e}", path.display()))?;
        return parse_release_metadata_json(&json, "R version availability metadata cache")
            .map_err(|e| -> Box<dyn Error> { e.into() });
    }

    let json = download_available_json()?;
    let available = parse_release_metadata_json(&json, "R version availability metadata")
        .map_err(|e| -> Box<dyn Error> { e.into() })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create `{}`: {e}", parent.display()))?;
    }
    fs::write(&path, json).map_err(|e| format!("failed to write `{}`: {e}", path.display()))?;
    Ok(available)
}

fn download_available_json() -> Result<String, Box<dyn Error>> {
    let json = String::from_utf8(rig_client::output(&["available", "--json", "--all"])?)
        .map_err(|e| format!("`rig available --json --all` returned non-UTF-8 output: {e}"))?;
    if json.trim().is_empty() {
        return Err("`rig available --json --all` returned empty output".into());
    }

    Ok(json)
}

fn requirement_uses_symbolic_name(requirement: &VersionRequirement) -> bool {
    matches!(requirement, VersionRequirement::Bare(req) if parse_version(req).is_none())
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

impl<'a> From<&'a AvailableR> for AvailableCandidate<'a> {
    fn from(value: &'a AvailableR) -> Self {
        Self {
            name: &value.name,
            version: &value.version,
            date: value.date.as_deref(),
        }
    }
}

impl<'a, 'b> From<&'a ReleaseMetadata<'b>> for AvailableCandidate<'a> {
    fn from(value: &'a ReleaseMetadata<'b>) -> Self {
        Self {
            name: value.name.as_ref(),
            version: value.version.as_ref(),
            date: Some(value.date.as_ref()),
        }
    }
}

impl From<AvailableCandidate<'_>> for AvailableR {
    fn from(value: AvailableCandidate<'_>) -> Self {
        Self {
            name: value.name.to_string(),
            version: value.version.to_string(),
            date: value.date.map(str::to_string),
        }
    }
}
