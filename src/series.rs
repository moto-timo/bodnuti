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

    /// The address in the patch's From header, lowercased.
    pub fn author_email(&self) -> Option<String> {
        let from = self.from.as_deref()?;
        let email = match (from.rfind('<'), from.rfind('>')) {
            (Some(start), Some(end)) if start < end => &from[start + 1..end],
            _ => from,
        };
        Some(email.trim().to_ascii_lowercase())
    }

    /// The display name in the patch's From header, or its address when it
    /// has no name.
    pub fn author(&self) -> Option<&str> {
        let from = self.from.as_deref()?.trim();
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
        Series::parse(&output.stdout)
    }

    /// Reads a series from mbox data: one or more messages, as `b4 mbox` or
    /// `git format-patch --stdout` writes them.
    pub fn parse(mbox: &[u8]) -> Result<Series> {
        Series::from_messages(split_mbox(mbox))
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
        // A thread can carry a later revision posted as a reply, such as a
        // [PATCH v2] of one patch of a v1 series. It is its own series: keep
        // only patches of the cover letter's revision, or, without one, of
        // the first patch in the input (the root b4 fetched the thread from).
        let (version, total) = cover
            .as_ref()
            .or(patches.first())
            .map(|p| (p.version, p.total))
            .unwrap_or((1, 1));
        patches.retain(|p| p.version == version && p.total == total);
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
    fn a_later_revision_in_the_thread_is_not_part_of_the_series() {
        let diff = "x\ndiff --git a/a b/a\n";
        let mbox = [
            message("[PATCH 1/2] a: one", diff),
            message("[PATCH 2/2] strace: upgrade 7.2 -> 7.3", diff),
            message("[PATCH v2] strace: upgrade 7.2 -> 7.3", diff),
        ]
        .concat();
        let series = Series::from_messages(split_mbox(mbox.as_bytes())).unwrap();
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
        assert!(Series::from_messages(split_mbox(mbox.as_bytes())).is_err());
    }
}
