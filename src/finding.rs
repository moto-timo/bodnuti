//! What a check reports, independent of the tool that found it.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Patchtest,
    Oelint,
    Shellcheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub tool: Tool,
    /// The tool's own check id, e.g. `oelint.vars.inconspaces`. Kept as text
    /// because the ids belong to the upstream tools and change with them.
    pub check: String,
    pub severity: Severity,
    /// 1-based index of the patch the finding is attributed to.
    pub patch: Option<u32>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
}

/// A tool run on the touched files of a tree, once before the series and
/// once after it, so that only what the series adds is reported.
pub trait Checker {
    fn tool(&self) -> Tool;
    fn lint(&self, tree: &Path, files: &[String]) -> anyhow::Result<Vec<Finding>>;
}

/// A checker that could not produce a result. Never the author's fault, and
/// never silently treated as a pass.
#[derive(Debug, Clone, Serialize)]
pub struct ToolError {
    pub tool: Tool,
    pub message: String,
}

/// Keeps the head findings that base does not have.
///
/// Findings are compared on file, check and message, not line, because a
/// patch moves lines. The comparison counts: three identical base findings
/// against four in head leave one new finding.
pub fn new_findings(base: &[Finding], head: Vec<Finding>) -> Vec<Finding> {
    let key = |f: &Finding| (f.file.clone(), f.check.clone(), f.message.clone());
    let mut remaining: HashMap<_, usize> = HashMap::new();
    for f in base {
        *remaining.entry(key(f)).or_default() += 1;
    }
    head.into_iter()
        .filter(|f| match remaining.get_mut(&key(f)) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(check: &str, line: u32) -> Finding {
        Finding {
            tool: Tool::Oelint,
            check: check.into(),
            severity: Severity::Warning,
            patch: None,
            file: Some("foo.bb".into()),
            line: Some(line),
            message: "msg".into(),
        }
    }

    #[test]
    fn moved_lines_are_not_new() {
        let base = [finding("a", 10)];
        let head = vec![finding("a", 14)];
        assert!(new_findings(&base, head).is_empty());
    }

    #[test]
    fn duplicates_count_as_a_multiset() {
        let base = vec![finding("a", 1); 3];
        let head = vec![finding("a", 1); 4];
        assert_eq!(new_findings(&base, head).len(), 1);
    }

    #[test]
    fn a_new_check_is_reported() {
        let base = [finding("a", 1)];
        let head = vec![finding("a", 1), finding("b", 2)];
        let new = new_findings(&base, head);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].check, "b");
    }
}
