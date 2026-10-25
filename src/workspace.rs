//! A throwaway worktree of the target layer with the series applied.
//!
//! The worktree is detached at the base commit and removed on drop, so the
//! layer checkout bodnuti was pointed at is never modified beyond git's
//! worktree bookkeeping.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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
}
