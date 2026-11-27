//! Layer 2 of model review (designs/PHASE3.md §4): the context a model
//! gets for one patch, gathered in the check container from the commit
//! `git am` made, with no model and no network.
//!
//! Everything here comes from a patch, so it is untrusted: paths are
//! resolved inside the tree only, symlinks and submodules are skipped, and
//! names reach git as arguments after `-e` or `--`, never through a shell.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::git;

/// One file of context, cut at a line boundary.
const FILE_CAP: usize = 12 * 1024;
/// The cover letter and the series' subjects.
const SERIES_CAP: usize = 4 * 1024;
/// All context items of one patch, the diff not counted.
const CONTEXT_CAP: usize = 48 * 1024;
/// `git grep` hits kept per name.
const GREP_HITS: usize = 10;
/// A name with more hits than this says nothing about the patch.
const TOO_COMMON: u64 = 150;
/// Changed names searched for, most specific first.
const NAMES: usize = 12;
const CUT: &str = "[... cut at the per-file cap ...]\n";

/// The layer 2 context of one patch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PatchContext {
    pub index: u32,
    pub diff: String,
    pub items: Vec<Item>,
    /// `kind:path` of items left out at the per-patch cap.
    pub dropped: Vec<String>,
    /// Line counts of every file the request names, for validating a
    /// model's file and line (§5.3).
    pub lines: BTreeMap<String, u32>,
    /// The commit git am made of the patch, for layer 4's tools (§6).
    /// Absent in contexts queued before it was recorded.
    #[serde(default)]
    pub commit: String,
    #[serde(skip)]
    pub size: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub kind: String,
    pub path: String,
    pub text: String,
}

impl PatchContext {
    fn add(&mut self, kind: &str, path: &str, text: &str) {
        if self.items.iter().any(|i| i.kind == kind && i.path == path) {
            return;
        }
        let text = cap(text, FILE_CAP);
        if self.size + text.len() > CONTEXT_CAP {
            self.dropped.push(format!("{kind}:{path}"));
            return;
        }
        self.size += text.len();
        self.items.push(Item {
            kind: kind.into(),
            path: path.into(),
            text,
        });
    }

    fn file(&mut self, path: &str, text: &str) {
        self.lines
            .entry(path.into())
            .or_insert(text.lines().count() as u32);
    }
}

/// The cover letter and the subjects of the other patches, so a patch is
/// reviewed as part of its series (a dependency named in the cover letter,
/// a caller changed in a later patch). Nothing for a single patch.
pub fn add_series(ctx: &mut PatchContext, series: &crate::series::Series) {
    if series.patches.len() < 2 && series.cover.is_none() {
        return;
    }
    let mut text = String::new();
    if let Some(cover) = &series.cover {
        text.push_str(&format!(
            "Cover letter: {}\n\n{}\n\n",
            cover.subject,
            cover.commit_message()
        ));
    }
    text.push_str("Patches of the series:\n");
    for p in &series.patches {
        let mark = if p.index == ctx.index {
            " (this patch)"
        } else {
            ""
        };
        text.push_str(&format!("{}/{} {}{mark}\n", p.index, p.total, p.summary()));
    }
    // The series comes first, and outside the per-patch cap: it is short
    // once cut.
    ctx.items.insert(
        0,
        Item {
            kind: "series".into(),
            path: String::new(),
            text: cap(&text, SERIES_CAP),
        },
    );
}

/// The documentation entries a patch's changed lines name.
const DOCS_CAP: usize = 8 * 1024;

/// The Yocto and BitBake documentation as bench/phase3/docs_index.py
/// writes it: name to plain text.
#[derive(Debug, Default, Deserialize)]
pub struct Docs {
    #[serde(default)]
    variables: BTreeMap<String, String>,
    #[serde(default)]
    classes: BTreeMap<String, String>,
    #[serde(default)]
    flags: BTreeMap<String, String>,
}

impl Docs {
    pub fn load(path: &Path) -> Result<Docs> {
        let text =
            std::fs::read(path).map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        serde_json::from_slice(&text)
            .map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))
    }
}

