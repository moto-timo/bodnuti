//! Three of patchtest's metadata checks that read only the diff and the
//! commit message (designs/DESIGN.md §3.4b).
//!
//! patchtest's metadata suite parses recipes with tinfoil, which runs code
//! from the patch, so bodnuti does not run it. These three of its tests can
//! do without a recipe parse: a line longer than 200 characters, a change
//! to LIC_FILES_CHKSUM that the commit message does not explain with a
//! `License-Update:` line, and a use of the deprecated CVE_CHECK_IGNORE. bodnuti checks every added line rather than
//! copying patchtest's pattern, which matches `+ ` anywhere in a line.

use crate::finding::{Finding, Severity, Tool};
use crate::series::{Patch, Series};

/// patchtest's limit, `patch_max_line_length`.
const MAX_LINE: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Change {
    Added,
    Removed,
}

/// An added or removed line of a diff.
#[derive(Debug, PartialEq)]
struct Line<'a> {
    file: &'a str,
    change: Change,
    /// The line's number in the patched file; for a removed line, the
    /// number of the line that follows where it was.
    number: u32,
    /// The line without its `+` or `-`.
    text: &'a str,
}

/// The added and removed lines of every hunk, read by the counts in the
/// hunk headers, so that an added line starting `++` or a removed one
/// starting `--` is not taken for a file header.
fn changed_lines(diff: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut file: Option<&str> = None;
    let (mut old_left, mut new_left, mut number) = (0u32, 0u32, 0u32);
    for line in diff.lines() {
        if old_left > 0 || new_left > 0 {
            let change = match line.as_bytes().first() {
                Some(b'+') => Some(Change::Added),
                Some(b'-') => Some(Change::Removed),
                // "\ No newline at end of file"
                Some(b'\\') => continue,
                // A context line; mailers may strip its space when it is
                // empty.
                _ => None,
            };
            if change != Some(Change::Added) {
                old_left = old_left.saturating_sub(1);
            }
            if change != Some(Change::Removed) {
                new_left = new_left.saturating_sub(1);
            }
            if let (Some(change), Some(file)) = (change, file) {
                out.push(Line {
                    file,
                    change,
                    number,
                    text: &line[1..],
                });
            }
            if change != Some(Change::Removed) {
                number += 1;
            }
        } else if let Some(rest) = line.strip_prefix("diff --git a/") {
            file = rest.split_once(" b/").map(|(_, b)| b);
        } else if let Some((old, new)) = hunk_header(line) {
            (old_left, new_left, number) = (old.1, new.1, new.0);
        }
    }
    out
}

/// The `(start, count)` of each side of `@@ -a,b +c,d @@`.
fn hunk_header(line: &str) -> Option<((u32, u32), (u32, u32))> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, _) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    Some((range(old)?, range(new)?))
}

fn range(text: &str) -> Option<(u32, u32)> {
    match text.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((text.parse().ok()?, 1)),
    }
}

fn is_metadata(path: &str) -> bool {
    [".bb", ".bbappend", ".inc"]
        .iter()
        .any(|ext| path.ends_with(ext))
}

/// A line of a LIC_FILES_CHKSUM value: its assignment, or a continuation
/// line with a checksum. `;md5=` and `;sha256=` occur nowhere else: SRC_URI
/// uses `sha256sum`.
fn is_license_line(text: &str) -> bool {
    text.trim_start().starts_with("LIC_FILES_CHKSUM")
        || text.contains(";md5=")
        || text.contains(";sha256=")
}

/// An assignment to CVE_CHECK_IGNORE, with or without an override, flag
/// or operator, but not to a longer name that starts with it.
fn is_cve_check_ignore(text: &str) -> bool {
    text.trim_start()
        .strip_prefix("CVE_CHECK_IGNORE")
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
}

/// Releases before Nanbield, where CVE_CHECK_IGNORE is not yet deprecated
/// and patchtest skips the check.
fn allows_cve_check_ignore(release: &str) -> bool {
    matches!(release, "kirkstone" | "dunfell")
}

