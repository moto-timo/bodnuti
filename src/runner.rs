//! Fetching and checking one series (designs/PHASE2.md section 5).
//!
//! Both steps run in the bodnuti image, through the configured container
//! command. The fetch has network access and sees only the series directory;
//! the check has no network. Only computing patch-ids and reading git trees
//! for the path check run on the host, and neither executes anything from
//! the patch.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::applied;
use crate::git;
use crate::kind::Kind;
use crate::report::Report;
use crate::series::{NoPatches, Series};
use crate::watch::{CheckResult, Config, Route, SeriesRecord, Status};
use crate::workspace::resolve_base;

/// The clone patchtest always comes from.
const OECORE: &str = "openembedded-core";

/// trivy's vulnerability database, under the cache directory.
const TRIVY_DB: &str = "trivy";

/// How old the trivy database may get before it is downloaded again.
const TRIVY_DB_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

pub struct Runner<'a> {
    pub config: &'a Config,
}

impl Runner<'_> {
    fn container(&self) -> Result<Command> {
        let (program, args) = self
            .config
            .runner
            .split_first()
            .context("empty runner command")?;
        let mut cmd = Command::new(program);
        cmd.args(args);
        Ok(cmd)
    }

    /// Downloads trivy's vulnerability database into `cache/trivy/` when it
    /// is older than a day, in the image with network. Checks of Django
    /// applications mount it read-only (designs/PHASE2.md §5.3). Returns
    /// whether it downloaded.
    pub fn refresh_trivy_db(&self) -> Result<bool> {
        let dir = self.config.cache.join(TRIVY_DB);
        // A stamp of bodnuti's own: trivy leaves its files untouched when it
        // finds its database still current.
        let stamp = dir.join(".refreshed");
        let fresh = std::fs::metadata(&stamp)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < TRIVY_DB_MAX_AGE);
        if fresh {
            return Ok(false);
        }
        std::fs::create_dir_all(&dir)?;
        let output = self
            .container()?
            .arg("-v")
            .arg(format!("{}:/trivy", dir.display()))
            .args(["--entrypoint", "trivy", &self.config.image])
            .args([
                "fs",
                "--quiet",
                "--cache-dir",
                "/trivy",
                "--download-db-only",
            ])
            .output()
            .context("starting the container to download the trivy database")?;
        if !output.status.success() {
            bail!(
                "trivy fs --download-db-only failed: {}",
                last_line(&output.stderr)
            );
        }
        std::fs::write(&stamp, b"")?;
        Ok(true)
    }

    fn series_dir(&self, list: &str, root: &str) -> PathBuf {
        self.config.state.join("series").join(list).join(root)
    }

    /// Moves a routed, complete series on to `Checked`, `Duplicate` or
    /// `Unrouted`. An error leaves it `Complete` for the caller to retry.
    /// `members` are the other Message-IDs seen in the series, to fetch its
    /// thread through when lore does not have the root.
    pub fn process(
        &self,
        list: &str,
        route: Route,
        record: &mut SeriesRecord,
        members: &[String],
    ) -> Result<()> {
        let root = record.tracked.root.clone();
        let repo_name = record.repo.clone().context("series has no repository")?;
        let repo = self.config.cache.join(&repo_name);
        let dir = self.series_dir(list, &root);
        std::fs::create_dir_all(&dir)?;

        let mbox = self.fetch(&dir, &root, members)?;
        let series = match Series::load(&[&mbox]) {
            Err(err) if err.downcast_ref::<NoPatches>().is_some() => {
                record.status = Status::NoPatches;
                record.reason = Some(
                    "the thread has no patches: a pull request, or a cover letter whose \
                     patches were sent outside its thread"
                        .into(),
                );
                return Ok(());
            }
            loaded => loaded?,
        };
        let ids = patch_ids(&series)?;
        let base = resolve_base(&repo, &series, None);
        let branch = branch_name(&base);
        record.branch = Some(branch.clone());
        if let Some(earlier) = self.all_known(&ids, list, &root, &branch)? {
            record.status = Status::Duplicate;
            record.reason = Some(format!(
                "every patch was already checked for {branch} in {earlier}"
            ));
            return Ok(());
        }
        if route == Route::SubjectTag {
            let missing = missing_paths(&repo, &base, &series)?;
            if !missing.is_empty() {
                // A series merged before it was checked fails the path check
                // too: the merge renamed or removed its files.
                let last_seen = crate::lore::parse_time(&record.tracked.last_seen).unwrap_or(0);
                if let Some(landed) =
                    applied::already_applied(&repo, &base, &branch, &series, &ids, last_seen)?
                {
                    record.applied = landed;
                    record.status = Status::Applied;
                    record.reason =
                        Some(format!("already applied to {branch} when it was checked"));
                    return Ok(());
                }
                record.status = Status::Unrouted;
                record.reason = Some(format!(
                    "the series changes paths that do not exist in {repo_name}: {}",
                    missing.join(", ")
                ));
                return Ok(());
            }
        }

        let report = self.check(&dir, &repo_name, &root)?;
        std::fs::write(dir.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
        std::fs::write(dir.join("report.txt"), report.text())?;
        record.result = Some(CheckResult::of(&report));
        record.status = Status::Checked;
        record.reason = None;
        self.remember(&ids, list, &root, &branch)?;
        Ok(())
    }

    /// `b4 mbox` in the image, saving the thread as `<root>.mbx` in the
    /// series directory. When lore does not have the root (a parent on
    /// another list, say), the thread is fetched through the series' other
    /// messages in turn (designs/PHASE2.md §5.0).
    fn fetch(&self, dir: &Path, root: &str, members: &[String]) -> Result<PathBuf> {
        let name = format!("{root}.mbx");
        let mbox = dir.join(&name);
        let mut error = String::new();
        for id in std::iter::once(root).chain(members.iter().map(String::as_str)) {
            // A file left by an earlier attempt must not pass for this one.
            if mbox.exists() {
                std::fs::remove_file(&mbox)?;
            }
            // The name is given, because b4 would otherwise derive it from
            // the Message-ID and rewrite some characters, such as `=`.
            let output = self
                .container()?
                .arg("-v")
                .arg(format!("{}:/out", dir.display()))
                .args(["--entrypoint", "b4", &self.config.image])
                .args(["mbox", "-o", "/out", "-n", &name, id])
                .output()
                .context("starting the container to fetch the series")?;
            if output.status.success() && mbox.is_file() {
                return Ok(mbox);
            }
            if error.is_empty() {
                error = last_line(&output.stderr);
            }
        }
        bail!("b4 mbox failed: {error}")
    }

    /// `bodnuti check --json` in the image, without network.
    fn check(&self, dir: &Path, repo: &str, root: &str) -> Result<Report> {
        let mut cmd = self.container()?;
        cmd.args(["--network", "none", "-v"])
            .arg(format!("{}:/repo", self.config.cache.join(repo).display()))
            .arg("-v")
            .arg(format!("{}:/in:ro", dir.display()));
        if repo != OECORE {
            cmd.arg("-v")
                .arg(format!(
                    "{}:/oecore:ro",
                    self.config.cache.join(OECORE).display()
                ))
                .args(["-e", "BODNUTI_OECORE=/oecore"]);
        }
        let kind = self.config.repos.get(repo).map_or(Kind::Layer, |r| r.kind);
        let trivy = self.config.cache.join(TRIVY_DB);
        if kind == Kind::DjangoApp && trivy.join("db/trivy.db").is_file() {
            cmd.arg("-v").arg(format!("{}:/trivy:ro", trivy.display()));
        }
        let output = cmd
            .args([&self.config.image, "check", "--json", "--repo", "/repo"])
            .args(["--kind", kind.as_str()])
            .arg(format!("/in/{root}.mbx"))
            .output()
            .context("starting the container to check the series")?;
        // Exit 1 means findings and 2 means a checker could not run; both
        // still produce a report. No report at all is a failed run.
        serde_json::from_slice(&output.stdout)
            .with_context(|| format!("check produced no report: {}", last_line(&output.stderr)))
    }

    /// The series every patch-id was first checked in for `branch`, if all
    /// are known from another series. The same patch sent to two branches
    /// (a backport) is checked for each.
    fn all_known(
        &self,
        ids: &[(String, u32)],
        list: &str,
        root: &str,
        branch: &str,
    ) -> Result<Option<String>> {
        let this = format!("{list}/{root}");
        let mut first = None;
        for (id, _) in ids {
            let owners = read_owners(&self.config.state, id)?;
            match owners.iter().find(|o| o.branch.as_deref() == Some(branch)) {
                Some(o) if o.series != this => first = first.or(Some(o.series.clone())),
                _ => return Ok(None),
            }
        }
        Ok(first)
    }

    /// Records the series as the first one checked with each patch-id for
    /// `branch`, with the patch's position in it.
    fn remember(&self, ids: &[(String, u32)], list: &str, root: &str, branch: &str) -> Result<()> {
        let dir = self.config.state.join("patch-ids");
        std::fs::create_dir_all(&dir)?;
        for (id, index) in ids {
            if read_owners(&self.config.state, id)?
                .iter()
                .any(|o| o.branch.as_deref() == Some(branch))
            {
                continue;
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join(id))?;
            writeln!(file, "{list}/{root} {branch} {index}")?;
        }
        Ok(())
    }
}

