//! A patch series read from mbox files.
//!
//! b4 writes a whole series into one mbox, while `git format-patch` writes one
//! file per patch. Both are accepted: every input file is split into messages,
//! and the messages are ordered by their `n/m` subject counter.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// One message of a series, kept byte for byte so it can be handed to
/// `git am` and patchtest unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patch {
    pub subject: String,
    pub message_id: Option<String>,
    /// The bracketed tags before the summary, e.g. `["scarthgap", "PATCH v2 3/5"]`.
    pub tags: Vec<String>,
    pub version: u32,
    /// 1-based position in the series; 0 for a cover letter.
    pub index: u32,
    pub total: u32,
    /// The From header as sent, for the published pages. Reports written
    /// before it was recorded do not have it.
    #[serde(default)]
    pub from: Option<String>,
    #[serde(skip)]
    pub raw: Vec<u8>,
}

impl Patch {
    /// The commit message: the body before the `---` line or the first
    /// diff.
    pub fn commit_message(&self) -> String {
        let text = String::from_utf8_lossy(&self.decoded()).into_owned();
        let body = text.split_once("\n\n").map_or("", |(_, b)| b);
        body.lines()
            .take_while(|l| *l != "---" && !l.starts_with("diff --git"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn is_cover_letter(&self) -> bool {
        self.index == 0 && self.total > 0
    }

    /// The message as sent, with a quoted-printable body decoded to 8bit.
    ///
    /// Mailing list software often re-encodes patches as quoted-printable.
    /// git am decodes them, but patchtest parses the raw message and its
    /// diffs, and reports every patch of such a series as having malformed
    /// diff lines. Messages in any other encoding are returned unchanged.
    // ponytail: quoted-printable only; base64-encoded patch mail is rare.
    pub fn decoded(&self) -> Vec<u8> {
        let Some(split) = find(&self.raw, b"\n\n") else {
            return self.raw.clone();
        };
        let (head, body) = (&self.raw[..split + 1], &self.raw[split + 2..]);
        let head_text = String::from_utf8_lossy(head);
        let cte = head_text.lines().find(|l| {
            l.to_ascii_lowercase()
                .starts_with("content-transfer-encoding:")
        });
        let Some(cte) = cte.filter(|l| l.to_ascii_lowercase().contains("quoted-printable")) else {
            return self.raw.clone();
        };
        let mut out = head_text
            .replace(cte, "Content-Transfer-Encoding: 8bit")
            .into_bytes();
        out.push(b'\n');
        out.extend(decode_quoted_printable(body));
        out
    }

    /// Repository paths the patch changes, after the change.
    pub fn files(&self) -> Vec<String> {
        self.changes().into_iter().map(|(_, new)| new).collect()
    }

    /// The subject without its leading `[...]` tags: what `git log
    /// --format=%s` shows once the patch is applied.
    pub fn summary(&self) -> &str {
        let mut rest = self.subject.trim_start();
        while let Some(after) = rest.strip_prefix('[') {
            let Some(end) = after.find(']') else { break };
            rest = after[end + 1..].trim_start();
        }
        rest.trim()
    }

    /// Who wrote the patch: the in-body `From:` line that git format-patch
    /// writes when the sender is not the author, as for a backport, else
    /// the From header. git am takes the author from the same place, so
    /// this is the author of the commit the patch becomes.
    pub fn author_from(&self) -> Option<&str> {
        self.in_body_from().or(self.from.as_deref())
    }

    /// A `From:` line opening the body, if there is one.
    fn in_body_from(&self) -> Option<&str> {
        let text = std::str::from_utf8(&self.raw).ok()?;
        let body = text
            .split_once("\n\n")
            .or_else(|| text.split_once("\r\n\r\n"))?
            .1;
        let first = body.lines().next()?;
        first
            .strip_prefix("From:")
            .map(str::trim)
            .filter(|v| !v.is_empty())
    }

    /// The address of the patch's author (see `author_from`), lowercased.
    pub fn author_email(&self) -> Option<String> {
        let from = self.author_from()?;
        let email = match (from.rfind('<'), from.rfind('>')) {
            (Some(start), Some(end)) if start < end => &from[start + 1..end],
            _ => from,
        };
        Some(email.trim().to_ascii_lowercase())
    }

    /// The display name in the patch's From header, or its address when it
    /// has no name.
    pub fn author(&self) -> Option<&str> {
        let from = self.author_from()?.trim();
        let name = match from.rfind('<') {
            Some(start) => from[..start].trim().trim_matches('"').trim(),
            None => "",
        };
        Some(if name.is_empty() { from } else { name })
    }

    /// Paths the patch creates (`new file mode`), which exist in no tree
    /// before it.
    pub fn added_files(&self) -> Vec<String> {
        self.files_with_mode("new file mode")
    }

    /// Paths the patch removes (`deleted file mode`).
    pub fn deleted_files(&self) -> Vec<String> {
        self.files_with_mode("deleted file mode")
    }

    /// Paths whose extended header (between `diff --git` and the first hunk)
    /// has a line starting with `mode`.
    fn files_with_mode(&self, mode: &str) -> Vec<String> {
        let text = String::from_utf8_lossy(&self.raw);
        let mut out = Vec::new();
        let mut current: Option<String> = None;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("diff --git a/") {
                current = rest.split_once(" b/").map(|(_, b)| b.to_string());
            } else if line.starts_with(mode)
                && let Some(path) = current.take()
            {
                out.push(path);
            } else if line.starts_with("@@") {
                current = None;
            }
        }
        out
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Series {
    pub cover: Option<Patch>,
    pub patches: Vec<Patch>,
}

/// The input has no patches: a cover letter alone, such as a pull request,
/// or one whose patches were sent outside its thread.
#[derive(Debug)]
pub struct NoPatches;

impl std::fmt::Display for NoPatches {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("no patches found in the input")
    }
}

impl std::error::Error for NoPatches {}

impl Series {
    pub fn load(paths: &[impl AsRef<Path>]) -> Result<Series> {
        let mut messages = Vec::new();
        for path in paths {
            let path = path.as_ref();
            let data =
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            messages.extend(split_mbox(&data));
        }
        Series::from_messages(messages, None)
    }

    /// Like `load`, for the series whose root is the message `root`: a
    /// thread fetched for a revision sent as a reply, such as a `[PATCH v2]`
    /// of one patch of a v1 series, holds the v1 series too.
    pub fn load_rooted(paths: &[impl AsRef<Path>], root: &str) -> Result<Series> {
        let mut messages = Vec::new();
        for path in paths {
            let path = path.as_ref();
            let data =
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            messages.extend(split_mbox(&data));
        }
        Series::from_messages(messages, Some(root))
    }

    /// The messages of several mboxes as one mbox, each Message-ID once, in
    /// the order first seen: the threads of a series sent unthreaded
    /// (designs/PHASE2.md §3.3).
    pub fn merge_mboxes(mboxes: &[Vec<u8>]) -> Vec<u8> {
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for data in mboxes {
            for raw in split_mbox(data) {
                let id = parse_message(raw.clone()).ok().and_then(|m| m.message_id);
                if id.is_some_and(|id| !seen.insert(id)) {
                    continue;
                }
                out.extend_from_slice(&raw);
                if !out.ends_with(b"\n") {
                    out.push(b'\n');
                }
                if !out.ends_with(b"\n\n") {
                    out.push(b'\n');
                }
            }
        }
        out
    }

    /// The series' messages, cover letter first, as an mbox.
    pub fn mbox(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for m in self.cover.iter().chain(&self.patches) {
            out.extend_from_slice(&m.raw);
            if !out.ends_with(b"\n") {
                out.push(b'\n');
            }
            if !out.ends_with(b"\n\n") {
                out.push(b'\n');
            }
        }
        out
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
        // The thread may hold other revisions: check the one asked for.
        let root = id.rsplit('/').find(|p| p.contains('@')).unwrap_or(id);
        Series::from_messages(split_mbox(&output.stdout), Some(root))
    }

    /// Reads a series from mbox data: one or more messages, as `b4 mbox` or
    /// `git format-patch --stdout` writes them.
    pub fn parse(mbox: &[u8]) -> Result<Series> {
        Series::from_messages(split_mbox(mbox), None)
    }

    /// The text of every message in mbox data that is not a reply, decoded,
    /// for a thread that has no patches, such as a pull request.
    pub fn letters(mbox: &[u8]) -> Vec<String> {
        split_mbox(mbox)
            .into_iter()
            .filter_map(|raw| parse_message(raw).ok())
            .filter(|m| {
                let s = m.subject.trim_start().to_ascii_lowercase();
                !["re:", "aw:", "fwd:", "fw:"]
                    .iter()
                    .any(|p| s.starts_with(p))
            })
            .map(|m| String::from_utf8_lossy(&m.decoded()).into_owned())
            .collect()
    }

    /// The patches attached to the thread's messages that are not replies,
    /// rather than sent inline, by file name: MIME parts named `*.patch` or
    /// `*.diff`, or typed `text/x-patch` or `text/x-diff`. The names come
    /// from mail, so only file-name characters are kept.
    pub fn attached_patches(mbox: &[u8]) -> Vec<String> {
        let mut names = Vec::new();
        for raw in split_mbox(mbox) {
            let Ok(m) = parse_message(raw.clone()) else {
                continue;
            };
            let s = m.subject.trim_start().to_ascii_lowercase();
            if ["re:", "aw:", "fwd:", "fw:"]
                .iter()
                .any(|p| s.starts_with(p))
            {
                continue;
            }
            let text = String::from_utf8_lossy(&raw);
            let mut typed = false;
            for line in text.lines() {
                let lower = line.to_ascii_lowercase();
                if lower.contains("text/x-patch") || lower.contains("text/x-diff") {
                    typed = true;
                }
                for key in ["filename=", "name="] {
                    let Some(at) = lower.find(key) else { continue };
                    let value = line[at + key.len()..].trim_start_matches('"');
                    let name: String = value
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || "._-+".contains(*c))
                        .take(100)
                        .collect();
                    let lower = name.to_ascii_lowercase();
                    if (lower.ends_with(".patch") || lower.ends_with(".diff"))
                        && !names.contains(&name)
                    {
                        names.push(name);
                    }
                    break;
                }
            }
            if typed && names.is_empty() {
                names.push("an unnamed patch".into());
            }
        }
        names
    }

    fn from_messages(messages: Vec<Vec<u8>>, root: Option<&str>) -> Result<Series> {
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
            return Err(NoPatches.into());
        }
        // A thread can carry a later revision posted as a reply, such as a
        // [PATCH v2] of one patch of a v1 series. It is its own series: keep
        // only patches of the root's revision when the root is one of the
        // patches, else of the cover letter's, or, without one, of the first
        // patch in the input (the root b4 fetched the thread from).
        let root = root.map(|r| r.trim_matches(|c| c == '<' || c == '>'));
        let (version, total) = patches
            .iter()
            .find(|p| root.is_some() && p.message_id.as_deref() == root)
            .or(cover.as_ref())
            .or(patches.first())
            .map(|p| (p.version, p.total))
            .unwrap_or((1, 1));
        patches.retain(|p| p.version == version && p.total == total);
        if cover.as_ref().is_some_and(|c| c.version != version) {
            cover = None;
        }
        patches.sort_by_key(|p| p.index);
        Ok(Series { cover, patches })
    }

    /// The tags that may name a target branch, such as `scarthgap` or
    /// `meta-python`. Which of them is a real branch is up to the repository,
    /// so no codename list is kept here.
    ///
    /// A branch can also sit inside the PATCH tag, as b4's subject prefixes
    /// put it (`[PATCH scarthgap 00/19]`), so the words of a PATCH tag other
    /// than PATCH, RFC, RESEND, `vN` and `n/m` count too.
    pub fn branch_candidates(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for patch in self.cover.iter().chain(&self.patches) {
            for tag in &patch.tags {
                let words: Vec<&str> = if tag.to_ascii_uppercase().contains("PATCH") {
                    tag.split_whitespace()
                        .filter(|w| !is_patch_tag_word(w))
                        .collect()
                } else {
                    vec![tag.as_str()]
                };
                for word in words {
                    if !out.contains(&word) {
                        out.push(word);
                    }
                }
            }
        }
        out
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Decodes a quoted-printable body: `=XX` is a byte, and `=` at the end of a
/// line joins it to the next one.
fn decode_quoted_printable(body: &[u8]) -> Vec<u8> {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i] != b'=' {
            out.push(body[i]);
            i += 1;
            continue;
        }
        match (body.get(i + 1).copied(), body.get(i + 2).copied()) {
            (Some(b'\n'), _) => i += 2,
            (Some(b'\r'), Some(b'\n')) => i += 3,
            (Some(a), Some(b)) if hex(a).is_some() && hex(b).is_some() => {
                out.push(hex(a).unwrap_or(0) * 16 + hex(b).unwrap_or(0));
                i += 3;
            }
            _ => {
                out.push(b'=');
                i += 1;
            }
        }
    }
    out
}

/// The words of a PATCH tag that are not a branch name.
fn is_patch_tag_word(word: &str) -> bool {
    let upper = word.to_ascii_uppercase();
    matches!(upper.as_str(), "PATCH" | "RFC" | "RESEND")
        || (upper.starts_with('V') && upper[1..].parse::<u32>().is_ok())
        || word
            .split_once('/')
            .is_some_and(|(n, m)| n.parse::<u32>().is_ok() && m.parse::<u32>().is_ok())
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
    let from = header(&text, "From");
    Ok(Patch {
        subject,
        message_id,
        tags,
        version,
        index,
        total,
        from,
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
        if let Some((key, rest)) = line.split_once(':')
            && key.eq_ignore_ascii_case(name)
        {
            value = Some(rest.trim().to_string());
        }
    }
    value
}

/// Collects the leading `[...]` groups of a subject.
pub fn subject_tags(subject: &str) -> Vec<String> {
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
pub fn counters(tags: &[String]) -> (u32, u32, u32) {
    let (mut version, mut index, mut total) = (1, 1, 1);
    for tag in tags
        .iter()
        .filter(|t| t.to_ascii_uppercase().contains("PATCH"))
    {
        for word in tag.split_whitespace() {
            if let Some(v) = word.strip_prefix(['v', 'V']).and_then(|v| v.parse().ok()) {
                version = v;
            } else if let Some((n, m)) = word.split_once('/')
                && let (Ok(n), Ok(m)) = (n.parse(), m.parse())
            {
                (index, total) = (n, m);
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
    fn loads_the_revision_a_thread_was_fetched_for() {
        // A v1 series and, in its thread, a [PATCH v2] of its second patch.
        let mbox = [
            message("[PATCH 0/2] upgrades", "cover\n"),
            message("[PATCH 1/2] a: upgrade", "one\ndiff --git a/a b/a\n"),
            message("[PATCH 2/2] b: upgrade", "two\ndiff --git a/b b/b\n"),
            message("[PATCH v2] b: upgrade", "fixed\ndiff --git a/b b/b\n"),
        ]
        .concat();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), &mbox).unwrap();
        let v1 = Series::load(&[file.path()]).unwrap();
        assert_eq!(v1.patches.len(), 2);
        let id = "<21@example.org>"; // "[PATCH v2] b: upgrade" is 21 bytes
        let v2 = Series::load_rooted(&[file.path()], id).unwrap();
        assert!(v2.cover.is_none());
        assert_eq!(v2.patches.len(), 1);
        assert_eq!(v2.patches[0].subject, "[PATCH v2] b: upgrade");
        assert!(v2.patches[0].commit_message().starts_with("fixed"));
        // Written back, it loads as itself alone.
        std::fs::write(file.path(), v2.mbox()).unwrap();
        let again = Series::load(&[file.path()]).unwrap();
        assert_eq!(again.patches.len(), 1);
        assert_eq!(again.patches[0].version, 2);
    }

    #[test]
    fn names_patches_sent_as_attachments() {
        let mail = "From x Mon Sep 17 00:00:00 2001\n\
            From: A Dev <a@example.org>\n\
            Subject: [meta-networking][wrynose][PATCH] mdio-tools: upgrade\n\
            Content-Type: multipart/mixed; boundary=\"b\"\n\n\
            --b\nContent-Type: text/plain\n\nWhy.\n\
            --b\nContent-Type: image/png; name=\"image.png\"\n\nxx\n\
            --b\nContent-Type: text/x-patch;\n\
            \tname=\"0001-mdio-tools-upgrade.patch\"\n\
            Content-Disposition: attachment;\n\
            \tfilename=\"0001-mdio-tools-upgrade.patch\"\n\
            Content-Transfer-Encoding: base64\n\nRnJvbSAx\n--b--\n\n\
            From y Mon Sep 17 00:00:00 2001\n\
            From: B <b@example.org>\n\
            Subject: Re: [PATCH] mdio-tools: upgrade\n\n\
            Please send it inline; see reply.patch\n";
        assert_eq!(
            Series::attached_patches(mail.as_bytes()),
            vec!["0001-mdio-tools-upgrade.patch"]
        );
        assert!(Series::parse(mail.as_bytes()).is_err());
    }

    #[test]
    fn merges_the_threads_of_a_series_sent_unthreaded() {
        let one = [
            message("[PATCH 0/2] x", "cover\n"),
            message("Re: [PATCH 0/2] x", "reply\n"),
        ]
        .concat();
        let two = [message("[PATCH 1/2] a: x", "one\ndiff --git a/a b/a\n")].concat();
        let three = [message("[PATCH 2/2] bbbb: x", "two\ndiff --git a/b b/b\n")].concat();
        // The cover letter's thread again, as a member's fetch can return.
        let merged = Series::merge_mboxes(&[
            one.clone().into_bytes(),
            two.into_bytes(),
            three.into_bytes(),
            one.into_bytes(),
        ]);
        let series = Series::parse(&merged).unwrap();
        assert!(series.cover.is_some());
        assert_eq!(series.patches.len(), 2);
        assert_eq!(
            String::from_utf8_lossy(&merged)
                .matches("Subject: [PATCH 0/2] x")
                .count(),
            1
        );
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
        let series = Series::from_messages(split_mbox(mbox.as_bytes()), None).unwrap();
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
    fn a_later_revision_in_the_thread_is_not_part_of_the_series() {
        let diff = "x\ndiff --git a/a b/a\n";
        let mbox = [
            message("[PATCH 1/2] a: one", diff),
            message("[PATCH 2/2] strace: upgrade 7.2 -> 7.3", diff),
            message("[PATCH v2] strace: upgrade 7.2 -> 7.3", diff),
        ]
        .concat();
        let series = Series::from_messages(split_mbox(mbox.as_bytes()), None).unwrap();
        let subjects: Vec<_> = series.patches.iter().map(|p| p.subject.as_str()).collect();
        assert_eq!(
            subjects,
            vec![
                "[PATCH 1/2] a: one",
                "[PATCH 2/2] strace: upgrade 7.2 -> 7.3"
            ]
        );
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
    fn reads_a_branch_inside_the_patch_tag() {
        for (subject, want) in [
            (
                "[PATCH scarthgap 00/19] Disable compromised CA certificates",
                vec!["scarthgap"],
            ),
            ("[PATCH blacksail] python3-pip: upgrade", vec!["blacksail"]),
            ("[PATCH RFC v2 1/3] sstate: replay", vec![]),
            (
                "[OE-core][wrynose][PATCH v2 0/5] curl: fixes",
                vec!["OE-core", "wrynose"],
            ),
        ] {
            let raw = message(subject, "x");
            let patch = parse_message(raw.into_bytes()).unwrap();
            let series = Series {
                cover: None,
                patches: vec![patch],
            };
            assert_eq!(series.branch_candidates(), want, "{subject}");
        }
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
        let series = Series::from_messages(split_mbox(mbox.as_bytes()), None).unwrap();
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
    fn decodes_quoted_printable_for_patchtest() {
        let raw = "From x Mon Sep 17 00:00:00 2001\nSubject: [PATCH] foo: bar\n\
                   Content-Type: text/plain; charset=\"utf-8\"\n\
                   Content-Transfer-Encoding: quoted-printable\n\n\
                   ---\ndiff --git a/a.bb b/a.bb\n-A =3D \"1\"\n+A =3D \"2 =E2=80=94 long =\nline\"\n";
        let patch = parse_message(raw.as_bytes().to_vec()).unwrap();
        let text = String::from_utf8(patch.decoded()).unwrap();
        assert!(
            text.contains("Content-Transfer-Encoding: 8bit\n\n---"),
            "{text}"
        );
        assert!(
            text.contains("-A = \"1\"\n+A = \"2 \u{2014} long line\"\n"),
            "{text}"
        );
        let plain = parse_message(message("[PATCH] x", "A =3D 1").into_bytes()).unwrap();
        assert_eq!(plain.decoded(), plain.raw);
    }

    #[test]
    fn reads_summary_and_author() {
        let raw = "From x Mon Sep 17 00:00:00 2001\nFrom: Ann Dev <Ann@Example.org>\n\
                   Subject: [OE-core][scarthgap][PATCH v2 1/2] glib-2.0: patch CVE-2026-1\n\nx\n";
        let patch = parse_message(raw.as_bytes().to_vec()).unwrap();
        assert_eq!(patch.summary(), "glib-2.0: patch CVE-2026-1");
        assert_eq!(patch.author_email().as_deref(), Some("ann@example.org"));
        assert_eq!(patch.author(), Some("Ann Dev"));
        // A backport sent by someone else names its author in the body.
        let raw = "From: Ann Dev <ann@example.org>\nSubject: [wrynose][PATCH] go: fix\n\n\
                   From: Khem Raj <raj.khem@gmail.com>\n\nBody.\n";
        let backport = parse_message(raw.as_bytes().to_vec()).unwrap();
        assert_eq!(
            backport.author_email().as_deref(),
            Some("raj.khem@gmail.com")
        );
        assert_eq!(backport.author(), Some("Khem Raj"));
        let quoted = parse_message(b"From: \"Bo\" <bo@x>\nSubject: [PATCH] x\n\nx\n".to_vec());
        assert_eq!(quoted.unwrap().author(), Some("Bo"));
        let bare = parse_message(b"From: bo@x\nSubject: [PATCH] x\n\nx\n".to_vec());
        assert_eq!(bare.unwrap().author(), Some("bo@x"));
    }

    #[test]
    fn lists_added_files() {
        let raw = "From x Mon Sep 17 00:00:00 2001\nSubject: [PATCH] foo\n\n---\n\
                   diff --git a/a.bb b/a.bb\nindex 1..2 100644\n@@ -1 +1 @@\n-new file mode in a hunk\n\
                   diff --git a/files/fix.patch b/files/fix.patch\nnew file mode 100644\n";
        let patch = parse_message(raw.as_bytes().to_vec()).unwrap();
        assert_eq!(patch.added_files(), vec!["files/fix.patch"]);
    }

    #[test]
    fn rejects_input_without_patches() {
        let mbox = message("[PATCH 0/1] cover only", "x");
        assert!(Series::from_messages(split_mbox(mbox.as_bytes()), None).is_err());
    }
}
