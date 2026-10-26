//! GitHub Actions runs of the Django applications' staging branches,
//! traced to the series they carried (designs/PHASE2.md §5.8).
//!
//! The maintainer pushes the patches under review to `master-next` on
//! GitHub, where CI builds and tests them and Safety scans their
//! dependencies. Each finished run names the commit it tested; the patches
//! on top of `master` there are matched to checked series by patch-id, and
//! the run is recorded on each series it carried. A run tests every patch
//! on the branch at once, so a failure is tied to a series only once two
//! runs agree: both failed where the run of their workflow before had
//! passed, and both newly carried the series.
//!
//! Runs come from GitHub's public API, without a token. Everything in them
//! is untrusted: commits are checked to be object names before git sees
//! them, and URLs to point at github.com.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::git;
use crate::runner::read_owners;
use crate::watch::{Config, ListState};

/// The branch the maintainers push patches under review to.
pub const BRANCH: &str = "master-next";

/// How long between looks at GitHub: the poll runs every five minutes, and
/// GitHub allows 60 requests an hour without a token.
const INTERVAL_SECS: u64 = 20 * 60;

/// Runs fetched per repository and look.
const PER_PAGE: u32 = 20;

/// A run as recorded on a series it carried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CiRun {
    /// The GitHub repository, `owner/name`.
    pub repo: String,
    pub workflow: String,
    pub id: u64,
    pub url: String,
    /// `success`, `failure`, `cancelled` and so on, as GitHub says.
    pub conclusion: String,
    pub created_at: String,
    /// The series' patches the run carried, by number.
    pub patches: Vec<u32>,
    /// How many series the run carried, this one included.
    pub series: usize,
    /// A failure tied to this series: the second run that failed newly
    /// carrying it.
    #[serde(default)]
    pub tied: bool,
}

#[derive(Deserialize)]
struct RunsPage {
    workflow_runs: Vec<ApiRun>,
}

#[derive(Deserialize)]
struct ApiRun {
    id: u64,
    name: String,
    event: String,
    status: String,
    conclusion: Option<String>,
    head_sha: String,
    html_url: String,
    created_at: String,
}

/// What `state/ci.json` keeps between polls.
#[derive(Debug, Default, Serialize, Deserialize)]
struct CiState {
    /// When GitHub was last asked, in seconds since the epoch.
    last_poll: u64,
    /// The last run handled of each `<repo> <workflow>`.
    last: BTreeMap<String, Seen>,
    /// How many runs failed newly carrying each `<list>/<root> <repo>
    /// <workflow>`. Per repository: two repositories running on the same
    /// push are one test, not two.
    new_failures: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Seen {
    id: u64,
    conclusion: String,
    /// The patch-ids on top of `master` the run tested.
    stack: BTreeSet<String>,
}

/// Looks at GitHub for runs of the configured repositories' staging
/// branches, at most every `INTERVAL_SECS`, and records each finished run
/// on the series it carried.
pub fn poll(config: &Config, now: u64) -> Result<()> {
    let path = config.state.join("ci.json");
    let mut state: CiState = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).context("parsing ci.json")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => CiState::default(),
        Err(e) => return Err(e).context("reading ci.json"),
    };
    let watched: Vec<(&String, &String)> = config
        .repos
        .iter()
        .flat_map(|(name, repo)| repo.github.iter().map(move |slug| (name, slug)))
        .collect();
    if watched.is_empty() || now < state.last_poll + INTERVAL_SECS {
        return Ok(());
    }
    state.last_poll = now;
    let mut lists: BTreeMap<String, ListState> = BTreeMap::new();
    for (name, slug) in watched {
        let clone = config.cache.join(name);
        if !clone.is_dir() {
            continue;
        }
        let mut runs = match fetch_runs(slug) {
            Ok(runs) => runs,
            Err(err) => {
                eprintln!("bodnuti: {slug}: {err:#}");
                continue;
            }
        };
        runs.sort_by_key(|r| r.id);
        for run in runs {
            let key = format!("{slug} {}", run.name);
            if state.last.get(&key).is_some_and(|s| s.id >= run.id) {
                continue;
            }
            let Some(stack) = stack(&clone, slug, &run.head_sha)? else {
                continue;
            };
            let conclusion = run.conclusion.clone().unwrap_or_default();
            let previous = state.last.get(&key).cloned().unwrap_or_default();
            let carried = carried(&config.state, name, &stack)?;
            // New to this run, against the one before it.
            let newly_failed = conclusion == "failure" && previous.conclusion == "success";
            for (series, patches) in &carried {
                let new = patches.iter().any(|(id, _)| !previous.stack.contains(id));
                let mut tied = false;
                if newly_failed && new {
                    let count = state
                        .new_failures
                        .entry(format!("{series} {key}"))
                        .or_default();
                    *count += 1;
                    tied = *count >= 2;
                }
                let record = CiRun {
                    repo: slug.clone(),
                    workflow: run.name.clone(),
                    id: run.id,
                    url: run.html_url.clone(),
                    conclusion: conclusion.clone(),
                    created_at: run.created_at.clone(),
                    patches: patches.iter().map(|(_, k)| *k).collect(),
                    series: carried.len(),
                    tied,
                };
                add_run(config, &mut lists, series, record)?;
            }
            state.last.insert(
                key,
                Seen {
                    id: run.id,
                    conclusion,
                    stack: stack.into_iter().collect(),
                },
            );
        }
    }
    for (list, list_state) in &lists {
        list_state.save(&config.state, list)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&state)?).context("writing ci.json")
}

