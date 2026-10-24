//! shellcheck on the shell a series touches.
//!
//! Shell task functions run under `/bin/sh`, which is dash on many build
//! hosts, and scripts shipped to targets often run under BusyBox ash.
//! Bashisms that pass on the author's bash host break there.
//!
//! Two kinds of source are checked:
//! - shell functions in metadata (`do_install() { ... }` in recipes, includes
//!   and classes), passed with everything else blanked out so shellcheck's
//!   line numbers are the file's own;
//! - files with a shell shebang, passed whole.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::finding::{Checker, Finding, Severity, Tool};

pub struct ShellCheck {
    /// The shellcheck command and its leading arguments, e.g. a
    /// `docker run --rm -i koalaman/shellcheck:v0.10.0` invocation.
    pub command: Vec<String>,
}

/// One input for shellcheck.
#[derive(Debug, PartialEq)]
struct Script {
    shell: &'static str,
    text: String,
    /// Shell extracted from metadata, where BitBake variables are expanded
    /// before the shell runs and so are never "assigned" in shell terms.
    from_metadata: bool,
}

fn is_metadata(path: &str) -> bool {
    [".bb", ".bbappend", ".inc", ".bbclass"]
        .iter()
        .any(|ext| path.ends_with(ext))
}

fn shebang_shell(text: &str) -> Option<&'static str> {
    let first = text.lines().next()?.strip_prefix("#!")?;
    let interpreter = first.split_whitespace().last()?;
    match interpreter.rsplit('/').next()? {
        "sh" | "ash" => Some("sh"),
        "dash" => Some("dash"),
        "bash" => Some("bash"),
        _ => None,
    }
}

/// Blanks every line that is not inside a shell function, keeping line
/// numbers. Function headers are renamed, because BitBake names such as
/// `do_install:append` or `pkg_postinst:${PN}` are not valid shell names.
// ponytail: a closing brace is recognised only at the start of a line, which
// is how OE metadata writes it; one-line functions are skipped.
fn shell_functions(text: &str) -> Option<String> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut found = false;
    // The terminator of an open here-document, and whether it was opened
    // with `<<-`, which allows leading tabs before it. Its body may start a
    // line with `}` (device tree source does) without ending the function.
    let mut heredoc: Option<(String, bool)> = None;
    for (n, line) in text.lines().enumerate() {
        if inside {
            if let Some((end, tabs)) = &heredoc {
                let candidate = if *tabs {
                    line.trim_start_matches('\t')
                } else {
                    line
                };
                if candidate == end {
                    heredoc = None;
                }
                out.push(line.to_string());
            } else if line.starts_with('}') {
                inside = false;
                out.push("}".to_string());
            } else {
                heredoc = heredoc_start(line);
                out.push(without_inline_python(line));
            }
            continue;
        }
        let header = line
            .trim_start()
            .trim_start_matches("fakeroot ")
            .trim_start();
        let is_function = header.ends_with('{')
            && header.contains("()")
            && !header.starts_with("python")
            && !header.starts_with("def ");
        if is_function {
            inside = true;
            found = true;
            out.push(format!("bb_function_{n}() {{"));
        } else {
            out.push(String::new());
        }
    }
    found.then(|| out.join("\n") + "\n")
}

/// The terminator of a here-document a line opens, if any.
// ponytail: one here-document per line, which is how OE metadata uses them.
fn heredoc_start(line: &str) -> Option<(String, bool)> {
    let (_, rest) = line.split_once("<<")?;
    if rest.starts_with('<') {
        return None; // a <<< here-string has no terminator
    }
    let (tabs, rest) = match rest.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, rest),
    };
    let word = rest.split_whitespace().next()?;
    let word = word.trim_matches(|c| c == '\'' || c == '"');
    (!word.is_empty()).then(|| (word.to_string(), tabs))
}

