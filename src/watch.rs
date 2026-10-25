//! The phase 2 watcher's configuration and on-disk state (designs/PHASE2.md).
//!
//! Everything lives under the configured state directory:
//!
//! ```text
//! state/
//!   lock
//!   lists/<list>.json          cursor, thread map and every series on the list
//!   series/<list>/<root>/      per-series files: the fetched mbox and reports
//! ```
//!
//! One JSON file per list keeps a poll to one read and one atomic write per
//! list. At about a thousand series per list per month the file stays in the
//! low megabytes.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::lore;
use crate::series::subject_tags;
use crate::track::{Tracked, Tracker};

/// How far before the newest message already seen each poll starts, so a
/// message lore indexes late is not missed. Duplicates are dropped by
/// Message-ID.
const OVERLAP_SECS: u64 = 600;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The bodnuti image checks run in.
    pub image: String,
    /// The container command, before the image. Its default is the
    /// rootless, read-only Podman invocation from docs/DEPLOY.md.
    #[serde(default = "default_runner")]
    pub runner: Vec<String>,
    pub state: PathBuf,
    pub cache: PathBuf,
    #[serde(default = "default_settle")]
    pub settle_minutes: u64,
    /// How far back the first poll of a list looks.
    #[serde(default = "default_initial")]
    pub initial_hours: u64,
    #[serde(rename = "list")]
    pub lists: Vec<ListConfig>,
    /// Clone name under `cache/` to its upstream URL.
    #[serde(default)]
    pub repos: BTreeMap<String, String>,
}

fn default_runner() -> Vec<String> {
    [
        "podman",
        "run",
        "--rm",
        "--userns=keep-id:uid=1000,gid=1000",
        "--read-only",
        "--tmpfs",
        "/tmp:rw,size=2g",
        "--cap-drop=ALL",
        "--security-opt",
        "no-new-privileges",
        "--pids-limit",
        "512",
        "--memory",
        "2g",
        "--cpus",
        "2",
    ]
    .map(String::from)
    .to_vec()
}

/// Attempts at fetching and checking a series before it is marked failed.
pub const MAX_ATTEMPTS: u32 = 3;