/// The finished push runs of `slug`'s staging branch, newest first, with
/// their untrusted fields checked.
fn fetch_runs(slug: &str) -> Result<Vec<ApiRun>> {
    let url = format!(
        "https://api.github.com/repos/{slug}/actions/runs?branch={BRANCH}&event=push&status=completed&per_page={PER_PAGE}"
    );
    let mut response = ureq::get(&url)
        .header("User-Agent", crate::lore::USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .call()
        .with_context(|| format!("fetching {url}"))?;
    let page: RunsPage = serde_json::from_str(
        &response
            .body_mut()
            .read_to_string()
            .context("reading GitHub's answer")?,
    )
    .context("parsing GitHub's runs")?;
    Ok(runs_of(page, slug))
}

/// The runs worth recording: finished pushes, with a commit that is an
/// object name and a page on github.com.
fn runs_of(page: RunsPage, slug: &str) -> Vec<ApiRun> {
    let prefix = format!("https://github.com/{slug}/actions/runs/");
    page.workflow_runs
        .into_iter()
        .filter(|r| r.event == "push" && r.status == "completed")
        .filter(|r| r.head_sha.len() == 40 && r.head_sha.bytes().all(|b| b.is_ascii_hexdigit()))
        .filter(|r| r.html_url.starts_with(&prefix))
        .collect()
}

/// The patch-ids of the commits `sha` has on top of `master`, fetching it
/// from GitHub when the clone does not have it. None when it cannot be
/// fetched, such as after a force push removed it.
fn stack(clone: &Path, slug: &str, sha: &str) -> Result<Option<Vec<String>>> {
    let have =
        |sha: &str| git::run(clone, &["cat-file", "-e", &format!("{sha}^{{commit}}")]).is_ok();
    if !have(sha) {
        let url = format!("https://github.com/{slug}");
        if git::run(clone, &["fetch", "--quiet", "--no-tags", "--", &url, sha]).is_err()
            || !have(sha)
        {
            return Ok(None);
        }
    }
    stack_of(clone, sha).map(Some)
}

/// The patch-ids of the commits `sha` has on top of `origin/master`.
fn stack_of(clone: &Path, sha: &str) -> Result<Vec<String>> {
    let log = git::command(clone)
        .args(["log", "-p", "--no-merges", "--no-color"])
        .arg(format!("origin/master..{sha}"))
        .output()
        .context("running git log")?;
    Ok(crate::workspace::patch_id_lines(&log.stdout)?
        .into_iter()
        .map(|(id, _)| id)
        .collect())
}

/// The checked series of repository `name` whose patches are in `stack`:
/// `<list>/<root>` to each patch's id and number.
fn carried(
    state: &Path,
    name: &str,
    stack: &[String],
) -> Result<BTreeMap<String, Vec<(String, u32)>>> {
    let mut out: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
    for id in stack {
        for owner in read_owners(state, id)? {
            if owner.repo.as_deref().is_some_and(|r| r != name) {
                continue;
            }
            let Some(index) = owner.index else { continue };
            let patches = out.entry(owner.series).or_default();
            if !patches.iter().any(|(p, _)| p == id) {
                patches.push((id.clone(), index));
            }
        }
    }
    Ok(out)
}

/// Adds `run` to the record of `series` (`<list>/<root>`), once.
fn add_run(
    config: &Config,
    lists: &mut BTreeMap<String, ListState>,
    series: &str,
    run: CiRun,
) -> Result<()> {
    let Some((list, root)) = series.split_once('/') else {
        return Ok(());
    };
    if !lists.contains_key(list) {
        lists.insert(list.to_string(), ListState::load(&config.state, list)?);
    }
    let Some(record) = lists.get_mut(list).and_then(|s| s.series.get_mut(root)) else {
        return Ok(());
    };
    if !record.ci.iter().any(|r| r.id == run.id) {
        record.ci.push(run);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_finished_pushes_with_sound_fields() {
        let page: RunsPage = serde_json::from_value(serde_json::json!({
            "workflow_runs": [
                {"id": 3, "name": "CI", "event": "push", "status": "completed",
                 "conclusion": "success", "head_sha": "a".repeat(40),
                 "html_url": "https://github.com/o/r/actions/runs/3",
                 "created_at": "2026-10-26T10:00:00Z"},
                {"id": 4, "name": "CI", "event": "push", "status": "completed",
                 "conclusion": "success", "head_sha": "--upload-pack=x",
                 "html_url": "https://github.com/o/r/actions/runs/4",
                 "created_at": "2026-10-26T10:00:00Z"},
                {"id": 5, "name": "CI", "event": "push", "status": "completed",
                 "conclusion": "failure", "head_sha": "b".repeat(40),
                 "html_url": "https://evil.example/o/r/actions/runs/5",
                 "created_at": "2026-10-26T10:00:00Z"},
                {"id": 6, "name": "CodeQL", "event": "dynamic", "status": "completed",
                 "conclusion": "success", "head_sha": "c".repeat(40),
                 "html_url": "https://github.com/o/r/actions/runs/6",
                 "created_at": "2026-10-26T10:00:00Z"}
            ]
        }))
        .unwrap();
        let runs = runs_of(page, "o/r");
        assert_eq!(runs.iter().map(|r| r.id).collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn finds_the_series_a_stack_carries() {
        let dir = tempfile::tempdir().unwrap();
        let ids = dir.path().join("patch-ids");
        std::fs::create_dir_all(&ids).unwrap();
        std::fs::write(
            ids.join("p1"),
            "yocto-patches/a@x master 1 layerindex-web\n",
        )
        .unwrap();
        std::fs::write(ids.join("p2"), "yocto-patches/a@x master 2\n").unwrap();
        std::fs::write(
            ids.join("p3"),
            "yocto-patches/b@x master 1 error-report-web\n",
        )
        .unwrap();
        let stack = ["p1".to_string(), "p2".into(), "p3".into(), "p4".into()];
        let carried = carried(dir.path(), "layerindex-web", &stack).unwrap();
        assert_eq!(carried.len(), 1);
        let patches: Vec<u32> = carried["yocto-patches/a@x"]
            .iter()
            .map(|(_, k)| *k)
            .collect();
        assert_eq!(patches, [1, 2]);
    }
}
