//! The result of `bodnuti check`, as JSON or as text for a terminal or a
//! mailing-list reply.

use std::fmt::Write;

use serde::Serialize;

use crate::finding::{Finding, Severity, ToolError};
use crate::patchtest::Counts;
use crate::series::Series;
use crate::workspace::Applied;

#[derive(Serialize)]
pub struct Report {
    pub series: Series,
    pub base: String,
    pub applied: Vec<Applied>,
    pub findings: Vec<Finding>,
    pub tool_errors: Vec<ToolError>,
    /// patchtest results that are not findings: passes and skips.
    pub patchtest: Option<Counts>,
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
                let _ = writeln!(out, "  {}", line(f));
            }
        }
        let unattributed: Vec<&Finding> =
            self.findings.iter().filter(|f| f.patch.is_none()).collect();
        if !unattributed.is_empty() {
            let _ = writeln!(out, "\nseries");
            for f in unattributed {
                let _ = writeln!(out, "  {}", line(f));
            }
        }
        if !self.tool_errors.is_empty() {
            let _ = writeln!(out, "\nnot checked");
            for e in &self.tool_errors {
                let _ = writeln!(out, "  {:?}: {}", e.tool, e.message.replace('\n', "\n    "));
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

/// `ERROR   oelint      path:line  message (check)`
fn line(f: &Finding) -> String {
    let severity = format!("{:?}", f.severity).to_uppercase();
    let tool = format!("{:?}", f.tool).to_lowercase();
    let place = match (&f.file, f.line) {
        (Some(file), Some(n)) => format!("{file}:{n}  "),
        (Some(file), None) => format!("{file}  "),
        _ => String::new(),
    };
    format!(
        "{severity:<7} {tool:<10}  {place}{} ({})",
        f.message, f.check
    )
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
    fn groups_findings_under_their_patch() {
        let text = report(vec![finding(Severity::Warning)], Vec::new()).text();
        assert!(
            text.starts_with("bodnuti: [PATCH] foo: bar\nbase: 0123456789ab\n"),
            "{text}"
        );
        assert!(text.contains(
            "\n[PATCH] foo: bar\n  WARNING shellcheck  meta/classes-recipe/uboot-sign.bbclass:426  \
             In POSIX sh, echo flags are undefined. (SC3037)\n"
        ), "{text}");
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
                .contains("not checked\n  Oelint: oelint-adv exited with 2")
        );
    }
}