/// The documentation of the task flags, classes and variables the patch's
/// changed lines name, in that order, so a finding rests on what the
/// manual says rather than on the model's memory (designs/PHASE3.md §4).
/// Added on the host, outside the per-patch cap, at most DOCS_CAP; again
/// for a context that has it already does nothing.
pub fn add_docs(ctx: &mut PatchContext, docs: &Docs) {
    if ctx.items.iter().any(|i| i.kind == "docs") {
        return;
    }
    let mut flags = Vec::new();
    let mut classes = Vec::new();
    let mut variables = Vec::new();
    let push = |list: &mut Vec<String>, name: &str| {
        if !list.iter().any(|n| n == name) {
            list.push(name.to_string());
        }
    };
    for line in ctx.diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/")
            && let Some(name) = path.trim_end().strip_suffix(".bbclass")
        {
            push(&mut classes, name.rsplit('/').next().unwrap_or(name));
            continue;
        }
        let Some(code) = line
            .strip_prefix('+')
            .or(line.strip_prefix('-'))
            .filter(|_| !line.starts_with("+++") && !line.starts_with("---"))
        else {
            continue;
        };
        let words = code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'));
        let mut after_inherit = false;
        for w in words.filter(|w| !w.is_empty()) {
            if after_inherit {
                push(&mut classes, w);
            } else if w.len() > 2 && w.starts_with(|c: char| c.is_ascii_uppercase()) {
                push(&mut variables, w);
            }
            after_inherit |= w == "inherit" || w == "inherit_defer";
        }
        for flag in code.split('[').skip(1).filter_map(|s| s.split(']').next()) {
            push(&mut flags, flag);
        }
    }
    let mut text = String::new();
    let entries = flags
        .iter()
        .filter_map(|n| Some((format!("[{n}] task flag"), docs.flags.get(n)?)))
        .chain(
            classes
                .iter()
                .filter_map(|n| Some((format!("{n} class"), docs.classes.get(n)?))),
        )
        .chain(
            variables
                .iter()
                .filter_map(|n| Some((n.clone(), docs.variables.get(n)?))),
        );
    for (name, entry) in entries {
        let block = format!("{name}: {entry}\n\n");
        if text.len() + block.len() > DOCS_CAP {
            break;
        }
        text.push_str(&block);
    }
    if !text.is_empty() {
        ctx.items.push(Item {
            kind: "docs".into(),
            path: String::new(),
            text,
        });
    }
}

/// `text` cut to at most `limit` bytes at a line boundary, marked as cut.
fn cap(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let head = &text.as_bytes()[..limit];
    let end = head.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    // A newline is ASCII, so `end` is a character boundary.
    format!("{}{CUT}", &text[..end])
}

