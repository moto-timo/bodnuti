//! The result of `bodnuti check`, as JSON or as text for a terminal or a
//! mailing-list reply.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::finding::{Finding, Severity, ToolError};
use crate::patchtest::Counts;
use crate::series::Series;
use crate::workspace::Applied;

#[derive(Serialize, Deserialize)]
pub struct Report {
    pub series: Series,
    pub base: String,
    pub applied: Vec<Applied>,
    pub findings: Vec<Finding>,
    pub tool_errors: Vec<ToolError>,
    /// patchtest results that are not findings: passes and skips.
    pub patchtest: Option<Counts>,
    /// Problems the series removes, from checkers that report fixes (a
    /// dependency advisory a pin upgrade fixes). Not findings: a series
    /// that only fixes is clean. Reports written before it have none.
    #[serde(default)]
    pub fixed: Vec<Finding>,
    /// The lines of the patched file around each finding that has a file
    /// and line, keyed by `excerpt_key`. Only the check has the patched
    /// tree, so the pages read them from here (designs/PHASE2.md §6.1).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub excerpts: BTreeMap<String, Excerpt>,
}

/// A few numbered lines of a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Excerpt {
    /// The number of the first line.
    pub first: u32,
    pub lines: Vec<String>,
}

/// Lines shown before and after a finding's line.
const EXCERPT_CONTEXT: u32 = 3;

/// Excerpt lines are cut to this many characters.
const EXCERPT_WIDTH: usize = 200;

pub fn excerpt_key(file: &str, line: u32) -> String {
    format!("{file}:{line}")
}

/// The excerpts for `findings` from `tree`, the tree as the series leaves
/// it. A file that cannot be read, or a path that leaves the tree, has
/// none.
pub fn excerpts(tree: &Path, findings: &[Finding]) -> BTreeMap<String, Excerpt> {
    let mut files: BTreeMap<&str, Option<Vec<String>>> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for f in findings {
        let (Some(file), Some(line)) = (f.file.as_deref(), f.line) else {
            continue;
        };
        if line == 0 || file.starts_with('/') || file.split('/').any(|p| p == "..") {
            continue;
        }
        let text = files.entry(file).or_insert_with(|| {
            let bytes = std::fs::read(tree.join(file)).ok()?;
            Some(
                String::from_utf8_lossy(&bytes)
                    .lines()
                    .map(String::from)
                    .collect(),
            )
        });
        let Some(text) = text else { continue };
        let last = (line + EXCERPT_CONTEXT).min(u32::try_from(text.len()).unwrap_or(u32::MAX));
        if line > last {
            continue;
        }
        let first = line.saturating_sub(EXCERPT_CONTEXT).max(1);
        let lines = text[first as usize - 1..last as usize]
            .iter()
            .map(|l| l.chars().take(EXCERPT_WIDTH).collect())
            .collect();
        out.insert(excerpt_key(file, line), Excerpt { first, lines });
    }
    out
}

impl Report {
    /// 0 when clean, 1 with findings, 2 when a checker could not run.
    pub fn exit_code(&self) -> u8 {
        if !self.tool_errors.is_empty() {
            2
        } else if !self.findings.is_empty() {
            1
        } else {
            0
        }
    }

    /// Findings grouped under the patch they belong to, in series order.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let first = self.series.cover.as_ref().or(self.series.patches.first());
        let subject = first.map_or("", |p| p.subject.as_str());
        let _ = writeln!(out, "bodnuti: {subject}");
        let _ = writeln!(out, "base: {}", &self.base[..self.base.len().min(12)]);

        for patch in &self.series.patches {
            let findings: Vec<&Finding> = self
                .findings
                .iter()
                .filter(|f| f.patch == Some(patch.index))
                .collect();
            if findings.is_empty() {
                continue;
            }
            let _ = writeln!(out, "\n{}", patch.subject);
            for f in findings {
                out.push_str(&self.text_block(f, &severity_label(f.severity), true));
            }
        }
        let unattributed: Vec<&Finding> =
            self.findings.iter().filter(|f| f.patch.is_none()).collect();
        if !unattributed.is_empty() {
            let _ = writeln!(out, "\nseries");
            for f in unattributed {
                out.push_str(&self.text_block(f, &severity_label(f.severity), true));
            }
        }
        if !self.fixed.is_empty() {
            let _ = writeln!(out, "\nfixes");
            for f in &self.fixed {
                // The line of a fix is in the tree before the series, which
                // the excerpts do not show.
                out.push_str(&self.text_block(f, "FIXED", false));
            }
        }
        if !self.tool_errors.is_empty() {
            let _ = writeln!(out, "\nnot checked");
            for e in &self.tool_errors {
                let _ = writeln!(out, "\n  {}", tool_name(e.tool));
                for line in e.message.lines() {
                    let _ = writeln!(out, "      {line}");
                }
            }
        }

