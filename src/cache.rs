use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::ArgMatches;

use crate::runtime::{count_files, ir_cache_dir, rscript_for_env};

const TOOL_CACHE_CLEANER: &str = include_str!("../driver/cache-clean.R");

pub(crate) fn cmd_cache(matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    match matches.subcommand() {
        Some(("clean", matches)) => {
            cmd_cache_clean(matches.get_flag("force"), matches.get_flag("all"))
        }
        Some(("dir", _)) => cmd_cache_dir(),
        _ => unreachable!("clap requires a cache subcommand"),
    }
}

pub(crate) fn cmd_cache_clean(_force: bool, all: bool) -> Result<(), Box<dyn Error>> {
    let cache_dir = ir_cache_dir()?;
    if all {
        return cmd_cache_clean_all(cache_dir);
    }

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

fn cmd_cache_clean_all(cache_dir: PathBuf) -> Result<(), Box<dyn Error>> {
    clean_tool_caches_with_r()?;
    clear_labeled_cache("ir cache", &cache_dir)
}

fn clear_labeled_cache(label: &str, path: &Path) -> Result<(), Box<dyn Error>> {
    if !path.exists() {
        println!("No {label} found at: {}", path.display());
        return Ok(());
    }

    let files = count_files(path)?;
    println!("Clearing {label} at: {}", path.display());
    fs::remove_dir_all(path)
        .map_err(|e| format!("failed to remove {label} `{}`: {e}", path.display()))?;
    println!(
        "Removed {files} {} from {label}",
        if files == 1 { "file" } else { "files" }
    );
    Ok(())
}

fn clean_tool_caches_with_r() -> Result<(), Box<dyn Error>> {
    let rscript = rscript_for_env()?;
    let mut child = Command::new(&rscript)
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| cache_cleaner_spawn_error(&rscript, e))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or("failed to open Rscript stdin for cache cleaner")?;
    stdin
        .write_all(TOOL_CACHE_CLEANER.as_bytes())
        .map_err(|e| format!("failed to write cache cleaner to Rscript stdin: {e}"))?;
    drop(stdin);

    let status = child
        .wait()
        .map_err(|e| format!("failed to wait for R cache cleaner: {e}"))?;

    if !status.success() {
        return Err(format!("R cache cleaner failed with status {status}").into());
    }

    Ok(())
}

fn cache_cleaner_spawn_error(rscript: &OsStr, err: io::Error) -> String {
    if err.kind() == io::ErrorKind::NotFound {
        format!(
            "could not find `{}` on PATH. Install R or set IR_RSCRIPT.",
            rscript.to_string_lossy()
        )
    } else {
        format!("failed to launch `{}`: {err}", rscript.to_string_lossy())
    }
}

pub(crate) fn cmd_cache_dir() -> Result<(), Box<dyn Error>> {
    println!("{}", ir_cache_dir()?.display());
    Ok(())
}
