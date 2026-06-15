use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const EMBEDDED_AVAILABLE_BUILD_DATE: &str = "2026-06-03";
const EMBEDDED_AVAILABLE: &[AvailableCandidate<'static>] = &[
    AvailableCandidate {
        name: "3.6.0",
        version: "3.6.0",
        date: Some("2019-04-26"),
    },
    AvailableCandidate {
        name: "3.6.1",
        version: "3.6.1",
        date: Some("2019-07-05"),
    },
    AvailableCandidate {
        name: "3.6.2",
        version: "3.6.2",
        date: Some("2019-12-12"),
    },
    AvailableCandidate {
        name: "3.6.3",
        version: "3.6.3",
        date: Some("2020-02-29"),
    },
    AvailableCandidate {
        name: "4.0.0",
        version: "4.0.0",
        date: Some("2020-04-24"),
    },
    AvailableCandidate {
        name: "4.0.1",
        version: "4.0.1",
        date: Some("2020-06-06"),
    },
    AvailableCandidate {
        name: "4.0.2",
        version: "4.0.2",
        date: Some("2020-06-22"),
    },
    AvailableCandidate {
        name: "4.0.3",
        version: "4.0.3",
        date: Some("2020-10-10"),
    },
    AvailableCandidate {
        name: "4.0.4",
        version: "4.0.4",
        date: Some("2021-02-15"),
    },
    AvailableCandidate {
        name: "4.0.5",
        version: "4.0.5",
        date: Some("2021-03-31"),
    },
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

#[derive(Debug, serde::Deserialize, serde::Serialize)]
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
    #[serde(default)]
    default: bool,
    binary: PathBuf,
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
    let available = available_for_exclude_newer(&exclude_newer, &installed)?;

    if let Some(installed) = installed
        .iter()
        .filter(|version| !installed_is_symbolic_prerelease(version))
        .filter(|version| installed_released_before_or_on(version, &available, &exclude_newer))
        .max_by(|a, b| compare_versions(&a.version, &b.version))
    {
        return installed.rscript();
    }

    let required = required_available_version_for_date(&available, &exclude_newer)?;
    Err(format!(
        "No installed R is available for exclude-newer {}. Run `rig install {}` to install R {}.",
        exclude_newer, required.name, required.version
    )
    .into())
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
    rig_json(&["available", "--all", "--json"])
}

fn rig_list() -> Result<Vec<InstalledR>, Box<dyn Error>> {
    rig_json(&["list", "--json"])
}

fn rig_json<T: serde::de::DeserializeOwned>(args: &[&str]) -> Result<T, Box<dyn Error>> {
    let output = rig_output(args)?;
    let json = clean_rig_json_output(&output)?;

    serde_json::from_str(&json)
        .map_err(|e| format!("failed to parse `rig {}` JSON: {e}", args.join(" ")).into())
}

fn clean_rig_json_output(output: &[u8]) -> Result<String, Box<dyn Error>> {
    let output = std::str::from_utf8(output)
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
                "could not find `rig` on PATH. Install rig to use `r-version`.".to_string()
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
        if embedded_available_covers_exclude_newer(exclude_newer) {
            return required_available_version_from_candidates(
                req,
                requirement,
                Some(exclude_newer),
                EMBEDDED_AVAILABLE.iter().copied(),
            );
        }

        let available = cached_rig_available()?;
        return required_available_version_from_candidates(
            req,
            requirement,
            Some(exclude_newer),
            available.iter().map(AvailableCandidate::from),
        );
    }

    let available = rig_available()?;
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
    candidates
        .into_iter()
        .filter(|version| released_before_or_on(version, exclude_newer))
        .filter(|version| requirement.matches_candidate(version.name, version.version, &[]))
        .max_by(|a, b| compare_versions(a.version, b.version))
        .map(|candidate| AvailableR::from(&candidate))
        .ok_or_else(|| {
            let suffix = exclude_newer
                .map(|date| format!(" before or on {date}"))
                .unwrap_or_default();
            format!("could not resolve R version `{req}` with available R versions{suffix}").into()
        })
}

fn available_for_exclude_newer(
    exclude_newer: &str,
    installed: &[InstalledR],
) -> Result<Vec<AvailableR>, Box<dyn Error>> {
    if embedded_available_covers_exclude_newer(exclude_newer)
        && installed
            .iter()
            .all(|version| embedded_available_can_decide_installed(version, exclude_newer))
    {
        return Ok(EMBEDDED_AVAILABLE.iter().map(AvailableR::from).collect());
    }

    cached_rig_available()
}

