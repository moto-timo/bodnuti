//! A throwaway worktree of the target layer with the series applied.
//!
//! The worktree is detached at the base commit and removed on drop, so the
//! layer checkout bodnuti was pointed at is never modified beyond git's
//! worktree bookkeeping.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use crate::git;
use crate::series::Series;

pub struct Workspace {
    repo: PathBuf,
    dir: TempDir,
    pub base: String,
}

/// The commit `git am` created for one patch of the series.
#[derive(Debug, Serialize, Deserialize)]
pub struct Applied {
    pub index: u32,
    pub commit: String,
}

impl Workspace {
    /// Adds a detached worktree of `repo` at `base`.
    pub fn create(repo: &Path, base: &str) -> Result<Workspace> {
        let base = git::run(
            repo,
            &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
        )
        .with_context(|| format!("base {base} is not a commit in {}", repo.display()))?;
        let dir = tempfile::Builder::new().prefix("bodnuti-").tempdir()?;
        let path = dir.path().join("tree");
        git::run(
            repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--quiet",
                path_str(&path)?,
                &base,
            ],
        )?;
        Ok(Workspace {
            repo: repo.to_path_buf(),
            dir,
            base,
        })
    }

    pub fn path(&self) -> PathBuf {
        self.dir.path().join("tree")
    }

    /// Applies every patch in order. A patch that does not apply stops the
    /// series, because later patches build on it.
    pub fn apply(&self, series: &Series) -> Result<Vec<Applied>> {
        let tree = self.path();
        let mut applied = Vec::new();
        for patch in &series.patches {
            let file = self.dir.path().join(format!("{:04}.patch", patch.index));
            std::fs::write(&file, &patch.raw)?;
            let output = git::command(&tree)
                .envs(git::COMMITTER)
                // Repository hooks are not part of the patch under review.
                .args(["-c", "core.hooksPath=/dev/null", "am", "--3way", "--quiet"])
                .arg(&file)
                .output()?;
            if !output.status.success() {
                let _ = git::run(&tree, &["am", "--abort"]);
                return Err(NotApplied {
                    index: patch.index,
                    total: patch.total,
                    base: self.base[..12].to_string(),
                    reason: git_error(&String::from_utf8_lossy(&output.stderr)),
                }
                .into());
            }
            let commit = git::run(&tree, &["rev-parse", "HEAD"])?;
            applied.push(Applied {
                index: patch.index,
                commit,
            });
        }
        Ok(applied)
    }
}

/// A patch of the series that `git am --3way` could not apply. Not a tool
/// failure: the series targets another base, needs a rebase, or is already
/// applied.
#[derive(Debug)]
pub struct NotApplied {
    pub index: u32,
    pub total: u32,
    pub base: String,
    pub reason: String,
}

impl std::fmt::Display for NotApplied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "patch {}/{} does not apply to {}: {}",
            self.index, self.total, self.base, self.reason
        )
    }
}

impl std::error::Error for NotApplied {}

/// git am's own `error:` line, without the hints it prints after it.
fn git_error(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines
        .iter()
        .find_map(|l| l.strip_prefix("error: "))
        .or(lines.first().copied())
        .unwrap_or("git am failed")
        .to_string()
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let tree = self.path();
        if let Some(tree) = tree.to_str() {
            let _ = git::run(&self.repo, &["worktree", "remove", "--force", tree]);
        }
    }
}

/// Picks the base commit: an explicit `--base`, else the first subject tag
/// that names a branch in the repository (`[scarthgap]`), else the
/// repository's default remote branch, else its HEAD.
pub fn resolve_base(repo: &Path, series: &Series, explicit: Option<&str>) -> String {
    if let Some(base) = explicit {
        return base.to_string();
    }
    for tag in series.branch_candidates() {
        let pattern_heads = format!("refs/heads/{tag}");
        let pattern_remotes = format!("refs/remotes/*/{tag}");
        if let Ok(found) = git::run(
            repo,
            &[
                "for-each-ref",
                "--count=1",
                "--format=%(refname)",
                &pattern_heads,
                &pattern_remotes,
            ],
        ) && !found.is_empty()
        {
            return found;
        }
    }
    git::run(repo, &["rev-parse", "--abbrev-ref", "origin/HEAD"]).unwrap_or_else(|_| "HEAD".into())
}

