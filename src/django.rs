//! Django's own checks on a Django application (designs/PHASE2.md §5.5):
//! `manage.py check` and `makemigrations --check`, in a venv with the
//! application's dependencies and with the database swapped for sqlite.
//!
//! These import the series' settings and models, which runs its code: they
//! belong in the offline, read-only container, like the documentation
//! build.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};

use crate::finding::{Checker, Finding, Severity, Tool};

pub struct Django {
    /// The Python of the application's venv.
    pub python: Option<PathBuf>,
}

/// The settings bodnuti runs Django with: the application's, with an
/// in-memory database and a secret key.
fn settings_module(app: &str) -> String {
    format!(
        "import os\n\
         os.environ.setdefault(\"SECRET_KEY\", \"bodnuti-check\")\n\
         from {app} import *  # noqa: F403\n\
         DATABASES = {{\"default\": {{\"ENGINE\": \"django.db.backends.sqlite3\", \"NAME\": \":memory:\"}}}}\n\
         SECRET_KEY = globals().get(\"SECRET_KEY\") or \"bodnuti-check\"\n"
    )
}

/// The settings module `manage.py` names in its `DJANGO_SETTINGS_MODULE`
/// default, if it is a plain dotted name.
fn app_settings(manage: &str) -> Option<String> {
    let rest = &manage[manage.find("\"DJANGO_SETTINGS_MODULE\"")? + 24..];
    let start = rest.find(['"', '\''])? + 1;
    let end = start + rest[start..].find(['"', '\''])?;
    let name = &rest[start..end];
    let plain = !name.is_empty()
        && name.split('.').all(|part| {
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        });
    plain.then(|| name.to_string())
}

impl Checker for Django {
    fn tool(&self) -> Tool {
        Tool::Django
    }

    /// A model change can break a check anywhere in the project.
    fn touched_files_only(&self) -> bool {
        false
    }

    fn lint(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        // Without Python changes, the checks cannot change.
        if !files.iter().any(|f| f.ends_with(".py")) {
            return Ok(Vec::new());
        }
        let python = self
            .python
            .as_ref()
            .filter(|p| p.is_file())
            .context("no venv with this Django application's dependencies (--django-python)")?;
        let manage = std::fs::read_to_string(tree.join("manage.py"))
            .context("the application has no manage.py")?;
        let app = app_settings(&manage).context("manage.py names no DJANGO_SETTINGS_MODULE")?;
        let shim = tempfile::tempdir()?;
        std::fs::write(
            shim.path().join("bodnuti_settings.py"),
            settings_module(&app),
        )?;
        let path = format!("{}:{}", shim.path().display(), tree.display());
        let run = |args: &[&str]| -> Result<Output> {
            Command::new(python)
                .current_dir(tree)
                .env("PYTHONPATH", &path)
                .env("DJANGO_SETTINGS_MODULE", "bodnuti_settings")
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .arg("manage.py")
                .args(args)
                .output()
                .with_context(|| format!("running {}", python.display()))
        };

        let check = run(&["check"])?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&check.stdout),
            String::from_utf8_lossy(&check.stderr)
        );
        let mut findings = parse_check(&text);
        if !check.status.success() && findings.is_empty() {
            bail!("manage.py check failed: {}", last_line(&text));
        }

        // System checks ran above; --skip-checks keeps their errors from
        // stopping this one.
        let migrations = run(&["makemigrations", "--check", "--dry-run", "--skip-checks"])?;
        let out = String::from_utf8_lossy(&migrations.stdout);
        if !migrations.status.success() {
            if !out.contains("Migrations for") {
                let err = String::from_utf8_lossy(&migrations.stderr);
                bail!("manage.py makemigrations failed: {}", last_line(&err));
            }
            findings.push(Finding {
                tool: Tool::Django,
                check: "django.migrations".into(),
                severity: Severity::Error,
                patch: None,
                file: None,
                line: None,
                message: format!("model changes without a migration:\n{}", out.trim_end()),
            });
        }
        Ok(findings)
    }
}

/// System check messages: `<object>: (<id>) <message>`, followed by an
/// optional `HINT:` line.
fn parse_check(text: &str) -> Vec<Finding> {
    let mut findings: Vec<Finding> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(hint) = trimmed.strip_prefix("HINT:") {
            if let Some(last) = findings.last_mut() {
                last.message.push_str(&format!("\nHint: {}", hint.trim()));
            }
            continue;
        }
        let Some((object, rest)) = trimmed.split_once(": (") else {
            continue;
        };
        let Some((id, message)) = rest.split_once(") ") else {
            continue;
        };
        let level = id.rsplit('.').next().and_then(|code| code.chars().next());
        let severity = match level {
            Some('E' | 'C') => Severity::Error,
            Some('W') => Severity::Warning,
            Some('I' | 'D') => Severity::Info,
            _ => continue,
        };
        findings.push(Finding {
            tool: Tool::Django,
            check: format!("django.{id}"),
            severity,
            patch: None,
            file: None,
            line: None,
            message: format!("{object}: {message}"),
        });
    }
    findings
}

fn last_line(text: &str) -> &str {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_system_check_messages() {
        let out = "SystemCheckError: System check identified some issues:\n\nERRORS:\n\
            Post.Build.probe: (fields.E300) Field defines a relation with model 'NoSuchModel', which is either not installed, or is abstract.\n\
            \tHINT: Check the model name.\n\n\
            WARNINGS:\n?: (security.W004) You have not set a value for the SECURE_HSTS_SECONDS setting.\n\n\
            System check identified 2 issues (0 silenced).\n";
        let findings = parse_check(out);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].check, "django.fields.E300");
        assert_eq!(findings[0].severity, Severity::Error);
        assert!(
            findings[0]
                .message
                .starts_with("Post.Build.probe: Field defines")
        );
        assert!(
            findings[0]
                .message
                .ends_with("\nHint: Check the model name.")
        );
        assert_eq!(findings[1].check, "django.security.W004");
        assert_eq!(findings[1].severity, Severity::Warning);
    }

    #[test]
    fn finds_the_settings_module_in_manage_py() {
        let manage = "def main():\n    os.environ.setdefault(\"DJANGO_SETTINGS_MODULE\", \"project.settings\")\n";
        assert_eq!(app_settings(manage).as_deref(), Some("project.settings"));
        let bad = "os.environ.setdefault(\"DJANGO_SETTINGS_MODULE\", \"x; import os\")";
        assert_eq!(app_settings(bad), None);
    }
}
