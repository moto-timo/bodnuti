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
//! CodeQL alerts on `master` are attributed the same way, once a series has
//! landed: the alert's line, blamed at the commit it was found at, names
//! the commit that wrote it, which is matched to a series by patch-id.
//!
//! Runs come from GitHub's public API, without a token; code scanning
//! alerts need one even for public repositories, read from
//! `BODNUTI_GITHUB_TOKEN`, and without it they are skipped. Everything from
//! GitHub is untrusted: commits are checked to be object names and paths to
//! stay in the tree before git sees them, and URLs to point at github.com.

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
    /// `success`, `failure`, `cancelled` and so on, as GitHub says, or
    /// `skipped` for a run whose jobs all skipped their own steps, such as
    /// a Safety scan in a repository without a Safety key.
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

#[derive(Deserialize)]
struct JobsPage {
    jobs: Vec<ApiJob>,
}

#[derive(Deserialize)]
struct ApiJob {
    conclusion: Option<String>,
    #[serde(default)]
    steps: Vec<ApiStep>,
}

#[derive(Deserialize)]
struct ApiStep {
    name: String,
    conclusion: Option<String>,
}

/// Whether every job of a run skipped the steps the workflow defines: a
/// run GitHub calls a success that tested nothing. GitHub's own steps
/// (setting up, checking out, cleaning up) run either way.
fn all_skipped(jobs: &[ApiJob]) -> bool {
    let own = |s: &&ApiStep| {
        !(s.name == "Set up job"
            || s.name == "Complete job"
            || s.name.starts_with("Run actions/")
            || s.name.starts_with("Post "))
    };
    !jobs.is_empty()
        && jobs.iter().all(|j| {
            j.conclusion.as_deref() == Some("skipped") || {
                let steps: Vec<&ApiStep> = j.steps.iter().filter(own).collect();
                !steps.is_empty()
                    && steps
                        .iter()
                        .all(|s| s.conclusion.as_deref() == Some("skipped"))
            }
        })
}

/// A CodeQL alert as recorded on the series whose patch wrote its line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeqlAlert {
    /// The GitHub repository, `owner/name`.
    pub repo: String,
    pub number: u64,
    pub rule: String,
    /// The security severity (`high`, `medium`), else the rule's.
    pub severity: String,
    pub path: String,
    pub line: u32,
    pub message: String,
    pub url: String,
    /// The series' patch that wrote the line.
    pub patch: u32,
}

#[derive(Deserialize)]
struct ApiAlert {
    number: u64,
    html_url: String,
    rule: ApiRule,
    most_recent_instance: ApiInstance,
}

#[derive(Deserialize)]
struct ApiRule {
    id: String,
    severity: Option<String>,
    security_severity_level: Option<String>,
}

#[derive(Deserialize)]
struct ApiInstance {
    commit_sha: String,
    message: ApiMessage,
    location: ApiLocation,
}

#[derive(Deserialize)]
struct ApiMessage {
    text: String,
}

