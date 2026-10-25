//! bodnuti: automated patch review for the Yocto Project and OpenEmbedded.

mod docs;
mod finding;
mod git;
mod lore;
mod oelint;
mod patchtest;
mod report;
mod runner;
mod series;
mod shell;
mod srcuri;
mod track;
mod watch;
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
    /// Find new patch series on lore lists.
    Poll(PollArgs),
}

#[derive(Args)]
struct PollArgs {
    /// The watcher configuration.
    #[arg(long, default_value = "~/.config/bodnuti/watch.toml")]
    config: PathBuf,
    /// Print the series on the given lists, without the configuration or
    /// any state.
    #[arg(long)]
    dry_run: bool,
    /// With --dry-run: how far back to look, in hours.
    #[arg(long, default_value_t = 24)]
    hours: u64,
    /// With --dry-run: lore inbox names, e.g. openembedded-core.
    lists: Vec<String>,
}

/// Polls every configured list and records its series in the state
/// directory. A list that fails is reported and the others still run.
fn poll(args: PollArgs) -> Result<()> {
    if args.dry_run {
        return poll_dry_run(&args);
    }
    let config = watch::Config::load(&watch::expand(&args.config))?;
    let _lock = watch::Lock::take(&config.state)?;
    let now = lore::now();
    let mut failed = 0;
    for list in &config.lists {
        let result = (|| -> Result<()> {
            let mut state = watch::ListState::load(&config.state, &list.name)?;
            let entries = safe_entries(lore::fetch(
                &list.name,
                &state.since(config.initial_hours, now),
            )?);
            let new = state.ingest(&entries, now, config.settle_minutes * 60);
            state.route(list, &config.repos);
            state.save(&config.state, &list.name)?;
            check_ready(&config, list, &mut state)?;
            let count = |status| state.series.values().filter(|r| r.status == status).count();
            println!(
                "{}: {} messages, {new} new series; {} waiting, {} checked, {} duplicate, {} unrouted, {} failed",
                list.name,
                entries.len(),
                count(watch::Status::Seen) + count(watch::Status::Complete),
                count(watch::Status::Checked),
                count(watch::Status::Duplicate),
                count(watch::Status::Unrouted),
                count(watch::Status::Failed),
            );

            Ok(())
        })();
        if let Err(err) = result {
            eprintln!("bodnuti: {}: {err:#}", list.name);
            failed += 1;
        }
    }
    if failed > 0 {
        anyhow::bail!("{failed} of {} lists failed", config.lists.len());
    }
    Ok(())
}

/// Fetches and checks every routed, complete series on the list, oldest
/// first, saving the state after each so a crash loses at most one check.
fn check_ready(
    config: &watch::Config,
    list: &watch::ListConfig,
    state: &mut watch::ListState,
) -> Result<()> {
    let runner = runner::Runner { config };
    let mut ready: Vec<(String, String)> = state
        .series
        .values()
        .filter(|r| r.status == watch::Status::Complete && r.repo.is_some())
        .map(|r| (r.tracked.last_seen.clone(), r.tracked.root.clone()))
        .collect();
    ready.sort();
    for (_, root) in ready {
        let Some(record) = state.series.get_mut(&root) else {
            continue;
        };
        if let Err(err) = runner.process(&list.name, list.route, record) {
            record.attempts += 1;
            record.reason = Some(format!("{err:#}"));
            if record.attempts >= watch::MAX_ATTEMPTS {
                record.status = watch::Status::Failed;
            }
        }
        println!(
            "  {:?} {}{}",
            record.status,
            record.tracked.subject,
            record
                .reason
                .as_deref()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default()
        );
        state.save(&config.state, &list.name)?;
    }
    Ok(())
}

/// Drops entries whose Message-ID cannot be used as a file name.
fn safe_entries(entries: Vec<lore::Entry>) -> Vec<lore::Entry> {
    entries
        .into_iter()
        .filter(|e| {
            let ok = lore::safe_id(&e.message_id);
            if !ok {
                eprintln!(
                    "bodnuti: skipping a message with an unusable Message-ID: {:?}",
                    e.message_id
                );
            }
            ok
        })
        .collect()
}

