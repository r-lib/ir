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

    if exclude_newer.is_none() {
        return Err(missing_r_version_error(req, &requirement).into());
    }

    let required =
        rig_releases::required_available_version(req, &requirement, exclude_newer.as_deref())?;
    let requested = r_selection::rig_install_hint(&requirement);
    let required_version = requested.unwrap_or(&required.version);
    let install_hint = requested.unwrap_or(&required.name);
    Err(format!(
        "R {required_version} is required but is not installed. Run `rig install {install_hint}`."
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
        "`exclude-newer` {exclude_newer} implies `r-version: {req}` because R {req} was the latest R minor version available on that date, but no matching R is installed. Run `rig install {req}`, set `IR_RSCRIPT`, or specify `r-version` or `--r-version`."
    )
    .into())
}

fn missing_r_version_error(req: &str, requirement: &r_selection::VersionRequirement) -> String {
    if let Some(version) = r_selection::rig_install_hint(requirement) {
        return format!("R {req} is required but is not installed. Run `rig install {version}`.");
    }

    format!(
        "R {req} is required but no matching R is installed. Install a matching R with `rig install`, or specify a different `r-version` or `--r-version`."
    )
}
