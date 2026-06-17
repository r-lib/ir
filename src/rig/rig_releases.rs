use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use super::r_selection::{self, AvailableCandidate, VersionRequirement};
use super::rig_client::{self, AvailableR};

const EMBEDDED_AVAILABLE_BUILD_DATE: &str = "2026-06-03";
const EMBEDDED_AVAILABLE: &[AvailableCandidate<'static>] = &[
    AvailableCandidate {
        name: "4.1.3",
        version: "4.1.3",
        date: Some("2022-03-10"),
    },
    AvailableCandidate {
        name: "4.2.3",
        version: "4.2.3",
        date: Some("2023-03-15"),
    },
    AvailableCandidate {
        name: "4.3.3",
        version: "4.3.3",
        date: Some("2024-02-29"),
    },
    AvailableCandidate {
        name: "4.4.3",
        version: "4.4.3",
        date: Some("2025-02-28"),
    },
    AvailableCandidate {
        name: "4.5.3",
        version: "4.5.3",
        date: Some("2026-03-11"),
    },
    AvailableCandidate {
        name: "4.6.0",
        version: "4.6.0",
        date: Some("2026-04-24"),
    },
];

// R-x-y-0 release dates from upstream R tags. These track when a minor line
// became available; `rig available` tracks patch releases for installation.
const EMBEDDED_MINOR_RELEASES_BUILD_DATE: &str = "2026-06-17";

struct MinorRelease {
    version: &'static str,
    date: &'static str,
}

const EMBEDDED_MINOR_RELEASES: &[MinorRelease] = &[
    MinorRelease {
        version: "3.0",
        date: "2013-04-03",
    },
    MinorRelease {
        version: "3.1",
        date: "2014-04-10",
    },
    MinorRelease {
        version: "3.2",
        date: "2015-04-16",
    },
    MinorRelease {
        version: "3.3",
        date: "2016-05-03",
    },
    MinorRelease {
        version: "3.4",
        date: "2017-04-21",
    },
    MinorRelease {
        version: "3.5",
        date: "2018-04-23",
    },
    MinorRelease {
        version: "3.6",
        date: "2019-04-26",
    },
    MinorRelease {
        version: "4.0",
        date: "2020-04-24",
    },
    MinorRelease {
        version: "4.1",
        date: "2021-05-18",
    },
    MinorRelease {
        version: "4.2",
        date: "2022-04-22",
    },
    MinorRelease {
        version: "4.3",
        date: "2023-04-21",
    },
    MinorRelease {
        version: "4.4",
        date: "2024-04-24",
    },
    MinorRelease {
        version: "4.5",
        date: "2025-04-11",
    },
    MinorRelease {
        version: "4.6",
        date: "2026-04-24",
    },
];

pub(crate) fn required_available_version(
    req: &str,
    requirement: &VersionRequirement,
    exclude_newer: Option<&str>,
) -> Result<AvailableR, Box<dyn Error>> {
    if let Some(exclude_newer) = exclude_newer {
        if exclude_newer <= EMBEDDED_AVAILABLE_BUILD_DATE {
            return required_available_version_from_candidates(
                req,
                requirement,
                Some(exclude_newer),
                EMBEDDED_AVAILABLE.iter().copied(),
            );
        }

        let available = cached_available()?;
        return required_available_version_from_candidates(
            req,
            requirement,
            Some(exclude_newer),
            available.iter().map(AvailableCandidate::from),
        );
    }

    let available = rig_client::available()?;
    required_available_version_from_candidates(
        req,
        requirement,
        None,
        available.iter().map(AvailableCandidate::from),
    )
}

pub(crate) fn latest_minor_version_on(exclude_newer: &str) -> Result<String, Box<dyn Error>> {
    if exclude_newer <= EMBEDDED_MINOR_RELEASES_BUILD_DATE {
        let release = r_selection::select_latest_available_candidate(
            exclude_newer,
            EMBEDDED_MINOR_RELEASES.iter().map(AvailableCandidate::from),
        )?;
        return Ok(release.version.to_string());
    }

    let available = refresh_all_available_cache()?;
    let available_minor_releases = available_minor_releases(&available)?;
    let release = r_selection::select_latest_available_candidate(
        exclude_newer,
        EMBEDDED_MINOR_RELEASES
            .iter()
            .map(AvailableCandidate::from)
            .chain(
                available_minor_releases
                    .iter()
                    .map(AvailableCandidate::from),
            ),
    )?;
    Ok(release.version.to_string())
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

fn cached_available() -> Result<Vec<AvailableR>, Box<dyn Error>> {
    let path = available_cache_path()?;
    if path.exists() {
        let json = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read `{}`: {e}", path.display()))?;
        return parse_available_json(&json);
    }

    fetch_available_into_cache(&path, &["available", "--json"])
}

fn refresh_all_available_cache() -> Result<Vec<AvailableR>, Box<dyn Error>> {
    let path = all_available_cache_path()?;
    fetch_available_into_cache(&path, &["available", "--all", "--json"])
}

fn available_cache_path() -> Result<PathBuf, Box<dyn Error>> {
    Ok(crate::runtime::ir_cache_dir()?
        .join("rig")
        .join("available.json"))
}

fn all_available_cache_path() -> Result<PathBuf, Box<dyn Error>> {
    Ok(crate::runtime::ir_cache_dir()?
        .join("rig")
        .join("available-all.json"))
}

fn fetch_available_into_cache(
    path: &Path,
    args: &[&str],
) -> Result<Vec<AvailableR>, Box<dyn Error>> {
    let json = String::from_utf8(rig_client::output(args)?)
        .map_err(|e| format!("`rig {}` returned non-UTF-8 output: {e}", args.join(" ")))?;
    let available = parse_available_json(&json)?;
    let json = serde_json::to_string_pretty(&available)
        .map_err(|e| format!("failed to serialize cached rig available JSON: {e}"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create `{}`: {e}", parent.display()))?;
    }
    fs::write(path, json).map_err(|e| format!("failed to write `{}`: {e}", path.display()))?;
    Ok(available)
}

fn available_minor_releases(available: &[AvailableR]) -> Result<Vec<AvailableR>, Box<dyn Error>> {
    available
        .iter()
        .filter_map(|release| {
            if release.name != release.version || !is_minor_zero_release(&release.version) {
                return None;
            }

            Some(
                r_selection::major_minor_version(&release.version).map(|version| AvailableR {
                    name: version.clone(),
                    version,
                    date: release.date.clone(),
                }),
            )
        })
        .collect()
}

fn is_minor_zero_release(version: &str) -> bool {
    let mut parts = version.split('.');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(_), Some(_), Some("0"), None)
    )
}

fn parse_available_json(json: &str) -> Result<Vec<AvailableR>, Box<dyn Error>> {
    let mut versions: Vec<AvailableR> = serde_json::from_str(json)
        .map_err(|e| format!("failed to parse `rig available --json` JSON: {e}"))?;

    for version in &mut versions {
        if let Some(date) = version.date.as_deref() {
            version.date = Some(
                r_selection::iso_date_prefix(date)
                    .ok_or_else(|| {
                        format!(
                            "rig available returned invalid release date `{}` for R {}",
                            date, version.version
                        )
                    })?
                    .to_string(),
            );
        }
    }

    Ok(versions)
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

impl<'a> From<&'a MinorRelease> for AvailableCandidate<'a> {
    fn from(value: &'a MinorRelease) -> Self {
        Self {
            name: value.version,
            version: value.version,
            date: Some(value.date),
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