fn default_settle() -> u64 {
    15
}
fn default_initial() -> u64 {
    24
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListConfig {
    /// lore inbox name.
    pub name: String,
    /// The clone every series on this list is checked against.
    pub repo: Option<String>,
    #[serde(default)]
    pub route: Route,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Route {
    /// Every series goes to the list's `repo`.
    #[default]
    Fixed,
    /// The series' subject tag names the repository (yocto-patches).
    SubjectTag,
}

pub fn expand(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), std::env::var_os("HOME")) {
        (Ok(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => path.to_path_buf(),
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut config: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        config.state = expand(&config.state);
        config.cache = expand(&config.cache);
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        // Both become container volume sources, which must be absolute.
        for (key, path) in [("state", &self.state), ("cache", &self.cache)] {
            if !path.is_absolute() {
                bail!(
                    "{key} must be an absolute path or start with ~/, not {}",
                    path.display()
                );
            }
        }
        for list in &self.lists {
            // List names become file names.
            if !lore::safe_id(&list.name) || list.name.contains('@') {
                bail!("list name {:?} is not a lore inbox name", list.name);
            }
            match (list.route, &list.repo) {
                (Route::Fixed, Some(repo)) if self.repos.contains_key(repo) => {}
                (Route::Fixed, Some(repo)) => {
                    bail!("list {}: repo {repo} is not in [repos]", list.name)
                }
                (Route::Fixed, None) => bail!(
                    "list {}: needs a repo, or route = \"subject-tag\"",
                    list.name
                ),
                (Route::SubjectTag, None) => {}
                (Route::SubjectTag, Some(_)) => {
                    bail!("list {}: route = \"subject-tag\" takes no repo", list.name)
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Some patches have arrived.
    Seen,
    /// Every patch has arrived, or the series has settled.
    Complete,
    /// No configured repository can take the series; `reason` says why.
    Unrouted,
    /// Checked; the report is in the series directory.
    Checked,
    /// Every patch was already checked in an earlier series (a resend).
    Duplicate,
    /// Fetching or checking failed `MAX_ATTEMPTS` times; `reason` says why.
    Failed,
    /// Every patch has landed upstream; `applied` says where.
    Applied,
}

/// Where a checked patch landed upstream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Landed {
    pub commit: String,
    pub branch: String,
    /// The patch-id matched. When false, only the subject and author
    /// matched: the maintainer probably edited the patch before applying it.
    pub exact: bool,
}

/// What a check found, kept with the series for the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub findings: usize,
    pub errors: usize,
    pub tool_errors: usize,
}

impl CheckResult {
    pub fn of(report: &crate::report::Report) -> CheckResult {
        CheckResult {
            findings: report.findings.len(),
            errors: report
                .findings
                .iter()
                .filter(|f| f.severity == crate::finding::Severity::Error)
                .count(),
            tool_errors: report.tool_errors.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesRecord {
    #[serde(flatten)]
    pub tracked: Tracked,
    pub status: Status,
    /// The clone under `cache/` the series is checked against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// Why the series is unrouted, a duplicate, or failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<CheckResult>,
    /// The branch the series was checked against, e.g. `master` or
    /// `scarthgap`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Patch index to where it landed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applied: BTreeMap<u32, Landed>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ListState {
    /// Seconds since the epoch of the newest message seen.
    pub cursor: Option<u64>,
    pub root_of: HashMap<String, String>,
    pub series: BTreeMap<String, SeriesRecord>,
}

impl ListState {
    fn path(state: &Path, list: &str) -> PathBuf {
        state.join("lists").join(format!("{list}.json"))
    }

    pub fn load(state: &Path, list: &str) -> Result<ListState> {
        let path = ListState::path(state, list);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ListState::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Writes the state atomically: a crash leaves the old file or the new
    /// one, never half of either.
    pub fn save(&self, state: &Path, list: &str) -> Result<()> {
        let path = ListState::path(state, list);
        let dir = path.parent().context("state path has no parent")?;
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(".{list}.json.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// The `dt:` start for the next poll.
    pub fn since(&self, initial_hours: u64, now: u64) -> String {
        let start = match self.cursor {
            Some(cursor) => cursor.saturating_sub(OVERLAP_SECS),
            None => now.saturating_sub(initial_hours * 3600),
        };
        lore::utc(start)
    }

    /// Merges feed entries: new series start as `Seen`, known series gain
    /// the patches that arrived, and a series becomes `Complete` once all
    /// its patches are in or it has had no new message for `settle` seconds.
    /// Returns the number of new series.
    pub fn ingest(&mut self, entries: &[lore::Entry], now: u64, settle: u64) -> usize {
        let mut tracker = Tracker {
            root_of: std::mem::take(&mut self.root_of),
            series: self
                .series
                .iter()
                .map(|(root, r)| (root.clone(), r.tracked.clone()))
                .collect(),
        };
        tracker.ingest(entries);
        self.root_of = tracker.root_of;
        let mut new = 0;
        for (root, tracked) in tracker.series {
            match self.series.get_mut(&root) {
                Some(record) => record.tracked = tracked,
                None => {
                    new += 1;
                    self.series.insert(
                        root,
                        SeriesRecord {
                            tracked,
                            status: Status::Seen,
                            repo: None,
                            reason: None,
                            attempts: 0,
                            result: None,
                            branch: None,
                            applied: BTreeMap::new(),
                        },
                    );
                }
            }
        }
        for e in entries {
            if let Some(t) = lore::parse_time(&e.updated) {
                self.cursor = Some(self.cursor.map_or(t, |c| c.max(t)));
            }
        }
        for record in self.series.values_mut() {
            let settled = lore::parse_time(&record.tracked.last_seen)
                .is_some_and(|t| now.saturating_sub(t) >= settle);
            if record.status == Status::Seen && (record.tracked.complete() || settled) {
                record.status = Status::Complete;
            }
        }
        new
    }
}

/// The configured repository a subject tag names, if any. Any tag counts,
/// whatever its position: yocto-patches has both `[pseudo][PATCH]` and
/// `[PATCH][yocto-autobuilder-helper]`.
fn repo_from_tags(subject: &str, repos: &BTreeMap<String, String>) -> Option<String> {
    subject_tags(subject).iter().find_map(|tag| {
        repos
            .keys()
            .find(|repo| repo.eq_ignore_ascii_case(tag.trim()))
            .cloned()
    })
}

impl ListState {
    /// Chooses the repository for every complete series that has none yet.
    /// A series no configured repository can take becomes `Unrouted`.
    pub fn route(&mut self, list: &ListConfig, repos: &BTreeMap<String, String>) {
        for record in self.series.values_mut() {
            if record.status != Status::Complete || record.repo.is_some() {
                continue;
            }
            let repo = match list.route {
                Route::Fixed => list.repo.clone(),
                Route::SubjectTag => repo_from_tags(&record.tracked.subject, repos),
            };
            match repo {
                Some(repo) => record.repo = Some(repo),
                None => {
                    record.status = Status::Unrouted;
                    record.reason = Some(format!(
                        "no subject tag names a configured repository (tags: {})",
                        subject_tags(&record.tracked.subject).join(", ")
                    ));
                }
            }
        }
    }
}

/// Holds `state/lock` for the life of a poll or publish, so two timer runs
/// never work on the same state.
pub struct Lock {
    _file: File,
}

impl Lock {
    pub fn take(state: &Path) -> Result<Lock> {
        std::fs::create_dir_all(state)?;
        let file = File::create(state.join("lock"))?;
        match file.try_lock() {
            Ok(()) => Ok(Lock { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => {
                bail!("another bodnuti run holds {}", state.join("lock").display())
            }
            Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, subject: &str, parent: Option<&str>, t: &str) -> lore::Entry {
        lore::Entry {
            message_id: id.into(),
            subject: subject.into(),
            in_reply_to: parent.map(String::from),
            updated: t.into(),
        }
    }

    const CONFIG: &str = r#"
        image = "localhost/bodnuti"
        state = "/srv/state"
        cache = "/srv/cache"

        [[list]]
        name = "openembedded-core"
        repo = "openembedded-core"

        [[list]]
        name = "yocto-patches"
        route = "subject-tag"

        [repos]
        openembedded-core = "https://git.openembedded.org/openembedded-core"
    "#;

    #[test]
    fn expands_a_home_relative_path() {
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(
                expand(Path::new("~/state")),
                PathBuf::from(home).join("state")
            );
        }
        assert_eq!(expand(Path::new("/srv/state")), PathBuf::from("/srv/state"));
    }

    #[test]
    fn reads_and_checks_the_config() {
        let mut c: Config = toml::from_str(CONFIG).unwrap();
        c.validate().unwrap();
        assert_eq!(c.settle_minutes, 15);
        assert_eq!(c.lists[1].route, Route::SubjectTag);
        c.lists[0].repo = Some("meta-oe".into());
        assert!(
            c.validate()
                .unwrap_err()
                .to_string()
                .contains("not in [repos]")
        );
        assert!(toml::from_str::<Config>(&CONFIG.replace("cache =", "cash =")).is_err());
        let mut relative: Config = toml::from_str(&CONFIG.replace("/srv/state", "state")).unwrap();
        relative.lists.truncate(1);
        assert!(
            relative
                .validate()
                .unwrap_err()
                .to_string()
                .contains("absolute")
        );
    }

    #[test]
    fn state_survives_a_save_and_completes_across_polls() {
        let dir = tempfile::tempdir().unwrap();
        let t0 = lore::parse_time("2026-10-24T10:00:00Z").unwrap();
        let mut s = ListState::default();
        let new = s.ingest(
            &[entry(
                "1@x",
                "[PATCH 1/2] a: b",
                None,
                "2026-10-24T10:00:00Z",
            )],
            t0,
            900,
        );
        assert_eq!(new, 1);
        assert_eq!(s.series["1@x"].status, Status::Seen);
        s.save(dir.path(), "oe").unwrap();

        // The second patch replies to the first, which only the saved thread
        // map still knows about.
        let mut s = ListState::load(dir.path(), "oe").unwrap();
        assert_eq!(s.since(24, t0), "20261024095000");
        let new = s.ingest(
            &[entry(
                "2@x",
                "[PATCH 2/2] a: c",
                Some("1@x"),
                "2026-10-24T10:01:00Z",
            )],
            t0 + 60,
            900,
        );
        assert_eq!(new, 0);
        assert_eq!(s.series["1@x"].status, Status::Complete);
        assert_eq!(s.cursor, lore::parse_time("2026-10-24T10:01:00Z"));
    }

    #[test]
    fn a_series_settles_without_all_its_patches() {
        let t0 = lore::parse_time("2026-10-24T10:00:00Z").unwrap();
        let mut s = ListState::default();
        s.ingest(
            &[entry(
                "1@x",
                "[PATCH 1/3] a: b",
                None,
                "2026-10-24T10:00:00Z",
            )],
            t0,
            900,
        );
        assert_eq!(s.series["1@x"].status, Status::Seen);
        s.ingest(&[], t0 + 900, 900);
        assert_eq!(s.series["1@x"].status, Status::Complete);
    }

    #[test]
    fn routes_by_fixed_repo_or_by_any_subject_tag() {
        let c: Config = toml::from_str(&format!(
            "{CONFIG}\nlayerindex-web = \"x\"\nyocto-autobuilder-helper = \"y\"\n"
        ))
        .unwrap();
        let t = lore::parse_time("2026-10-24T10:00:00Z").unwrap();
        let mut patches = ListState::default();
        patches.ingest(
            &[
                entry(
                    "a@x",
                    "[layerindex-web][PATCH 1/1] fix",
                    None,
                    "2026-10-24T10:00:00Z",
                ),
                entry(
                    "b@x",
                    "[PATCH][Yocto-Autobuilder-Helper] abint",
                    None,
                    "2026-10-24T10:00:00Z",
                ),
                entry(
                    "c@x",
                    "[PATCH] mystery: change",
                    None,
                    "2026-10-24T10:00:00Z",
                ),
            ],
            t,
            900,
        );
        patches.route(&c.lists[1], &c.repos);
        assert_eq!(
            patches.series["a@x"].repo.as_deref(),
            Some("layerindex-web")
        );
        assert_eq!(
            patches.series["b@x"].repo.as_deref(),
            Some("yocto-autobuilder-helper")
        );
        let c_rec = &patches.series["c@x"];
        assert_eq!(c_rec.status, Status::Unrouted);
        assert!(c_rec.reason.as_deref().unwrap().contains("tags: PATCH"));

        let mut core = ListState::default();
        core.ingest(
            &[entry(
                "d@x",
                "[PATCH] curl: fix",
                None,
                "2026-10-24T10:00:00Z",
            )],
            t,
            900,
        );
        core.route(&c.lists[0], &c.repos);
        assert_eq!(
            core.series["d@x"].repo.as_deref(),
            Some("openembedded-core")
        );
    }

    #[test]
    fn a_second_run_cannot_take_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let _held = Lock::take(dir.path()).unwrap();
        assert!(Lock::take(dir.path()).is_err());
    }
}
