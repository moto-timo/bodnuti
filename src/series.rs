//! A patch series read from mbox files.
//!
//! b4 writes a whole series into one mbox, while `git format-patch` writes one
//! file per patch. Both are accepted: every input file is split into messages,
//! and the messages are ordered by their `n/m` subject counter.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Serialize;

/// One message of a series, kept byte for byte so it can be handed to
/// `git am` and patchtest unchanged.
#[derive(Debug, Clone, Serialize)]
pub struct Patch {
    pub subject: String,
    pub message_id: Option<String>,
    /// The bracketed tags before the summary, e.g. `["scarthgap", "PATCH v2 3/5"]`.
    pub tags: Vec<String>,
    pub version: u32,
    /// 1-based position in the series; 0 for a cover letter.
    pub index: u32,
    pub total: u32,
    #[serde(skip)]
    pub raw: Vec<u8>,
}

impl Patch {
    pub fn is_cover_letter(&self) -> bool {
        self.index == 0 && self.total > 0
    }

    /// Repository paths the patch changes, after the change.
    pub fn files(&self) -> Vec<String> {
        self.changes().into_iter().map(|(_, new)| new).collect()
    }

    /// `(old, new)` path pairs from the `diff --git` lines; they differ for a
    /// rename, which every recipe version upgrade is.
    // ponytail: paths containing " b/" or quoted paths are misread; OE
    // metadata paths do not use them.
    pub fn changes(&self) -> Vec<(String, String)> {
        String::from_utf8_lossy(&self.raw)
            .lines()
            .filter_map(|l| l.strip_prefix("diff --git a/"))
            .filter_map(|l| {
                l.split_once(" b/")
                    .map(|(a, b)| (a.to_string(), b.to_string()))
            })
            .collect()
    }
}

#[derive(Debug, Serialize)]
pub struct Series {
    pub cover: Option<Patch>,
    pub patches: Vec<Patch>,
}

impl Series {
    pub fn load(paths: &[impl AsRef<Path>]) -> Result<Series> {
        let mut messages = Vec::new();
        for path in paths {
            let path = path.as_ref();
            let data =
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            messages.extend(split_mbox(&data));
        }
        Series::from_messages(messages)
    }

