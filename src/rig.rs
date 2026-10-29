use std::error::Error;
use std::ffi::OsString;

mod r_selection;
mod rig_client;
mod rig_releases;

pub fn resolve_rscript(req: &str, exclude_newer: Option<&str>) -> Result<OsString, Box<dyn Error>> {
    let exclude_newer = exclude_newer
        .map(|value| r_selection::parse_iso_date_field("exclude-newer", value))
        .transpose()?;
    let requirement = r_selection::parse_version_requirement(req)?;
    let latest_minor = exclude_newer
        .as_deref()
        .filter(|_| requirement.is_broad())
        .map(rig_releases::latest_minor_version_on)
        .transpose()?;

    // Preserve selection from rig's configured installation scope, then reuse
    // user installations before asking rig to download anything.
    for installed in [rig_client::list, rig_client::list_user] {
        let installed = installed()?;
        let selected = match latest_minor.as_deref() {
            Some(minor) => {
                r_selection::select_installed_r_through_minor(&requirement, &installed, minor)
            }
            None => r_selection::select_installed_r(&requirement, &installed),
        };
        if let Some(selected) = selected {
            return selected.rscript();
        }
    }

    let version = if requirement.is_broad() {
        let available = rig_client::available()?;
        r_selection::select_available_r(&requirement, &available, latest_minor.as_deref())
            .ok_or_else(|| match exclude_newer {
                Some(date) => format!(
                    "no released R available to install matches `r-version: {req}` and `exclude-newer: {date}`"
                ),
                None => format!("no released R available to install matches `r-version: {req}`"),
            })?
            .version
            .clone()
    } else {
        r_selection::rig_install_request(&requirement)
            .expect("non-broad requirements are version numbers or aliases")
            .to_string()
    };
    let installed = rig_client::install(&version)?;
    r_selection::check_installed_version(&version, &installed.version)?;
    installed.rscript()
}

pub fn resolve_rscript_for_exclude_newer(exclude_newer: &str) -> Result<OsString, Box<dyn Error>> {
    let exclude_newer = r_selection::parse_iso_date_field("exclude-newer", exclude_newer)?;
    let req = rig_releases::latest_minor_version_on(&exclude_newer)?;
    resolve_rscript(&req, None)
}
