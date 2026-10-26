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

use crate::kind::Kind;
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
    /// Clone name under `cache/` to its upstream URL and kind.
    #[serde(default)]
    pub repos: BTreeMap<String, Repo>,
    /// How far back the published site goes. Older series stay in `state/`.
    #[serde(default = "default_retain")]
    pub retain_months: u64,
    /// Where `bodnuti publish` copies the site with rsync, e.g.
    /// `bodnuti-web:./`. Without it the site is only written to
    /// `state/site/`.
    #[serde(default)]
    pub publish_to: Option<String>,
}

/// A `[repos]` entry: its upstream URL, a layer, or a table with the URL and
/// a kind (designs/PHASE2.md §5.2):
///
/// ```toml
/// openembedded-core = "https://git.openembedded.org/openembedded-core"
/// layerindex-web = { url = "https://git.yoctoproject.org/layerindex-web", kind = "django-app" }
/// ```
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(from = "RepoSpec")]
pub struct Repo {
    pub url: String,
    pub kind: Kind,
    /// Subject tags besides the repository's name that route to it, such
    /// as `AUH` for auto-upgrade-helper (designs/PHASE2.md §3.1.1).
    pub tags: Vec<String>,
    /// Where the maintainers stage patches before each branch
    /// (designs/PHASE2.md §5.6).
    pub staging: Option<Staging>,
}

/// The remote a clone fetches a [`Staging::Remote`] repository's staging
/// branches from.
pub const STAGING_REMOTE: &str = "staging";

/// Where a repository's maintainers stage the patches they take before they
/// reach each branch (designs/PHASE2.md §5.6):
///
/// ```toml
/// staging = "{branch}-next"
/// staging = { url = "https://git.openembedded.org/meta-openembedded-contrib", branches = { master = "yoe/mut" } }
/// ```
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Staging {
    /// A branch of the repository itself, `{branch}` standing for the
    /// branch it stages.
    Pattern(String),
    /// Branches of another repository, such as a contrib one, by the
    /// branch each stages. The clone fetches them as remote `staging`.
    Remote {
        url: String,
        branches: BTreeMap<String, String>,
    },
}