    /// Fetches a series from lore by Message-ID or lore URL.
    ///
    /// `b4 mbox` is used rather than `b4 am`, because `b4 am` rewrites
    /// subjects to `[PATCH n/m]` and drops tags such as `[scarthgap]` that
    /// select the target branch. The thread it returns includes replies,
    /// which `from_messages` drops.
    pub fn fetch(b4: &str, id: &str) -> Result<Series> {
        let output = Command::new(b4)
            .args(["--quiet", "mbox", "-o", "-"])
            .arg(id.trim_matches(|c| c == '<' || c == '>'))
            .output()
            .with_context(|| format!("running {b4}"))?;
        if !output.status.success() || output.stdout.is_empty() {
            bail!(
                "{b4} could not fetch {id}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Series::from_messages(split_mbox(&output.stdout))
    }

    fn from_messages(messages: Vec<Vec<u8>>) -> Result<Series> {
        let mut cover = None;
        let mut patches = Vec::new();
        for raw in messages {
            let patch = parse_message(raw)?;
            if !patch.is_cover_letter() && !is_patch(&patch) {
                continue;
            }
            if patch.is_cover_letter() {
                cover = Some(patch);
            } else {
                patches.push(patch);
            }
        }
        if patches.is_empty() {
            bail!("no patches found in the input");
        }
        patches.sort_by_key(|p| p.index);
        Ok(Series { cover, patches })
    }

    /// The tags that may name a target branch, such as `scarthgap` or
    /// `meta-python`. Which of them is a real branch is up to the repository,
    /// so no codename list is kept here.
    pub fn branch_candidates(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for patch in self.cover.iter().chain(&self.patches) {
            for tag in &patch.tags {
                if !tag.to_ascii_uppercase().contains("PATCH") && !out.contains(&tag.as_str()) {
                    out.push(tag);
                }
            }
        }
        out
    }
}

/// A reply, or any message without a diff, is part of the thread but not of
/// the series.
fn is_patch(patch: &Patch) -> bool {
    let subject = patch.subject.to_ascii_lowercase();
    let reply = ["re:", "aw:", "fwd:", "fw:"]
        .iter()
        .any(|p| subject.starts_with(p));
    !reply && patch.raw.windows(12).any(|w| w == b"\ndiff --git ")
}

/// Whether a command-line input names a series on lore rather than a file:
/// a Message-ID, with or without angle brackets, or an https URL.
pub fn is_remote(input: &str) -> bool {
    !Path::new(input).exists() && (input.starts_with("https://") || input.contains('@'))
}

/// Splits an mbox on separator lines that start a message.
fn split_mbox(data: &[u8]) -> Vec<Vec<u8>> {
    let mut messages = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    let mut after_blank = true;
    for line in data.split_inclusive(|&b| b == b'\n') {
        if after_blank && is_separator(line) && !current.is_empty() {
            messages.push(std::mem::take(&mut current));
        }
        current.extend_from_slice(line);
        after_blank = line.iter().all(|b| b.is_ascii_whitespace());
    }
    if current.iter().any(|b| !b.is_ascii_whitespace()) {
        messages.push(current);
    }
    messages
}

/// A separator is `From <sender> <date>`, and every writer bodnuti reads
/// (git format-patch, b4, lore) ends the date with a four-digit year. A body
/// line that merely starts with "From " does not.
// ponytail: matches the writers above, not every mbox dialect; mboxrd
// unescaping (">From ") is left to git am.
fn is_separator(line: &[u8]) -> bool {
    let Some(rest) = line.strip_prefix(b"From ") else {
        return false;
    };
    let text = String::from_utf8_lossy(rest);
    text.split_whitespace()
        .last()
        .is_some_and(|year| year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit()))
}

fn parse_message(raw: Vec<u8>) -> Result<Patch> {
    let text = String::from_utf8_lossy(&raw);
    let subject = header(&text, "Subject").context("message has no Subject header")?;
    let message_id = header(&text, "Message-ID")
        .or_else(|| header(&text, "Message-Id"))
        .map(|id| id.trim_matches(|c| c == '<' || c == '>').to_string());
    let tags = subject_tags(&subject);
    let (version, index, total) = counters(&tags);
    Ok(Patch {
        subject,
        message_id,
        tags,
        version,
        index,
        total,
        raw,
    })
}

/// Returns an unfolded header value from the message's header block.
// ponytail: RFC 2047 encoded words are left encoded; subjects rarely use them.
fn header(text: &str, name: &str) -> Option<String> {
    let mut lines = text.lines().skip_while(|l| l.starts_with("From "));
    let mut value: Option<String> = None;
    for line in lines.by_ref() {
        if line.is_empty() {
            break;
        }
        if let Some(v) = value.as_mut() {
            if line.starts_with([' ', '\t']) {
                v.push(' ');
                v.push_str(line.trim());
                continue;
            }
            break;
        }
        if let Some((key, rest)) = line.split_once(':') {
            if key.eq_ignore_ascii_case(name) {
                value = Some(rest.trim().to_string());
            }
        }
    }
    value
}

/// Collects the leading `[...]` groups of a subject.
fn subject_tags(subject: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut rest = subject.trim_start();
    while let Some(after) = rest.strip_prefix('[') {
        let Some(end) = after.find(']') else { break };
        tags.push(after[..end].trim().to_string());
        rest = after[end + 1..].trim_start();
    }
    tags
}

/// Reads `vN` and `n/m` from the PATCH tag; a lone patch is `1/1`.
fn counters(tags: &[String]) -> (u32, u32, u32) {
    let (mut version, mut index, mut total) = (1, 1, 1);
    for tag in tags
        .iter()
        .filter(|t| t.to_ascii_uppercase().contains("PATCH"))
    {
        for word in tag.split_whitespace() {
            if let Some(v) = word.strip_prefix(['v', 'V']).and_then(|v| v.parse().ok()) {
                version = v;
            } else if let Some((n, m)) = word.split_once('/') {
                if let (Ok(n), Ok(m)) = (n.parse(), m.parse()) {
                    (index, total) = (n, m);
                }
            }
        }
    }
    (version, index, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(subject: &str, body: &str) -> String {
        format!(
            "From 1234 Mon Sep 17 00:00:00 2001\nFrom: A Developer <a@example.org>\nSubject: {subject}\nMessage-Id: <{}@example.org>\n\n{body}\n",
            subject.len()
        )
    }

    #[test]
    fn splits_a_b4_mbox_and_orders_by_counter() {
        let mbox = [
            message(
                "[PATCH v2 2/2] curl: fix ptest",
                "second\ndiff --git a/a b/a\n",
            ),
            message("[PATCH v2 0/2] curl fixes", "cover\n"),
            message(
                "[PATCH v2 1/2] curl: upgrade 8.9.1 -> 8.10.0",
                "From a quoted line stays in the body\ndiff --git a/a b/a\n",
            ),
        ]
        .concat();
        let series = Series::from_messages(split_mbox(mbox.as_bytes())).unwrap();
        assert_eq!(
            series.cover.as_ref().unwrap().subject,
            "[PATCH v2 0/2] curl fixes"
        );
        let subjects: Vec<_> = series.patches.iter().map(|p| p.index).collect();
        assert_eq!(subjects, vec![1, 2]);
        assert!(
            series
                .patches
                .iter()
                .all(|p| p.version == 2 && p.total == 2)
        );
        // A body line starting with "From " that does not follow a blank line
        // must not start a new message.
        assert_eq!(series.patches.len(), 2);
    }

    #[test]
    fn reads_branch_and_layer_tags() {
        let raw = message(
            "[OE-core][scarthgap][PATCH 3/5] openssl: fix CVE-2026-0001",
            "x",
        );
        let patch = parse_message(raw.into_bytes()).unwrap();
        assert_eq!(patch.tags, vec!["OE-core", "scarthgap", "PATCH 3/5"]);
        assert_eq!((patch.version, patch.index, patch.total), (1, 3, 5));
        let series = Series {
            cover: None,
            patches: vec![patch],
        };
        assert_eq!(series.branch_candidates(), vec!["OE-core", "scarthgap"]);
    }

    #[test]
    fn unfolds_a_long_subject_and_strips_message_id_brackets() {
        let raw =
            "From x\nSubject: [PATCH] a very long\n subject line\nMessage-ID: <abc@host>\n\nbody\n";
        let patch = parse_message(raw.as_bytes().to_vec()).unwrap();
        assert_eq!(patch.subject, "[PATCH] a very long subject line");
        assert_eq!(patch.message_id.as_deref(), Some("abc@host"));
        assert_eq!((patch.index, patch.total), (1, 1));
    }

    #[test]
    fn lists_changed_files() {
        let raw = "From x Mon Sep 17 00:00:00 2001\nSubject: [PATCH] foo\n\n---\n\
                   diff --git a/meta/recipes-x/foo/foo_1.0.bb b/meta/recipes-x/foo/foo_1.1.bb\n\
                   similarity index 90%\n\
                   diff --git a/meta/recipes-x/foo/files/fix.patch b/meta/recipes-x/foo/files/fix.patch\n";
        let patch = parse_message(raw.as_bytes().to_vec()).unwrap();
        assert_eq!(
            patch.files(),
            vec![
                "meta/recipes-x/foo/foo_1.1.bb",
                "meta/recipes-x/foo/files/fix.patch"
            ]
        );
    }

    #[test]
    fn drops_replies_from_a_thread() {
        let mbox = [
            message(
                "[scarthgap][PATCH 1/1] dropbear: fix",
                "x\ndiff --git a/a b/a\n",
            ),
            message(
                "Re: [scarthgap][PATCH 1/1] dropbear: fix",
                "> diff --git a/a b/a\nLooks good\n",
            ),
        ]
        .concat();
        let series = Series::from_messages(split_mbox(mbox.as_bytes())).unwrap();
        assert_eq!(series.patches.len(), 1);
        assert_eq!(series.branch_candidates(), vec!["scarthgap"]);
    }

    #[test]
    fn tells_message_ids_from_files() {
        assert!(is_remote("20261023192932.159420-1-a@example.com"));
        assert!(is_remote("<abc@host>"));
        assert!(is_remote(
            "https://lore.kernel.org/openembedded-core/abc@host/"
        ));
        assert!(!is_remote("series.mbx"));
        let file = tempfile::NamedTempFile::new().unwrap();
        assert!(!is_remote(file.path().to_str().unwrap()));
    }

    #[test]
    fn rejects_input_without_patches() {
        let mbox = message("[PATCH 0/1] cover only", "x");
        assert!(Series::from_messages(split_mbox(mbox.as_bytes())).is_err());
    }
}