/// `path` normalized inside the tree, or None if it is absolute, empty or
/// climbs out of it.
pub(crate) fn inside(path: &str) -> Option<String> {
    if path.starts_with('/') || path.contains(['\0', '\n', '\\']) {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The text of a regular file at `rev`, or None if it is missing, outside
/// the tree, a symlink or a submodule.
pub(crate) fn blob(tree: &Path, rev: &str, path: &str) -> Option<String> {
    let path = inside(path)?;
    let entry = git::run(
        tree,
        &["--literal-pathspecs", "ls-tree", "-z", rev, "--", &path],
    )
    .ok()?;
    // <mode> SP <type> SP <oid> TAB <path>
    let (meta, name) = entry.trim_end_matches('\0').split_once('\t')?;
    let mut meta = meta.split(' ');
    let (mode, kind, oid) = (meta.next()?, meta.next()?, meta.next()?);
    if name != path || kind != "blob" || !matches!(mode, "100644" | "100755") {
        return None;
    }
    let out = git::command(tree)
        .args(["cat-file", "blob", oid])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Raw stdout of git, which may hold anything a patch put in the tree.
pub(crate) fn git_out(tree: &Path, args: &[&str]) -> Result<String> {
    let out = git::command(tree).args(args).output()?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The context of the patch `git am` committed as `commit` in `tree`.
pub fn gather(tree: &Path, index: u32, commit: &str) -> Result<PatchContext> {
    let diff = git_out(
        tree,
        &["format-patch", "-1", "--stdout", "--no-signature", commit],
    )?;
    let status = git_out(
        tree,
        &["show", "--format=", "--name-status", "-M", "-z", commit],
    )?;
    let mut touched = Vec::new();
    let mut renames = Vec::new();
    let mut fields = status.split('\0').filter(|f| !f.is_empty());
    while let Some(code) = fields.next() {
        if code.starts_with(['R', 'C']) {
            let (Some(old), Some(new)) = (fields.next(), fields.next()) else {
                break;
            };
            if code.starts_with('R') {
                renames.push(old.to_string());
            }
            touched.push(new.to_string());
        } else if let Some(path) = fields.next()
            && code != "D"
        {
            touched.push(path.to_string());
        }
    }

    let mut ctx = PatchContext {
        index,
        diff,
        commit: commit.to_string(),
        ..Default::default()
    };
    let mut texts = BTreeMap::new();
    for path in &touched {
        if let Some(text) = blob(tree, commit, path) {
            ctx.file(path, &text);
            texts.insert(path.clone(), text);
        }
    }
    const META: [&str; 6] = [".bb", ".bbappend", ".inc", ".bbclass", ".conf", ".py"];
    for (path, text) in &texts {
        if META.iter().any(|e| path.ends_with(e)) {
            ctx.add("file", path, text);
        }
    }

    for (path, text) in &texts {
        if ![".bb", ".bbappend", ".inc"]
            .iter()
            .any(|e| path.ends_with(e))
        {
            continue;
        }
        let subs = recipe_names(path);
        for name in directives(text, &["require", "include"]) {
            if let Some(p) = resolve(tree, commit, path, name, &subs)
                && !ctx.lines.contains_key(&p)
                && let Some(t) = blob(tree, commit, &p)
            {
                ctx.file(&p, &t);
                ctx.add("require", &p, &t);
            }
        }
        for line in directives(text, &["inherit"]) {
            for class in line.split_whitespace().filter(|c| !c.contains('$')) {
                if let Some((p, t)) = find_class(tree, commit, class)
                    && !ctx.lines.contains_key(&p)
                {
                    ctx.file(&p, &t);
                    ctx.add("class", &p, &t);
                }
            }
        }
    }

    for name in changed_names(&ctx.diff.clone()) {
        if too_common(tree, commit, &name) {
            continue;
        }
        let hits = grep(tree, commit, &name, &touched);
        if hits.is_empty() {
            continue;
        }
        for hit in &hits {
            if let Some(path) = hit_path(hit)
                && !ctx.lines.contains_key(path)
                && let Some(t) = blob(tree, commit, path)
            {
                ctx.file(path, &t);
            }
        }
        ctx.add("grep", &name, &hits.join("\n--\n"));
    }

    for old in renames.iter().filter(|o| o.ends_with(".bb")) {
        if let Some(t) = blob(tree, &format!("{commit}^"), old) {
            ctx.add("previous-version", old, &t);
        }
    }
    Ok(ctx)
}

/// What each `require`/`include` (or `inherit`) line names.
fn directives<'a>(text: &'a str, words: &[&str]) -> Vec<&'a str> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            words.iter().find_map(|w| {
                let rest = line.strip_prefix(w)?;
                rest.starts_with([' ', '\t']).then(|| rest.trim())
            })
        })
        .map(|rest| {
            if words.contains(&"inherit") {
                rest
            } else {
                rest.split_whitespace().next().unwrap_or("")
            }
        })
        .filter(|r| !r.is_empty())
        .collect()
}