/// Lists the series on each list since `hours` ago.
fn poll_dry_run(args: &PollArgs) -> Result<()> {
    if args.lists.is_empty() {
        anyhow::bail!("--dry-run needs at least one list name");
    }
    let since = lore::hours_ago(args.hours);
    for list in &args.lists {
        let safe = safe_entries(lore::fetch(list, &since)?);
        let mut tracker = track::Tracker::default();
        tracker.ingest(&safe);
        println!(
            "{list}: {} messages, {} series",
            safe.len(),
            tracker.series.len()
        );
        let mut series: Vec<_> = tracker.series.values().collect();
        series.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        for s in series {
            let state = if s.complete() { "complete" } else { "partial " };
            println!(
                "  {state} {}/{}  {}  <{}>",
                s.seen.len(),
                s.total,
                s.subject,
                s.root
            );
        }
    }
    Ok(())
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
///
/// Yocto codenames are single lowercase words (`scarthgap`, `blacksail`).
/// Anything else, such as a commit hash given with `--base`, a tag, or a
/// branch like `scarthgap-next`, is not a release oelint-adv knows, so it
/// gets `latest`.
fn release_for(base: &str) -> String {
    match base.rsplit('/').next() {
        Some(name)
            if !matches!(name, "master" | "main")
                && !name.is_empty()
                && name.bytes().all(|b| b.is_ascii_lowercase()) =>
        {
            name.to_string()
        }
        _ => "latest".to_string(),
    }
}

/// A recipe file's directory, name and extension: `dir/foo_1.2.bb` gives
/// `("dir", "foo", "bb")`. Includes and other files give `None`.
fn recipe_key(path: &str) -> Option<(&str, &str, &str)> {
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
    let (stem, ext) = file.rsplit_once('.')?;
    if !matches!(ext, "bb" | "bbappend") {
        return None;
    }
    let (name, _version) = stem.rsplit_once('_')?;
    Some((dir, name, ext))
}

/// Recipe upgrades that git shows as a delete and an add rather than a
/// rename, because the recipe changed too much. A deleted `foo_1.0.bb` pairs
/// with an added `foo_2.0.bb` in the same directory, when the series deletes
/// and adds exactly one such file; anything less clear is left unpaired.
fn recipe_upgrades(series: &Series) -> Vec<(String, String)> {
    let deleted: Vec<String> = series
        .patches
        .iter()
        .flat_map(|p| p.deleted_files())
        .collect();
    let added: Vec<String> = series
        .patches
        .iter()
        .flat_map(|p| p.added_files())
        .collect();
    let mut pairs = Vec::new();
    for old in &deleted {
        let Some(key) = recipe_key(old) else { continue };
        let same = |p: &&String| recipe_key(p) == Some(key);
        let olds: Vec<&String> = deleted.iter().filter(same).collect();
        let news: Vec<&String> = added.iter().filter(same).collect();
        if let ([_], [new]) = (olds.as_slice(), news.as_slice()) {
            pairs.push((old.clone(), (*new).clone()));
        }
    }
    pairs
}

/// Maps each base path the series touches to its path after the series,
/// following renames across patches (`a` renamed to `b`, then `b` to `c`),
/// and recipe upgrades git did not detect as renames.
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
    for (old, new) in recipe_upgrades(series) {
        head_of.insert(old, new);
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
    // A series that does not apply is a result for its author, not a failure
    // of bodnuti: report it, and skip the checks that need the patched tree.
    let applied = match workspace.apply(series) {
        Ok(applied) => applied,
        Err(err) => {
            let not_applied = err.downcast::<workspace::NotApplied>()?;
            let finding = Finding {
                tool: Tool::Bodnuti,
                check: "bodnuti.apply".into(),
                severity: finding::Severity::Error,
                patch: Some(not_applied.index),
                file: None,
                line: None,
                message: format!(
                    "does not apply to {}: {}",
                    not_applied.base, not_applied.reason
                ),
            };
            return Ok((Vec::new(), vec![finding]));
        }
    };
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
    let args = match Cli::parse().command {
        Command::Check(args) => args,
        Command::Poll(args) => {
            return match poll(args) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("bodnuti: {err:#}");
                    ExitCode::from(2)
                }
            };
        }
    };
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
    fn pairs_a_rewritten_recipe_upgrade() {
        let mut p = patch(
            "diff --git a/r/ck/ck_0.4.6.bb b/r/ck/ck_0.4.6.bb\ndeleted file mode 100644\n\
             diff --git a/r/ck/ck_2.0.0.bb b/r/ck/ck_2.0.0.bb\nnew file mode 100644\n\
             diff --git a/r/ck/ck/old.patch b/r/ck/ck/old.patch\ndeleted file mode 100644\n\
             diff --git a/r/other/other_1.0.bb b/r/other/other_1.0.bb\nnew file mode 100644\n",
        );
        p.index = 1;
        let series = Series {
            cover: None,
            patches: vec![p],
        };
        assert_eq!(
            recipe_upgrades(&series),
            vec![(
                "r/ck/ck_0.4.6.bb".to_string(),
                "r/ck/ck_2.0.0.bb".to_string()
            )]
        );
        assert_eq!(
            head_paths(&series)
                .get("r/ck/ck_0.4.6.bb")
                .map(String::as_str),
            Some("r/ck/ck_2.0.0.bb")
        );
    }

    #[test]
    fn ambiguous_recipe_changes_stay_unpaired() {
        // Two versions added for one removed: which one replaced it is not
        // clear, so neither is paired.
        let p = patch(
            "diff --git a/d/foo_1.bb b/d/foo_1.bb\ndeleted file mode 100644\n\
             diff --git a/d/foo_2.bb b/d/foo_2.bb\nnew file mode 100644\n\
             diff --git a/d/foo_3.bb b/d/foo_3.bb\nnew file mode 100644\n",
        );
        let series = Series {
            cover: None,
            patches: vec![p],
        };
        assert!(recipe_upgrades(&series).is_empty());
        assert_eq!(
            recipe_key("d/foo-bar_1.2.3.bb"),
            Some(("d", "foo-bar", "bb"))
        );
        assert_eq!(recipe_key("d/foo.inc"), None);
    }

    #[test]
    fn release_follows_the_base_branch() {
        assert_eq!(release_for("refs/remotes/origin/scarthgap"), "scarthgap");
        assert_eq!(release_for("origin/master"), "latest");
        assert_eq!(release_for("HEAD"), "latest");
        assert_eq!(release_for("upstream/master~2"), "latest");
        assert_eq!(release_for("ae5cafacec8e"), "latest");
        assert_eq!(release_for("388cd4770e04~1"), "latest");
        assert_eq!(
            release_for("refs/remotes/upstream/scarthgap-next"),
            "latest"
        );
        assert_eq!(release_for("refs/heads/kirkstone"), "kirkstone");
    }
}
