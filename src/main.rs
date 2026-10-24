//! bodnuti: automated patch review for the Yocto Project and OpenEmbedded.

mod git;
mod series;
mod workspace;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::json;

use crate::series::Series;
use crate::workspace::{Workspace, resolve_base};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check a patch series given as mbox files.
    Check {
        /// The layer repository the series targets.
        #[arg(long)]
        repo: PathBuf,
        /// Commit to apply the series to. Default: a branch named by a subject
        /// tag such as [scarthgap], else origin/HEAD, else HEAD.
        #[arg(long)]
        base: Option<String>,
        /// mbox files: one b4 mbox, or one `git format-patch` file per patch.
        #[arg(required = true)]
        mbox: Vec<PathBuf>,
    },
}

fn check(repo: PathBuf, base: Option<String>, mbox: Vec<PathBuf>) -> Result<()> {
    let series = Series::load(&mbox)?;
    let base = resolve_base(&repo, &series, base.as_deref());
    let workspace = Workspace::create(&repo, &base)?;
    let applied = workspace.apply(&series)?;
    let report = json!({ "series": series, "base": workspace.base, "applied": applied });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Check { repo, base, mbox } => check(repo, base, mbox),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("bodnuti: {err:#}");
            ExitCode::from(2)
        }
    }
}