        let _ = writeln!(out);
        if let Some(c) = &self.patchtest {
            let _ = writeln!(
                out,
                "patchtest: {} pass, {} fail, {} skip",
                c.pass, c.fail, c.skip
            );
        }
        let count = |s: Severity| self.findings.iter().filter(|f| f.severity == s).count();
        let _ = writeln!(
            out,
            "{} findings: {} errors, {} warnings, {} info",
            self.findings.len(),
            count(Severity::Error),
            count(Severity::Warning),
            count(Severity::Info)
        );
        out
    }
}

impl Report {
    /// The excerpt recorded for a finding, if any.
    pub fn excerpt(&self, f: &Finding) -> Option<&Excerpt> {
        self.excerpts.get(&excerpt_key(f.file.as_deref()?, f.line?))
    }

    /// One finding as a block of text (designs/PHASE2.md §6.1):
    ///
    /// ```text
    ///
    ///   ERROR   oelint oelint.vars.inconspaces  meta/foo/foo.bb:12
    ///       Assignment should be enclosed in double quotes
    ///        11 | SRC_URI = "..."
    ///     >  12 | A:append= "x"
    /// ```
    fn text_block(&self, f: &Finding, label: &str, with_excerpt: bool) -> String {
        let mut out = String::new();
        let place = location(f).map(|p| format!("  {p}")).unwrap_or_default();
        let _ = writeln!(
            out,
            "\n  {label:<7} {} {}{place}",
            tool_name(f.tool),
            f.check
        );
        for line in f.message.lines() {
            let _ = writeln!(out, "      {line}");
        }
        if let Some(e) = self.excerpt(f).filter(|_| with_excerpt) {
            for (number, text, here) in numbered(e, f.line) {
                let mark = if here { '>' } else { ' ' };
                let _ = writeln!(out, "    {mark} {number} | {text}");
            }
        }
        out
    }
}

/// An excerpt's lines with right-aligned numbers, and whether each is the
/// finding's line.
pub fn numbered(e: &Excerpt, line: Option<u32>) -> Vec<(String, &str, bool)> {
    let last = e.first as usize + e.lines.len().saturating_sub(1);
    let width = last.to_string().len();
    e.lines
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let n = e.first as usize + i;
            (
                format!("{n:>width$}"),
                text.as_str(),
                line.is_some_and(|l| l as usize == n),
            )
        })
        .collect()
}

/// `file:line`, `file`, or nothing.
pub fn location(f: &Finding) -> Option<String> {
    match (&f.file, f.line) {
        (Some(file), Some(n)) => Some(format!("{file}:{n}")),
        (Some(file), None) => Some(file.clone()),
        _ => None,
    }
}

pub fn tool_name(tool: crate::finding::Tool) -> String {
    format!("{tool:?}").to_lowercase()
}

