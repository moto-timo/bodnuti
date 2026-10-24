//! The one place bodnuti starts git.
//!
//! Git honours `GIT_DIR`, `GIT_WORK_TREE` and their relatives before it looks
//! at the working directory. When bodnuti itself runs under git (a hook,
//! `git rebase --exec`), those point at the caller's repository, so every
//! command here strips them and names its directory explicitly.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

const REPOSITORY_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
];

/// A git command that runs in `dir`, with repository overrides removed.
pub fn command(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir);
    strip_repository_env(&mut cmd);
    cmd
}

/// Removes the repository overrides from a command that runs git itself,
/// such as patchtest.
pub fn strip_repository_env(cmd: &mut Command) {
    for var in REPOSITORY_ENV {
        cmd.env_remove(var);
    }
}

/// Runs git in `dir` and returns trimmed stdout, or an error carrying stderr.
pub fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let output = command(dir)
        .args(args)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_every_repository_override() {
        let cmd = command(Path::new("/tmp"));
        let removed: Vec<_> = cmd
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(key, _)| key.to_string_lossy().into_owned())
            .collect();
        for var in REPOSITORY_ENV {
            assert!(removed.iter().any(|r| r == var), "{var} is not removed");
        }
    }

    #[test]
    fn run_reports_stderr_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(dir.path(), &["rev-parse", "HEAD"]).unwrap_err();
        assert!(
            err.to_string().contains("git rev-parse HEAD failed"),
            "{err}"
        );
    }
}