impl Staging {
    /// The ref of the staging branch for `branch` in the clone, if there is
    /// one.
    pub fn ref_for(&self, branch: &str) -> Option<String> {
        match self {
            Staging::Pattern(p) => Some(format!("origin/{}", p.replace("{branch}", branch))),
            Staging::Remote { branches, .. } => branches
                .get(branch)
                .map(|b| format!("{STAGING_REMOTE}/{b}")),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RepoSpec {
    Url(String),
    Table(RepoTable),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoTable {
    url: String,
    #[serde(default)]
    kind: Kind,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    staging: Option<Staging>,
}

impl From<RepoSpec> for Repo {
    fn from(spec: RepoSpec) -> Repo {
        match spec {
            RepoSpec::Url(url) => Repo {
                url,
                kind: Kind::Layer,
                tags: Vec::new(),
                staging: None,
            },
            RepoSpec::Table(t) => Repo {
                url: t.url,
                kind: t.kind,
                tags: t.tags,
                staging: t.staging,
            },
        }
    }
}

fn default_runner() -> Vec<String> {
    [
        "podman",
        // Under systemd, podman writes each container's events and output to
        // the journal. bodnuti reads the output itself and logs one line per
        // series, so both would only repeat it.
        "--events-backend=none",
        "run",
        "--log-driver=none",
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
fn default_retain() -> u64 {
    6
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
        // The destination is an rsync argument; one that starts with a dash
        // would be read as an option.
        if self
            .publish_to
            .as_deref()
            .is_some_and(|d| d.is_empty() || d.starts_with('-'))
        {
            bail!(
                "publish_to {:?} is not an rsync destination",
                self.publish_to
            );
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
    /// The thread has no patches: a pull request, or a cover letter whose
    /// patches were sent outside its thread (designs/PHASE2.md §5.0).
    #[serde(rename = "no-patches")]
    NoPatches,
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
    /// The repositories a subject tag names, when it names more than one;
    /// `repo` is the one the path check picked, or the first before it ran
    /// (designs/PHASE2.md §3.1.1). Absent in older state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    /// For a resend, the series (`<list>/<root>`) its patches were checked
    /// in. Absent in older state, where `reason` names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<String>,
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
                            candidates: Vec::new(),
                            duplicate_of: None,
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
/// Every configured repository a subject tag names, by its name or one of
/// its `tags`, in the order of the tags and then of `[repos]`. A tag may
/// name several: `[AUH]` is both auto-upgrade-helper's own tag and the one
/// its bot sends OE-Core upgrades under. The path check picks among them.
fn repos_from_tags(subject: &str, repos: &BTreeMap<String, Repo>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in tag_words(subject) {
        for (name, repo) in repos {
            let named = name.eq_ignore_ascii_case(&word)
                || repo.tags.iter().any(|t| t.eq_ignore_ascii_case(&word));
            if named && !out.contains(name) {
                out.push(name.clone());
            }
        }
    }
    out
}

/// The subject's tags, with a PATCH tag split into its words: b4 puts the
/// repository in it, as in `[PATCH yocto-autobuilder2]`.
fn tag_words(subject: &str) -> Vec<String> {
    subject_tags(subject)
        .iter()
        .flat_map(|tag| {
            if tag.to_ascii_uppercase().contains("PATCH") {
                tag.split_whitespace().map(String::from).collect()
            } else {
                vec![tag.trim().to_string()]
            }
        })
        .collect()
}

impl ListState {
    /// Chooses the repository for every complete series that has none yet.
    /// A series no configured repository can take becomes `Unrouted`.
    ///
    /// Series left unrouted because no subject tag named a repository are
    /// tried again on every poll, so adding a repository to `[repos]` picks
    /// up its earlier series. That costs only a comparison of tags. Series
    /// unrouted by the path check (§3.1) have a repository and are not
    /// retried: that would fetch them again for the same result.
    pub fn route(&mut self, list: &ListConfig, repos: &BTreeMap<String, Repo>) {
        for record in self.series.values_mut() {
            let unrouted_by_tag = record.status == Status::Unrouted;
            if !matches!(record.status, Status::Complete | Status::Unrouted)
                || record.repo.is_some()
            {
                continue;
            }
            let candidates = match list.route {
                Route::Fixed => list.repo.iter().cloned().collect(),
                Route::SubjectTag => repos_from_tags(&record.tracked.subject, repos),
            };
            match candidates.first().cloned() {
                Some(repo) => {
                    record.repo = Some(repo);
                    // The runner's path check chooses when there is a choice.
                    record.candidates = if candidates.len() > 1 {
                        candidates
                    } else {
                        Vec::new()
                    };
                    if unrouted_by_tag {
                        record.status = Status::Complete;
                        record.reason = None;
                    }
                }
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
    fn reads_where_a_repository_stages_its_branches() {
        let config: Config = toml::from_str(&format!(
            "{CONFIG}meta-openembedded = {{ url = \"u\", staging = {{ url = \"c\", branches = \
             {{ master = \"yoe/mut\", scarthgap = \"anujm/scarthgap\" }} }} }}\n\
             bitbake = {{ url = \"u\", staging = \"{{branch}}-next\" }}\n"
        ))
        .unwrap();
        let moe = config.repos["meta-openembedded"].staging.as_ref().unwrap();
        assert_eq!(moe.ref_for("master").as_deref(), Some("staging/yoe/mut"));
        assert_eq!(
            moe.ref_for("scarthgap").as_deref(),
            Some("staging/anujm/scarthgap")
        );
        assert_eq!(moe.ref_for("wrynose"), None);
        let oe = config.repos["bitbake"].staging.as_ref().unwrap();
        assert_eq!(
            oe.ref_for("scarthgap").as_deref(),
            Some("origin/scarthgap-next")
        );
    }

    #[test]
    fn a_tag_alias_can_name_several_repositories() {
        let c: Config = toml::from_str(&format!(
            "{CONFIG}auto-upgrade-helper = {{ url = \"u\", kind = \"python\", tags = [\"AUH\"] }}\n"
        ))
        .unwrap();
        let mut repos = c.repos.clone();
        repos.get_mut("openembedded-core").unwrap().tags = vec!["AUH".into()];
        assert_eq!(
            repos_from_tags("[AUH][PATCH] steps: add an option", &repos),
            ["auto-upgrade-helper", "openembedded-core"]
        );
        assert_eq!(
            repos_from_tags("[auh] curl: upgrading to 8.23.0 SUCCEEDED", &repos),
            ["auto-upgrade-helper", "openembedded-core"]
        );
        assert!(repos_from_tags("[PATCH] x: y", &repos).is_empty());

        let t = lore::parse_time("2026-10-24T10:00:00Z").unwrap();
        let mut s = ListState::default();
        s.ingest(
            &[entry(
                "a@x",
                "[AUH][PATCH] steps: add",
                None,
                "2026-10-24T10:00:00Z",
            )],
            t,
            900,
        );
        s.route(&c.lists[1], &repos);
        let r = &s.series["a@x"];
        assert_eq!(r.repo.as_deref(), Some("auto-upgrade-helper"));
        assert_eq!(r.candidates, ["auto-upgrade-helper", "openembedded-core"]);
    }

    #[test]
    fn a_repository_is_a_url_or_a_table_with_a_kind() {
        let c: Config = toml::from_str(&format!(
            "{CONFIG}layerindex-web = {{ url = \"https://git.yoctoproject.org/layerindex-web\", kind = \"django-app\" }}\n"
        ))
        .unwrap();
        assert_eq!(c.repos["openembedded-core"].kind, Kind::Layer);
        let web = &c.repos["layerindex-web"];
        assert_eq!(web.kind, Kind::DjangoApp);
        let helper: Config = toml::from_str(&format!(
            "{CONFIG}h = {{ url = \"u\", kind = \"python\" }}\n"
        ))
        .unwrap();
        assert_eq!(helper.repos["h"].kind, Kind::Python);
        assert_eq!(web.url, "https://git.yoctoproject.org/layerindex-web");
        for bad in [
            "x = { url = \"u\", kind = \"rails-app\" }",
            "x = { url = \"u\", knid = \"django-app\" }",
        ] {
            assert!(
                toml::from_str::<Config>(&format!("{CONFIG}{bad}\n")).is_err(),
                "{bad}"
            );
        }
    }

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
        assert_eq!(c.retain_months, 6);
        assert_eq!(c.publish_to, None);
        assert_eq!(c.lists[1].route, Route::SubjectTag);
        c.publish_to = Some("--rsh=evil".into());
        assert!(c.validate().unwrap_err().to_string().contains("rsync"));
        c.publish_to = Some("bodnuti-web:./".into());
        c.validate().unwrap();
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
                entry(
                    "d@x",
                    "[PATCH yocto-autobuilder-helper] schedulers: fix",
                    None,
                    "2026-10-24T10:00:00Z",
                ),
            ],
            t,
            900,
        );
        patches.route(&c.lists[1], &c.repos);
        assert_eq!(
            patches.series["d@x"].repo.as_deref(),
            Some("yocto-autobuilder-helper")
        );
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

        // A repository added later picks up the series its tag names.
        let mut repos = c.repos.clone();
        repos.insert(
            "mystery".into(),
            Repo {
                url: "u".into(),
                kind: Kind::Layer,
                tags: Vec::new(),
                staging: None,
            },
        );
        patches.series.get_mut("c@x").unwrap().tracked.subject =
            "[mystery][PATCH] mystery: change".into();
        // Unrouted by the path check: it has a repository, and stays.
        let a_rec = patches.series.get_mut("a@x").unwrap();
        a_rec.status = Status::Unrouted;
        a_rec.reason = Some("the series changes paths that do not exist".into());
        patches.route(&c.lists[1], &repos);
        let c_rec = &patches.series["c@x"];
        assert_eq!(
            (c_rec.status, c_rec.repo.as_deref(), c_rec.reason.as_deref()),
            (Status::Complete, Some("mystery"), None)
        );
        assert_eq!(patches.series["a@x"].status, Status::Unrouted);

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
