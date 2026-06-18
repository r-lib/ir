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
    Err(format!(
        "R {} is required but is not installed. Run `rig install {}`.",
        required.version, required.name
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