/// A series checked on a staging branch because it does not apply to the
/// default branch (designs/PHASE2.md §5.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Staged {
    /// The staging branch, as `--staging` named it without its remote
    /// (`master-next`).
    pub branch: String,
    /// The default branch, likewise (`master`).
    pub default: String,
    /// The default branch's commit the series does not apply to.
    pub default_base: String,
    /// Why it does not: the patch, and git am's error.
    pub reason: String,
}

impl Staged {
    /// Why the series was checked on the staging branch, in one line.
    pub fn note(&self) -> String {
        format!(
            "does not apply to {} {}: {}; checked on {}, which carries the patches it builds on",
            self.default,
            self.default_base,
            self.reason.trim_end_matches('.'),
            self.branch
        )
    }
}

/// The base on `staging` to check `series` on, when it targets the
/// repository's default branch at `base` and does not apply there
/// (designs/PHASE2.md §5.6): the staging branch just before the series, if
/// it already carries some of it, else its tip. None when the series
/// applies at `base`, when `base` is not the default branch, when the clone
/// has no `staging`, or when the series does not apply there either.
pub fn staged_base(
    repo: &Path,
    base: &str,
    staging: &str,
    series: &Series,
) -> Result<Option<(String, Staged)>> {
    let full = |r: &str| git::run(repo, &["rev-parse", "--symbolic-full-name", r]).ok();
    let default = full("origin/HEAD");
    if default.is_none() || full(base) != default {
        return Ok(None);
    }
    let Some(failed) = not_applied(repo, base, series)? else {
        return Ok(None);
    };
    let Ok(tip) = git::run(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{staging}^{{commit}}"),
        ],
    ) else {
        return Ok(None);
    };
    let on = match first_carried(repo, base, &tip, series)? {
        Some(commit) => git::run(repo, &["rev-parse", &format!("{commit}^")])?,
        None => tip,
    };
    if not_applied(repo, &on, series)?.is_some() {
        return Ok(None);
    }
    let staged = Staged {
        branch: branch_label(staging),
        default: branch_label(&default.unwrap_or_default()),
        default_base: failed.base.clone(),
        reason: format!("patch {}/{}: {}", failed.index, failed.total, failed.reason),
    };
    Ok(Some((on, staged)))
}

/// Why `series` does not apply at `base`, tried in a throwaway worktree;
/// None when it applies.
fn not_applied(repo: &Path, base: &str, series: &Series) -> Result<Option<NotApplied>> {
    let workspace = Workspace::create(repo, base)?;
    match workspace.apply(series) {
        Ok(_) => Ok(None),
        Err(err) => Ok(Some(err.downcast::<NotApplied>()?)),
    }
}

/// The oldest commit of `base..tip` that is one of the series' patches,
/// by patch-id: a staging branch often carries the series under check.
fn first_carried(repo: &Path, base: &str, tip: &str, series: &Series) -> Result<Option<String>> {
    let wanted: Vec<String> = patch_ids(series)?.into_iter().map(|(id, _)| id).collect();
    let log = git::command(repo)
        .args(["log", "-p", "--no-merges", "--reverse", "--no-color"])
        .arg(format!("{base}..{tip}"))
        .output()
        .context("running git log")?;
    if !log.status.success() {
        bail!("git log {base}..{tip} failed");
    }
    // `git patch-id` prints "<patch-id> <commit>" per commit, in log order.
    Ok(patch_id_lines(&log.stdout)?
        .into_iter()
        .find(|(id, _)| wanted.contains(id))
        .map(|(_, commit)| commit))
}

/// `git patch-id --stable` for each patch, which is the same for a patch
/// whatever its commit message, so a resend with a reworded message matches.
pub fn patch_ids(series: &Series) -> Result<Vec<(String, u32)>> {
    let mut ids = Vec::new();
    for patch in &series.patches {
        let (id, _) = patch_id_lines(&patch.raw)?
            .into_iter()
            .next()
            .context("git patch-id printed nothing")?;
        ids.push((id, patch.index));
    }
    Ok(ids)
}

