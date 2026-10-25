//! Touched JSON files must still parse (designs/PHASE2.md §5.4).
//! yocto-autobuilder-helper's `config.json` is read when the autobuilder
//! starts, and a stray comma there stops it.

use std::path::Path;

use anyhow::Result;

use crate::finding::{Checker, Finding, Severity, Tool};

pub struct Json;

impl Checker for Json {
    fn tool(&self) -> Tool {
        Tool::Json
    }

    fn lint(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();
        for file in files.iter().filter(|f| f.ends_with(".json")) {
            let Ok(text) = std::fs::read_to_string(tree.join(file)) else {
                continue;
            };
            if let Err(err) = serde_json::from_str::<serde_json::Value>(&text) {
                findings.push(Finding {
                    tool: Tool::Json,
                    check: "json.syntax".into(),
                    severity: Severity::Error,
                    patch: None,
                    file: Some(file.clone()),
                    line: u32::try_from(err.line()).ok(),
                    message: format!("not valid JSON: {err}"),
                });
            }
        }
        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_json_that_does_not_parse() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ok.json"), "{\"a\": [1, 2]}\n").unwrap();
        std::fs::write(dir.path().join("config.json"), "{\n  \"a\": 1,\n}\n").unwrap();
        let files = vec!["ok.json".into(), "config.json".into(), "gone.json".into()];
        let findings = Json.lint(dir.path(), &files).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file.as_deref(), Some("config.json"));
        assert_eq!(findings[0].line, Some(3));
        assert_eq!(findings[0].severity, Severity::Error);
    }
}