/// Findings for one patch, for a series that targets `release`.
fn check_patch(patch: &Patch, release: &str) -> Vec<Finding> {
    let text = String::from_utf8_lossy(&patch.decoded()).into_owned();
    let lines = changed_lines(&text);
    let mut findings = Vec::new();

    for line in &lines {
        let length = line.text.chars().count();
        if line.change == Change::Added && !line.file.ends_with(".patch") && length > MAX_LINE {
            findings.push(Finding {
                tool: Tool::Bodnuti,
                check: "bodnuti.metadata.line-length".into(),
                severity: Severity::Error,
                patch: Some(patch.index),
                file: Some(line.file.to_string()),
                line: Some(line.number),
                message: format!(
                    "line is {length} characters long; patchtest fails a patch that adds a line over {MAX_LINE}"
                ),
            });
        }
        if line.change == Change::Added
            && is_metadata(line.file)
            && is_cve_check_ignore(line.text)
            && !allows_cve_check_ignore(release)
        {
            findings.push(Finding {
                tool: Tool::Bodnuti,
                check: "bodnuti.metadata.cve-check-ignore".into(),
                severity: Severity::Error,
                patch: Some(patch.index),
                file: Some(line.file.to_string()),
                line: Some(line.number),
                message: "CVE_CHECK_IGNORE is deprecated; set CVE_STATUS instead".into(),
            });
        }
    }

    let explained = patch
        .commit_message()
        .lines()
        .any(|l| l.starts_with("License-Update:"));
    if explained {
        return findings;
    }
    // A recipe the patch adds has no license to update, and one it deletes
    // none left. An upgrade git did not detect as a rename looks like both,
    // and is not checked.
    let added = patch.added_files();
    let deleted = patch.deleted_files();
    let mut files: Vec<&str> = Vec::new();
    for line in &lines {
        if is_metadata(line.file)
            && !added.iter().chain(&deleted).any(|f| f == line.file)
            && !files.contains(&line.file)
        {
            files.push(line.file);
        }
    }
    for file in files {
        let license = |change| {
            let mut texts: Vec<&str> = lines
                .iter()
                .filter(|l| l.file == file && l.change == change && is_license_line(l.text))
                .map(|l| l.text.trim())
                .collect();
            texts.sort_unstable();
            texts
        };
        let (removed, added) = (license(Change::Removed), license(Change::Added));
        // A value moved but unchanged is not a license change.
        if removed == added {
            continue;
        }
        let first = lines
            .iter()
            .find(|l| l.file == file && is_license_line(l.text))
            .map(|l| l.number);
        findings.push(Finding {
            tool: Tool::Bodnuti,
            check: "bodnuti.metadata.license-update".into(),
            severity: Severity::Error,
            patch: Some(patch.index),
            file: Some(file.to_string()),
            line: first,
            message: "LIC_FILES_CHKSUM changed without a \"License-Update:\" line in the commit message saying why".into(),
        });
    }
    findings
}