/// Replaces BitBake inline Python, `${@...}`, with a plain word. BitBake
/// expands it before the shell runs, and shellcheck would otherwise read
/// `"${@...}"` as the shell's `"$@"` array.
fn without_inline_python(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("${@") {
        out.push_str(&rest[..start]);
        let mut depth = 0;
        let mut end = rest.len();
        for (i, c) in rest[start..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push_str("${BB_INLINE_PYTHON}");
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn script_for(path: &str, text: &str) -> Option<Script> {
    if is_metadata(path) {
        let text = shell_functions(text)?;
        return Some(Script {
            shell: "sh",
            text,
            from_metadata: true,
        });
    }
    let shell = shebang_shell(text)?;
    Some(Script {
        shell,
        text: text.to_string(),
        from_metadata: false,
    })
}

#[derive(Deserialize)]
struct Output {
    comments: Vec<Comment>,
}

#[derive(Deserialize)]
struct Comment {
    line: u32,
    code: u32,
    level: String,
    message: String,
}

impl Checker for ShellCheck {
    fn tool(&self) -> Tool {
        Tool::Shellcheck
    }

    fn lint(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        self.lint_files(tree, files)
    }
}

impl ShellCheck {
    /// Checks the given repository-relative files that exist in `tree`.
    fn lint_files(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();
        for file in files {
            let Ok(text) = std::fs::read_to_string(tree.join(file)) else {
                continue;
            };
            if let Some(script) = script_for(file, &text) {
                findings.extend(self.run(file, &script)?);
            }
        }
        Ok(findings)
    }

    fn run(&self, file: &str, script: &Script) -> Result<Vec<Finding>> {
        let (program, leading) = self
            .command
            .split_first()
            .context("empty --shellcheck command")?;
        let mut cmd = Command::new(program);
        cmd.args(leading).args([
            "--format=json1",
            "--severity=warning",
            "--shell",
            script.shell,
        ]);
        if script.from_metadata {
            cmd.arg("--exclude=SC2154");
        }
        let mut child = cmd
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("running {}", self.command.join(" ")))?;
        child
            .stdin
            .take()
            .context("shellcheck stdin")?
            .write_all(script.text.as_bytes())?;
        let output = child.wait_with_output()?;
        // 0 is clean and 1 means comments; 2-4 are parse, usage and runtime
        // errors that must not read as a clean result.
        if !matches!(output.status.code(), Some(0 | 1)) {
            bail!(
                "shellcheck failed on {file}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        parse(file, &String::from_utf8_lossy(&output.stdout))
    }
}

fn parse(file: &str, json: &str) -> Result<Vec<Finding>> {
    let output: Output = serde_json::from_str(json).context("shellcheck output is not json1")?;
    // SC1xxx errors mean shellcheck could not parse the input and checked
    // nothing in it. The same error in the base would cancel out in the
    // comparison, so it is reported instead of silently passing.
    if let Some(c) = output
        .comments
        .iter()
        .find(|c| (1000..2000).contains(&c.code) && c.level == "error")
    {
        bail!(
            "shellcheck could not parse {file} at line {}: {} (SC{}); its shell was not checked",
            c.line,
            c.message,
            c.code
        );
    }
    Ok(output
        .comments
        .into_iter()
        .map(|c| Finding {
            tool: Tool::Shellcheck,
            check: format!("SC{}", c.code),
            severity: match c.level.as_str() {
                "error" => Severity::Error,
                "warning" => Severity::Warning,
                _ => Severity::Info,
            },
            patch: None,
            file: Some(file.to_string()),
            line: Some(c.line),
            message: c.message,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECIPE: &str = "SUMMARY = \"x\"\n\
                          do_install:append() {\n\
                          \techo -e \"${FOO}\" > ${D}/x\n\
                          }\n\
                          python do_y() {\n\
                          \tbb.note('x')\n\
                          }\n\
                          fakeroot do_z() {\n\
                          \ttrue\n\
                          }\n";

    #[test]
    fn keeps_shell_functions_on_their_own_lines() {
        let text = shell_functions(RECIPE).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), RECIPE.lines().count());
        assert_eq!(lines[0], "");
        assert_eq!(lines[1], "bb_function_1() {");
        assert_eq!(lines[2], "\techo -e \"${FOO}\" > ${D}/x");
        assert_eq!(lines[3], "}");
        // The python task is blanked, body included.
        assert!(lines[4..7].iter().all(|l| l.is_empty()));
        assert_eq!(lines[7], "bb_function_7() {");
    }

    #[test]
    fn a_brace_inside_a_heredoc_does_not_end_the_function() {
        let class = "do_its() {\n\tcat << EOF > x.its\n/ {\n};\nEOF\n\techo -e x\n}\n";
        let text = shell_functions(class).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[3], "};");
        assert_eq!(lines[5], "\techo -e x");
        assert_eq!(lines[6], "}");
    }

    #[test]
    fn heredoc_terminators() {
        assert_eq!(
            heredoc_start("\tcat << EOF >> f"),
            Some(("EOF".into(), false))
        );
        assert_eq!(heredoc_start("cat <<-'END'"), Some(("END".into(), true)));
        assert_eq!(heredoc_start("grep x <<< \"$v\""), None);
        assert_eq!(heredoc_start("echo hi"), None);
    }

    #[test]
    fn inline_python_becomes_a_word() {
        let line = r#"if [ "${@bb.utils.contains('A', 'b', 'yes', 'no', d)}" = "yes" ]; then ${D}"#;
        assert_eq!(
            without_inline_python(line),
            r#"if [ "${BB_INLINE_PYTHON}" = "yes" ]; then ${D}"#
        );
    }

    #[test]
    fn a_parse_error_is_not_a_clean_result() {
        let json = r#"{"comments":[{"line":4,"code":1073,"level":"error","message":"Couldn't parse this here document."}]}"#;
        let err = parse("x.bbclass", json).unwrap_err();
        assert!(
            err.to_string()
                .contains("could not parse x.bbclass at line 4"),
            "{err}"
        );
    }

    #[test]
    fn metadata_without_shell_is_skipped() {
        assert!(script_for("foo_1.0.bb", "SUMMARY = \"x\"\n").is_none());
    }

    #[test]
    fn scripts_are_chosen_by_shebang() {
        assert_eq!(shebang_shell("#!/bin/sh\necho"), Some("sh"));
        assert_eq!(shebang_shell("#!/usr/bin/env bash\n"), Some("bash"));
        assert_eq!(shebang_shell("#!/usr/bin/python3\n"), None);
        assert!(script_for("files/run.py", "#!/usr/bin/python3\n").is_none());
        let script = script_for("files/weston-start", "#!/bin/sh\necho\n").unwrap();
        assert_eq!(script.shell, "sh");
        assert!(!script.from_metadata);
    }

    #[test]
    fn parses_json1() {
        let json = r#"{"comments":[{"file":"-","line":3,"endLine":3,"column":2,"endColumn":6,"level":"warning","code":3037,"message":"In POSIX sh, echo flags are undefined.","fix":null}]}"#;
        let findings = parse("meta/classes-recipe/uboot-sign.bbclass", json).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, "SC3037");
        assert_eq!(findings[0].line, Some(3));
        assert_eq!(findings[0].severity, Severity::Warning);
    }
}
