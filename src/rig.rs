use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const EMBEDDED_AVAILABLE_BUILD_DATE: &str = "2026-06-03";
const EMBEDDED_AVAILABLE: &[AvailableCandidate<'static>] = &[
    AvailableCandidate {
        name: "4.1.0",
        version: "4.1.0",
        date: Some("2021-05-18"),
    },
    AvailableCandidate {
        name: "4.1.1",
        version: "4.1.1",
        date: Some("2021-08-10"),
    },
    AvailableCandidate {
        name: "4.1.2",
        version: "4.1.2",
        date: Some("2021-11-01"),
    },
    AvailableCandidate {
        name: "4.1.3",
        version: "4.1.3",
        date: Some("2022-03-10"),
    },
    AvailableCandidate {
        name: "4.2.0",
        version: "4.2.0",
        date: Some("2022-04-22"),
    },
    AvailableCandidate {
        name: "4.2.1",
        version: "4.2.1",
        date: Some("2022-06-23"),
    },
    AvailableCandidate {
        name: "4.2.2",
        version: "4.2.2",
        date: Some("2022-10-31"),
    },
    AvailableCandidate {
        name: "4.2.3",
        version: "4.2.3",
        date: Some("2023-03-15"),
    },
    AvailableCandidate {
        name: "4.3.0",
        version: "4.3.0",
        date: Some("2023-04-21"),
    },
    AvailableCandidate {
        name: "4.3.1",
        version: "4.3.1",
        date: Some("2023-06-16"),
    },
    AvailableCandidate {
        name: "4.3.2",
        version: "4.3.2",
        date: Some("2023-10-31"),
    },
    AvailableCandidate {
        name: "4.3.3",
        version: "4.3.3",
        date: Some("2024-02-29"),
    },
    AvailableCandidate {
        name: "4.4.0",
        version: "4.4.0",
        date: Some("2024-04-24"),
    },
    AvailableCandidate {
        name: "4.4.1",
        version: "4.4.1",
        date: Some("2024-06-14"),
    },
    AvailableCandidate {
        name: "4.4.2",
        version: "4.4.2",
        date: Some("2024-10-31"),
    },
    AvailableCandidate {
        name: "4.4.3",
        version: "4.4.3",
        date: Some("2025-02-28"),
    },
    AvailableCandidate {
        name: "4.5.0",
        version: "4.5.0",
        date: Some("2025-04-11"),
    },
    AvailableCandidate {
        name: "4.5.1",
        version: "4.5.1",
        date: Some("2025-06-13"),
    },
    AvailableCandidate {
        name: "4.5.2",
        version: "4.5.2",
        date: Some("2025-10-31"),
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

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct AvailableR {
    name: String,
    version: String,
    date: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct AvailableRCache {
    known_through: String,
    checked_on: String,
    versions: Vec<AvailableR>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum AvailableRCacheFile {
    Current(AvailableRCache),
    Legacy(Vec<AvailableR>),
}

#[derive(Debug)]
struct InstalledR {
    name: String,
    version: String,
    aliases: Vec<String>,
    default: bool,
    binary: PathBuf,
}

#[derive(Debug, serde::Deserialize)]
struct InstalledRRecord {
    name: Option<String>,
    version: Option<String>,
    aliases: Option<Vec<String>>,
    #[serde(default)]
    default: bool,
    binary: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug)]
struct AvailableCandidate<'a> {
    name: &'a str,
    version: &'a str,
    date: Option<&'a str>,
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
    let installed = rig_list()?;

    if let Some(installed) = installed
        .iter()
        .filter(|version| requirement.matches_installed(version))
        .max_by(|a, b| compare_versions(&a.version, &b.version))
    {
        return installed.rscript();
    }

    let required = required_available_version(req, &requirement, exclude_newer.as_deref())?;
    Err(format!(
        "R {} is required but is not installed. Run `rig install {}`.",
        required.version, required.name
    )
    .into())
}

pub fn resolve_rscript_for_exclude_newer(exclude_newer: &str) -> Result<OsString, Box<dyn Error>> {
    let exclude_newer = parse_iso_date_field("exclude-newer", exclude_newer)?;
    let installed = rig_list()?;
    let embedded_selection =
        select_installed_r_for_date(&installed, EMBEDDED_AVAILABLE, &exclude_newer);
    let embedded_required = required_available_version_for_date_from_candidates(
        &exclude_newer,
        EMBEDDED_AVAILABLE.iter().copied(),
    )
    .ok();
    let has_unknown_installed_release =
        installed_has_unknown_stable_release_newer_than(&installed, embedded_selection);

    let needs_available_refresh = if exclude_newer.as_str() > EMBEDDED_AVAILABLE_BUILD_DATE {
        embedded_selection.is_none() || has_unknown_installed_release
    } else {
        embedded_selection.is_none()
            && (embedded_required.is_none() || has_unknown_installed_release)
    };

    if needs_available_refresh {
        let available = cached_rig_available(&exclude_newer)?;
        let candidates: Vec<_> = available.iter().map(AvailableCandidate::from).collect();
        if let Some(r) = select_installed_r_for_date(&installed, &candidates, &exclude_newer) {
            return r.rscript();
        }
        if embedded_selection.is_none() {
            let required = required_available_version_for_date_from_candidates(
                &exclude_newer,
                candidates.iter().copied(),
            )?;
            return Err(format!(
                "No installed R is available for exclude-newer {}. Run `rig install {}` to install R {}.",
                exclude_newer, required.name, required.version
            )
            .into());
        }
    }

    if let Some(r) = embedded_selection {
        return r.rscript();
    }

    let required = embedded_required.ok_or_else(|| {
        format!("could not resolve an R version available before or on {exclude_newer}")
    })?;
    Err(format!(
        "No installed R is available for exclude-newer {}. Run `rig install {}` to install R {}.",
        exclude_newer, required.name, required.version
    )
    .into())
}

fn select_installed_r_for_date<'a>(
    installed: &'a [InstalledR],
    available: &[AvailableCandidate<'_>],
    exclude_newer: &str,
) -> Option<&'a InstalledR> {
    installed
        .iter()
        .filter(|version| stable_installed_release_candidate(version))
        .filter(|version| installed_released_before_or_on(version, available, exclude_newer))
        .max_by(|a, b| compare_versions(&a.version, &b.version))
}

fn installed_has_unknown_stable_release_newer_than(
    installed: &[InstalledR],
    selected: Option<&InstalledR>,
) -> bool {
    installed.iter().any(|version| {
        stable_installed_release_candidate(version)
            && !EMBEDDED_AVAILABLE
                .iter()
                .any(|available| matches_available_candidate(version, available))
            && selected
                .map(|selected| compare_versions(&version.version, &selected.version).is_gt())
                .unwrap_or(true)
    })
}

/// Rscript of rig's default R install (`"default": true` in `rig list --json`),
/// or `None` when rig is absent, has no default, or the binary is missing.
///
/// Best-effort: the caller falls back to a bare `"Rscript"` on `None`, so any
/// failure here (rig not on PATH, unparseable output) resolves to `None` rather
/// than aborting the run. On rig-managed Windows the only `Rscript` on PATH is a
/// `.bat` shim that `std::process::Command` cannot spawn, so resolving the real
/// `Rscript.exe` from the default install's `binary` is what makes the
/// no-`--r-version` path work there.
pub fn default_rscript() -> Option<OsString> {
    let default = rig_list().ok()?.into_iter().find(|r| r.default)?;
    let rscript = rscript_from_r_binary(&default.binary);
    rscript.exists().then(|| rscript.into_os_string())
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
    let output = rig_output(&["available", "--all", "--json"])?;
    let json = clean_rig_json_output(&output)?;
    parse_rig_available_json(&json)
}

fn cached_rig_available(exclude_newer: &str) -> Result<Vec<AvailableR>, Box<dyn Error>> {
    if let Some(versions) = read_cached_rig_available(exclude_newer)? {
        return Ok(versions);
    }

    let versions = rig_available()?;
    write_cached_rig_available(&versions)?;
    Ok(versions)
}

fn read_cached_rig_available(
    exclude_newer: &str,
) -> Result<Option<Vec<AvailableR>>, Box<dyn Error>> {
    let path = rig_available_cache_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let json = fs::read_to_string(&path)
        .map_err(|e| format!("failed to read `{}`: {e}", path.display()))?;
    let cache_file: AvailableRCacheFile = serde_json::from_str(&json)
        .map_err(|e| format!("failed to parse `{}`: {e}", path.display()))?;
    let mut cache = match cache_file {
        AvailableRCacheFile::Current(cache) => cache,
        AvailableRCacheFile::Legacy(versions) => {
            drop(versions);
            return Ok(None);
        }
    };
    let stored_known_through =
        parse_iso_date_field("rig available cache known_through", &cache.known_through)?;
    let checked_on = parse_iso_date_field("rig available cache checked_on", &cache.checked_on)?;
    normalize_available_release_dates(&mut cache.versions)?;
    let release_known_through =
        latest_available_release_date(&cache.versions).unwrap_or(EMBEDDED_AVAILABLE_BUILD_DATE);
    let known_through = stored_known_through.as_str().min(release_known_through);
    let cache_coverage = known_through.max(checked_on.as_str());
    let today = current_utc_date()?;
    let required_coverage = exclude_newer.min(today.as_str());

    if cache_coverage < required_coverage {
        return Ok(None);
    }

    Ok(Some(cache.versions))
}

fn write_cached_rig_available(versions: &[AvailableR]) -> Result<(), Box<dyn Error>> {
    let path = rig_available_cache_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create `{}`: {e}", parent.display()))?;
    }
    let cache = AvailableRCache {
        known_through: latest_available_release_date(versions)
            .unwrap_or(EMBEDDED_AVAILABLE_BUILD_DATE)
            .to_string(),
        checked_on: current_utc_date()?,
        versions: versions.to_vec(),
    };
    let json = serde_json::to_string_pretty(&cache)
        .map_err(|e| format!("failed to serialize cached rig available JSON: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("failed to write `{}`: {e}", path.display()))?;
    Ok(())
}

fn rig_available_cache_path() -> Result<PathBuf, Box<dyn Error>> {
    Ok(crate::runtime::ir_cache_dir()?
        .join("rig")
        .join("available.json"))
}

fn rig_list() -> Result<Vec<InstalledR>, Box<dyn Error>> {
    let output = rig_output(&["list", "--json"])?;
    let json = clean_rig_json_output(&output)?;
    let versions: Vec<InstalledRRecord> = serde_json::from_str(&json)
        .map_err(|e| format!("failed to parse `rig list --json` JSON: {e}"))?;
    Ok(versions
        .into_iter()
        .filter_map(InstalledR::from_record)
        .collect())
}

fn clean_rig_json_output(output: &[u8]) -> Result<String, Box<dyn Error>> {
    let output = String::from_utf8(output.to_vec())
        .map_err(|e| format!("`rig --json` returned non-UTF-8 output: {e}"))?;
    Ok(output
        .lines()
        .filter(|line| !line.starts_with("[INFO]"))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn rig_output(args: &[&str]) -> Result<Vec<u8>, Box<dyn Error>> {
    let output = Command::new("rig")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                "could not find `rig` on PATH. Install rig to use `r-version` or `exclude-newer`."
                    .to_string()
            } else {
                format!("failed to launch `rig`: {e}")
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`rig {}` failed: {stderr}", args.join(" ")).into());
    }

    Ok(output.stdout)
}

fn required_available_version(
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
    }

    let available = if let Some(exclude_newer) = exclude_newer {
        cached_rig_available(exclude_newer)?
    } else {
        rig_available()?
    };
    required_available_version_from_candidates(
        req,
        requirement,
        exclude_newer,
        available.iter().map(AvailableCandidate::from),
    )
}

fn required_available_version_from_candidates<'a>(
    req: &str,
    requirement: &VersionRequirement,
    exclude_newer: Option<&str>,
    candidates: impl IntoIterator<Item = AvailableCandidate<'a>>,
) -> Result<AvailableR, Box<dyn Error>> {
    candidates
        .into_iter()
        .filter(|version| released_before_or_on(version, exclude_newer))
        .filter(|version| {
            stable_release_candidate(version)
                || requirement.matches_requested_symbolic_candidate(version)
        })
        .filter(|version| requirement.matches_candidate(version.name, version.version, &[]))
        .max_by(|a, b| compare_versions(a.version, b.version))
        .map(AvailableR::from)
        .ok_or_else(|| {
            let suffix = exclude_newer
                .map(|date| format!(" before or on {date}"))
                .unwrap_or_default();
            format!("could not resolve R version `{req}` with available R versions{suffix}").into()
        })
}

fn required_available_version_for_date_from_candidates<'a>(
    exclude_newer: &str,
    candidates: impl IntoIterator<Item = AvailableCandidate<'a>>,
) -> Result<AvailableR, Box<dyn Error>> {
    candidates
        .into_iter()
        .filter(|version| released_before_or_on(version, Some(exclude_newer)))
        .filter(stable_release_candidate)
        .max_by(|a, b| compare_versions(a.version, b.version))
        .map(AvailableR::from)
        .ok_or_else(|| {
            format!("could not resolve an R version available before or on {exclude_newer}").into()
        })
}

