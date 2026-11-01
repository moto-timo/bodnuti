//! oelint-adv, run on the metadata files a series touches.
//!
//! oelint-adv also reports problems in classes and includes a recipe pulls in,
//! so its raw output says little about the patch. Running it on the base tree
//! and on the patched tree, and keeping only what is new in touched files,
//! is what makes its output attributable to the author.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::finding::{Checker, Finding, Severity, Tool};

pub struct Oelint {
    pub program: String,
    /// An oelint-adv `--release` value: a codename such as `scarthgap`, or
    /// `latest` for master.
    pub release: String,
}

/// oelint-adv checks whose result is not about the series. Each matches a
/// check id or a prefix of one.
/// - `homepageping` fetches HOMEPAGE over the network, so its result varies
///   between runs and always fails without network access;
/// - `bbclass.underscores` fires on the stub recipe that lints a `.inc`
///   file, not on anything in the patch.
const SUPPRESSED: &[&str] = &["oelint.vars.homepageping", "oelint.bbclass.underscores"];

/// oelint-adv checks about layout rather than correctness: their findings
/// are kept apart as style, shown on request and not counted
/// (designs/PHASE2.md §6.1.1). Variable ordering, FILES and SECTION
/// overrides, multi-line indentation and out-of-context variables are
/// oelint-adv style that OE-Core does not follow (25 merged OE-Core
/// commits, October 2026); the rest made up about 470 of the 919 findings
/// on the first 397 published series.
const STYLE: &[&str] = &[
    "oelint.var.order",
    "oelint.var.filesoverride",
    "oelint.var.override",
    "oelint.vars.multilineident",
    "oelint.vars.outofcontext",
    "oelint.vars.notneededspace",
    "oelint.var.suggestedvar",
    "oelint.vars.dependsordered",
    "oelint.tabs.notabs",
    "oelint.var.bbclassextend",
    "oelint.task.docstrings",
    "oelint.newline.consecutive",
];

/// Whether `check` is one of `ids` or below one of them.
fn matches(ids: &[&str], check: &str) -> bool {
    ids.iter().any(|s| {
        check == *s
            || check
                .strip_prefix(s)
                .is_some_and(|rest| rest.starts_with('.'))
    })
}

fn suppressed(check: &str) -> bool {
    matches(SUPPRESSED, check)
}

/// Whether an oelint-adv check is a style check.
pub fn is_style(check: &str) -> bool {
    matches(STYLE, check)
}

/// Whether the file is BitBake metadata, which oelint-adv understands: not
/// a recipe's `file://` payload in `files/`, and a `.conf` only under
/// `conf/`.
pub fn lintable(path: &str) -> bool {
    let dirs: Vec<&str> = path.split('/').rev().skip(1).collect();
    if dirs.contains(&"files") {
        return false;
    }
    if path.ends_with(".conf") {
        return dirs.contains(&"conf");
    }
    [".bb", ".bbappend", ".inc", ".bbclass"]
        .iter()
        .any(|ext| path.ends_with(ext))
}

/// What pulls a `.inc` or `.conf` file into oelint-adv, which checks such a
/// file only through something that includes it: on its own it produces no
/// findings at all.
#[derive(Debug, PartialEq)]
enum Stub {
    /// A recipe, for includes of recipes and classes.
    Recipe,
    /// A distro configuration, for `conf/distro/` files such as
    /// `maintainers.inc`. Through a recipe, every `:pn-<recipe>` override
    /// in them names a recipe other than the stub, and
    /// `oelint.vars.specific` reports each one.
    Distro,
    /// A machine configuration, for `conf/machine/` files.
    Machine,
}

fn stub_for(path: &str) -> Option<Stub> {
    if !(path.ends_with(".inc") || path.ends_with(".conf")) {
        return None;
    }
    Some(if path.contains("conf/distro/") {
        Stub::Distro
    } else if path.contains("conf/machine/") {
        Stub::Machine
    } else {
        Stub::Recipe
    })
}

impl Checker for Oelint {
    fn tool(&self) -> Tool {
        Tool::Oelint
    }

    fn lint(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        self.lint_files(tree, files)
    }
}

