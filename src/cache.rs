use std::error::Error;
use std::fs;

use clap::ArgMatches;

use crate::runtime::{count_files, ir_cache_dir};

pub(crate) fn cmd_cache(matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    match matches.subcommand() {
        Some(("clean", matches)) => cmd_cache_clean(matches.get_flag("force")),
        Some(("dir", _)) => cmd_cache_dir(),
        _ => unreachable!("clap requires a cache subcommand"),
    }
}

pub(crate) fn cmd_cache_clean(_force: bool) -> Result<(), Box<dyn Error>> {
    let cache_dir = ir_cache_dir()?;
    if !cache_dir.exists() {
        println!("No cache found at: {}", cache_dir.display());
        return Ok(());
    }

    let files = count_files(&cache_dir)?;
    println!("Clearing cache at: {}", cache_dir.display());
    fs::remove_dir_all(&cache_dir)
        .map_err(|e| format!("failed to remove cache `{}`: {e}", cache_dir.display()))?;
    println!(
        "Removed {files} {}",
        if files == 1 { "file" } else { "files" }
    );
    Ok(())
}

pub(crate) fn cmd_cache_dir() -> Result<(), Box<dyn Error>> {
    println!("{}", ir_cache_dir()?.display());
    Ok(())
}
