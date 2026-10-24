//! bodnuti: automated patch review for the Yocto Project and OpenEmbedded.

mod docs;
mod finding;
mod git;
mod oelint;
mod patchtest;
mod report;
mod series;
mod shell;
mod srcuri;
mod workspace;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::finding::{Checker, Finding, Tool, ToolError, new_findings};
use crate::oelint::Oelint;
use crate::patchtest::Patchtest;
use crate::report::Report;
use crate::series::Series;
use crate::shell::ShellCheck;
use crate::workspace::{Applied, Workspace, resolve_base};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check a patch series given as mbox files.
    Check(CheckArgs),
}

#[derive(Args)]
struct CheckArgs {
    /// The layer repository the series targets.
    #[arg(long)]
    repo: PathBuf,
    /// Commit to apply the series to. Default: a branch named by a subject
    /// tag such as [scarthgap], else origin/HEAD, else HEAD.
    #[arg(long)]
    base: Option<String>,
    /// oelint-adv release. Default: the base branch name when the base is a
    /// release branch, else latest.
    #[arg(long)]
    release: Option<String>,
    /// The oelint-adv program to run.
    /// Default: oelint-adv beside the bodnuti executable, else on PATH.
    #[arg(long, env = "BODNUTI_OELINT")]
    oelint: Option<String>,
    /// An openembedded-core checkout providing patchtest. Default: the
    /// target repository, when it is openembedded-core.
    #[arg(long, env = "BODNUTI_OECORE")]
    oecore: Option<PathBuf>,
    /// The shellcheck command, split on whitespace.
    #[arg(
        long,
        env = "BODNUTI_SHELLCHECK",
        default_value = "docker run --rm -i koalaman/shellcheck:v0.10.0"
    )]
    shellcheck: String,
    /// The Python interpreter with patchtest's requirements installed.
    /// Default: python3 beside the bodnuti executable, else on PATH.
    #[arg(long, env = "BODNUTI_PYTHON")]
    python: Option<String>,
    /// Print the report as JSON instead of text.
    #[arg(long)]
    json: bool,
    /// The b4 program, used to fetch a series by Message-ID.
    /// Default: b4 beside the bodnuti executable, else on PATH.
    #[arg(long, env = "BODNUTI_B4")]
    b4: Option<String>,
    /// A lore Message-ID or URL of any patch in the series, or mbox files:
    /// one b4 mbox, or one `git format-patch` file per patch.
    #[arg(required = true)]
    input: Vec<String>,
}

/// The oelint-adv release for a base like `refs/remotes/origin/scarthgap`.
fn release_for(base: &str) -> String {
    match base.rsplit('/').next() {
        Some(name) if !matches!(name, "master" | "main" | "HEAD") && !name.contains('~') => {
            name.to_string()
        }
        _ => "latest".to_string(),
    }
}

/// Maps each base path the series touches to its path after the series,
/// following renames across patches (`a` renamed to `b`, then `b` to `c`).
fn head_paths(series: &Series) -> HashMap<String, String> {
    let mut head_of: HashMap<String, String> = HashMap::new();
    for patch in &series.patches {
        for (old, new) in patch.changes() {
            let origin = head_of
                .iter()
                .find(|(_, head)| **head == old)
                .map(|(base, _)| base.clone())
                .unwrap_or(old);
            head_of.insert(origin, new);
        }
    }
    head_of
}

/// Runs every checker before and after the series and keeps what is new.
fn lint(
    checkers: &[&dyn Checker],
    workspace: &Workspace,
    series: &Series,
    tool_errors: &mut Vec<ToolError>,
) -> Result<(Vec<Applied>, Vec<Finding>)> {
    let files: Vec<String> = series.patches.iter().flat_map(|p| p.files()).collect();
    let head_of = head_paths(series);
    let base_files: Vec<String> = head_of.keys().cloned().collect();

    let tree = workspace.path();
    let before: Vec<_> = checkers
        .iter()
        .map(|c| c.lint(&tree, &base_files))
        .collect();
    let applied = workspace.apply(series)?;
    let after: Vec<_> = checkers.iter().map(|c| c.lint(&tree, &files)).collect();

    let mut findings = Vec::new();
    for ((checker, before), after) in checkers.iter().zip(before).zip(after) {
        match (before, after) {
            (Ok(mut before), Ok(after)) => {
                // Compare a renamed file's findings under its new name, or
                // every existing problem in an upgraded recipe would count as
                // new.
                for f in &mut before {
                    if let Some(head) = f.file.as_ref().and_then(|p| head_of.get(p)) {
                        f.file = Some(head.clone());
                    }
                }
                let after = after.into_iter().filter(|f| {
                    !checker.touched_files_only()
                        || f.file.as_ref().is_some_and(|p| files.contains(p))
                });
                findings.extend(new_findings(&before, after.collect()));
            }
            (Err(err), _) | (_, Err(err)) => tool_errors.push(ToolError {
                tool: checker.tool(),
                message: format!("{err:#}"),
            }),
        }
    }
    for finding in &mut findings {
        finding.patch = series
            .patches
            .iter()
            .rev()
            .find(|p| finding.file.as_ref().is_some_and(|f| p.files().contains(f)))
            .map(|p| p.index);
    }
    Ok((applied, findings))
}