/// One series a patch-id was checked in.
#[derive(Debug, PartialEq)]
pub struct Owner {
    /// `<list>/<root>`.
    pub series: String,
    /// The branch it was checked for; unknown in records from before
    /// branches were kept.
    pub branch: Option<String>,
    pub index: Option<u32>,
}

/// Every series a patch-id was checked in: one line each, as
/// `<list>/<root> <branch> <index>`.
pub fn read_owners(state: &Path, id: &str) -> Result<Vec<Owner>> {
    let text = match std::fs::read_to_string(state.join("patch-ids").join(id)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            Some(Owner {
                series: words.next()?.to_string(),
                branch: words.next().map(String::from),
                index: words.next().and_then(|i| i.parse().ok()),
            })
        })
        .collect())
}

/// The branch a base ref names: `refs/remotes/origin/scarthgap` gives
/// `scarthgap`, `origin/master` gives `master`.
fn branch_name(base: &str) -> String {
    base.rsplit('/').next().unwrap_or(base).to_string()
}

/// `git patch-id --stable` for each patch, which is the same for a patch
/// whatever its commit message, so a resend with a reworded message matches.
fn patch_ids(series: &Series) -> Result<Vec<(String, u32)>> {
    let mut ids = Vec::new();
    for patch in &series.patches {
        let mut child = git::command(Path::new("."))
            .args(["patch-id", "--stable"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("running git patch-id")?;
        child
            .stdin
            .take()
            .context("git stdin")?
            .write_all(&patch.raw)?;
        let output = child.wait_with_output()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let id = text
            .split_whitespace()
            .next()
            .context("git patch-id printed nothing")?;
        ids.push((id.to_string(), patch.index));
    }
    Ok(ids)
}

/// Paths the series modifies or deletes that do not exist at its base in
/// `repo`. Added files are skipped: they exist in no tree yet.
fn missing_paths(repo: &Path, base: &str, series: &Series) -> Result<Vec<String>> {
    let mut missing = Vec::new();
    for patch in &series.patches {
        let added = patch.added_files();
        for (old, new) in patch.changes() {
            if added.contains(&new) || missing.contains(&old) {
                continue;
            }
            // A missing path is the answer sought, not an error to log.
            let found = git::command(repo)
                .args(["cat-file", "-e"])
                .arg(format!("{base}:{old}"))
                .stderr(std::process::Stdio::null())
                .status()
                .context("running git cat-file")?;
            if !found.success() {
                missing.push(old);
            }
        }
    }
    Ok(missing)
}

/// The most telling line of a failed command's stderr: the first one that
/// mentions an error, since container runtimes follow it with usage hints,
/// else the last line.
fn last_line(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines
        .iter()
        .find(|l| l.contains("rror"))
        .or(lines.last())
        .map_or_else(String::new, |l| l.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lore::Entry;
    use crate::watch::ListState;

    /// Stands in for `podman run`: answers `b4 mbox` by writing the test
    /// mbox, and `check` by printing a report with one finding.
    const STUB: &str = r#"#!/bin/sh
out=""; root=""; check=0; kind=""; prev=""; trivy=""; db=0; name=""
for a in "$@"; do
  case "$a" in
    *:/out) out="${a%:/out}" ;;
    *:/trivy) trivy="${a%:/trivy}" ;;
    --download-db-only) db=1 ;;
    check) check=1 ;;
  esac
  [ "$prev" = --kind ] && kind="$a"
  [ "$prev" = -n ] && name="$a"
  prev="$a"
  root="$a"