fn embedded_available_covers_exclude_newer(exclude_newer: &str) -> bool {
    exclude_newer <= EMBEDDED_AVAILABLE_BUILD_DATE
        && latest_embedded_available_for_exclude_newer(exclude_newer).is_some()
}

fn latest_embedded_available_for_exclude_newer(
    exclude_newer: &str,
) -> Option<AvailableCandidate<'static>> {
    EMBEDDED_AVAILABLE
        .iter()
        .copied()
        .filter(|version| released_before_or_on(version, Some(exclude_newer)))
        .max_by(|a, b| compare_versions(a.version, b.version))
}

fn embedded_available_can_decide_installed(installed: &InstalledR, exclude_newer: &str) -> bool {
    if embedded_available_includes_installed(installed) {
        return true;
    }

    latest_embedded_available_for_exclude_newer(exclude_newer)
        .map(|version| compare_versions(&installed.version, version.version).is_gt())
        .unwrap_or(false)
}

fn embedded_available_includes_installed(installed: &InstalledR) -> bool {
    installed_is_symbolic_prerelease(installed)
        || EMBEDDED_AVAILABLE
            .iter()
            .any(|version| available_candidate_matches_installed(version, installed))
}

fn installed_released_before_or_on(
    installed: &InstalledR,
    available: &[AvailableR],
    exclude_newer: &str,
) -> bool {
    available.iter().any(|version| {
        available_name_is_concrete(&version.name)
            && available_matches_installed(version, installed)
            && version
                .date
                .as_deref()
                .and_then(iso_date_prefix)
                .map(|date| date <= exclude_newer)
                .unwrap_or(false)
    })
}

fn available_matches_installed(available: &AvailableR, installed: &InstalledR) -> bool {
    available.version == installed.version
        || available.name == installed.name
        || installed
            .aliases
            .iter()
            .any(|alias| alias == &available.name)
}

fn available_candidate_matches_installed(
    available: &AvailableCandidate<'_>,
    installed: &InstalledR,
) -> bool {
    available.version == installed.version
        || available.name == installed.name
        || installed
            .aliases
            .iter()
            .any(|alias| alias == available.name)
}

fn required_available_version_for_date(
    available: &[AvailableR],
    exclude_newer: &str,
) -> Result<AvailableR, Box<dyn Error>> {
    available
        .iter()
        .map(AvailableCandidate::from)
        .filter(|version| available_name_is_concrete(version.name))
        .filter(|version| released_before_or_on(version, Some(exclude_newer)))
        .max_by(|a, b| compare_versions(a.version, b.version))
        .map(|candidate| AvailableR::from(&candidate))
        .ok_or_else(|| {
            format!("could not resolve an R version before or on {exclude_newer}").into()
        })
}

fn installed_is_symbolic_prerelease(installed: &InstalledR) -> bool {
    symbolic_prerelease_name(&installed.name)
        || installed
            .aliases
            .iter()
            .any(|alias| symbolic_prerelease_name(alias))
}

fn symbolic_prerelease_name(value: &str) -> bool {
    matches!(value, "devel" | "next")
}

fn available_name_is_concrete(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_digit())
}

fn cached_rig_available() -> Result<Vec<AvailableR>, Box<dyn Error>> {
    let path = crate::runtime::ir_cache_dir()?
        .join("rig")
        .join("available-all.json");
    if path.exists() {
        let json = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read `{}`: {e}", path.display()))?;
        return parse_rig_available_json(&json);
    }

    let available = rig_available()?;
    let json = serde_json::to_string_pretty(&available)
        .map_err(|e| format!("failed to serialize cached rig available JSON: {e}"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create `{}`: {e}", parent.display()))?;
    }
    fs::write(&path, json).map_err(|e| format!("failed to write `{}`: {e}", path.display()))?;
    Ok(available)
}

fn parse_rig_available_json(json: &str) -> Result<Vec<AvailableR>, Box<dyn Error>> {
    let mut versions: Vec<AvailableR> = serde_json::from_str(json)
        .map_err(|e| format!("failed to parse `rig available --all --json` JSON: {e}"))?;

    for version in &mut versions {
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

    Ok(versions)
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

impl<'a> From<&'a AvailableR> for AvailableCandidate<'a> {
    fn from(value: &'a AvailableR) -> Self {
        Self {
            name: &value.name,
            version: &value.version,
            date: value.date.as_deref(),
        }
    }
}

impl From<&AvailableCandidate<'_>> for AvailableR {
    fn from(value: &AvailableCandidate<'_>) -> Self {
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
