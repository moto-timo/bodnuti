//! ruff on the Python files a series touches, in Django applications and
//! other Python repositories (designs/PHASE2.md §5.2, §5.4).
//!
//! bodnuti chooses the rules, so every application is checked the same way
//! whatever its `ruff.toml` selects; the rest of the project's configuration
//! (excluded paths, per-file ignores) still applies. The rules find bugs and
//! security problems, not style.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::finding::{Checker, Finding, Severity, Tool};

/// ruff's classic defaults, the Django rules and the security rules.
const SELECT: &str = "E4,E7,E9,F,DJ,S";

/// Model member order (style), assert (used throughout tests), and any
/// subprocess call, which carry little signal in these code bases.
const IGNORE: &str = "DJ012,S101,S603,S607";

pub struct Ruff {
    pub program: String,
}

#[derive(Deserialize)]
struct Diagnostic {
    /// None for a syntax error.
    code: Option<String>,
    message: String,
    filename: String,
    location: Location,
}

#[derive(Deserialize)]
struct Location {
    row: u32,
}

/// A Python source: a `.py` file, BuildBot's `master.cfg`, or a script
/// whose first line is a `python` shebang, as most of
/// yocto-autobuilder-helper's are. ruff checks any file it is given by name.
fn is_python(tree: &Path, file: &str) -> bool {
    let path = tree.join(file);
    if !path.is_file() {
        return false;
    }
    let name = file.rsplit('/').next().unwrap_or(file);
    if file.ends_with(".py") || name == "master.cfg" {
        return true;
    }
    if name.contains('.') {
        return false;
    }
    let mut first = [0u8; 128];
    let n = std::fs::File::open(&path)
        .and_then(|mut f| std::io::Read::read(&mut f, &mut first))
        .unwrap_or(0);
    let line = first[..n].split(|&b| b == b'\n').next().unwrap_or(&[]);
    line.starts_with(b"#!") && String::from_utf8_lossy(line).contains("python")
}

impl Checker for Ruff {
    fn tool(&self) -> Tool {
        Tool::Ruff
    }

    fn lint(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        let tree = &tree.canonicalize()?;
        let python: Vec<&String> = files.iter().filter(|f| is_python(tree, f)).collect();
        if python.is_empty() {
            return Ok(Vec::new());
        }
        // Run from the tree so ruff finds the project's configuration.
        // --force-exclude keeps the project's excluded paths excluded even
        // though the files are named. The tree may be read-only: no cache.
        let output = Command::new(&self.program)
            .current_dir(tree)
            .args(["check", "--no-cache", "--force-exclude", "--exit-zero"])
            .args([
                "--output-format",
                "json",
                "--select",
                SELECT,
                "--ignore",
                IGNORE,
            ])
            .arg("--")
            .args(&python)
            .output()
            .with_context(|| format!("running {}", self.program))?;
        if !output.status.success() {
            bail!(
                "{} failed: {}",
                self.program,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        parse(tree, &String::from_utf8_lossy(&output.stdout))
    }
}

fn parse(tree: &Path, json: &str) -> Result<Vec<Finding>> {
    let diagnostics: Vec<Diagnostic> =
        serde_json::from_str(json).context("ruff output is not the expected JSON")?;
    Ok(diagnostics
        .into_iter()
        .map(|d| {
            let file = Path::new(&d.filename)
                .strip_prefix(tree)
                .map_or(d.filename.clone(), |p| p.to_string_lossy().into_owned());
            let check = d.code.unwrap_or_else(|| "syntax-error".into());
            Finding {
                tool: Tool::Ruff,
                severity: severity(&check),
                check,
                patch: None,
                file: Some(file),
                line: Some(d.location.row),
                message: d.message,
            }
        })
        .collect())
}

/// Code that cannot run, or names that do not exist, are errors; the rest
/// are warnings.
fn severity(code: &str) -> Severity {
    let fatal = code == "syntax-error"
        || code.starts_with("E9")
        || ["F821", "F822", "F823"].contains(&code);
    if fatal {
        Severity::Error
    } else {
        Severity::Warning
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ruff_json() {
        let tree = Path::new("/w/app");
        let json = r#"[
          {"code": "DJ008", "message": "Model does not define `__str__` method",
           "filename": "/w/app/Post/models.py", "location": {"column": 7, "row": 25},
           "severity": "error"},
          {"code": null, "message": "SyntaxError: Expected ')', found newline",
           "filename": "/w/app/x.py", "location": {"column": 6, "row": 1}},
          {"code": "F821", "message": "Undefined name `foo`",
           "filename": "/w/app/y.py", "location": {"column": 1, "row": 3}}
        ]"#;
        let findings = parse(tree, json).unwrap();
        assert_eq!(findings[0].file.as_deref(), Some("Post/models.py"));
        assert_eq!(findings[0].line, Some(25));
        assert_eq!(findings[0].check, "DJ008");
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[1].check, "syntax-error");
        assert_eq!(findings[1].severity, Severity::Error);
        assert_eq!(findings[2].severity, Severity::Error);
    }

    #[test]
    fn checks_only_existing_python_files() {
        let dir = tempfile::tempdir().unwrap();
        let ruff = Ruff {
            program: "/nonexistent/ruff".into(),
        };
        // Nothing to check, so ruff is not run at all.
        std::fs::write(dir.path().join("run.sh"), "#!/bin/sh\n").unwrap();
        let files = vec!["README.md".into(), "gone.py".into(), "run.sh".into()];
        assert!(ruff.lint(dir.path(), &files).unwrap().is_empty());
    }

    #[test]
    fn finds_python_by_name_or_shebang() {
        let dir = tempfile::tempdir().unwrap();
        let tree = dir.path();
        for (name, text) in [
            ("a.py", ""),
            ("master.cfg", "x = 1\n"),
            ("run-config", "#!/usr/bin/env python3\nimport os\n"),
            ("run.sh", "#!/bin/sh\n"),
            ("other.cfg", "#!/usr/bin/env python3\n"),
            ("README", "python\n"),
        ] {
            std::fs::write(tree.join(name), text).unwrap();
        }
        let python: Vec<&str> = [
            "a.py",
            "master.cfg",
            "run-config",
            "run.sh",
            "other.cfg",
            "README",
        ]
        .into_iter()
        .filter(|f| is_python(tree, f))
        .collect();
        assert_eq!(python, ["a.py", "master.cfg", "run-config"]);
    }
}