/// `${PN}` and friends from a recipe's file name.
fn recipe_names(path: &str) -> [(&'static str, String); 4] {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let (pn, pv) = stem.split_once('_').unwrap_or((stem, ""));
    [
        ("${PN}", pn.into()),
        ("${BPN}", pn.into()),
        ("${PV}", pv.into()),
        ("${BP}", format!("{pn}-{pv}")),
    ]
}

/// Where a `require` from `from` points, tried beside it, under `meta/`
/// and from the top, as BitBake's search path would for oe-core.
fn resolve(
    tree: &Path,
    rev: &str,
    from: &str,
    name: &str,
    subs: &[(&str, String)],
) -> Option<String> {
    let mut name = name.to_string();
    for (k, v) in subs {
        name = name.replace(k, v);
    }
    if name.contains("${") {
        return None;
    }
    let dir = from.rsplit_once('/').map_or("", |(d, _)| d);
    [format!("{dir}/{name}"), format!("meta/{name}"), name]
        .iter()
        .filter_map(|c| inside(c))
        .find(|c| blob(tree, rev, c).is_some())
}

fn find_class(tree: &Path, rev: &str, name: &str) -> Option<(String, String)> {
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.+".contains(c))
    {
        return None;
    }
    ["meta/classes-recipe", "meta/classes-global", "meta/classes"]
        .iter()
        .map(|d| format!("{d}/{name}.bbclass"))
        .find_map(|p| blob(tree, rev, &p).map(|t| (p, t)))
}

fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The function a changed line defines: `foo(`, `def foo(`, `python foo (`.
fn defined(body: &str) -> Option<&str> {
    let mut rest = body.trim_start();
    for kw in ["python", "def"] {
        if let Some(r) = rest.strip_prefix(kw)
            && r.starts_with(char::is_whitespace)
        {
            rest = r.trim_start();
        }
    }
    let end = rest.find(|c: char| !word(c)).unwrap_or(rest.len());
    let name = &rest[..end];
    let first = name.chars().next()?;
    (!first.is_ascii_digit() && name.is_ascii() && rest[end..].trim_start().starts_with('('))
        .then_some(name)
}

/// The variable a changed line assigns: `FOO = `, `FOO:append:x += `.
fn assigned(body: &str) -> Option<&str> {
    let mut rest = body.trim_start();
    if let Some(r) = rest.strip_prefix("export")
        && r.starts_with(char::is_whitespace)
    {
        rest = r.trim_start();
    }
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
        return None;
    }
    let mut after = &rest[end..];
    if let Some(r) = after.strip_prefix(':') {
        let n = r
            .find(|c: char| !(word(c) || "${}-:".contains(c)))
            .unwrap_or(r.len());
        after = &r[n..];
    }
    let after = after.trim_start();
    ["??=", "?=", ":=", "+=", "=+", ".=", "=.", "="]
        .iter()
        .any(|op| after.starts_with(op))
        .then_some(name)
}

/// Lower-case words of four or more characters followed by `(`.
fn calls(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find(word) {
        let tail = &rest[start..];
        let end = tail.find(|c: char| !word(c)).unwrap_or(tail.len());
        let w = &tail[..end];
        if w.len() >= 4
            && w.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
            && w.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && tail[end..].trim_start().starts_with('(')
        {
            out.push(w);
        }
        rest = &tail[end..];
    }
    out
}

/// Names the diff adds or changes, most specific first: functions, then
/// variables, then calls. Lines of added patch files are not code.
fn changed_names(diff: &str) -> Vec<String> {
    const SKIP: [&str; 24] = [
        "python",
        "self",
        "print",
        "range",
        "open",
        "join",
        "split",
        "replace",
        "format",
        "append",
        "startswith",
        "endswith",
        "exists",
        "getVar",
        "setVar",
        "appendVar",
        "contains",
        "remove",
        "items",
        "values",
        "keys",
        "isinstance",
        "sorted",
        "return",
    ];
    let (mut defs, mut vars, mut called) = (Vec::new(), Vec::new(), Vec::new());
    let mut code = false;
    for line in diff.lines() {
        if line.starts_with("diff --git") {
            let l = line.trim_end();
            code = !(l.ends_with(".patch") || l.ends_with(".diff"));
            continue;
        }
        if !code || line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        let Some(body) = line.strip_prefix(['+', '-']) else {
            continue;
        };
        defs.extend(defined(body));
        vars.extend(assigned(body));
        called.extend(calls(line));
    }
    let mut seen: Vec<String> = Vec::new();
    for n in defs.into_iter().chain(vars).chain(called) {
        if n.len() > 3 && !SKIP.contains(&n) && !seen.iter().any(|s| s == n) {
            seen.push(n.to_string());
        }
    }
    seen.truncate(NAMES);
    seen
}

