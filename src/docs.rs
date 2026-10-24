//! Documentation checks for Sphinx projects: yocto-docs (`documentation/`)
//! and the BitBake user manual (`doc/`).
//!
//! The checks are the ones the projects' own Makefiles define, so bodnuti
//! holds a patch to the rules its maintainers already apply:
//! - the Sphinx html build, whose warnings `make` turns into errors with `-W`;
//! - `sphinx-lint` on the touched `.rst` files;
//! - yocto-docs' `tools/check-glossaries` and `tools/check-confusables`, when
//!   the tree has them.
//!
//! Vale (`make stylecheck`) is not run: it needs `vale sync` to download its
//! style packages, and its prose rules are far noisier than the rest.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::finding::{Checker, Finding, Severity, Tool};

pub struct Docs {
    /// A Python with Sphinx, the projects' Sphinx extensions and sphinx-lint.
    pub python: String,
}

/// The Sphinx project directory in a tree, relative to its root.
fn docs_dir(tree: &Path) -> Option<&'static str> {
    ["documentation", "doc"]
        .into_iter()
        .find(|d| tree.join(d).join("conf.py").is_file())
}

/// Files the docs build would execute. A series that changes any of them
/// would have its own code run by the build, so the build is refused.
fn runs_code(docs: &str, file: &str) -> bool {
    file.starts_with(docs) && (file.ends_with(".py") || file.starts_with(&format!("{docs}/tools/")))
}

impl Checker for Docs {
    fn tool(&self) -> Tool {
        Tool::Docs
    }

    /// A broken reference can show up in a file the series did not touch,
    /// for example when it removes a label, so all new warnings count.
    fn touched_files_only(&self) -> bool {
        false
    }

    fn lint(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        let tree = &tree.canonicalize()?;
        let Some(docs) = docs_dir(tree) else {
            return Ok(Vec::new());
        };
        if !files.iter().any(|f| f.starts_with(&format!("{docs}/"))) {
            return Ok(Vec::new());
        }
        if let Some(file) = files.iter().find(|f| runs_code(docs, f)) {
            bail!(
                "documentation not built: the series changes {file}, which the build would execute; check it in a confined environment"
            );
        }
        let dir = tree.join(docs);
        let rst: Vec<&String> = files
            .iter()
            .filter(|f| f.ends_with(".rst") && tree.join(f).is_file())
            .collect();

        let mut findings = self.sphinx(tree, &dir)?;
        if !rst.is_empty() {
            let out = self.run(&dir, |c| {
                c.args(["-m", "sphinxlint"])
                    .args(rst.iter().map(|f| tree.join(f)));
            })?;
            findings.extend(out.lines().filter_map(|l| sphinx_lint_line(tree, l)));
        }
        for (script, with_files) in [("check-glossaries", false), ("check-confusables", true)] {
            let path = dir.join("tools").join(script);
            if !path.is_file() {
                continue;
            }
            let out = self.run(&dir, |c| {
                c.arg(&path).args(["--docs-dir", "."]);
                if with_files {
                    c.args(rst.iter().map(|f| tree.join(f)));
                }
            })?;
            findings.extend(
                out.lines()
                    .filter_map(|l| script_line(tree, &dir, script, l)),
            );
        }
        Ok(findings)
    }
}

