use std::borrow::Cow;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ReleaseMetadata<'a> {
    pub(crate) name: Cow<'a, str>,
    pub(crate) version: Cow<'a, str>,
    pub(crate) date: Cow<'a, str>,
}

#[derive(serde::Deserialize)]
struct RVersionRelease {
    #[serde(default)]
    name: Option<String>,
    version: String,
    #[serde(default)]
    semver: Option<String>,
    #[serde(default)]
    date: Option<String>,
}

pub(crate) fn parse_release_metadata_json(
    json: &str,
    source: &str,
) -> Result<Vec<ReleaseMetadata<'static>>, String> {
    let versions: Vec<RVersionRelease> =
        serde_json::from_str(json).map_err(|e| format!("failed to parse {source} JSON: {e}"))?;
    let mut releases = Vec::new();

    for version in versions {
        let name = version.name.as_deref().unwrap_or(&version.version);
        if matches!(name, "devel" | "next") {
            continue;
        };
        let raw_version = version.semver.as_deref().unwrap_or(&version.version);
        let Some(release_version) = release_version(raw_version) else {
            continue;
        };
        let Some(date) = version.date.as_deref() else {
            continue;
        };
        let date = iso_date_prefix(date)
            .ok_or_else(|| {
                format!(
                    "R version availability metadata returned invalid release date `{}` for R {}",
                    date, release_version
                )
            })?
            .to_string();
        releases.push(ReleaseMetadata {
            name: Cow::Owned(name.to_string()),
            version: Cow::Owned(release_version),
            date: Cow::Owned(date),
        });
    }

    if releases.is_empty() {
        return Err(format!(
            "R version availability metadata in {source} did not contain any R releases"
        ));
    }

    Ok(releases)
}

fn release_version(semver: &str) -> Option<String> {
    let mut parts = semver.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    let patch = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    for part in [major, minor, patch] {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
    }

    Some(semver.to_string())
}

fn iso_date_prefix(value: &str) -> Option<&str> {
    let date = value.get(..10)?;
    if is_iso_date(date) {
        Some(date)
    } else {
        None
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
