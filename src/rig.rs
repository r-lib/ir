use std::error::Error;
use std::ffi::OsString;

use crate::lock::{r_install_lock_path, FileLock};

mod r_selection;
mod rig_client;
mod rig_releases;

pub fn resolve_rscript(req: &str, exclude_newer: Option<&str>) -> Result<OsString, Box<dyn Error>> {
    if let Some(exclude_newer) = exclude_newer {
        r_selection::parse_iso_date_field("exclude-newer", exclude_newer)?;
    }
    let requirement = r_selection::parse_version_requirement(req)?;
    resolve_or_install_rscript(req, &requirement)
}

pub fn resolve_rscript_for_exclude_newer(exclude_newer: &str) -> Result<OsString, Box<dyn Error>> {
    let exclude_newer = r_selection::parse_iso_date_field("exclude-newer", exclude_newer)?;
    let req = rig_releases::latest_minor_version_on(&exclude_newer)?;
    let requirement = r_selection::parse_version_requirement(&req)?;

    resolve_or_install_rscript(&req, &requirement)
}

fn resolve_or_install_rscript(
    req: &str,
    requirement: &r_selection::VersionRequirement,
) -> Result<OsString, Box<dyn Error>> {
    let install_request = r_selection::install_request(requirement);
    if let Ok(r_selection::InstallRequest::Resolve(selector)) = &install_request {
        return resolve_or_install_resolved_selector(req, selector);
    }

    let ambient_installed = rig_client::list()?;
    if let Some(installed) = r_selection::select_installed_r(requirement, &ambient_installed) {
        return installed.rscript();
    }

    let cache_dir = std::path::absolute(crate::runtime::ir_cache_dir()?)?;
    let user_installation = rig_client::UserInstallation::new(&cache_dir)?;
    rig_client::require_user_mode(&user_installation)?;
    let _install_lock = FileLock::acquire(&r_install_lock_path(&cache_dir))?;
    let cached_installed = rig_client::list_user(&user_installation)?;
    if let Some(installed) = r_selection::select_installed_r(requirement, &cached_installed) {
        return installed.rscript();
    }

    let install_target = match install_request? {
        r_selection::InstallRequest::Direct(target) => target.to_string(),
        r_selection::InstallRequest::Resolve(_) => unreachable!("handled before ambient selection"),
        r_selection::InstallRequest::Available => {
            let available = rig_client::available_user(&user_installation)?;
            r_selection::select_available_r(req, requirement, &available)?
                .version
                .clone()
        }
    };

    let install_requirement = r_selection::parse_version_requirement(&install_target)?;
    if let Some(installed) =
        r_selection::select_installed_r(&install_requirement, &ambient_installed)
    {
        return installed.rscript();
    }
    if let Some(installed) =
        r_selection::select_installed_r(&install_requirement, &cached_installed)
    {
        return installed.rscript();
    }

    rig_client::install_user(&user_installation, &install_target)?;
    let installed = rig_client::list_user(&user_installation)?;
    let installed = r_selection::select_installed_r(requirement, &installed)
        .or_else(|| r_selection::select_installed_r(&install_requirement, &installed))
        .ok_or_else(|| {
            format!(
                "rig installed R `{install_target}` in ir's cache, but did not report an installation matching `{req}`"
            )
        })?;
    installed.rscript()
}

fn resolve_or_install_resolved_selector(
    req: &str,
    selector: &str,
) -> Result<OsString, Box<dyn Error>> {
    let cache_dir = std::path::absolute(crate::runtime::ir_cache_dir()?)?;
    let user_installation = rig_client::UserInstallation::new(&cache_dir)?;
    rig_client::require_user_mode(&user_installation)?;
    let _install_lock = FileLock::acquire(&r_install_lock_path(&cache_dir))?;

    let mut resolved_selector = None;
    let version =
        if let Some(version) = rig_client::cached_resolution(&user_installation, selector)? {
            version
        } else {
            let version = rig_client::resolve_user(&user_installation, selector)?;
            resolved_selector = Some((selector, version.clone()));
            version
        };
    if !r_selection::is_numeric_version(&version) {
        return Err(format!(
            "cached or current rig resolution for `{selector}` has unsupported R version `{version}`"
        )
        .into());
    }
    let concrete_requirement = r_selection::parse_version_requirement(&version)?;

    let ambient_installed = rig_client::list()?;
    if let Some(installed) =
        r_selection::select_installed_r(&concrete_requirement, &ambient_installed)
    {
        cache_new_resolution(&user_installation, resolved_selector)?;
        return installed.rscript();
    }

    let cached_installed = rig_client::list_user(&user_installation)?;
    if let Some(installed) =
        r_selection::select_installed_r(&concrete_requirement, &cached_installed)
    {
        cache_new_resolution(&user_installation, resolved_selector)?;
        return installed.rscript();
    }

    rig_client::install_user(&user_installation, &version)?;
    let installed = rig_client::list_user(&user_installation)?;
    let installed = r_selection::select_installed_r(&concrete_requirement, &installed)
        .ok_or_else(|| {
            format!(
                "rig installed R `{version}` in ir's cache, but did not report an installation matching `{req}`"
            )
        })?;
    cache_new_resolution(&user_installation, resolved_selector)?;
    installed.rscript()
}

fn cache_new_resolution(
    installation: &rig_client::UserInstallation,
    resolution: Option<(&str, String)>,
) -> Result<(), Box<dyn Error>> {
    if let Some((selector, version)) = resolution {
        rig_client::cache_resolution(installation, selector, &version)?;
    }
    Ok(())
}