/// Lines over 200 characters, LIC_FILES_CHKSUM changes not explained in
/// the commit message, and added CVE_CHECK_IGNORE, in every patch of a
/// series that targets `release`, as oelint-adv names it.
pub fn check(series: &Series, release: &str) -> Vec<Finding> {
    series
        .patches
        .iter()
        .flat_map(|patch| check_patch(patch, release))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const UVLOOP: &str = include_str!("../tests/fixtures/python3-uvloop-enable-ptest.mbx");
    const TIMEDATE: &str = include_str!("../tests/fixtures/libtimedate-perl-upgrade.mbx");

    fn series(mail: &str) -> Series {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), mail).unwrap();
        Series::load(&[file.path()]).unwrap()
    }

    fn checks(findings: &[Finding]) -> Vec<(&str, Option<&str>, Option<u32>)> {
        findings
            .iter()
            .map(|f| (f.check.as_str(), f.file.as_deref(), f.line))
            .collect()
    }

    #[test]
    fn reads_lines_by_the_hunk_counts() {
        let diff = "diff --git a/a.bb b/a.bb\n\
                    --- a/a.bb\n\
                    +++ b/a.bb\n\
                    @@ -3,3 +3,3 @@ X\n \
                    keep\n\
                    --- removed\n\
                    +++ added\n\
                    \n\
                    -- \n\
                    2.47.0\n";
        assert_eq!(
            changed_lines(diff),
            vec![
                Line {
                    file: "a.bb",
                    change: Change::Removed,
                    number: 4,
                    text: "-- removed",
                },
                Line {
                    file: "a.bb",
                    change: Change::Added,
                    number: 4,
                    text: "++ added",
                },
            ]
        );
    }

    #[test]
    fn reports_the_long_line_patchtest_failed() {
        assert_eq!(
            checks(&check(&series(UVLOOP), "latest")),
            vec![(
                "bodnuti.metadata.line-length",
                Some("meta/recipes-devtools/python/python3-uvloop/run-ptest"),
                Some(LONG_LINE),
            )]
        );
    }

    /// The number of the long `-k` line in the new `run-ptest`.
    const LONG_LINE: u32 = 11;

    #[test]
    fn a_wrapped_line_is_fine() {
        let long = UVLOOP
            .lines()
            .find(|l| l.starts_with('+') && l.len() > MAX_LINE)
            .unwrap();
        let wrapped = format!("{} \\\n+    {}", &long[..150], &long[150..]);
        assert!(check(&series(&UVLOOP.replace(long, &wrapped)), "latest").is_empty());
    }

    #[test]
    fn reports_the_license_change_patchtest_failed() {
        assert_eq!(
            checks(&check(&series(TIMEDATE), "latest")),
            vec![(
                "bodnuti.metadata.license-update",
                Some("meta/recipes-extended/perl/libtimedate-perl_2.35.bb"),
                Some(LICENSE_LINE),
            )]
        );
    }

    /// Where the LIC_FILES_CHKSUM line is in the new recipe.
    const LICENSE_LINE: u32 = 9;

    #[test]
    fn a_license_update_tag_explains_the_change() {
        let tagged = TIMEDATE.replacen(
            "\n---\n",
            "\nLicense-Update: README license text moved to LICENSE\n---\n",
            1,
        );
        assert!(check(&series(&tagged), "latest").is_empty());
    }

    #[test]
    fn a_changed_checksum_on_a_continuation_line_counts() {
        let diff = "diff --git a/r.bb b/r.bb\n\
                    --- a/r.bb\n\
                    +++ b/r.bb\n\
                    @@ -1,2 +1,2 @@\n \
                    LIC_FILES_CHKSUM = \"file://A;md5=1 \\\n\
                    -                    file://B;md5=2\"\n\
                    +                    file://B;md5=3\"\n";
        let mail =
            format!("From x Mon Sep 17 00:00:00 2001\nSubject: [PATCH] r: upgrade\n\n---\n{diff}");
        assert_eq!(
            checks(&check(&series(&mail), "latest")),
            vec![("bodnuti.metadata.license-update", Some("r.bb"), Some(2))]
        );
    }

    #[test]
    fn a_new_recipe_or_a_moved_value_is_not_a_change() {
        let new = "diff --git a/r.bb b/r.bb\n\
                   new file mode 100644\n\
                   --- /dev/null\n\
                   +++ b/r.bb\n\
                   @@ -0,0 +1 @@\n\
                   +LIC_FILES_CHKSUM = \"file://A;md5=1\"\n";
        let moved = "diff --git a/r.bb b/r.bb\n\
                     --- a/r.bb\n\
                     +++ b/r.bb\n\
                     @@ -1,2 +1,2 @@\n\
                     -LIC_FILES_CHKSUM = \"file://A;md5=1\"\n \
                     SUMMARY = \"r\"\n\
                     +LIC_FILES_CHKSUM = \"file://A;md5=1\"\n";
        for diff in [new, moved] {
            let mail =
                format!("From x Mon Sep 17 00:00:00 2001\nSubject: [PATCH] r\n\n---\n{diff}");
            assert!(check(&series(&mail), "latest").is_empty());
        }
    }

    /// A recipe patch adding `lines` after the first line of `r.bb`.
    fn adding(lines: &str) -> Series {
        let added: String = lines.lines().map(|l| format!("+{l}\n")).collect();
        let count = lines.lines().count() + 1;
        series(&format!(
            "From x Mon Sep 17 00:00:00 2001\nSubject: [PATCH] r: ignore a CVE\n\n---\n\
             diff --git a/r.bb b/r.bb\n--- a/r.bb\n+++ b/r.bb\n\
             @@ -1 +1,{count} @@\n SUMMARY = \"r\"\n{added}"
        ))
    }

    #[test]
    fn reports_added_cve_check_ignore() {
        let s = adding(
            "CVE_CHECK_IGNORE += \"CVE-2024-12345\"\n\
             CVE_CHECK_IGNORE:append = \" CVE-2024-6\"\n\
             CVE_CHECK_IGNORE_EXTRA = \"x\"\n\
             CVE_STATUS[CVE-2024-7] = \"not-applicable-config: x\"",
        );
        let cve = ("bodnuti.metadata.cve-check-ignore", Some("r.bb"));
        assert_eq!(
            checks(&check(&s, "latest")),
            vec![(cve.0, cve.1, Some(2)), (cve.0, cve.1, Some(3))]
        );
        assert_eq!(check(&s, "scarthgap").len(), 2);
    }

    #[test]
    fn kirkstone_and_dunfell_still_use_cve_check_ignore() {
        let s = adding("CVE_CHECK_IGNORE += \"CVE-2024-12345\"");
        for release in ["kirkstone", "dunfell"] {
            assert!(check(&s, release).is_empty());
        }
    }
}