pub fn severity_label(severity: Severity) -> String {
    format!("{severity:?}").to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Tool;
    use crate::series::Series;

    fn report(findings: Vec<Finding>, tool_errors: Vec<ToolError>) -> Report {
        let raw = "From x Mon Sep 17 00:00:00 2001\nSubject: [PATCH] foo: bar\n\nbody\ndiff --git a/x b/x\n";
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), raw).unwrap();
        Report {
            series: Series::load(&[file.path()]).unwrap(),
            base: "0123456789abcdef".into(),
            applied: Vec::new(),
            findings,
            tool_errors,
            patchtest: Some(Counts {
                pass: 8,
                fail: 0,
                skip: 2,
            }),
            fixed: Vec::new(),
            excerpts: BTreeMap::new(),
        }
    }

    fn finding(severity: Severity) -> Finding {
        Finding {
            tool: Tool::Shellcheck,
            check: "SC3037".into(),
            severity,
            patch: Some(1),
            file: Some("meta/classes-recipe/uboot-sign.bbclass".into()),
            line: Some(426),
            message: "In POSIX sh, echo flags are undefined.".into(),
        }
    }

    #[test]
    fn records_the_lines_around_a_finding() {
        let dir = tempfile::tempdir().unwrap();
        let text: String = (1..=10).map(|n| format!("line {n}\n")).collect();
        std::fs::write(dir.path().join("a.bb"), text).unwrap();
        let mut near_start = finding(Severity::Error);
        near_start.file = Some("a.bb".into());
        near_start.line = Some(2);
        let mut last = near_start.clone();
        last.line = Some(10);
        let mut outside = near_start.clone();
        outside.file = Some("../a.bb".into());
        let mut gone = near_start.clone();
        gone.line = Some(11);
        let excerpts = excerpts(dir.path(), &[near_start, last, outside, gone]);
        assert_eq!(excerpts.len(), 2);
        let e = &excerpts["a.bb:2"];
        assert_eq!((e.first, e.lines.len()), (1, 5));
        assert_eq!(e.lines[1], "line 2");
        let e = &excerpts["a.bb:10"];
        assert_eq!((e.first, e.lines.len()), (7, 4));
    }

    #[test]
    fn shows_the_excerpt_under_the_message() {
        let mut f = finding(Severity::Error);
        f.message = "first line\nsecond line".into();
        let mut r = report(vec![f], Vec::new());
        r.excerpts.insert(
            excerpt_key("meta/classes-recipe/uboot-sign.bbclass", 426),
            Excerpt {
                first: 424,
                lines: vec!["a".into(), "b".into(), "echo -e x".into(), "d".into()],
            },
        );
        let text = r.text();
        assert!(
            text.contains(
                "      first line\n      second line\n      424 | a\n      425 | b\n    \
                 > 426 | echo -e x\n      427 | d\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn lists_fixes_apart_from_findings() {
        let mut r = report(Vec::new(), Vec::new());
        r.fixed.push(Finding {
            tool: Tool::Bodnuti,
            check: "CVE-2026-112393".into(),
            severity: Severity::Error,
            patch: Some(1),
            file: Some("requirements.txt".into()),
            line: Some(30),
            message: "Pillow 12.3.0: heap overflow (fixed in 13.0.0)".into(),
        });
        let text = r.text();
        assert!(
            text.contains(
                "\nfixes\n\n  FIXED   bodnuti CVE-2026-112393  requirements.txt:30\n      \
                 Pillow 12.3.0: heap overflow (fixed in 13.0.0)\n"
            ),
            "{text}"
        );
        assert!(text.ends_with("0 findings: 0 errors, 0 warnings, 0 info\n"));
        assert_eq!(r.exit_code(), 0);
    }

    #[test]
    fn groups_findings_under_their_patch() {
        let text = report(vec![finding(Severity::Warning)], Vec::new()).text();
        assert!(
            text.starts_with("bodnuti: [PATCH] foo: bar\nbase: 0123456789ab\n"),
            "{text}"
        );
        assert!(
            text.contains(
                "\n[PATCH] foo: bar\n\n  WARNING shellcheck SC3037  \
                 meta/classes-recipe/uboot-sign.bbclass:426\n      \
                 In POSIX sh, echo flags are undefined.\n"
            ),
            "{text}"
        );
        assert!(text.contains("patchtest: 8 pass, 0 fail, 2 skip"));
        assert!(text.ends_with("1 findings: 0 errors, 1 warnings, 0 info\n"));
    }

    #[test]
    fn exit_code_prefers_tool_errors() {
        assert_eq!(report(Vec::new(), Vec::new()).exit_code(), 0);
        assert_eq!(
            report(vec![finding(Severity::Info)], Vec::new()).exit_code(),
            1
        );
        let err = ToolError {
            tool: Tool::Oelint,
            message: "oelint-adv exited with 2".into(),
        };
        let r = report(vec![finding(Severity::Error)], vec![err]);
        assert_eq!(r.exit_code(), 2);
        assert!(
            r.text()
                .contains("not checked\n\n  oelint\n      oelint-adv exited with 2\n")
        );
    }
}
