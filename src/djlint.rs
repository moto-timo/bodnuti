//! djLint on the Django templates a series touches (designs/PHASE2.md §5.2).
//!
//! djLint's django profile reports formatting as well as bugs. bodnuti keeps
//! the rules that find template bugs and accessibility problems, and drops
//! formatting, which is left to the projects' own tooling.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::finding::{Checker, Finding, Severity, Tool};

/// Template bugs: tags that do not run as written, are unbalanced, or
/// duplicate an attribute or id.
const BUGS: &[&str] = &[
    "H019", "H025", "H037", "H041", "H053", "T027", "T034", "T038", "T039", "T040", "T041", "T042",
    "T043", "T044", "T045",
];

/// Accessibility: missing lang, alt and image sizes, button types, ARIA
/// mistakes, nested interactive elements.
const ACCESSIBILITY: &[&str] = &[
    "H005", "H006", "H013", "H016", "H043", "H045", "H046", "H047", "H048", "H049", "H050", "H051",
    "H054",
];

/// One finding per line: file, line:column, code, message.
const FORMAT: &str = "{filename}\t{line}\t{code}\t{message}";

pub struct Djlint {
    pub program: String,
}

/// A Django template: an HTML file under a `templates` directory.
fn is_template(path: &str) -> bool {
    path.ends_with(".html") && path.split('/').any(|part| part == "templates")
}

impl Checker for Djlint {
    fn tool(&self) -> Tool {
        Tool::Djlint
    }

    fn lint(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        let templates: Vec<&String> = files
            .iter()
            .filter(|f| is_template(f) && tree.join(f).is_file())
            .collect();
        if templates.is_empty() {
            return Ok(Vec::new());
        }
        let output = Command::new(&self.program)
            .current_dir(tree)
            .args([
                "--lint",
                "--profile=django",
                "--linter-output-format",
                FORMAT,
            ])
            .arg("--")
            .args(&templates)
            .output()
            .with_context(|| format!("running {}", self.program))?;
        // 0 when clean, 1 when there are findings; a crash is a traceback.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !matches!(output.status.code(), Some(0 | 1)) || stderr.contains("Traceback") {
            bail!("{} failed: {}", self.program, stderr.trim());
        }
        Ok(parse(&String::from_utf8_lossy(&output.stdout)))
    }
}

/// Reads djLint's output in `FORMAT`. Other lines (progress, summary) are
/// skipped, as are the rules bodnuti does not report.
fn parse(text: &str) -> Vec<Finding> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, '\t');
            let (file, position, code, message) = (
                fields.next()?,
                fields.next()?,
                fields.next()?,
                fields.next()?,
            );
            let line = position.split(':').next()?.parse().ok()?;
            let severity = if BUGS.contains(&code) {
                Severity::Warning
            } else if ACCESSIBILITY.contains(&code) {
                Severity::Info
            } else {
                return None;
            };
            Some(Finding {
                tool: Tool::Djlint,
                check: code.to_string(),
                severity,
                patch: None,
                file: Some(file.trim_start_matches("./").to_string()),
                line: Some(line),
                message: message.trim().to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_bugs_and_accessibility_only() {
        let out = "Linting 2/2 files\n\n\
            templates/404.html\t12:0\tH014\tFound extra blank lines.\n\
            templates/404.html\t14:0\tT045\tTemplate tag inside an html comment still runs.\n\
            templates/base.html\t51:24\tH043\tButton tag should have a type attribute.\n\
            \nLinted 2 files, found 3 errors.\n";
        let findings = parse(out);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].check, "T045");
        assert_eq!(findings[0].line, Some(14));
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[1].file.as_deref(), Some("templates/base.html"));
        assert_eq!(findings[1].severity, Severity::Info);
    }

    #[test]
    fn templates_are_html_under_a_templates_directory() {
        assert!(is_template("templates/base.html"));
        assert!(is_template("layerindex/templates/layerindex/detail.html"));
        assert!(!is_template("static/index.html"));
        assert!(!is_template("templates/email.txt"));
    }
}
