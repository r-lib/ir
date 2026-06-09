//! `ir` — self-describing R scripts.
//!
//! Runs a standalone R script whose dependencies are declared in YAML
//! frontmatter at the top of the file:
//!
//! ```r
//! #!/usr/bin/env -S ir run
//! #| packages:
//! #|   - dplyr>=1.0
//! #|   - tidyr
//! #| r-version: ">= 4.0"
//! #| isolated: true
//! #| exclude-newer: "2024-01-15"
//!
//! library(dplyr)
//! 1 + 1
//! ```
//!
//! The pipeline has two phases:
//!
//!   1. Rust extracts and parses the leading `#| ` YAML frontmatter block. If
//!      the resolution cache is warm, Rust reuses the cached library path
//!      directly. Otherwise, a private R session (`driver/resolve.R`) receives
//!      the package refs on stdin, resolves them with pak, hashes the install
//!      refs into a content-addressed library path under the cache directory,
//!      and materialises that path as a light-weight library of symlinks into
//!      renv's package cache. The path is reported back to us.
//!
//!   2. We launch the user's script in a fresh R session with that library
//!      prepended to `.libPaths()`. With `--isolated`, the user library is
//!      dropped.

use std::env;
use std::error::Error;

mod cache;
mod cli;
mod quarto;
mod resolve_cache;
mod rig;
mod runtime;
mod script;
mod tool;

fn main() {
    if let Err(err) = try_main() {
        match err.downcast::<clap::Error>() {
            Ok(err) => err.exit(),
            Err(err) => {
                eprintln!("ir: {err}");
                std::process::exit(1);
            }
        }
    }
}

fn try_main() -> Result<(), Box<dyn Error>> {
    let argv: Vec<String> = env::args().collect();
    let matches = cli::root().try_get_matches_from(argv.clone())?;
    match matches.subcommand() {
        Some(("run", _)) => {
            let run = cli::parse_run_args(argv[2..].to_vec())?;
            runtime::cmd_run(
                &run.source,
                &run.rscript_args,
                &run.with_deps,
                run.r_requirement.as_deref(),
                &run.script_args,
                run.isolated,
            )
        }
        Some(("render", _)) => {
            let render = cli::parse_render_args(argv[2..].to_vec())?;
            runtime::cmd_render(
                &render.source,
                &render.rscript_args,
                &render.with_deps,
                render.r_requirement.as_deref(),
                &render.render_args,
                render.isolated,
            )
        }
        Some(("tool", matches)) => match matches.subcommand() {
            Some(("run", _)) => {
                let run =
                    cli::parse_tool_run_args(argv[3..].to_vec(), cli::ToolRunInvocation::ToolRun)?;
                tool::cmd_tool_run(&run)
            }
            Some(("rx", _)) => {
                let run = cli::parse_tool_run_args(argv[3..].to_vec(), cli::ToolRunInvocation::Rx)?;
                tool::cmd_tool_run(&run)
            }
            Some(("install", _)) => {
                let install = cli::parse_tool_install_args(argv[3..].to_vec())?;
                tool::cmd_tool_install(&install)
            }
            _ => unreachable!("clap requires a tool subcommand"),
        },
        Some(("cache", matches)) => cache::cmd_cache(matches),
        _ => Ok(()),
    }
}
