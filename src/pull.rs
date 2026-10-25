//! Getting the commits of a `git request-pull` pull request as a series
//! (designs/PHASE2.md §5.0).
//!
//! The repository, branch and commits come from mail; `request_pull`
//! validated them. The fetch runs on the host, over https only, into a ref
//! of bodnuti's own, which is deleted once the patches are written out.

use std::path::Path;

use anyhow::{Context, Result, bail};

use bodnuti::request_pull::PullRequest;

use crate::git;

/// A pull request larger than this is not checked: it would hold the
/// worker for hours.
pub const MAX_COMMITS: usize = 300;

/// Fetches the pull request's branch into `refs/bodnuti/pull/<root>` in
/// `repo` and checks that it still has the head commit.
pub fn fetch(repo: &Path, pr: &PullRequest, root: &str) -> Result<String> {
    let local = format!("refs/bodnuti/pull/{root}");
    let mut cmd = git::command(repo);
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .args([
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.https.allow=always",
        ])
        // Give up on a stalled transfer rather than hold the poll.
        .args([
            "-c",
            "http.lowSpeedLimit=1000",
            "-c",
            "http.lowSpeedTime=60",
        ])
        .args(["fetch", "--quiet", "--no-tags", "--", &pr.url])
        .arg(format!("+refs/heads/{}:{local}", pr.branch));
    let output = cmd.output().context("running git fetch")?;
    if !output.status.success() {
        bail!(
            "fetching {} {}: {}",
            pr.url,
            pr.branch,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let on_branch = git::command(repo)
        .args(["merge-base", "--is-ancestor", &pr.head, &local])
        .status()
        .context("running git merge-base")?;
    if !on_branch.success() {
        delete(repo, &local);
        bail!(
            "the pull request's head {} is no longer on {} {}",
            &pr.head[..12],
            pr.url,
            pr.branch
        );
    }
    Ok(local)
}

/// The pull request's commits as mbox data, `git format-patch base..head`,
/// or None when there are more than `MAX_COMMITS`.
pub fn mbox(repo: &Path, pr: &PullRequest) -> Result<Option<Vec<u8>>> {
    let range = format!("{}..{}", pr.base, pr.head);
    let count: usize = git::run(repo, &["rev-list", "--count", "--no-merges", &range])?
        .trim()
        .parse()
        .context("git rev-list --count")?;
    if count > MAX_COMMITS {
        return Ok(None);
    }
    let output = git::command(repo)
        .args(["format-patch", "--stdout", "--no-signature", &range])
        .output()
        .context("running git format-patch")?;
    if !output.status.success() {
        bail!(
            "git format-patch {range}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(Some(output.stdout))
}

/// The upstream branch the pull request is based on: the one that contains
/// its base commit, master when several do (the base predates a stable
/// branch), master when none does.
pub fn branch(repo: &Path, pr: &PullRequest) -> String {
    let containing = git::run(
        repo,
        &[
            "branch",
            "-r",
            "--contains",
            &pr.base,
            "--format=%(refname:short)",
        ],
    )
    .unwrap_or_default();
    let branches: Vec<&str> = containing
        .lines()
        .filter_map(|b| b.trim().strip_prefix("origin/"))
        .filter(|b| *b != "HEAD")
        .collect();
    match branches.as_slice() {
        [one] => one.to_string(),
        _ => "master".into(),
    }
}

pub fn delete(repo: &Path, local: &str) {
    let _ = git::run(repo, &["update-ref", "-d", local]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(repo: &Path, file: &str, subject: &str) -> String {
        std::fs::write(repo.join(file), subject).unwrap();
        git::run(repo, &["add", file]).unwrap();
        git::run(
            repo,
            &[
                "-c",
                "user.name=A",
                "-c",
                "user.email=a@x",
                "commit",
                "-qm",
                subject,
            ],
        )
        .unwrap();
        git::run(repo, &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .into()
    }

    #[test]
    fn writes_the_pull_requests_commits_and_finds_its_branch() {
        let dir = tempfile::tempdir().unwrap();
        let up = dir.path().join("up");
        std::fs::create_dir(&up).unwrap();
        git::run(&up, &["init", "-q", "-b", "master"]).unwrap();
        commit(&up, "a", "a: add");
        git::run(&up, &["checkout", "-q", "-b", "wrynose"]).unwrap();
        let base = commit(&up, "b", "b: add on wrynose");
        let clone = dir.path().join("clone");
        git::run(
            dir.path(),
            &["clone", "-q", up.to_str().unwrap(), clone.to_str().unwrap()],
        )
        .unwrap();
        // The contributor's commits on top of wrynose.
        commit(&clone, "c", "c: one");
        let head = commit(&clone, "d", "d: two");
        let pr = PullRequest {
            base,
            head,
            url: "unused".into(),
            branch: "unused".into(),
        };
        let mbox = String::from_utf8(mbox(&clone, &pr).unwrap().unwrap()).unwrap();
        assert!(mbox.contains("Subject: [PATCH 1/2] c: one"), "{mbox}");
        assert!(mbox.contains("Subject: [PATCH 2/2] d: two"), "{mbox}");
        assert_eq!(branch(&clone, &pr), "wrynose");
    }
}