done
if [ "$db" = 1 ]; then mkdir -p "$trivy/db" && : > "$trivy/db/trivy.db"; exit 0; fi
case "$root" in gone@*) echo "Server returned an error: 404" >&2; exit 1 ;; esac
if [ "$check" = 1 ]; then
  echo "$kind" > "$BODNUTI_TEST_REPORT.kind"
  cat "$BODNUTI_TEST_REPORT"; exit 1
fi
cp "$BODNUTI_TEST_MBOX" "$out/$name"
"#;

    const MBOX: &str = "From x Mon Sep 17 00:00:00 2001\nSubject: [PATCH] foo: fix\nMessage-Id: <a@x>\n\n---\n\
                        diff --git a/meta/foo.bb b/meta/foo.bb\n--- a/meta/foo.bb\n+++ b/meta/foo.bb\n@@ -1 +1 @@\n-A = \"1\"\n+A = \"2\"\n";

    const REPORT: &str = r#"{"series":{"cover":null,"patches":[{"subject":"[PATCH] foo: fix","message_id":"a@x","tags":["PATCH"],"version":1,"index":1,"total":1}]},
      "base":"0123456789abcdef","applied":[],"tool_errors":[],"patchtest":{"pass":10,"fail":0,"skip":5},
      "findings":[{"tool":"oelint","check":"oelint.vars.inconspaces","severity":"error","patch":1,"file":"meta/foo.bb","line":1,"message":"Assignment should be ..."}]}"#;

    fn setup() -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("stub.sh");
        std::fs::write(&stub, STUB).unwrap();
        let mbox = dir.path().join("in.mbx");
        std::fs::write(&mbox, MBOX).unwrap();
        let report = dir.path().join("report.json");
        std::fs::write(&report, REPORT).unwrap();
        let config: Config = toml::from_str(&format!(
            "image = \"bodnuti\"\nstate = \"{0}/state\"\ncache = \"{0}/cache\"\n\
             runner = [\"env\", \"BODNUTI_TEST_MBOX={2}\", \"BODNUTI_TEST_REPORT={3}\", \"sh\", \"{1}\"]\n\
             [[list]]\nname = \"openembedded-core\"\nrepo = \"openembedded-core\"\n\
             [repos]\nopenembedded-core = \"https://git.openembedded.org/openembedded-core\"\n",
            dir.path().display(),
            stub.display(),
            mbox.display(),
            report.display()
        ))
        .unwrap();
        (dir, config)
    }

    fn routed(root: &str) -> SeriesRecord {
        let mut state = ListState::default();
        let entry = Entry {
            message_id: root.into(),
            subject: "[PATCH] foo: fix".into(),
            in_reply_to: None,
            updated: "2026-10-24T10:00:00Z".into(),
        };
        state.ingest(
            &[entry],
            crate::lore::parse_time("2026-10-24T10:00:00Z").unwrap(),
            900,
        );
        let mut record = state.series.remove(root).unwrap();
        record.repo = Some("openembedded-core".into());
        record
    }

    #[test]
    fn reports_the_error_line_not_the_usage_hint() {
        let stderr = b"Unable to find image 'x:latest' locally\n\
            docker: Error response from daemon: pull access denied for x\n\nRun 'docker run --help' for more information\n";
        assert_eq!(
            last_line(stderr),
            "docker: Error response from daemon: pull access denied for x"
        );
        assert_eq!(last_line(b"one\ntwo\n"), "two");
    }

    #[test]
    fn fetches_through_another_message_when_lore_lacks_the_root() {
        let (dir, config) = setup();
        let runner = Runner { config: &config };
        let mut record = routed("gone@x");
        let err = runner
            .process("openembedded-core", Route::Fixed, &mut record, &[])
            .unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");

        runner
            .process(
                "openembedded-core",
                Route::Fixed,
                &mut record,
                &["a@x".to_string()],
            )
            .unwrap();
        assert_eq!(record.status, Status::Checked);
        let dir = dir.path().join("state/series/openembedded-core/gone@x");
        assert!(dir.join("gone@x.mbx").is_file());
        assert!(!dir.join("a@x.mbx").exists());
    }

    #[test]
    fn a_thread_without_patches_is_not_a_failure() {
        let (dir, config) = setup();
        std::fs::write(
            dir.path().join("in.mbx"),
            "From x Mon Sep 17 00:00:00 2001\nSubject: [PATCH 00/99] Wrynose pull request\n\
             Message-Id: <a@x>\n\nPlease pull.\n",
        )
        .unwrap();
        let runner = Runner { config: &config };
        let mut record = routed("a@x");
        runner
            .process("openembedded-core", Route::Fixed, &mut record, &[])
            .unwrap();
        assert_eq!(record.status, Status::NoPatches);
        assert!(record.reason.as_deref().unwrap().contains("pull request"));
    }

    #[test]
    fn downloads_the_trivy_database_once_a_day() {
        let (dir, config) = setup();
        let runner = Runner { config: &config };
        assert!(runner.refresh_trivy_db().unwrap());
        assert!(dir.path().join("cache/trivy/db/trivy.db").is_file());
        // Fresh: not downloaded again.
        assert!(!runner.refresh_trivy_db().unwrap());
    }

    #[test]
    fn checks_a_series_then_links_its_resend() {
        let (dir, config) = setup();
        let runner = Runner { config: &config };

        let mut first = routed("a@x");
        runner
            .process("openembedded-core", Route::Fixed, &mut first, &[])
            .unwrap();
        assert_eq!(first.status, Status::Checked);
        let result = first.result.as_ref().unwrap();
        assert_eq!(
            (result.findings, result.errors, result.tool_errors),
            (1, 1, 0)
        );
        // The repository's kind reaches the check.
        let kind = std::fs::read_to_string(dir.path().join("report.json.kind")).unwrap();
        assert_eq!(kind.trim(), "layer");
        let series_dir = dir.path().join("state/series/openembedded-core/a@x");
        assert!(series_dir.join("report.json").is_file());
        assert!(
            std::fs::read_to_string(series_dir.join("report.txt"))
                .unwrap()
                .contains("inconspaces")
        );

        // A v2 with the same diff is the same patch.
        let mut resend = routed("b@x");
        runner
            .process("openembedded-core", Route::Fixed, &mut resend, &[])
            .unwrap();
        assert_eq!(resend.status, Status::Duplicate);
        assert!(
            resend
                .reason
                .as_deref()
                .unwrap()
                .contains("openembedded-core/a@x")
        );
    }

    #[test]
    fn a_backport_of_the_same_patch_is_checked_for_its_branch() {
        let (dir, mut config) = setup();
        let repo = dir.path().join("cache/openembedded-core");
        std::fs::create_dir_all(&repo).unwrap();
        git::run(&repo, &["init", "-q", "-b", "master"]).unwrap();
        std::fs::create_dir_all(repo.join("meta")).unwrap();
        std::fs::write(repo.join("meta/foo.bb"), "A = \"1\"\n").unwrap();
        git::run(&repo, &["add", "."]).unwrap();
        git::run(
            &repo,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@x",
                "commit",
                "-qm",
                "init",
            ],
        )
        .unwrap();
        git::run(&repo, &["branch", "scarthgap"]).unwrap();
        let runner = Runner { config: &config };
        let mut master = routed("m@x");
        runner
            .process("openembedded-core", Route::Fixed, &mut master, &[])
            .unwrap();
        assert_eq!(master.status, Status::Checked);

        // The same diff, sent for scarthgap.
        let backport = dir.path().join("backport.mbx");
        std::fs::write(
            &backport,
            MBOX.replace("[PATCH] foo", "[scarthgap][PATCH] foo"),
        )
        .unwrap();
        for arg in &mut config.runner {
            if arg.starts_with("BODNUTI_TEST_MBOX=") {
                *arg = format!("BODNUTI_TEST_MBOX={}", backport.display());
            }
        }
        let runner = Runner { config: &config };
        let mut stable = routed("s@x");
        runner
            .process("openembedded-core", Route::Fixed, &mut stable, &[])
            .unwrap();
        assert_eq!(stable.status, Status::Checked, "{:?}", stable.reason);
        assert_eq!(stable.branch.as_deref(), Some("scarthgap"));
        let owners = read_owners(
            &config.state,
            &patch_ids(&Series::load(&[&backport]).unwrap()).unwrap()[0].0,
        )
        .unwrap();
        assert_eq!(owners.len(), 2);
        assert_eq!(owners[1].branch.as_deref(), Some("scarthgap"));
        assert_eq!(owners[1].index, Some(1));
    }

    #[test]
    fn a_routed_series_whose_paths_are_not_in_the_repo_is_unrouted() {
        let (dir, config) = setup();
        let repo = dir.path().join("cache/openembedded-core");
        std::fs::create_dir_all(&repo).unwrap();
        git::run(&repo, &["init", "-q", "-b", "master"]).unwrap();
        std::fs::write(repo.join("README"), "x").unwrap();
        git::run(&repo, &["add", "README"]).unwrap();
        git::run(
            &repo,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@x",
                "commit",
                "-qm",
                "init",
            ],
        )
        .unwrap();

        let runner = Runner { config: &config };
        let mut record = routed("c@x");
        runner
            .process("yocto-patches", Route::SubjectTag, &mut record, &[])
            .unwrap();
        assert_eq!(record.status, Status::Unrouted);
        assert!(record.reason.as_deref().unwrap().contains("meta/foo.bb"));
    }
}