// ponytail: searches the whole tree, not oe-core's meta/ and scripts/
// only as the bench does, so layers with other top directories work.
pub(crate) const EXCLUDE: &str = ":!meta/files/common-licenses";

fn too_common(tree: &Path, rev: &str, name: &str) -> bool {
    let Ok(out) = git_out(tree, &["grep", "-c", "-w", "-e", name, rev, "--", EXCLUDE]) else {
        return false;
    };
    let total: u64 = out
        .lines()
        .filter_map(|l| l.rsplit_once(':')?.1.parse::<u64>().ok())
        .sum();
    total > TOO_COMMON
}

/// Up to `GREP_HITS` hit groups for `name`, three lines of context each,
/// leaving out files already given whole.
fn grep(tree: &Path, rev: &str, name: &str, touched: &[String]) -> Vec<String> {
    let Ok(out) = git_out(
        tree,
        &[
            "grep", "-z", "-n", "-w", "-C", "3", "-e", name, rev, "--", EXCLUDE,
        ],
    ) else {
        return Vec::new();
    };
    let prefix = format!("{rev}:");
    out.split("\n--\n")
        .map(|g| {
            // -z ends the path and the line number with NUL, so a path with
            // "-<digits>-" in it (CVE-2024-6197.patch) still splits right.
            g.lines()
                .map(|l| l.strip_prefix(&prefix).unwrap_or(l).replacen('\0', ":", 2))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|g| !g.is_empty() && !hit_path(g).is_some_and(|p| touched.iter().any(|t| t == p)))
        .take(GREP_HITS)
        .collect()
}

/// The path of a `git grep -n` line: what comes before `:12:` or `-12-`.
/// The path of a grep hit, `path:line:text` as grep() writes it.
fn hit_path(line: &str) -> Option<&str> {
    let b = line.as_bytes();
    (0..b.len()).find_map(|i| {
        if b[i] != b':' {
            return None;
        }
        let digits = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        let j = i + 1 + digits;
        (digits > 0 && j < b.len() && b[j] == b':').then(|| &line[..i])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    const RECIPE: &str = "meta/recipes-foo/foo/foo_1.0.bb";

    fn docs() -> Docs {
        let map = |kv: &[(&str, &str)]| {
            kv.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        Docs {
            variables: map(&[("DEPENDS", "Build-time deps."), ("LICENSE", "The license.")]),
            classes: map(&[("ptest-cargo", "Cargo test suites."), ("cmake", "CMake.")]),
            flags: map(&[("postfuncs", "Functions after the task.")]),
        }
    }

    #[test]
    fn docs_for_the_changed_lines_flags_first() {
        let mut ctx = PatchContext {
            diff: "--- a/meta/classes-recipe/ptest-cargo.bbclass\n\
                   +++ b/meta/classes-recipe/ptest-cargo.bbclass\n\
                   @@ -1,3 +1,3 @@\n LICENSE = \"MIT\"\n\
                   -DEPENDS += \"zlib\"\n\
                   +do_install_ptest_base[postfuncs] += \"do_install_ptest_cargo\"\n\
                   +inherit cmake unknown\n"
                .into(),
            ..Default::default()
        };
        add_docs(&mut ctx, &docs());
        add_docs(&mut ctx, &docs());
        assert_eq!(ctx.items.len(), 1);
        assert_eq!(
            ctx.items[0].text,
            "[postfuncs] task flag: Functions after the task.\n\n\
             ptest-cargo class: Cargo test suites.\n\n\
             cmake class: CMake.\n\n\
             DEPENDS: Build-time deps.\n\n"
        );
    }

    #[test]
    fn no_docs_when_nothing_matches() {
        let mut ctx = PatchContext {
            diff: "+++ b/README\n+Some text\n".into(),
            ..Default::default()
        };
        add_docs(&mut ctx, &docs());
        assert!(ctx.items.is_empty());
    }

    fn copy(from: &Path, to: &Path) {
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let dest = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                fs::create_dir_all(&dest).unwrap();
                copy(&entry.path(), &dest);
            } else {
                fs::copy(entry.path(), dest).unwrap();
            }
        }
    }

    fn commit(tree: &Path, message: &str) {
        let mut add = git::command(tree);
        assert!(add.args(["add", "-A"]).status().unwrap().success());
        let mut cmd = git::command(tree);
        cmd.args(["-c", "user.name=t", "-c", "user.email=t@x", "commit", "-qm"])
            .arg(message)
            .envs(git::COMMITTER);
        assert!(cmd.status().unwrap().success());
    }

    /// The fixture layer committed as a base, in a fresh repository.
    fn layer() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let tree = dir.path().to_path_buf();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/context-layer");
        copy(&fixture, &tree);
        std::os::unix::fs::symlink("foo.inc", tree.join("meta/recipes-foo/foo/link.inc")).unwrap();
        git::run(&tree, &["init", "-q"]).unwrap();
        commit(&tree, "base");
        (dir, tree)
    }

    fn append(tree: &Path, path: &str, text: &str) {
        let path = tree.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = fs::read_to_string(&path).unwrap_or_default();
        fs::write(path, old + text).unwrap();
    }

    fn item<'a>(ctx: &'a PatchContext, kind: &str, path: &str) -> Option<&'a Item> {
        ctx.items.iter().find(|i| i.kind == kind && i.path == path)
    }

    #[test]
    fn resolves_requires_and_classes_inside_the_tree_only() {
        let (_dir, tree) = layer();
        append(&tree, RECIPE, "FOO_FEATURES = \"c\"\n");
        commit(&tree, "foo: set features");
        let ctx = gather(&tree, 1, "HEAD").unwrap();

        assert!(ctx.diff.contains("+FOO_FEATURES = \"c\""));
        assert!(item(&ctx, "file", RECIPE).is_some());
        assert!(item(&ctx, "require", "meta/recipes-foo/foo/foo.inc").is_some());
        // ${BPN} resolves from the file name.
        assert!(item(&ctx, "require", "meta/recipes-foo/foo/foo-common.inc").is_some());
        assert!(item(&ctx, "class", "meta/classes-recipe/myclass.bbclass").is_some());
        // Neither the escaping nor the absolute path, nor the symlink.
        for path in ctx.lines.keys().chain(ctx.items.iter().map(|i| &i.path)) {
            assert!(
                !path.contains("passwd") && !path.ends_with("link.inc"),
                "{path}"
            );
        }
        // The changed variable is searched for, outside the touched file.
        let grep = item(&ctx, "grep", "FOO_FEATURES").expect("no grep item");
        assert!(grep.text.contains("myclass.bbclass"), "{}", grep.text);
        assert!(!grep.text.contains("foo_1.0.bb"), "{}", grep.text);
        assert_eq!(ctx.lines["meta/classes-recipe/myclass.bbclass"], 3);
    }

    #[test]
    fn cuts_a_large_file_at_a_line_and_drops_past_the_patch_cap() {
        let (_dir, tree) = layer();
        let line = "# padding padding padding padding padding padding padding\n";
        let big = line.repeat(FILE_CAP / line.len() + 50);
        for i in 0..6 {
            append(&tree, &format!("meta/recipes-foo/foo/big{i}.inc"), &big);
            append(&tree, RECIPE, &format!("require big{i}.inc\n"));
        }
        commit(&tree, "foo: pad");
        let ctx = gather(&tree, 1, "HEAD").unwrap();

        // Touched, so given as a file rather than as a require.
        let first = item(&ctx, "file", "meta/recipes-foo/foo/big0.inc").unwrap();
        assert!(
            first.text.ends_with(&format!("\n{CUT}")),
            "not marked as cut"
        );
        assert!(first.text.len() <= FILE_CAP + CUT.len());
        let body = first.text.strip_suffix(CUT).unwrap();
        assert!(
            body.lines().all(|l| format!("{l}\n") == line),
            "cut mid-line"
        );

        let total: usize = ctx.items.iter().map(|i| i.text.len()).sum();
        assert!(total <= CONTEXT_CAP, "{total}");
        assert!(
            ctx.dropped.iter().any(|d| d.ends_with("big5.inc")),
            "{:?}",
            ctx.dropped
        );
    }

    #[test]
    fn a_symlinked_touched_file_is_not_read() {
        let (_dir, tree) = layer();
        std::os::unix::fs::symlink("/etc/hostname", tree.join("meta/host.conf")).unwrap();
        commit(&tree, "add a link");
        let ctx = gather(&tree, 1, "HEAD").unwrap();
        assert!(ctx.items.is_empty(), "{:?}", ctx.items);
        assert!(ctx.lines.is_empty());
    }

    #[test]
    fn names_in_an_added_patch_file_are_not_searched() {
        let (_dir, tree) = layer();
        append(
            &tree,
            "meta/recipes-foo/foo/files/fix.patch",
            "--- a/x.c\n+++ b/x.c\n@@ -1 +1 @@\n+patched_call(1);\n+PATCHED_VAR = 1\n",
        );
        append(
            &tree,
            RECIPE,
            "SRC_URI += \"file://fix.patch\"\nmy_task() {\n",
        );
        commit(&tree, "foo: add a fix");
        let ctx = gather(&tree, 1, "HEAD").unwrap();
        let names = changed_names(&ctx.diff);
        assert!(names.contains(&"my_task".to_string()), "{names:?}");
        assert!(
            !names
                .iter()
                .any(|n| n.starts_with("patched") || n == "PATCHED_VAR")
        );
        assert!(item(&ctx, "file", "meta/recipes-foo/foo/files/fix.patch").is_none());
    }

    #[test]
    fn keeps_the_previous_version_of_an_upgraded_recipe() {
        let (_dir, tree) = layer();
        git::run(&tree, &["mv", RECIPE, "meta/recipes-foo/foo/foo_1.1.bb"]).unwrap();
        commit(&tree, "foo: upgrade 1.0 -> 1.1");
        let ctx = gather(&tree, 1, "HEAD").unwrap();
        assert!(item(&ctx, "previous-version", RECIPE).is_some());
        assert!(item(&ctx, "file", "meta/recipes-foo/foo/foo_1.1.bb").is_some());
    }

    #[test]
    fn paths_stay_inside_the_tree() {
        assert_eq!(inside("a/./b/../c").as_deref(), Some("a/c"));
        assert_eq!(inside("a/../../etc/passwd"), None);
        assert_eq!(inside("/etc/passwd"), None);
        assert_eq!(inside(".."), None);
        assert_eq!(inside(""), None);
    }

    #[test]
    fn finds_definitions_variables_and_calls() {
        assert_eq!(defined(" python do_thing () {"), Some("do_thing"));
        assert_eq!(defined("def helper(d):"), Some("helper"));
        assert_eq!(assigned("FOO:append:class-target += \"x\""), Some("FOO"));
        assert_eq!(assigned("export CFLAGS ?= \"\""), Some("CFLAGS"));
        assert_eq!(assigned("  bbwarn \"x\""), None);
        assert_eq!(
            calls("+    x = oe_runmake(d) + Foo(y) + abc(z)"),
            ["oe_runmake"]
        );
        assert_eq!(
            hit_path("meta/a-b/c-1.0.bb:12:  FOO"),
            Some("meta/a-b/c-1.0.bb")
        );
        assert_eq!(hit_path("meta/x.bb:3:FOO"), Some("meta/x.bb"));
        assert_eq!(
            hit_path("meta/recipes-support/curl/curl/CVE-2024-6197.patch:12:+FOO"),
            Some("meta/recipes-support/curl/curl/CVE-2024-6197.patch")
        );
    }
}
