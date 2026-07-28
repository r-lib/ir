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

    if let Some(exclude_newer) = exclude_newer.filter(|_| requirement.is_broad()) {
        let latest_minor = rig_releases::latest_minor_version_on(&exclude_newer)?;
        if let Some(installed) =
            r_selection::select_installed_r_through_minor(&requirement, &installed, &latest_minor)
        {
            return installed.rscript();
        }

        return Err(format!(
            "`r-version: {req}` and `exclude-newer: {exclude_newer}` have no matching installed R. R {latest_minor} was the latest R minor released by that date. Install a compatible R with `rig install`, or adjust `r-version` or `exclude-newer`."
        )
        .into());
    }

    if let Some(installed) = r_selection::select_installed_r(&requirement, &installed) {
        return installed.rscript();
    }

    if r_selection::has_matching_devel(&requirement, &installed) {
        return Err(format!(
            "`r-version: {req}` matches only installed R-devel. Requirements that can match more than one R minor select released versions. Install a matching release with `rig install`, or opt into R-devel with `r-version: devel` or `--r-version devel`."
        )
        .into());
    }

    Err(missing_r_version_error(req, &requirement).into())
}

pub fn resolve_rscript_for_exclude_newer(exclude_newer: &str) -> Result<OsString, Box<dyn Error>> {
    let exclude_newer = r_selection::parse_iso_date_field("exclude-newer", exclude_newer)?;
    let installed = rig_client::list()?;
    let req = rig_releases::latest_minor_version_on(&exclude_newer)?;
    let requirement = r_selection::parse_version_requirement(&req)?;

    if let Some(installed) = r_selection::select_installed_r(&requirement, &installed) {
        return installed.rscript();
    }

    Err(format!(
        "`exclude-newer` {exclude_newer} implies `r-version: {req}` because R {req} was the latest R minor version available on that date, but no matching R is installed. Run `rig install {req}`, set `IR_RSCRIPT`, pass `--rscript`, or specify `r-version` or `--r-version`."
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
