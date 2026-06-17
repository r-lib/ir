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
    let installed = rig_client::list()?;

    if let Some(installed) = r_selection::select_installed_r(&requirement, &installed) {
        return installed.rscript();
    }

    let required =
        rig_releases::required_available_version(req, &requirement, exclude_newer.as_deref())?;
    Err(format!(
        "R {} is required but is not installed. Run `rig install {}`.",
        required.version, required.name
    )
    .into())
}

pub fn resolve_rscript_for_exclude_newer(exclude_newer: &str) -> Result<OsString, Box<dyn Error>> {
    let exclude_newer = r_selection::parse_iso_date_field("exclude-newer", exclude_newer)?;
    let req = rig_releases::latest_minor_version_on(&exclude_newer)?;
    let requirement = r_selection::parse_version_requirement(&req)?;
    let installed = rig_client::list()?;

    if let Some(installed) = r_selection::select_installed_r(&requirement, &installed) {
        return installed.rscript();
    }

    Err(format!(
        "`exclude-newer` {exclude_newer} implies `r-version: {req}` because R {req} was the latest R minor version available on that date, but no matching R is installed. Run `rig install {req}`, or specify `r-version` or `--r-version` to use a different installed R."
    )
    .into())
}
