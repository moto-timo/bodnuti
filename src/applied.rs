//! Matching checked patches to the upstream commits they became
//! (designs/PHASE2.md section 5.1).
//!
//! After the clones are fetched, the branch each checked series targets is
//! scanned from where the previous scan stopped. A commit whose patch-id was
//! checked for that branch marks the patch applied. A maintainer often edits
//! a patch before applying it, which changes its patch-id; for those, a
//! commit with the patch's subject and author marks it as probably applied.
//! Only commits on a series' own branch count: the same patch landing on
//! another branch is another series' backport.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::git;
use crate::lore;
use crate::runner::read_owners;
use crate::series::Series;
use crate::watch::{Config, Landed, ListState, Status};

/// How far before the oldest checked series the first scan of a branch
/// starts, for patches applied before the scan existed.
const FIRST_SCAN_SLACK_SECS: u64 = 86_400;

/// `state/upstream/<repo>.json`: the last commit scanned on each branch.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Upstream {
    branches: BTreeMap<String, String>,
}

impl Upstream {
    fn path(state: &Path, repo: &str) -> std::path::PathBuf {
        state.join("upstream").join(format!("{repo}.json"))
    }

    fn load(state: &Path, repo: &str) -> Result<Upstream> {
        match std::fs::read_to_string(Upstream::path(state, repo)) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Upstream::default()),
            Err(e) => Err(e.into()),
        }
    }

    fn save(&self, state: &Path, repo: &str) -> Result<()> {
        let path = Upstream::path(state, repo);
        std::fs::create_dir_all(path.parent().context("no parent")?)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Summary {
    pub exact: usize,
    pub probable: usize,
    pub series_applied: usize,
}

/// A checked series still waiting for patches to land.
struct Waiting {
    list: String,
    root: String,
    /// The branch it was checked for; only commits there can apply it.
    branch: String,
    series: Series,
}

/// Scans every repository that has checked, unapplied series, and updates
/// the records in `states` (list name to state).
pub fn scan(config: &Config, states: &mut BTreeMap<String, ListState>) -> Result<Summary> {
    let mut summary = Summary::default();
    let repos: BTreeSet<String> = states
        .values()
        .flat_map(|s| s.series.values())
        .filter(|r| r.status == Status::Checked)
        .filter_map(|r| r.repo.clone())
        .collect();
    for repo in repos {
        scan_repo(config, &repo, states, &mut summary)
            .with_context(|| format!("scanning upstream {repo}"))?;
    }
    Ok(summary)
}

/// The checked series of `repo` that have a known branch and a fetched mbox,
/// and the time of the oldest one's last message.
fn waiting(
    config: &Config,
    repo: &str,
    states: &BTreeMap<String, ListState>,
) -> (Vec<Waiting>, u64) {
    let mut out = Vec::new();
    let mut oldest = u64::MAX;
    for (list, state) in states {
        for record in state.series.values() {
            if record.status != Status::Checked || record.repo.as_deref() != Some(repo) {
                continue;
            }
            let Some(branch) = record.branch.clone() else {
                continue;
            };
            let root = &record.tracked.root;
            let mbox = config
                .state
                .join("series")
                .join(list)
                .join(root)
                .join(format!("{root}.mbx"));
            let Ok(series) = Series::load(&[&mbox]) else {
                continue;
            };
            if let Some(t) = lore::parse_time(&record.tracked.last_seen) {
                oldest = oldest.min(t);
            }
            out.push(Waiting {
                list: list.clone(),
                root: root.clone(),
                branch,
                series,
            });
        }
    }
    (out, oldest)
}

fn scan_repo(
    config: &Config,
    repo: &str,
    states: &mut BTreeMap<String, ListState>,
    summary: &mut Summary,
) -> Result<()> {
    let clone = config.cache.join(repo);
    let (waiting, oldest) = waiting(config, repo, states);
    if waiting.is_empty() {
        return Ok(());
    }

    let mut upstream = Upstream::load(&config.state, repo)?;
    let branches: BTreeSet<&str> = waiting.iter().map(|w| w.branch.as_str()).collect();
    for short in branches {
        let branch = format!("origin/{short}");
        let Ok(tip) = git::run(&clone, &["rev-parse", "--verify", "--quiet", &branch]) else {
            continue;
        };
        let range = match upstream.branches.get(&branch) {
            Some(last) if *last == tip => continue,
            Some(last) => vec![format!("{last}..{tip}")],
            None => vec![
                format!("--since=@{}", oldest.saturating_sub(FIRST_SCAN_SLACK_SECS)),
                tip.clone(),
            ],
        };

        for (id, commit) in commit_patch_ids(&clone, &range)? {
            for owner in read_owners(&config.state, &id)? {
                let Some(index) = owner.index else { continue };
                if owner.branch.as_deref() != Some(short) {
                    continue;
                }
                let Some((list, root)) = owner.series.split_once('/') else {
                    continue;
                };
                if let Some(record) = states.get_mut(list).and_then(|s| s.series.get_mut(root))
                    && record.status == Status::Checked
                    && !record.applied.contains_key(&index)
                {
                    let landed = Landed {
                        commit: commit.clone(),
                        branch: short.to_string(),
                        exact: true,
                    };
                    record.applied.insert(index, landed);
                    summary.exact += 1;
                }
            }
        }

        let commits = commit_subjects(&clone, &range)?;
        for w in waiting.iter().filter(|w| w.branch == short) {
            let Some(record) = states
                .get_mut(&w.list)
                .and_then(|s| s.series.get_mut(&w.root))
            else {
                continue;
            };
            for patch in &w.series.patches {
                if record.applied.contains_key(&patch.index) {
                    continue;
                }
                let email = patch.author_email();
                if let Some((commit, _, _)) = commits
                    .iter()
                    .find(|(_, e, s)| s == patch.summary() && Some(e) == email.as_ref())
                {
                    let landed = Landed {
                        commit: commit.clone(),
                        branch: short.to_string(),
                        exact: false,
                    };
                    record.applied.insert(patch.index, landed);
                    summary.probable += 1;
                }
            }
        }
        upstream.branches.insert(branch, tip);
    }
    upstream.save(&config.state, repo)?;

    for w in &waiting {
        if let Some(record) = states
            .get_mut(&w.list)
            .and_then(|s| s.series.get_mut(&w.root))
            && w.series
                .patches
                .iter()
                .all(|p| record.applied.contains_key(&p.index))
        {
            record.status = Status::Applied;
            summary.series_applied += 1;
        }
    }
    Ok(())
}

/// How far before a series' last message `already_applied` looks: patches
/// are usually applied within weeks, and older history only costs time.
const ALREADY_APPLIED_WINDOW_SECS: u64 = 90 * 86_400;

/// Where each patch of `series` landed in the history of `base`, if every
/// one did: by patch-id, else by subject and author. This recognizes a
/// series checked after it was merged, whose files the merge renamed or
/// removed. `ids` are the series' `(patch-id, index)` pairs and `last_seen`
/// the time of its last message.
pub fn already_applied(
    clone: &Path,
    base: &str,
    branch: &str,
    series: &Series,
    ids: &[(String, u32)],
    last_seen: u64,
) -> Result<Option<BTreeMap<u32, Landed>>> {
    let landed = landed(
        clone,
        base,
        branch,
        series,
        ids,
        last_seen.saturating_sub(ALREADY_APPLIED_WINDOW_SECS),
    )?;
    Ok((landed.len() == series.patches.len()).then_some(landed))
}

/// How far before a series' last message the check looks for its patches
/// in the base's history: a patch lands after it is sent, and a series'
/// first revision is rarely older than this.
pub const LANDED_BEFORE_CHECK_WINDOW_SECS: u64 = 30 * 86_400;

/// Where each patch of `series` that has landed in the history of `base`
/// since `since` landed: by patch-id, else by subject and author. The
/// scans only read commits that arrive after a series is checked, so the
/// check calls this for those already there: a series checked late, as a
/// backfill does, has often landed in part or in full.
pub fn landed(
    clone: &Path,
    base: &str,
    branch: &str,
    series: &Series,
    ids: &[(String, u32)],
    since: u64,
) -> Result<BTreeMap<u32, Landed>> {
    let range = vec![format!("--since=@{since}"), base.to_string()];
    let by_id: BTreeMap<String, String> = commit_patch_ids(clone, &range)?.into_iter().collect();
    let commits = commit_subjects(clone, &range)?;
    let mut landed = BTreeMap::new();
    for patch in &series.patches {
        let exact = ids
            .iter()
            .find(|(_, index)| *index == patch.index)
            .and_then(|(id, _)| by_id.get(id));
        let email = patch.author_email();
        let found = match exact {
            Some(commit) => Some((commit.clone(), true)),
            None => commits
                .iter()
                .find(|(_, e, s)| s == patch.summary() && Some(e) == email.as_ref())
                .map(|(commit, _, _)| (commit.clone(), false)),
        };
        let Some((commit, exact)) = found else {
            continue;
        };
        landed.insert(
            patch.index,
            Landed {
                commit,
                branch: branch.to_string(),
                exact,
            },
        );
    }
    Ok(landed)
}

/// `(patch-id, commit)` for every non-merge commit in `range`, in one
/// `git log -p | git patch-id --stable` pipeline.
fn commit_patch_ids(clone: &Path, range: &[String]) -> Result<Vec<(String, String)>> {
    let mut log = git::command(clone)
        .args(["log", "-p", "--no-merges"])
        .args(range)
        .stdout(Stdio::piped())
        .spawn()
        .context("running git log")?;
    let mut patch_id = Command::new("git");
    git::strip_repository_env(&mut patch_id);
    let output = patch_id
        .args(["patch-id", "--stable"])
        .stdin(log.stdout.take().context("git log stdout")?)
        .output()
        .context("running git patch-id")?;
    log.wait()?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| {
            l.split_once(' ')
                .map(|(id, c)| (id.to_string(), c.to_string()))
        })
        .collect())
}

