//! cppcheck on the C files a series touches (designs/PHASE2.md §5.4).
//!
//! Only cppcheck's `error` and `warning` findings are kept; style,
//! performance and portability are left to the projects. Findings that say
//! cppcheck could not analyse a file are dropped too: pseudo's
//! `ports/*/guts/*.c` are function bodies spliced into generated wrappers,
//! not C files of their own, and every one is a "syntax error" to cppcheck.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::finding::{Checker, Finding, Severity, Tool};

/// One finding per line: file, line, severity, id, message.
const TEMPLATE: &str = "{file}\t{line}\t{severity}\t{id}\t{message}";

/// Findings that mean cppcheck could not parse or preprocess the file.
const NOT_ANALYSED: &[&str] = &[
    "syntaxError",
    "unknownMacro",
    "internalAstError",
    "preprocessorErrorDirective",
    "cppcheckError",
    "internalError",
];

pub struct Cppcheck {
    pub program: String,
}

impl Checker for Cppcheck {
    fn tool(&self) -> Tool {
        Tool::Cppcheck
    }

    fn lint(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        let sources: Vec<&String> = files
            .iter()
            .filter(|f| (f.ends_with(".c") || f.ends_with(".h")) && tree.join(f).is_file())
            .collect();
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let output = Command::new(&self.program)
            .current_dir(tree)
            .args(["--quiet", "--enable=warning"])
            .args([
                "--suppress=missingInclude",
                "--suppress=missingIncludeSystem",
            ])
            .arg(format!("--template={TEMPLATE}"))
            // cppcheck takes no `--`; the `./` keeps a path from the
            // series from being read as an option.
            .args(sources.iter().map(|f| format!("./{f}")))
            .output()
            .with_context(|| format!("running {}", self.program))?;
        // Findings go to stderr; the exit status is 0 with or without them.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            bail!(
                "{} failed: {}{}",
                self.program,
                stderr.trim(),
                String::from_utf8_lossy(&output.stdout).trim()
            );
        }
        Ok(parse(&stderr))
    }
}

fn parse(text: &str) -> Vec<Finding> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(5, '\t');
            let (file, number, severity, id, message) = (
                fields.next()?,
                fields.next()?,
                fields.next()?,
                fields.next()?,
                fields.next()?,
            );
            let severity = match severity {
                "error" => Severity::Error,
                "warning" => Severity::Warning,
                _ => return None,
            };
            if NOT_ANALYSED.contains(&id) {
                return None;
            }
            Some(Finding {
                tool: Tool::Cppcheck,
                check: id.to_string(),
                severity,
                patch: None,
                file: Some(file.trim_start_matches("./").to_string()),
                line: number.parse().ok(),
                message: message.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_errors_and_warnings_it_could_analyse() {
        let out = "pseudo_client.c\t812\twarning\tnullPointerOutOfMemory\tIf memory allocation fails, then there is a possible null pointer dereference: path\n\
                   pseudo_db.c\t90\terror\tnullPointerArithmeticOutOfMemory\tIf memory allocation fails: pointer addition with NULL pointer.\n\
                   ports/linux/guts/open.c\t11\terror\tsyntaxError\tsyntax error\n\
                   pseudo_util.c\t5\tstyle\tvariableScope\tThe scope of the variable 'i' can be reduced.\n";
        let findings = parse(out);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].file.as_deref(), Some("pseudo_client.c"));
        assert_eq!(findings[0].line, Some(812));
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[1].check, "nullPointerArithmeticOutOfMemory");
        assert_eq!(findings[1].severity, Severity::Error);
    }
}
