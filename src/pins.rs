//! Known vulnerabilities in the dependency pins of a Django application's
//! requirements files, from trivy (designs/PHASE2.md §5.3).
//!
//! Checks run without network, so trivy reads a database the watcher
//! downloads daily, mounted read-only, and writes nothing.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::finding::{Checker, Finding, Severity, Tool};

pub struct Pins {
    pub program: String,
    /// trivy's cache directory holding `db/trivy.db`.
    pub db: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Output {
    #[serde(rename = "Results", default)]
    results: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    #[serde(rename = "Vulnerabilities", default)]
    vulnerabilities: Vec<Vulnerability>,
}

#[derive(Deserialize)]
struct Vulnerability {
    #[serde(rename = "VulnerabilityID")]
    id: String,
    #[serde(rename = "PkgName")]
    package: String,
    #[serde(rename = "InstalledVersion")]
    version: String,
    #[serde(rename = "FixedVersion", default)]
    fixed: Option<String>,
    #[serde(rename = "Severity", default)]
    severity: String,
    #[serde(rename = "Title", default)]
    title: Option<String>,
}

/// A pip requirements file: `requirements.txt`, `requirements-dev.txt` and
/// the like, in any directory.
fn is_requirements(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.starts_with("requirements") && name.ends_with(".txt")
}

impl Checker for Pins {
    fn tool(&self) -> Tool {
        Tool::Trivy
    }

    fn reports_fixes(&self) -> bool {
        true
    }

    fn lint(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        let requirements: Vec<&String> = files
            .iter()
            .filter(|f| is_requirements(f) && tree.join(f).is_file())
            .collect();
        if requirements.is_empty() {
            return Ok(Vec::new());
        }
        let db = self
            .db
            .as_ref()
            .filter(|db| db.join("db/trivy.db").is_file())
            .context("no trivy database; the watcher downloads it daily (--trivy-db)")?;
        let mut findings = Vec::new();
        for file in requirements {
            let output = Command::new(&self.program)
                .current_dir(tree)
                .args(["fs", "--quiet", "--cache-dir"])
                .arg(db)
                .args(["--cache-backend", "memory", "--skip-db-update"])
                .args(["--skip-java-db-update", "--offline-scan"])
                .args(["--scanners", "vuln", "--format", "json", "--"])
                .arg(file)
                .output()
                .with_context(|| format!("running {}", self.program))?;
            if !output.status.success() {
                bail!(
                    "{} failed on {file}: {}",
                    self.program,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            let text = std::fs::read_to_string(tree.join(file))?;
            findings.extend(parse(file, &text, &output.stdout)?);
        }
        Ok(findings)
    }
}

fn parse(file: &str, requirements: &str, json: &[u8]) -> Result<Vec<Finding>> {
    let output: Output =
        serde_json::from_slice(json).context("trivy output is not the expected JSON")?;
    Ok(output
        .results
        .into_iter()
        .flat_map(|t| t.vulnerabilities)
        .map(|v| {
            let fix = v
                .fixed
                .filter(|f| !f.is_empty())
                .map_or("no fixed version".into(), |f| format!("fixed in {f}"));
            let title = v.title.unwrap_or_else(|| v.id.clone());
            Finding {
                tool: Tool::Trivy,
                severity: severity(&v.severity),
                patch: None,
                file: Some(file.to_string()),
                line: pin_line(requirements, &v.package),
                message: format!("{} {}: {title} ({fix})", v.package, v.version),
                check: v.id,
            }
        })
        .collect())
}

fn severity(trivy: &str) -> Severity {
    match trivy {
        "CRITICAL" | "HIGH" => Severity::Error,
        "MEDIUM" => Severity::Warning,
        _ => Severity::Info,
    }
}

/// The line pinning `package`, compared as PyPI normalizes names (case,
/// and `-`, `_` and `.` alike).
fn pin_line(requirements: &str, package: &str) -> Option<u32> {
    let normalize = |s: &str| s.to_ascii_lowercase().replace(['_', '.'], "-");
    let wanted = normalize(package);
    requirements
        .lines()
        .position(|line| {
            let name = line
                .split(['=', '<', '>', '~', '!', '[', ';', ' '])
                .next()
                .unwrap_or("");
            normalize(name.trim()) == wanted
        })
        .and_then(|n| u32::try_from(n + 1).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"Results": [{"Target": "requirements.txt", "Vulnerabilities": [
        {"VulnerabilityID": "CVE-2026-114145", "PkgName": "GitPython", "InstalledVersion": "3.1.62",
         "FixedVersion": "3.2.1", "Severity": "HIGH",
         "Title": "GitPython: arbitrary code execution via improper submodule path validation"},
        {"VulnerabilityID": "GHSA-xxxx", "PkgName": "zope.interface", "InstalledVersion": "1.0",
         "Severity": "LOW"}
    ]}]}"#;

    #[test]
    fn parses_trivy_json() {
        let requirements =
            "-i https://example.org/simple/\nGitPython==3.1.62\nzope-interface==1.0\n";
        let findings = parse("requirements.txt", requirements, JSON.as_bytes()).unwrap();
        assert_eq!(findings[0].check, "CVE-2026-114145");
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(findings[0].line, Some(2));
        assert_eq!(
            findings[0].message,
            "GitPython 3.1.62: GitPython: arbitrary code execution via improper submodule path validation (fixed in 3.2.1)"
        );
        assert_eq!(findings[1].severity, Severity::Info);
        assert_eq!(findings[1].line, Some(3));
        assert_eq!(
            findings[1].message,
            "zope.interface 1.0: GHSA-xxxx (no fixed version)"
        );
    }

    #[test]
    fn requirements_files_and_a_missing_database() {
        assert!(is_requirements("requirements.txt"));
        assert!(is_requirements("docker/requirements-dev.txt"));
        assert!(!is_requirements("requirements.in"));
        assert!(!is_requirements("docs/requirements/index.rst"));

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "x==1\n").unwrap();
        let pins = Pins {
            program: "trivy".into(),
            db: None,
        };
        let files = vec!["requirements.txt".to_string()];
        let err = pins.lint(dir.path(), &files).unwrap_err();
        assert!(err.to_string().contains("no trivy database"), "{err}");
        // Nothing to scan: no database needed.
        assert!(
            pins.lint(dir.path(), &["README".into()])
                .unwrap()
                .is_empty()
        );
    }
}