#[derive(Deserialize)]
struct ApiLocation {
    path: String,
    start_line: u32,
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
    /// The CodeQL alerts already attributed, or found to be no checked
    /// series', per repository.
    #[serde(default)]
    alerts_seen: BTreeMap<String, BTreeSet<u64>>,
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
        if let Some(token) = token() {
            match fetch_alerts(slug, &token) {
                Ok(alerts) => {
                    let seen = state.alerts_seen.entry(slug.clone()).or_default();
                    for alert in alerts {
                        if !seen.insert(alert.number) {
                            continue;
                        }
                        if let Some((series, record)) =
                            attribute(&clone, &config.state, name, slug, &alert)?
                        {
                            add_alert(config, &mut lists, &series, record)?;
                        }
                    }
                }
                Err(err) => eprintln!("bodnuti: {slug}: CodeQL alerts: {err:#}"),
            }
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
            let mut conclusion = run.conclusion.clone().unwrap_or_default();
            if conclusion == "success" {
                match fetch_jobs(slug, run.id) {
                    Ok(jobs) if all_skipped(&jobs) => conclusion = "skipped".into(),
                    Ok(_) => {}
                    Err(err) => eprintln!("bodnuti: {slug}: run {}: {err:#}", run.id),
                }
            }
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
            // A skipped run tested nothing: the next run is compared with
            // the last one that did.
            let seen = if conclusion == "skipped" {
                Seen {
                    id: run.id,
                    ..previous
                }
            } else {
                Seen {
                    id: run.id,
                    conclusion,
                    stack: stack.into_iter().collect(),
                }
            };
            state.last.insert(key, seen);
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
    let page: RunsPage =
        serde_json::from_str(&get(&url, token().as_deref())?).context("parsing GitHub's runs")?;
    Ok(runs_of(page, slug))
}

/// The jobs of run `id`, with their steps.
fn fetch_jobs(slug: &str, id: u64) -> Result<Vec<ApiJob>> {
    let url = format!("https://api.github.com/repos/{slug}/actions/runs/{id}/jobs");
    let page: JobsPage =
        serde_json::from_str(&get(&url, token().as_deref())?).context("parsing GitHub's jobs")?;
    Ok(page.jobs)
}

/// The token for GitHub's code scanning API, if the worker has one.
fn token() -> Option<String> {
    std::env::var("BODNUTI_GITHUB_TOKEN")
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// A GET of GitHub's API, with the token when there is one.
fn get(url: &str, token: Option<&str>) -> Result<String> {
    let mut request = ureq::get(url)
        .header("User-Agent", crate::lore::USER_AGENT)
        .header("Accept", "application/vnd.github+json");
    if let Some(token) = token {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let mut response = request
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .call()
        .with_context(|| format!("fetching {url}"))?;
    response
        .body_mut()
        .read_to_string()
        .context("reading GitHub's answer")
}

/// The open CodeQL alerts of `slug` found on `master`, with their untrusted
/// fields checked.
fn fetch_alerts(slug: &str, token: &str) -> Result<Vec<ApiAlert>> {
    let url = format!(
        "https://api.github.com/repos/{slug}/code-scanning/alerts?state=open&ref=refs/heads/master&per_page=100"
    );
    let alerts: Vec<ApiAlert> =
        serde_json::from_str(&get(&url, Some(token))?).context("parsing GitHub's alerts")?;
    Ok(alerts_of(alerts, slug))
}

fn alerts_of(alerts: Vec<ApiAlert>, slug: &str) -> Vec<ApiAlert> {
    let prefix = format!("https://github.com/{slug}/security/code-scanning/");
    alerts
        .into_iter()
        .filter(|a| a.html_url.starts_with(&prefix))
        .filter(|a| is_object_name(&a.most_recent_instance.commit_sha))
        .filter(|a| a.most_recent_instance.location.start_line > 0)
        .filter(|a| safe_path(&a.most_recent_instance.location.path))
        .collect()
}

fn is_object_name(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A relative path inside the tree, not one git could read as an option.
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with(['/', '-'])
        && path.split('/').all(|p| !p.is_empty() && p != "..")
}

/// The series whose patch wrote the alert's line, by blaming the line at
/// the commit the alert was found at; None when no checked series did.
fn attribute(
    clone: &Path,
    state: &Path,
    name: &str,
    slug: &str,
    alert: &ApiAlert,
) -> Result<Option<(String, CodeqlAlert)>> {
    let at = &alert.most_recent_instance;
    if stack(clone, slug, &at.commit_sha)?.is_none() {
        return Ok(None);
    }
    let line = at.location.start_line.to_string();
    let Ok(blame) = git::run(
        clone,
        &[
            "blame",
            "--porcelain",
            "-L",
            &format!("{line},{line}"),
            &at.commit_sha,
            "--",
            &at.location.path,
        ],
    ) else {
        return Ok(None);
    };
    let Some(commit) = blame
        .split_whitespace()
        .next()
        .filter(|c| is_object_name(c))
    else {
        return Ok(None);
    };
    let show = git::command(clone)
        .args(["show", "--no-color", commit])
        .output()
        .context("running git show")?;
    let Some((id, _)) = crate::workspace::patch_id_lines(&show.stdout)?
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    let Some((series, patches)) = carried(state, name, &[id])?.into_iter().next() else {
        return Ok(None);
    };
    let rule = &alert.rule;
    let record = CodeqlAlert {
        repo: slug.to_string(),
        number: alert.number,
        rule: rule.id.clone(),
        severity: rule
            .security_severity_level
            .clone()
            .or_else(|| rule.severity.clone())
            .unwrap_or_default(),
        path: at.location.path.clone(),
        line: at.location.start_line,
        message: at.message.text.clone(),
        url: alert.html_url.clone(),
        patch: patches[0].1,
    };
    Ok(Some((series, record)))
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
pub fn carried(
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

/// Adds `alert` to the record of `series` (`<list>/<root>`), once.
fn add_alert(
    config: &Config,
    lists: &mut BTreeMap<String, ListState>,
    series: &str,
    alert: CodeqlAlert,
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
    if !record
        .codeql
        .iter()
        .any(|a| a.repo == alert.repo && a.number == alert.number)
    {
        record.codeql.push(alert);
    }
    Ok(())
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
    fn a_run_that_skipped_its_own_steps_tested_nothing() {
        let job = |conclusion: &str, steps: &[(&str, &str)]| ApiJob {
            conclusion: Some(conclusion.into()),
            steps: steps
                .iter()
                .map(|(n, c)| ApiStep {
                    name: (*n).into(),
                    conclusion: Some((*c).into()),
                })
                .collect(),
        };
        let housekeeping = [
            ("Set up job", "success"),
            ("Run actions/checkout@v5", "success"),
            ("Post Run actions/checkout@v5", "success"),
            ("Complete job", "success"),
        ];
        // yoctoproject/layerindex-web's Safety, without a Safety key.
        let mut skipped = housekeeping.to_vec();
        skipped.extend([("Build image", "skipped"), ("Safety scan", "skipped")]);
        assert!(all_skipped(&[job("success", &skipped)]));
        let mut ran = housekeeping.to_vec();
        ran.extend([("Build image", "success"), ("Safety scan", "skipped")]);
        assert!(!all_skipped(&[job("success", &ran)]));
        assert!(all_skipped(&[job("skipped", &[])]));
        assert!(!all_skipped(&[job("success", &housekeeping)]));
        assert!(!all_skipped(&[]));
    }

    #[test]
    fn keeps_alerts_with_sound_fields_only() {
        let alert = |n: u64, url: &str, sha: &str, path: &str| {
            serde_json::json!({"number": n, "html_url": url,
                "rule": {"id": "py/x", "severity": "error", "security_severity_level": "medium"},
                "most_recent_instance": {"commit_sha": sha, "message": {"text": "m"},
                    "location": {"path": path, "start_line": 3}}})
        };
        let ok = "https://github.com/o/r/security/code-scanning/";
        let sha = "a".repeat(40);
        let alerts: Vec<ApiAlert> = serde_json::from_value(serde_json::json!([
            alert(1, &format!("{ok}1"), &sha, "Post/views.py"),
            alert(2, "https://evil.example/2", &sha, "a.py"),
            alert(3, &format!("{ok}3"), "--output=x", "a.py"),
            alert(4, &format!("{ok}4"), &sha, "../etc/passwd"),
            alert(5, &format!("{ok}5"), &sha, "--exec=x"),
        ]))
        .unwrap();
        let kept: Vec<u64> = alerts_of(alerts, "o/r").iter().map(|a| a.number).collect();
        assert_eq!(kept, [1]);
    }

    #[test]
    fn attributes_an_alert_to_the_patch_that_wrote_its_line() {
        let repo = tempfile::tempdir().unwrap();
        let r = repo.path();
        let commit = |text: &str, msg: &str| {
            std::fs::write(r.join("views.py"), text).unwrap();
            git::run(r, &["add", "views.py"]).unwrap();
            git::run(
                r,
                &[
                    "-c",
                    "user.name=T",
                    "-c",
                    "user.email=t@x",
                    "commit",
                    "-qm",
                    msg,
                ],
            )
            .unwrap();
            git::run(r, &["rev-parse", "HEAD"]).unwrap()
        };
        git::run(r, &["init", "-q", "-b", "master"]).unwrap();
        commit("a = 1\n", "add views");
        let patch = commit("a = 1\nb = eval(x)\n", "views: evaluate x");
        let show = git::command(r).args(["show", &patch]).output().unwrap();
        let id = crate::workspace::patch_id_lines(&show.stdout)
            .unwrap()
            .remove(0)
            .0;
        let state = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(state.path().join("patch-ids")).unwrap();
        std::fs::write(
            state.path().join("patch-ids").join(&id),
            "yocto-patches/s@x master 2 error-report-web\n",
        )
        .unwrap();
        let alert: ApiAlert = serde_json::from_value(serde_json::json!({
            "number": 7, "html_url": "https://github.com/o/r/security/code-scanning/7",
            "rule": {"id": "py/code-injection", "severity": "error", "security_severity_level": "critical"},
            "most_recent_instance": {"commit_sha": patch, "message": {"text": "eval"},
                "location": {"path": "views.py", "start_line": 2}}
        }))
        .unwrap();
        let (series, found) = attribute(r, state.path(), "error-report-web", "o/r", &alert)
            .unwrap()
            .unwrap();
        assert_eq!(series, "yocto-patches/s@x");
        assert_eq!(
            (found.patch, found.severity.as_str(), found.line),
            (2, "critical", 2)
        );
        // Line 1 was written before any checked series.
        let mut old = alert;
        old.most_recent_instance.location.start_line = 1;
        assert!(
            attribute(r, state.path(), "error-report-web", "o/r", &old)
                .unwrap()
                .is_none()
        );
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
