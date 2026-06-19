use std::error::Error;
use std::ffi::OsString;

mod r_selection;
mod rig_client;
mod rig_releases;

pub fn resolve_rscript(req: &str, exclude_newer: Option<&str>) -> Result<OsString, Box<dyn Error>> {
    if let Some(exclude_newer) = exclude_newer {
        r_selection::parse_iso_date_field("exclude-newer", exclude_newer)?;
    }
    let requirement = r_selection::parse_version_requirement(req)?;
    let installed = rig_client::list()?;

    if let Some(installed) = r_selection::select_installed_r(&requirement, &installed) {
        return installed.rscript();
    }

    Err(missing_r_version_error(req, &requirement).into())
}

pub fn resolve_rscript_for_exclude_newer(exclude_newer: &str) -> Result<OsString, Box<dyn Error>> {
    let exclude_newer = r_selection::parse_iso_date_field("exclude-newer", exclude_newer)?;
    let installed = rig_client::list()?;

    if let Some(installed) = rig_releases::select_installed_r_on(&installed, &exclude_newer)? {
        return installed.rscript();
    }

    Err(format!(
        "no installed R release is available on or before `exclude-newer` {exclude_newer}. Install an R release available by that date, set `IR_RSCRIPT`, pass `--rscript`, or specify `r-version` or `--r-version`."
    )
    .into())
}

fn missing_r_version_error(req: &str, requirement: &r_selection::VersionRequirement) -> String {
    if let Some(version) = r_selection::rig_install_hint(requirement) {
        return format!(
            "R {version} is required but is not installed. Run `rig install {version}`."
        );
    }

    format!(
        "R {req} is required but no matching R is installed. Install a matching R with `rig install`, or specify a different `r-version` or `--r-version`."
    )
}