fn parse_rig_available_json(json: &str) -> Result<Vec<AvailableR>, Box<dyn Error>> {
    let mut versions: Vec<AvailableR> = serde_json::from_str(json)
        .map_err(|e| format!("failed to parse `rig available --json` JSON: {e}"))?;

    normalize_available_release_dates(&mut versions)?;
    Ok(versions)
}

fn normalize_available_release_dates(versions: &mut [AvailableR]) -> Result<(), Box<dyn Error>> {
    for version in versions {
        if let Some(date) = version.date.as_deref() {
            version.date = Some(
                iso_date_prefix(date)
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

    Ok(())
}

fn latest_available_release_date(versions: &[AvailableR]) -> Option<&str> {
    versions
        .iter()
        .map(AvailableCandidate::from)
        .filter(stable_release_candidate)
        .filter_map(|version| version.date)
        .max()
}

fn current_utc_date() -> Result<String, Box<dyn Error>> {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("failed to determine current time: {e}"))?
        .as_secs()
        / 86_400;
    Ok(unix_days_to_ymd(days as i64))
}

fn unix_days_to_ymd(days: i64) -> String {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y + if m <= 2 { 1 } else { 0 };

    format!("{y:04}-{m:02}-{d:02}")
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

fn stable_release_candidate(version: &AvailableCandidate<'_>) -> bool {
    version.name != "devel" && version.name != "next" && parse_version(version.version).is_some()
}

fn stable_installed_release_candidate(version: &InstalledR) -> bool {
    version.name != "devel"
        && version.name != "next"
        && !version
            .aliases
            .iter()
            .any(|alias| alias == "devel" || alias == "next")
        && parse_version(&version.version).is_some()
}

fn installed_released_before_or_on(
    installed: &InstalledR,
    available: &[AvailableCandidate<'_>],
    exclude_newer: &str,
) -> bool {
    available.iter().any(|version| {
        stable_release_candidate(version)
            && matches_available_candidate(installed, version)
            && released_before_or_on(version, Some(exclude_newer))
    })
}

fn matches_available_candidate(installed: &InstalledR, available: &AvailableCandidate<'_>) -> bool {
    installed.version == available.version
        || installed.name == available.name
        || installed
            .aliases
            .iter()
            .any(|alias| alias == available.name)
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

impl From<AvailableCandidate<'_>> for AvailableR {
    fn from(value: AvailableCandidate<'_>) -> Self {
        Self {
            name: value.name.to_string(),
            version: value.version.to_string(),
            date: value.date.map(str::to_string),
        }
    }
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
    fn matches_installed(&self, installed: &InstalledR) -> bool {
        self.matches_candidate(&installed.name, &installed.version, &installed.aliases)
    }

    fn matches_requested_symbolic_candidate(&self, version: &AvailableCandidate<'_>) -> bool {
        matches!(
            self,
            VersionRequirement::Bare(req)
                if (version.name == "devel" || version.name == "next") && req == version.name
        )
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
                    VersionOp::Eq => compare_version_parts(&candidate, required_version).is_eq(),
                }
            }
        }
    }
}

impl InstalledR {
    fn from_record(record: InstalledRRecord) -> Option<Self> {
        Some(Self {
            name: record.name?,
            version: record.version?,
            aliases: record.aliases.unwrap_or_default(),
            default: record.default,
            binary: record.binary?,
        })
    }

    fn rscript(&self) -> Result<OsString, Box<dyn Error>> {
        let rscript = rscript_from_r_binary(&self.binary);
        if !rscript.exists() {
            return Err(format!(
                "rig reported R {} at `{}`, but `{}` does not exist",
                self.version,
                self.binary.display(),
                rscript.display()
            )
            .into());
        }

        Ok(rscript.into_os_string())
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