/// `git patch-id --stable` on `input`: one `(patch-id, commit)` per patch.
fn patch_id_lines(input: &[u8]) -> Result<Vec<(String, String)>> {
    let mut child = git::command(Path::new("."))
        .args(["patch-id", "--stable"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("running git patch-id")?;
    let mut stdin = child.stdin.take().context("git stdin")?;
    // Written from a thread: a long log fills the pipe before git reads it
    // all, and git's output would fill the other.
    let input = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output()?;
    writer
        .join()
        .map_err(|_| anyhow::anyhow!("writing to git patch-id"))??;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| {
            let (id, commit) = l.split_once(' ')?;
            Some((id.to_string(), commit.to_string()))
        })
        .collect())
}

/// A branch without its remote: `origin/master-next`,
/// `refs/remotes/origin/master` and `refs/heads/master` give the branch.
fn branch_label(name: &str) -> String {
    name.strip_prefix("refs/remotes/")
        .and_then(|r| r.split_once('/').map(|(_, b)| b))
        .or_else(|| name.strip_prefix("refs/heads/"))
        .or_else(|| name.strip_prefix("origin/"))
        .unwrap_or(name)
        .to_string()
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str().context("worktree path is not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository with one commit on main and a `scarthgap` branch.
    fn layer() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git::run(p, &["init", "-q", "-b", "main"]).unwrap();
        std::fs::write(p.join("foo.bb"), "SUMMARY = \"foo\"\n").unwrap();
        git::run(p, &["add", "foo.bb"]).unwrap();
        git::run(
            p,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@x",
                "commit",
                "-qm",
                "foo: add",
            ],
        )
        .unwrap();
        git::run(p, &["branch", "scarthgap"]).unwrap();
        dir
    }

    fn format_patch(repo: &Path, contents: &str, subject: &str) -> Vec<u8> {
        std::fs::write(repo.join("foo.bb"), contents).unwrap();
        git::run(
            repo,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@x",
                "commit",
                "-qam",
                subject,
            ],
        )
        .unwrap();
        let mbox = git::run(repo, &["format-patch", "--stdout", "-1"]).unwrap();
        git::run(repo, &["reset", "-q", "--hard", "HEAD~1"]).unwrap();
        (mbox + "\n").into_bytes()
    }

    fn series(mbox: Vec<u8>) -> Series {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), mbox).unwrap();
        Series::load(&[file.path()]).unwrap()
    }

    #[test]
    fn applies_a_series_and_cleans_up() {
        let repo = layer();
        let s = series(format_patch(
            repo.path(),
            "SUMMARY = \"bar\"\n",
            "foo: change summary",
        ));
        let path;
        {
            let ws = Workspace::create(repo.path(), "main").unwrap();
            path = ws.path();
            let applied = ws.apply(&s).unwrap();
            assert_eq!(applied.len(), 1);
            assert_ne!(applied[0].commit, ws.base);
            let text = std::fs::read_to_string(path.join("foo.bb")).unwrap();
            assert_eq!(text, "SUMMARY = \"bar\"\n");
        }
        assert!(!path.exists(), "worktree was not removed");
        // The layer checkout itself is untouched.
        let text = std::fs::read_to_string(repo.path().join("foo.bb")).unwrap();
        assert_eq!(text, "SUMMARY = \"foo\"\n");
    }

    #[test]
    fn reports_a_patch_that_does_not_apply() {
        let repo = layer();
        let s = series(format_patch(
            repo.path(),
            "SUMMARY = \"bar\"\n",
            "foo: change summary",
        ));
        std::fs::write(repo.path().join("foo.bb"), "LICENSE = \"MIT\"\n").unwrap();
        git::run(
            repo.path(),
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@x",
                "commit",
                "-qam",
                "conflict",
            ],
        )
        .unwrap();
        let ws = Workspace::create(repo.path(), "main").unwrap();
        let err = ws.apply(&s).unwrap_err();
        assert!(
            err.to_string().contains("patch 1/1 does not apply"),
            "{err}"
        );
    }

    #[test]
    fn keeps_git_ams_error_line_not_its_hints() {
        let stderr = "warning: squelched 22 whitespace errors\n\
                      error: Failed to merge in the changes.\n\
                      hint: Use 'git am --show-current-patch=diff' to see the failed patch\n";
        assert_eq!(git_error(stderr), "Failed to merge in the changes.");
        assert_eq!(git_error("fatal: bad\n"), "fatal: bad");
    }

    #[test]
    fn resolves_the_base_from_a_branch_tag() {
        let repo = layer();
        let mbox = format_patch(repo.path(), "SUMMARY = \"bar\"\n", "foo: change summary");
        let tagged = String::from_utf8(mbox)
            .unwrap()
            .replace("[PATCH]", "[scarthgap][PATCH]");
        let s = series(tagged.into_bytes());
        assert_eq!(resolve_base(repo.path(), &s, None), "refs/heads/scarthgap");
        assert_eq!(resolve_base(repo.path(), &s, Some("v1.0")), "v1.0");
    }

    fn commit(repo: &Path, contents: &str, subject: &str) -> String {
        std::fs::write(repo.join("foo.bb"), contents).unwrap();
        git::run(
            repo,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@x",
                "commit",
                "-qam",
                subject,
            ],
        )
        .unwrap();
        git::run(repo, &["rev-parse", "HEAD"]).unwrap()
    }

    /// An upstream whose `master-next` stages a patch, `foo: add A`, and a
    /// series built on it, `foo: change A`, which master-next carries too;
    /// `next-early` has only the staged patch. Returns a clone of it, the
    /// series, and the staged patch's commit.
    fn staged_upstream() -> (TempDir, TempDir, Series, String) {
        let up = layer();
        let p = up.path();
        git::run(p, &["checkout", "-q", "-b", "master-next"]).unwrap();
        let staged = commit(p, "SUMMARY = \"foo\"\nA = \"1\"\n", "foo: add A");
        git::run(p, &["branch", "next-early"]).unwrap();
        commit(p, "SUMMARY = \"foo\"\nA = \"2\"\n", "foo: change A");
        let mbox = git::run(p, &["format-patch", "--stdout", "-1"]).unwrap();
        git::run(p, &["checkout", "-q", "main"]).unwrap();
        let clone = tempfile::tempdir().unwrap();
        git::run(clone.path(), &["clone", "-q", p.to_str().unwrap(), "."]).unwrap();
        (up, clone, series((mbox + "\n").into_bytes()), staged)
    }

    #[test]
    fn checks_a_series_on_staging_before_it() {
        let (_up, clone, s, staged) = staged_upstream();
        let repo = clone.path();
        // master-next carries the series itself: the base is just before it.
        let (on, how) = staged_base(repo, "origin/main", "origin/master-next", &s)
            .unwrap()
            .unwrap();
        assert_eq!(on, staged);
        assert_eq!(how.branch, "master-next");
        assert_eq!(how.default, "main");
        assert!(how.reason.starts_with("patch 1/1: "), "{}", how.reason);
        assert!(how.note().starts_with("does not apply to main "));
        // A staging branch without the series: its tip.
        let (on, how) = staged_base(repo, "origin/main", "origin/next-early", &s)
            .unwrap()
            .unwrap();
        assert_eq!(on, staged);
        assert_eq!(how.branch, "next-early");
    }

    #[test]
    fn falls_back_only_when_needed_and_possible() {
        let (_up, clone, s, _) = staged_upstream();
        let repo = clone.path();
        // No such staging branch in the clone.
        assert!(
            staged_base(repo, "origin/main", "origin/nope", &s)
                .unwrap()
                .is_none()
        );
        // Not the default branch: a series for another branch is not moved.
        assert!(
            staged_base(repo, "origin/next-early", "origin/master-next", &s)
                .unwrap()
                .is_none()
        );
        // A series that applies to the default branch stays there.
        let applies = series(format_patch(repo, "SUMMARY = \"bar\"\n", "foo: bar"));
        assert!(
            staged_base(repo, "origin/main", "origin/master-next", &applies)
                .unwrap()
                .is_none()
        );
    }
}