impl Oelint {
    /// Lints the given repository-relative files that exist in `tree`.
    ///
    /// Each `.inc` and `.conf` file is linted through a stub that
    /// `require`s it: a distro or machine configuration for files under
    /// `conf/distro/` or `conf/machine/`, else a recipe. The stubs live
    /// outside the tree, so their own findings (missing LICENSE and the
    /// like) are reported under their path and are dropped by the caller's
    /// touched-files filter.
    fn lint_files(&self, tree: &Path, files: &[String]) -> Result<Vec<Finding>> {
        // The stub's require path and oelint-adv's --relpaths must agree on
        // one spelling of the tree. On macOS /var is a symlink to
        // /private/var, and a mismatch turns repository paths into
        // ../../../var/... which then match no touched file.
        let tree = &tree.canonicalize()?;
        let present: Vec<&String> = files
            .iter()
            .filter(|f| lintable(f) && tree.join(f).is_file())
            .collect();
        if present.is_empty() {
            return Ok(Vec::new());
        }
        let stubs = tempfile::tempdir()?;
        let mut recipes = Vec::new();
        let mut confs = Vec::new();
        for (n, file) in present.iter().enumerate() {
            let require = format!("require {}\n", tree.join(file).display());
            let (args, stub) = match stub_for(file) {
                None => (&mut recipes, None),
                Some(Stub::Recipe) => (&mut recipes, Some(format!("bodnuti-stub{n}_1.0.bb"))),
                Some(Stub::Distro) => (
                    &mut confs,
                    Some(format!("conf/distro/bodnuti-stub{n}.conf")),
                ),
                Some(Stub::Machine) => (
                    &mut confs,
                    Some(format!("conf/machine/bodnuti-stub{n}.conf")),
                ),
            };
            if let Some(stub) = stub {
                let stub = stubs.path().join(stub);
                std::fs::create_dir_all(stub.parent().unwrap())?;
                std::fs::write(&stub, require)?;
                args.push(stub.into_os_string());
            }
            args.push(file.into());
        }
        let mut findings = self.run(tree, &[], recipes)?;
        // oelint-adv reads distro and machine configurations only in its
        // `all` mode, which also expands both branches of each conditional,
        // so the same finding can come once per branch.
        let mut seen = std::collections::HashSet::new();
        findings.extend(
            self.run(tree, &["--mode", "all"], confs)?
                .into_iter()
                .filter(|f| {
                    seen.insert((f.check.clone(), f.file.clone(), f.line, f.message.clone()))
                }),
        );
        Ok(findings)
    }

    /// Runs oelint-adv on `args`, from `tree`, with the `extra` options.
    fn run(
        &self,
        tree: &Path,
        extra: &[&str],
        args: Vec<std::ffi::OsString>,
    ) -> Result<Vec<Finding>> {
        if args.is_empty() {
            return Ok(Vec::new());
        }
        let out = tempfile::NamedTempFile::new()?;
        let status = Command::new(&self.program)
            .current_dir(tree)
            .args(["--quiet", "--relpaths", "--release", &self.release])
            .args(extra)
            .args(["--outputformat", "gitlab-codequality", "--output"])
            .arg(out.path())
            .args(args)
            .status()
            .with_context(|| format!("running {}", self.program))?;
        // 0 is clean and 1 means findings; anything else is a usage error or
        // a crash, which must not read as a clean result.
        if !matches!(status.code(), Some(0 | 1)) {
            bail!("{} exited with {status}", self.program);
        }
        let json = std::fs::read_to_string(out.path())?;
        if json.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(parse(&json)?
            .into_iter()
            .filter(|f| !suppressed(&f.check))
            .collect())
    }
}

#[derive(Deserialize)]
struct CodeQuality {
    description: String,
    check_name: String,
    location: Location,
}

#[derive(Deserialize)]
struct Location {
    path: String,
    lines: Lines,
}

#[derive(Deserialize)]
struct Lines {
    begin: u32,
}