impl Docs {
    /// Runs Python in `dir` and returns stdout and stderr together, since
    /// sphinx-lint reports on stderr and the yocto-docs tools on stdout. The
    /// tools signal findings with exit status 1, so only a crash or a missing
    /// module is an error.
    fn run(&self, dir: &Path, args: impl FnOnce(&mut Command)) -> Result<String> {
        let mut cmd = Command::new(&self.python);
        cmd.current_dir(dir);
        args(&mut cmd);
        let output = cmd
            .output()
            .with_context(|| format!("running {}", self.python))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Traceback") || stderr.contains("No module named") {
            bail!("{}", stderr.trim().lines().last().unwrap_or_default());
        }
        Ok(format!(
            "{}{stderr}",
            String::from_utf8_lossy(&output.stdout)
        ))
    }

    /// Generates versions the way the Makefile does, then builds html and
    /// returns its warnings.
    fn sphinx(&self, tree: &Path, dir: &Path) -> Result<Vec<Finding>> {
        if dir.join("set_versions.py").is_file() {
            self.run(dir, |c| {
                c.arg("set_versions.py");
            })?;
        }
        let out = tempfile::tempdir()?;
        let warnings = out.path().join("warnings.log");
        let html: PathBuf = out.path().join("html");
        let output = Command::new(&self.python)
            .current_dir(dir)
            .args(["-m", "sphinx", "-q", "-j", "auto", "-b", "html", "-w"])
            .arg(&warnings)
            .arg(".")
            .arg(&html)
            .output()
            .with_context(|| format!("running {} -m sphinx", self.python))?;
        let log = std::fs::read_to_string(&warnings).unwrap_or_default();
        // Sphinx exits 2 on a configuration error or crash, and writes the
        // reason to its warning file, not as a WARNING line.
        if !output.status.success() && !log.lines().any(|l| sphinx_line(tree, l).is_some()) {
            bail!(
                "sphinx failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(log.lines().filter_map(|l| sphinx_line(tree, l)).collect())
    }
}

fn relative(tree: &Path, path: &str) -> String {
    Path::new(path)
        .strip_prefix(tree)
        .map_or_else(|_| path.to_string(), |p| p.display().to_string())
}

fn finding(
    check: String,
    severity: Severity,
    file: String,
    line: Option<u32>,
    message: &str,
) -> Finding {
    Finding {
        tool: Tool::Docs,
        check,
        severity,
        patch: None,
        file: Some(file),
        line,
        message: message.trim().to_string(),
    }
}

/// `/tree/documentation/x.rst:12: WARNING: undefined label: 'foo' [ref.ref]`
fn sphinx_line(tree: &Path, line: &str) -> Option<Finding> {
    let (place, rest) = line
        .split_once(": WARNING: ")
        .or_else(|| line.split_once(": ERROR: "))?;
    let severity = if line.contains(": ERROR: ") {
        Severity::Error
    } else {
        Severity::Warning
    };
    let (path, number) = match place.rsplit_once(':') {
        Some((path, n)) if n.parse::<u32>().is_ok() => (path, n.parse().ok()),
        _ => (place, None),
    };
    let message = rest.rsplit_once(" [").map_or(rest, |(m, _)| m);
    Some(finding(
        "sphinx".into(),
        severity,
        relative(tree, path),
        number,
        message,
    ))
}

/// `/tree/documentation/x.rst:4: trailing whitespace (trailing-whitespace)`
fn sphinx_lint_line(tree: &Path, line: &str) -> Option<Finding> {
    let (place, rest) = line.split_once(": ")?;
    let (path, number) = place.rsplit_once(':')?;
    let (message, check) = rest.rsplit_once(" (")?;
    let check = format!("sphinx-lint.{}", check.trim_end_matches(')'));
    Some(finding(
        check,
        Severity::Warning,
        relative(tree, path),
        number.parse().ok(),
        message,
    ))
}

/// `WARNING: path[:line[:col]]: message` from the yocto-docs tools, whose
/// paths may be relative to the docs directory.
fn script_line(tree: &Path, dir: &Path, script: &str, line: &str) -> Option<Finding> {
    let rest = line.strip_prefix("WARNING: ")?;
    let (place, message) = rest.split_once(": ")?;
    let mut parts = place.split(':');
    let path = parts.next()?;
    let number = parts.next().and_then(|n| n.parse().ok());
    let absolute = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        dir.join(path)
    };
    let file = relative(tree, &absolute.display().to_string());
    Some(finding(
        script.to_string(),
        Severity::Warning,
        file,
        number,
        message,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TREE: &str = "/t";

    #[test]
    fn parses_sphinx_warnings() {
        let f = sphinx_line(
            Path::new(TREE),
            "/t/documentation/ref-manual/variables.rst:120: WARNING: undefined label: 'foo' [ref.ref]",
        )
        .unwrap();
        assert_eq!(
            f.file.as_deref(),
            Some("documentation/ref-manual/variables.rst")
        );
        assert_eq!(f.line, Some(120));
        assert_eq!(f.message, "undefined label: 'foo'");
        assert_eq!(f.severity, Severity::Warning);
        let no_line = sphinx_line(
            Path::new(TREE),
            "/t/documentation/index.rst: WARNING: document isn't included in any toctree",
        )
        .unwrap();
        assert_eq!(no_line.line, None);
        assert!(sphinx_line(Path::new(TREE), "Running Sphinx v9.2.0").is_none());
    }

    #[test]
    fn parses_sphinx_lint() {
        let f = sphinx_lint_line(
            Path::new(TREE),
            "/t/documentation/x.rst:4: role missing closing backtick: ':term:`FOO \\n' (missing-backtick-after-role)",
        )
        .unwrap();
        assert_eq!(f.check, "sphinx-lint.missing-backtick-after-role");
        assert_eq!(f.line, Some(4));
        assert_eq!(f.file.as_deref(), Some("documentation/x.rst"));
    }

    #[test]
    fn parses_yocto_docs_tools() {
        let dir = Path::new("/t/documentation");
        let f = script_line(
            Path::new(TREE),
            dir,
            "check-confusables",
            "WARNING: /t/documentation/x.rst:4:3: non-ASCII character U+201C should be replaced with '\"'",
        )
        .unwrap();
        assert_eq!(
            (f.file.as_deref(), f.line),
            (Some("documentation/x.rst"), Some(4))
        );
        let g = script_line(
            Path::new(TREE),
            dir,
            "check-glossaries",
            "WARNING: ref-manual/variables.rst: entries are not properly sorted:",
        )
        .unwrap();
        assert_eq!(
            g.file.as_deref(),
            Some("documentation/ref-manual/variables.rst")
        );
        assert_eq!(g.line, None);
    }

    #[test]
    fn refuses_to_run_changed_build_code() {
        assert!(runs_code("documentation", "documentation/conf.py"));
        assert!(runs_code(
            "documentation",
            "documentation/tools/check-glossaries"
        ));
        assert!(!runs_code(
            "documentation",
            "documentation/dev-manual/start.rst"
        ));
        assert!(!runs_code("documentation", "meta/lib/oe/utils.py"));
    }
}