/// A program installed beside the bodnuti executable, else its bare name
/// for a PATH lookup.
///
/// `pip install bodnuti` puts the binary in a venv's bin directory together
/// with that venv's python3, oelint-adv and b4, so the venv works without
/// being activated.
fn sibling(program: &str) -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(program)))
        .filter(|path| path.is_file())
        .and_then(|path| path.to_str().map(String::from))
        .unwrap_or_else(|| program.to_string())
}

fn check(args: CheckArgs) -> Result<Report> {
    let python = args.python.unwrap_or_else(|| sibling("python3"));
    let b4 = args.b4.unwrap_or_else(|| sibling("b4"));
    let series = match args.input.as_slice() {
        [one] if series::is_remote(one) => Series::fetch(&b4, one)?,
        inputs => Series::load(inputs)?,
    };
    let base = resolve_base(&args.repo, &series, args.base.as_deref());
    let workspace = Workspace::create(&args.repo, &base)?;
    let mut tool_errors = Vec::new();

    let oecore = args.oecore.unwrap_or_else(|| args.repo.clone());
    let same = |a: &PathBuf, b: &PathBuf| a.canonicalize().ok() == b.canonicalize().ok();
    let patchtest = Patchtest {
        python: python.clone(),
        oecore_target: same(&oecore, &args.repo),
        oecore,
    };
    let (mut findings, patchtest_counts) = match patchtest.run(&args.repo, &workspace.base, &series)
    {
        Ok((findings, counts)) => (findings, Some(counts)),
        Err(err) => {
            tool_errors.push(ToolError {
                tool: Tool::Patchtest,
                message: format!("{err:#}"),
            });
            (Vec::new(), None)
        }
    };

    let oelint = Oelint {
        program: args.oelint.unwrap_or_else(|| sibling("oelint-adv")),
        release: args.release.unwrap_or_else(|| release_for(&base)),
    };
    let shellcheck = ShellCheck {
        command: args
            .shellcheck
            .split_whitespace()
            .map(String::from)
            .collect(),
    };
    let docs = docs::Docs {
        python: python.clone(),
    };
    let (applied, mut lint_findings) = lint(
        &[&oelint, &shellcheck, &docs],
        &workspace,
        &series,
        &mut tool_errors,
    )?;
    lint_findings.extend(srcuri::leftover_files(&workspace.path(), &series));
    findings.extend(lint_findings);
    findings.sort_by(|a, b| {
        (a.patch, a.severity, &a.file, a.line).cmp(&(b.patch, b.severity, &b.file, b.line))
    });

    Ok(Report {
        series,
        base: workspace.base.clone(),
        applied,
        findings,
        tool_errors,
        patchtest: patchtest_counts,
    })
}

fn main() -> ExitCode {
    let Command::Check(args) = Cli::parse().command;
    let json = args.json;
    let report = match check(args) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("bodnuti: {err:#}");
            return ExitCode::from(2);
        }
    };
    if json {
        match serde_json::to_string_pretty(&report) {
            Ok(text) => println!("{text}"),
            Err(err) => {
                eprintln!("bodnuti: {err}");
                return ExitCode::from(2);
            }
        }
    } else {
        print!("{}", report.text());
    }
    ExitCode::from(report.exit_code())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(changes: &str) -> crate::series::Patch {
        let raw = format!("From x Mon Sep 17 00:00:00 2001\nSubject: [PATCH] x\n\n{changes}");
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), raw).unwrap();
        Series::load(&[file.path()]).unwrap().patches.remove(0)
    }

    #[test]
    fn follows_renames_across_patches() {
        let mut first =
            patch("diff --git a/foo_1.0.bb b/foo_1.1.bb\ndiff --git a/foo.inc b/foo.inc\n");
        first.index = 1;
        let mut second = patch("diff --git a/foo_1.1.bb b/foo_1.2.bb\n");
        second.index = 2;
        let series = Series {
            cover: None,
            patches: vec![first, second],
        };
        let head_of = head_paths(&series);
        assert_eq!(
            head_of.get("foo_1.0.bb").map(String::as_str),
            Some("foo_1.2.bb")
        );
        assert_eq!(head_of.get("foo.inc").map(String::as_str), Some("foo.inc"));
        assert_eq!(head_of.len(), 2);
    }

    #[test]
    fn release_follows_the_base_branch() {
        assert_eq!(release_for("refs/remotes/origin/scarthgap"), "scarthgap");
        assert_eq!(release_for("origin/master"), "latest");
        assert_eq!(release_for("HEAD"), "latest");
        assert_eq!(release_for("upstream/master~2"), "latest");
    }
}
