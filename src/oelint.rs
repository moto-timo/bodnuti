//! oelint-adv, run on the metadata files a series touches.
//!
//! oelint-adv also reports problems in classes and includes a recipe pulls in,
//! so its raw output says little about the patch. Running it on the base tree
//! and on the patched tree, and keeping only what is new in touched files,
//! is what makes its output attributable to the author.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::finding::{Checker, Finding, Severity, Tool};

pub struct Oelint {
    pub program: String,
    /// An oelint-adv `--release` value: a codename such as `scarthgap`, or
    /// `latest` for master.
    pub release: String,
}

/// oelint-adv checks whose result is not about the series. Each matches a
/// check id or a prefix of one.
/// - `homepageping` fetches HOMEPAGE over the network, so its result varies
///   between runs and always fails without network access;
/// - `bbclass.underscores` fires on the stub recipe that lints a `.inc`
///   file, not on anything in the patch.
const SUPPRESSED: &[&str] = &["oelint.vars.homepageping", "oelint.bbclass.underscores"];

/// oelint-adv checks about layout rather than correctness: their findings
/// are kept apart as style, shown on request and not counted
/// (designs/PHASE2.md §6.1.1). Variable ordering, FILES and SECTION
/// overrides, multi-line indentation and out-of-context variables are
/// oelint-adv style that OE-Core does not follow (25 merged OE-Core
/// commits, October 2026); the rest made up about 470 of the 919 findings
/// on the first 397 published series.
const STYLE: &[&str] = &[
    "oelint.var.order",
    "oelint.var.filesoverride",
    "oelint.var.override",
    "oelint.vars.multilineident",
    "oelint.vars.outofcontext",
    "oelint.vars.notneededspace",
    "oelint.var.suggestedvar",
    "oelint.vars.dependsordered",
    "oelint.tabs.notabs",
    "oelint.var.bbclassextend",
    "oelint.task.docstrings",
    "oelint.newline.consecutive",
];

/// Whether `check` is one of `ids` or below one of them.
fn matches(ids: &[&str], check: &str) -> bool {
    ids.iter().any(|s| {
        check == *s
            || check
                .strip_prefix(s)
                .is_some_and(|rest| rest.starts_with('.'))
    })
}

fn suppressed(check: &str) -> bool {
    matches(SUPPRESSED, check)
}

/// Whether an oelint-adv check is a style check.
pub fn is_style(check: &str) -> bool {
    matches(STYLE, check)
}

/// Whether oelint-adv understands the file.
pub fn lintable(path: &str) -> bool {
    [".bb", ".bbappend", ".inc", ".bbclass", ".conf"]
        .iter()
        .any(|ext| path.ends_with(ext))
}

/// oelint-adv checks a `.inc` or `.conf` file only as part of a recipe that
/// pulls it in; on its own such a file produces no findings at all.
fn needs_stub(path: &str) -> bool {
    path.ends_with(".inc") || path.ends_with(".conf")
}

impl Checker for Oelint {
    fn tool(&self) -> Tool {
        Tool::Oelint
    }

    fn lint(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        self.lint_files(tree, files)
    }
}

impl Oelint {
    /// Lints the given repository-relative files that exist in `tree`.
    ///
    /// Each `.inc` and `.conf` file is linted through a stub recipe that
    /// `require`s it. The stub lives outside the tree, so its own findings
    /// (missing LICENSE and the like) are reported under its path and are
    /// dropped by the caller's touched-files filter.
    fn lint_files(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        // The stub's require path and oelint-adv's --relpaths must agree on
        // one spelling of the tree. On macOS /var is a symlink to
        // /private/var, and a mismatch turns repository paths into
        // ../../../var/... which then match no touched file.
        let tree = &tree.canonicalize()?;
        let present: Vec<&String> = files
            .iter()
            .filter(|f| lintable(f) && tree.join(f).is_file())
            .collect();
        if present.is_empty() {
            return Ok(Vec::new());
        }
        let stubs = tempfile::tempdir()?;
        let mut args = Vec::new();
        for (n, file) in present.iter().enumerate() {
            if needs_stub(file) {
                let stub = stubs.path().join(format!("bodnuti-stub{n}_1.0.bb"));
                std::fs::write(&stub, format!("require {}\n", tree.join(file).display()))?;
                args.push(stub.into_os_string());
            }
            args.push(file.into());
        }
        let out = tempfile::NamedTempFile::new()?;
        let status = Command::new(&self.program)
            .current_dir(tree)
            .args(["--quiet", "--relpaths", "--release", &self.release])
            .args(["--outputformat", "gitlab-codequality", "--output"])
            .arg(out.path())
            .args(args)
            .status()
            .with_context(|| format!("running {}", self.program))?;
        // 0 is clean and 1 means findings; anything else is a usage error or
        // a crash, which must not read as a clean result.
        if !matches!(status.code(), Some(0 | 1)) {
            bail!("{} exited with {status}", self.program);
        }
        let json = std::fs::read_to_string(out.path())?;
        if json.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(parse(&json)?
            .into_iter()
            .filter(|f| !suppressed(&f.check))
            .collect())
    }
}

