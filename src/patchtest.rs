//! OE-Core's patchtest, run on the series' mbox files.
//!
//! Only the mbox and patch suites run here: commit message, shortlog,
//! Signed-off-by, and the headers of added `.patch` files. The metadata
//! suite parses recipes with tinfoil, which executes code from the patch,
//! so it belongs in the confined container and is not run by this module.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::finding::{Finding, Severity, Tool};
use crate::git;
use crate::series::Series;

/// Selects `test_mbox.py` and `test_patch.py` and nothing else.
const PATTERN: &str = "test_[mp][ab]*.py";

pub struct Patchtest {
    pub python: String,
    /// An openembedded-core checkout providing `scripts/patchtest` and its
    /// test suites. Patches to other layers are still checked with them.
    pub oecore: PathBuf,
    /// Whether the series targets openembedded-core itself. patchtest's
    /// target-mailing-list test assumes it does: it fails any patch to
    /// `documentation/` or with a `[meta-*]` subject tag, which is every
    /// patch on the docs and meta-openembedded lists.
    pub oecore_target: bool,
}

/// Tests that only make sense when the target is openembedded-core.
const OECORE_ONLY: &[&str] = &["test_mbox.test_target_mailing_list"];

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Counts {
    pub pass: u32,
    pub fail: u32,
    pub skip: u32,
}

impl Patchtest {
    /// Runs patchtest on every patch against `base`.
    ///
    /// patchtest creates and deletes branches in its `--repodir`, so it gets a
    /// shared clone rather than the layer checkout bodnuti was pointed at.
    pub fn run(&self, repo: &Path, base: &str, series: &Series) -> Result<(Vec<Finding>, Counts)> {
        let script = self.oecore.join("scripts/patchtest");
        if !script.is_file() {
            bail!("{} not found; pass --oecore", script.display());
        }
        let work = tempfile::tempdir()?;
        let clone = work.path().join("repo");
        let clone_str = clone.to_str().context("temporary path is not UTF-8")?;
        let repo_str = repo.to_str().context("repository path is not UTF-8")?;
        git::run(
            work.path(),
            &[
                "clone",
                "--quiet",
                "--shared",
                "--no-checkout",
                repo_str,
                clone_str,
            ],
        )?;
        git::run(&clone, &["checkout", "--quiet", "--detach", base])?;

        let input = work.path().join("patches");
        std::fs::create_dir(&input)?;
        for patch in &series.patches {
            std::fs::write(input.join(format!("{:04}.patch", patch.index)), &patch.raw)?;
        }

        let mut cmd = Command::new(&self.python);
        git::strip_repository_env(&mut cmd);
        cmd.envs(git::COMMITTER);
        let output = cmd
            .arg(&script)
            .arg("--directory")
            .arg(&input)
            .arg("--repodir")
            .arg(&clone)
            .arg("--testdir")
            .arg(self.oecore.join("meta/lib/patchtest/tests"))
            .args(["--base-commit", base, "--pattern", PATTERN, "--log-results"])
            .output()
            .with_context(|| format!("running {} {}", self.python, script.display()))?;
        // patchtest's exit status says nothing useful: it is 2 whenever its
        // pre-merge phase finds no tests, pass or fail. A traceback, or a
        // patch with no results, is what marks a broken run.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Traceback") {
            bail!("patchtest raised an exception: {}", tail(&stderr));
        }

        let mut findings = Vec::new();
        let mut counts = Counts::default();
        for patch in &series.patches {
            let log = input.join(format!("{:04}.patch.testresult", patch.index));
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            let before = (counts.pass, counts.fail, counts.skip);
            for finding in parse(&text, patch.index, &mut counts) {
                if !self.oecore_target && OECORE_ONLY.contains(&finding.check.as_str()) {
                    counts.fail -= 1;
                    counts.skip += 1;
                } else {
                    findings.push(finding);
                }
            }
            if before == (counts.pass, counts.fail, counts.skip) {
                bail!(
                    "patchtest produced no results for patch {}: {}",
                    patch.index,
                    tail(&stderr)
                );
            }
        }
        Ok((findings, counts))
    }
}

/// The most telling part of patchtest's stderr: git's own `fatal:` line when
/// there is one, since patchtest's final traceback only reports that an
/// earlier step failed, else the last lines.
fn tail(text: &str) -> String {
    if let Some(fatal) = text.lines().find(|l| l.contains("fatal: ")) {
        return fatal.trim().trim_end_matches('\'').to_string();
    }
    let lines: Vec<&str> = text.trim().lines().collect();
    lines[lines.len().saturating_sub(5)..].join("\n")
}

/// Parses `--log-results` lines: `RESULT: description[: issue] (test.id)`.
fn parse(text: &str, patch: u32, counts: &mut Counts) -> Vec<Finding> {
    let mut findings = Vec::new();
    for line in text.lines() {
        let Some((result, rest)) = line.split_once(": ") else {
            continue;
        };
        let Some((body, id)) = rest.rsplit_once(" (") else {
            continue;
        };
        let id = id.trim_end_matches(')');
        match result {
            "PASS" => counts.pass += 1,
            "SKIP" => counts.skip += 1,
            "FAIL" => {
                counts.fail += 1;
                // test_mbox.TestMbox.test_x -> test_mbox.test_x; the module
                // matters because both suites have a Signed-off-by test.
                let mut parts = id.split('.');
                let check = match (parts.next(), parts.next_back()) {
                    (Some(module), Some(test)) => format!("{module}.{test}"),
                    _ => id.to_string(),
                };
                let message = body.split_once(": ").map_or(body, |(_, issue)| issue);
                findings.push(Finding {
                    tool: Tool::Patchtest,
                    check,
                    severity: Severity::Error,
                    patch: Some(patch),
                    file: None,
                    line: None,
                    message: message.to_string(),
                });
            }
            _ => {}
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/patchtest.testresult");

    #[test]
    fn parses_real_results() {
        let mut counts = Counts::default();
        let findings = parse(FIXTURE, 3, &mut counts);
        assert_eq!(
            counts,
            Counts {
                pass: 8,
                fail: 5,
                skip: 2
            }
        );
        let checks: Vec<_> = findings.iter().map(|f| f.check.as_str()).collect();
        assert!(checks.contains(&"test_mbox.test_signed_off_by_presence"));
        assert!(checks.contains(&"test_patch.test_signed_off_by_presence"));
        let status = findings
            .iter()
            .find(|f| f.check == "test_patch.test_upstream_status_presence_format")
            .unwrap();
        assert!(
            status
                .message
                .starts_with("Added patch file is missing Upstream-Status")
        );
        assert!(
            findings
                .iter()
                .all(|f| f.patch == Some(3) && f.severity == Severity::Error)
        );
    }

    #[test]
    fn ignores_the_header_line() {
        let mut counts = Counts::default();
        assert!(parse("Patchtest results for patch 'x':\n\n", 1, &mut counts).is_empty());
        assert_eq!(counts, Counts::default());
    }
}