/// `(commit, author email, subject)` for every non-merge commit in `range`.
fn commit_subjects(clone: &Path, range: &[String]) -> Result<Vec<(String, String, String)>> {
    let mut args = vec![
        "log".to_string(),
        "--no-merges".into(),
        "--format=%H%x00%ae%x00%s".into(),
    ];
    args.extend(range.iter().cloned());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(git::run(clone, &args)?
        .lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, '\0');
            Some((
                parts.next()?.into(),
                parts.next()?.to_ascii_lowercase(),
                parts.next()?.into(),
            ))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::SeriesRecord;

    fn commit(repo: &Path, file: &str, text: &str, subject: &str) {
        std::fs::write(repo.join(file), text).unwrap();
        git::run(repo, &["add", file]).unwrap();
        git::run(
            repo,
            &[
                "-c",
                "user.name=Ann Dev",
                "-c",
                "user.email=ann@example.org",
                "commit",
                "-qm",
                subject,
            ],
        )
        .unwrap();
    }

    /// An "upstream" repository with master and scarthgap, and a clone of it
    /// as bodnuti would have.
    fn upstream(root: &Path) -> std::path::PathBuf {
        let up = root.join("upstream");
        std::fs::create_dir_all(&up).unwrap();
        git::run(&up, &["init", "-q", "-b", "master"]).unwrap();
        commit(&up, "a.bb", "A = \"1\"\n", "a: add");
        git::run(&up, &["branch", "scarthgap"]).unwrap();
        let clone = root.join("cache/oe");
        git::run(
            root,
            &["clone", "-q", up.to_str().unwrap(), clone.to_str().unwrap()],
        )
        .unwrap();
        up
    }

    /// Records a one-patch series checked for `branch`, whose mbox is `patch`.
    fn checked(
        config: &Config,
        states: &mut BTreeMap<String, ListState>,
        root: &str,
        branch: &str,
        patch: &str,
        id: &str,
    ) {
        let dir = config.state.join("series/oe-list").join(root);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{root}.mbx")), patch).unwrap();
        std::fs::create_dir_all(config.state.join("patch-ids")).unwrap();
        let ids = config.state.join("patch-ids").join(id);
        let mut text = std::fs::read_to_string(&ids).unwrap_or_default();
        text.push_str(&format!("oe-list/{root} {branch} 1\n"));
        std::fs::write(ids, text).unwrap();
        let state = states.entry("oe-list".to_string()).or_default();
        state.series.insert(
            root.into(),
            SeriesRecord {
                tracked: crate::track::Tracked {
                    root: root.into(),
                    subject: "[PATCH] a: change".into(),
                    total: 1,
                    seen: BTreeSet::from([1]),
                    last_seen: "2026-10-24T10:00:00Z".into(),
                },
                status: Status::Checked,
                repo: Some("oe".into()),
                reason: None,
                attempts: 0,
                result: None,
                branch: Some(branch.into()),
                applied: BTreeMap::new(),
                candidates: Vec::new(),
            },
        );
    }

    fn config(root: &Path) -> Config {
        toml::from_str(&format!(
            "image = \"x\"\nstate = \"{0}/state\"\ncache = \"{0}/cache\"\n\
             [[list]]\nname = \"oe-list\"\nrepo = \"oe\"\n[repos]\noe = \"x\"\n",
            root.display()
        ))
        .unwrap()
    }

    /// The patch for a change made in `up`, as a contributor would send it,
    /// with its patch-id. The change is then reset away.
    fn sent_patch(up: &Path, text: &str) -> (String, String) {
        commit(up, "a.bb", text, "a: change");
        let mbox = git::run(up, &["format-patch", "--stdout", "-1"]).unwrap() + "\n";
        let log = git::run(up, &["log", "-1", "-p", "--format=commit %H"]).unwrap();
        let mut child = Command::new("git")
            .args(["patch-id", "--stable"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(log.as_bytes())
            .unwrap();
        let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
        git::run(up, &["reset", "-q", "--hard", "HEAD~1"]).unwrap();
        (mbox, out.split_whitespace().next().unwrap().to_string())
    }

    fn fetch(config: &Config) {
        git::run(&config.cache.join("oe"), &["fetch", "-q"]).unwrap();
    }

    #[test]
    fn finds_a_series_merged_before_it_was_checked() {
        let dir = tempfile::tempdir().unwrap();
        let up = upstream(dir.path());
        let config = config(dir.path());
        let clone = config.cache.join("oe");
        let (mbox, id) = sent_patch(&up, "A = \"2\"\n");
        let series = Series::parse(mbox.as_bytes()).unwrap();
        let ids = vec![(id, 1)];
        let now = lore::now();

        assert_eq!(
            already_applied(&clone, "origin/master", "master", &series, &ids, now).unwrap(),
            None
        );
        commit(&up, "a.bb", "A = \"2\"\n", "a: change, as merged");
        fetch(&config);
        let landed = already_applied(&clone, "origin/master", "master", &series, &ids, now)
            .unwrap()
            .unwrap();
        assert!(landed[&1].exact);
        assert_eq!(landed[&1].branch, "master");
        // landed reports what is there, all of the series or not.
        let partial = landed_patches(&clone, &series, &ids);
        assert_eq!(partial.len(), 1);
    }

    fn landed_patches(
        clone: &Path,
        series: &Series,
        ids: &[(String, u32)],
    ) -> BTreeMap<u32, Landed> {
        // A day back, not 0: git does not reliably read --since=@0 as
        // the epoch.
        landed(
            clone,
            "origin/master",
            "master",
            series,
            ids,
            lore::now() - 86_400,
        )
        .unwrap()
    }

    #[test]
    fn an_applied_patch_matches_by_patch_id() {
        let dir = tempfile::tempdir().unwrap();
        let up = upstream(dir.path());
        let config = config(dir.path());
        let (mbox, id) = sent_patch(&up, "A = \"2\"\n");
        let mut states = BTreeMap::new();
        checked(&config, &mut states, "s1@x", "master", &mbox, &id);

        // Nothing applied yet.
        assert_eq!(scan(&config, &mut states).unwrap(), Summary::default());

        // The maintainer applies it unchanged, with a reworded message.
        commit(&up, "a.bb", "A = \"2\"\n", "a: change, as merged");
        fetch(&config);
        let summary = scan(&config, &mut states).unwrap();
        assert_eq!(
            summary,
            Summary {
                exact: 1,
                probable: 0,
                series_applied: 1
            }
        );
        let record = &states["oe-list"].series["s1@x"];
        assert_eq!(record.status, Status::Applied);
        assert!(record.applied[&1].exact);
        assert_eq!(record.applied[&1].branch, "master");
    }

    #[test]
    fn an_edited_patch_matches_by_subject_and_author() {
        let dir = tempfile::tempdir().unwrap();
        let up = upstream(dir.path());
        let config = config(dir.path());
        let (mbox, id) = sent_patch(&up, "A = \"2\"\n");
        let mut states = BTreeMap::new();
        checked(&config, &mut states, "s2@x", "master", &mbox, &id);
        scan(&config, &mut states).unwrap();

        // The maintainer changes the diff before applying it.
        commit(&up, "a.bb", "A = \"3\"\n", "a: change");
        fetch(&config);
        let summary = scan(&config, &mut states).unwrap();
        assert_eq!(
            summary,
            Summary {
                exact: 0,
                probable: 1,
                series_applied: 1
            }
        );
        assert!(!states["oe-list"].series["s2@x"].applied[&1].exact);
    }

    #[test]
    fn a_patch_landing_on_one_branch_does_not_apply_its_backport() {
        let dir = tempfile::tempdir().unwrap();
        let up = upstream(dir.path());
        let config = config(dir.path());
        let (mbox, id) = sent_patch(&up, "A = \"2\"\n");
        let mut states = BTreeMap::new();
        checked(&config, &mut states, "m@x", "master", &mbox, &id);
        checked(&config, &mut states, "s@x", "scarthgap", &mbox, &id);
        scan(&config, &mut states).unwrap();

        commit(&up, "a.bb", "A = \"2\"\n", "a: change");
        fetch(&config);
        let summary = scan(&config, &mut states).unwrap();
        assert_eq!(summary.series_applied, 1);
        assert_eq!(states["oe-list"].series["m@x"].status, Status::Applied);
        assert_eq!(states["oe-list"].series["s@x"].status, Status::Checked);
        assert!(states["oe-list"].series["s@x"].applied.is_empty());
    }
}