/// Parses gitlab-codequality output. The codequality severity scale is coarser
/// than oelint-adv's own, so severity and message are read from the
/// description, which is `path:line:severity:check:message [branch:...]`
/// (`[branch=...]` in the `all` mode).
fn parse(json: &str) -> Result<Vec<Finding>> {
    let entries: Vec<CodeQuality> =
        serde_json::from_str(json).context("oelint-adv output is not codequality JSON")?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let prefix = format!("{}:{}:", e.location.path, e.location.lines.begin);
            let tail = e
                .description
                .strip_prefix(&prefix)
                .unwrap_or(&e.description);
            let (severity, rest) = tail.split_once(':').unwrap_or(("warning", tail));
            let message = rest
                .strip_prefix(&format!("{}:", e.check_name))
                .unwrap_or(rest);
            let message = message.rsplit_once(" [branch").map_or(message, |(m, _)| m);
            Finding {
                tool: Tool::Oelint,
                check: e.check_name,
                severity: match severity {
                    "error" => Severity::Error,
                    "info" => Severity::Info,
                    _ => Severity::Warning,
                },
                patch: None,
                file: Some(e.location.path),
                line: Some(e.location.lines.begin),
                message: message.trim().to_string(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/oelint-codequality.json");

    #[test]
    fn parses_real_output() {
        let findings = parse(FIXTURE).unwrap();
        assert_eq!(findings.len(), 6);
        let append = findings
            .iter()
            .find(|f| f.check == "oelint.vars.inconspaces")
            .unwrap();
        assert_eq!(append.severity, Severity::Error);
        assert_eq!(
            append.file.as_deref(),
            Some("meta/classes-recipe/ptest.bbclass")
        );
        assert_eq!(append.line, Some(94));
        assert!(
            append.message.starts_with("Assignment should be"),
            "{}",
            append.message
        );
        assert!(!append.message.contains("[branch:"));
        assert!(findings.iter().any(|f| f.severity == Severity::Info));
        assert!(findings.iter().any(|f| f.severity == Severity::Warning));
    }

    #[test]
    fn suppression_and_style_match_whole_id_segments() {
        assert!(suppressed("oelint.vars.homepageping"));
        assert!(!suppressed("oelint.var.order.LICENSE"));
        assert!(is_style("oelint.var.order.LICENSE"));
        assert!(is_style("oelint.var.suggestedvar.BUGTRACKER"));
        assert!(!is_style("oelint.var.orderly"));
        assert!(!is_style("oelint.vars.inconspaces"));
        assert!(!is_style("oelint.vars.spacesassignment"));
    }

    #[test]
    fn conf_files_get_a_conf_stub() {
        assert_eq!(stub_for("meta/recipes-x/foo/foo_1.0.bb"), None);
        assert_eq!(stub_for("meta/recipes-x/foo/foo.inc"), Some(Stub::Recipe));
        assert_eq!(
            stub_for("meta/conf/distro/include/maintainers.inc"),
            Some(Stub::Distro)
        );
        assert_eq!(stub_for("conf/distro/poky.conf"), Some(Stub::Distro));
        assert_eq!(
            stub_for("meta/conf/machine/include/x86/arch-x86.inc"),
            Some(Stub::Machine)
        );
        assert_eq!(stub_for("conf/layer.conf"), Some(Stub::Recipe));
    }

    #[test]
    fn parses_all_mode_branch_suffix() {
        let json = r#"[{"description": "meta/conf/distro/include/maintainers.inc:3:error:oelint.vars.inconspaces:Assignment should be 'VAR += \"foo\"' not 'VAR += \" foo\"' [branch=true]", "check_name": "oelint.vars.inconspaces", "location": {"path": "meta/conf/distro/include/maintainers.inc", "lines": {"begin": 3}}}]"#;
        let findings = parse(json).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(
            !findings[0].message.contains("[branch"),
            "{}",
            findings[0].message
        );
    }

    /// Stands in for oelint-adv: logs each run's arguments, one per line
    /// after a `run` line, and reports one finding per run, twice with
    /// different branch suffixes in the `all` mode.
    const FAKE: &str = r#"#!/bin/sh
log="$(dirname "$0")/calls"
out=""; prev=""; all=0
echo run >> "$log"
for a in "$@"; do
  echo "$a" >> "$log"
  [ "$prev" = --output ] && out="$a"
  [ "$prev" = --mode ] && [ "$a" = all ] && all=1
  prev="$a"
done
entry() {
  printf '{"description": "%s:2:warning:oelint.vars.specific:x [branch=%s]", "check_name": "oelint.vars.specific", "location": {"path": "%s", "lines": {"begin": 2}}}' "$1" "$2" "$1"
}
if [ "$all" = 1 ]; then
  f=conf/distro/include/maintainers.inc
  echo "[$(entry $f true), $(entry $f false)]" > "$out"
else
  echo "[$(entry recipes-x/foo/foo_1.0.bb true)]" > "$out"
fi
exit 1
"#;

    #[test]
    fn configs_are_linted_in_their_own_all_mode_run() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("oelint-adv");
        std::fs::write(&fake, FAKE).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let tree = dir.path().join("tree");
        let files = [
            "conf/distro/include/maintainers.inc",
            "conf/machine/include/arch.inc",
            "recipes-x/foo/foo_1.0.bb",
            "recipes-x/foo/foo.inc",
        ];
        for f in files {
            let path = tree.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "A = \"1\"\n").unwrap();
        }
        let oelint = Oelint {
            program: fake.display().to_string(),
            release: "latest".into(),
        };
        let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
        let findings = oelint.lint_files(&tree, &files).unwrap();

        let calls = std::fs::read_to_string(dir.path().join("calls")).unwrap();
        let runs: Vec<Vec<&str>> = calls
            .split("run\n")
            .skip(1)
            .map(|r| r.lines().collect())
            .collect();
        assert_eq!(runs.len(), 2, "{calls}");
        let (recipes, confs) = (&runs[0], &runs[1]);
        assert!(!recipes.contains(&"--mode"), "{calls}");
        assert!(recipes.contains(&"recipes-x/foo/foo_1.0.bb"), "{calls}");
        assert!(recipes.contains(&"recipes-x/foo/foo.inc"), "{calls}");
        assert!(
            recipes
                .iter()
                .any(|a| a.ends_with("_1.0.bb") && a.contains("bodnuti-stub"))
        );
        assert!(confs.windows(2).any(|w| w == ["--mode", "all"]), "{calls}");
        assert!(
            confs.contains(&"conf/distro/include/maintainers.inc"),
            "{calls}"
        );
        assert!(confs.contains(&"conf/machine/include/arch.inc"), "{calls}");
        assert!(
            confs
                .iter()
                .any(|a| a.ends_with("conf/distro/bodnuti-stub0.conf"))
        );
        assert!(
            confs
                .iter()
                .any(|a| a.ends_with("conf/machine/bodnuti-stub1.conf"))
        );
        assert!(!confs.iter().any(|a| a.contains("recipes-x")), "{calls}");

        // One finding from the recipe run, and the all mode's two branch
        // expansions of the same finding counted once.
        let files: Vec<_> = findings
            .iter()
            .map(|f| f.file.as_deref().unwrap())
            .collect();
        assert_eq!(
            files,
            [
                "recipes-x/foo/foo_1.0.bb",
                "conf/distro/include/maintainers.inc"
            ]
        );
    }

    #[test]
    fn only_metadata_is_lintable() {
        assert!(lintable("meta/recipes-x/foo/foo_1.0.bb"));
        assert!(lintable("meta/classes-recipe/ptest.bbclass"));
        assert!(!lintable("meta/recipes-x/foo/files/fix.patch"));
        assert!(!lintable("documentation/index.rst"));
    }

    #[test]
    fn recipe_payload_is_not_lintable() {
        assert!(lintable("conf/layer.conf"));
        assert!(lintable("meta/conf/distro/include/maintainers.inc"));
        assert!(lintable("meta/conf/machine/qemux86-64.conf"));
        assert!(lintable("meta/recipes-x/foo/foo.inc"));
        assert!(!lintable("recipes-demo/iot-gateway/files/nginx.conf"));
        assert!(!lintable(
            "recipes-demo/iot-gateway/files/mqtt-auth/default.conf"
        ));
        assert!(!lintable(
            "meta/recipes-core/systemd/systemd/00-create-volatile.conf"
        ));
        assert!(!lintable("meta/files/wic/common.wks.inc"));
        assert!(!lintable("recipes-x/foo/files/conf/app.conf"));
    }
}