#[derive(Deserialize)]
struct CodeQuality {
    description: String,
    check_name: String,
    location: Location,
}

#[derive(Deserialize)]
struct Location {
    path: String,
    lines: Lines,
}

#[derive(Deserialize)]
struct Lines {
    begin: u32,
}

/// Parses gitlab-codequality output. The codequality severity scale is coarser
/// than oelint-adv's own, so severity and message are read from the
/// description, which is `path:line:severity:check:message [branch:...]`.
fn parse(json: &str) -> Result<Vec<Finding>> {
    let entries: Vec<CodeQuality> =
        serde_json::from_str(json).context("oelint-adv output is not codequality JSON")?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let prefix = format!("{}:{}:", e.location.path, e.location.lines.begin);
            let tail = e
                .description
                .strip_prefix(&prefix)
                .unwrap_or(&e.description);
            let (severity, rest) = tail.split_once(':').unwrap_or(("warning", tail));
            let message = rest
                .strip_prefix(&format!("{}:", e.check_name))
                .unwrap_or(rest);
            let message = message.rsplit_once(" [branch:").map_or(message, |(m, _)| m);
            Finding {
                tool: Tool::Oelint,
                check: e.check_name,
                severity: match severity {
                    "error" => Severity::Error,
                    "info" => Severity::Info,
                    _ => Severity::Warning,
                },
                patch: None,
                file: Some(e.location.path),
                line: Some(e.location.lines.begin),
                message: message.trim().to_string(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/oelint-codequality.json");

    #[test]
    fn parses_real_output() {
        let findings = parse(FIXTURE).unwrap();
        assert_eq!(findings.len(), 6);
        let append = findings
            .iter()
            .find(|f| f.check == "oelint.vars.inconspaces")
            .unwrap();
        assert_eq!(append.severity, Severity::Error);
        assert_eq!(
            append.file.as_deref(),
            Some("meta/classes-recipe/ptest.bbclass")
        );
        assert_eq!(append.line, Some(94));
        assert!(
            append.message.starts_with("Assignment should be"),
            "{}",
            append.message
        );
        assert!(!append.message.contains("[branch:"));
        assert!(findings.iter().any(|f| f.severity == Severity::Info));
        assert!(findings.iter().any(|f| f.severity == Severity::Warning));
    }

    #[test]
    fn suppression_and_style_match_whole_id_segments() {
        assert!(suppressed("oelint.vars.homepageping"));
        assert!(!suppressed("oelint.var.order.LICENSE"));
        assert!(is_style("oelint.var.order.LICENSE"));
        assert!(is_style("oelint.var.suggestedvar.BUGTRACKER"));
        assert!(!is_style("oelint.var.orderly"));
        assert!(!is_style("oelint.vars.inconspaces"));
        assert!(!is_style("oelint.vars.spacesassignment"));
    }

    #[test]
    fn only_metadata_is_lintable() {
        assert!(lintable("meta/recipes-x/foo/foo_1.0.bb"));
        assert!(lintable("meta/classes-recipe/ptest.bbclass"));
        assert!(!lintable("meta/recipes-x/foo/files/fix.patch"));
        assert!(!lintable("documentation/index.rst"));
    }
}
