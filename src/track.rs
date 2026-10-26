//! Grouping list messages into series.
//!
//! A series is named by its root: the cover letter (`0/N`), or patch `1/N`
//! when there is no cover letter, or the patch itself when it stands alone.
//! Later patches of the series reply to the root, directly or through
//! another patch.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::lore::Entry;
use crate::series::{counters, subject_tags};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tracked {
    pub root: String,
    pub subject: String,
    /// N from `k/N`; 1 for a single patch.
    pub total: u32,
    /// Patch numbers seen so far (1..=N; the cover letter is not counted).
    pub seen: BTreeSet<u32>,
    pub last_seen: String,
    /// For a revision of one patch sent as a reply to it, the patch it
    /// revises (designs/PHASE2.md §6.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revises: Option<Revises>,
}

/// The patch a revision sent as a reply revises: the root of its series
/// and the Message-ID of the patch, as first sent. A revision of a
/// revision revises the same patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Revises {
    pub series: String,
    pub message: String,
}

impl Tracked {
    /// Every patch of `k/N` has arrived.
    pub fn complete(&self) -> bool {
        (1..=self.total).all(|k| self.seen.contains(&k))
    }
}

#[derive(Debug, Default)]
pub struct Tracker {
    /// Message-ID of every patch or cover letter seen, to its series root.
    /// Kept between polls: a patch may reply to another patch of its series
    /// that arrived in an earlier poll.
    pub root_of: HashMap<String, String>,
    pub series: BTreeMap<String, Tracked>,
}

/// A patch or cover letter: a `[...PATCH...]` subject tag, and not a reply.
/// lore's `s:PATCH` search also matches subjects that merely contain the
/// word, such as auto-upgrade-helper failure reports (`[AUH] patch: ...`).
fn is_patch_mail(subject: &str) -> bool {
    let s = subject.trim_start().to_ascii_lowercase();
    let reply = ["re:", "aw:", "fwd:", "fw:"]
        .iter()
        .any(|p| s.starts_with(p));
    !reply
        && subject_tags(subject)
            .iter()
            .any(|t| t.to_ascii_uppercase().contains("PATCH"))
}

impl Tracker {
    /// Adds feed entries, in any order. Entries already seen are ignored, so
    /// overlapping polls are harmless.
    pub fn ingest(&mut self, entries: &[Entry]) {
        let mut entries: Vec<&Entry> = entries
            .iter()
            .filter(|e| is_patch_mail(&e.subject))
            .collect();
        // Oldest first, so a root is usually known before its replies.
        entries.sort_by(|a, b| a.updated.cmp(&b.updated));
        for e in entries {
            if self.root_of.contains_key(&e.message_id) {
                continue;
            }
            let (version, mut index, mut total) = counters(&subject_tags(&e.subject));
            // A later revision of a patch sent as a reply to it, such as a
            // [PATCH v2] answering patch 11/12 of a v1 series, is a series of
            // its own, complete on its own.
            let mut revises = None;
            let reroll = e.in_reply_to.as_ref().is_some_and(|parent| {
                let Some(root) = self.root_of.get(parent) else {
                    return false;
                };
                let Some(s) = self.series.get(root) else {
                    return false;
                };
                if counters(&subject_tags(&s.subject)).0 == version {
                    return false;
                }
                revises = Some(s.revises.clone().unwrap_or_else(|| Revises {
                    series: root.clone(),
                    message: parent.clone(),
                }));
                true
            });
            if reroll {
                (index, total) = (1, 1);
            }
            let root = if reroll || (index == 0 && total > 0) {
                e.message_id.clone()
            } else if let Some(parent) = &e.in_reply_to {
                // The parent is the root or another patch of the series. If it
                // has not been seen (it is older than the poll window), it is
                // still the best name for the series: b4 fetches its thread.
                self.root_of
                    .get(parent)
                    .cloned()
                    .unwrap_or_else(|| parent.clone())
            } else {
                e.message_id.clone()
            };
            self.root_of.insert(e.message_id.clone(), root.clone());
            let series = self.series.entry(root.clone()).or_insert_with(|| Tracked {
                root: root.clone(),
                subject: e.subject.clone(),
                total,
                seen: BTreeSet::new(),
                last_seen: e.updated.clone(),
                revises: revises.clone(),
            });
            if index == 0 || e.message_id == root {
                series.subject = e.subject.clone();
            }
            if index > 0 {
                series.seen.insert(index);
            }
            series.total = series.total.max(total);
            if e.updated > series.last_seen {
                series.last_seen = e.updated.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, subject: &str, parent: Option<&str>, t: &str) -> Entry {
        Entry {
            message_id: id.into(),
            subject: subject.into(),
            in_reply_to: parent.map(String::from),
            updated: format!("2026-10-24T10:{t}:00Z"),
        }
    }

    #[test]
    fn groups_a_series_without_a_cover_letter() {
        let mut t = Tracker::default();
        t.ingest(&[
            entry("c@x", "[PATCH 3/3] piglit: fix", Some("a@x"), "03"),
            entry("b@x", "[PATCH 2/3] pulseaudio: drop", Some("a@x"), "02"),
            entry("a@x", "[PATCH 1/3] consolekit: upgrade", None, "01"),
            entry(
                "r@x",
                "Re: [PATCH 1/3] consolekit: upgrade",
                Some("a@x"),
                "09",
            ),
        ]);
        assert_eq!(t.series.len(), 1);
        let s = &t.series["a@x"];
        assert!(s.complete());
        assert_eq!(s.subject, "[PATCH 1/3] consolekit: upgrade");
        assert_eq!(s.last_seen, "2026-10-24T10:03:00Z");
    }

    #[test]
    fn a_cover_letter_names_the_series_and_counts_nothing() {
        let mut t = Tracker::default();
        t.ingest(&[
            entry("0@x", "[PATCH RFC 0/2] sstate: replay", None, "00"),
            entry("1@x", "[PATCH RFC 1/2] sstate: replay", Some("0@x"), "01"),
        ]);
        let s = &t.series["0@x"];
        assert_eq!(s.subject, "[PATCH RFC 0/2] sstate: replay");
        assert!(!s.complete());
        t.ingest(&[entry(
            "2@x",
            "[PATCH RFC 2/2] oeqa: test",
            Some("0@x"),
            "02",
        )]);
        assert!(t.series["0@x"].complete());
    }

    #[test]
    fn resends_and_single_patches_are_their_own_series() {
        let mut t = Tracker::default();
        t.ingest(&[
            entry("v1@x", "[PATCH] curl: fix", None, "00"),
            entry("v2@x", "[PATCH v2] curl: fix", None, "05"),
            // Overlapping polls deliver the same entry twice.
            entry("v2@x", "[PATCH v2] curl: fix", None, "05"),
        ]);
        assert_eq!(t.series.len(), 2);
        assert!(t.series["v2@x"].complete());
    }

    #[test]
    fn a_revision_sent_as_a_reply_is_its_own_series() {
        let mut t = Tracker::default();
        t.ingest(&[
            entry("0@x", "[PATCH 0/2] upgrades", None, "00"),
            entry("1@x", "[PATCH 1/2] a: upgrade", Some("0@x"), "01"),
            entry("2@x", "[PATCH 2/2] b: upgrade", Some("0@x"), "02"),
            // The author's fix to patch 2, sent in its thread.
            entry("v2@x", "[PATCH v2] b: upgrade", Some("2@x"), "30"),
        ]);
        assert_eq!(t.series.len(), 2);
        assert!(t.series["0@x"].complete());
        let v2 = &t.series["v2@x"];
        assert!(v2.complete());
        assert_eq!(v2.subject, "[PATCH v2] b: upgrade");
        let revises = |id: &str| t.series[id].revises.clone().map(|r| (r.series, r.message));
        assert_eq!(revises("v2@x"), Some(("0@x".into(), "2@x".into())));
        assert_eq!(revises("0@x"), None);
        // A reply of the same revision still belongs to its series.
        t.ingest(&[entry("3@x", "[PATCH 2/2] b: upgrade", Some("1@x"), "40")]);
        assert_eq!(t.series.len(), 2);
        // A v3 answering the v2 revises the same patch.
        t.ingest(&[entry("v3@x", "[PATCH v3] b: upgrade", Some("v2@x"), "50")]);
        let revises = |id: &str| t.series[id].revises.clone().map(|r| (r.series, r.message));
        assert_eq!(revises("v3@x"), Some(("0@x".into(), "2@x".into())));
    }

    #[test]
    fn mail_without_a_patch_tag_is_ignored() {
        let mut t = Tracker::default();
        t.ingest(&[
            entry(
                "auh@x",
                "[AUH] patch: upgrading to 2.8.1 FAILED",
                None,
                "00",
            ),
            entry(
                "tag@x",
                "[PATCH][yocto-autobuilder-helper] abint: be verbose",
                None,
                "01",
            ),
        ]);
        assert_eq!(t.series.keys().collect::<Vec<_>>(), vec!["tag@x"]);
    }

    #[test]
    fn a_patch_whose_root_is_older_than_the_window_joins_it() {
        let mut t = Tracker::default();
        t.ingest(&[entry("3@x", "[PATCH 3/3] x: y", Some("1@x"), "10")]);
        assert_eq!(t.series["1@x"].seen, BTreeSet::from([3]));
        assert!(!t.series["1@x"].complete());
    }

    #[test]
    fn groups_the_real_feed() {
        let entries = crate::lore::parse(include_str!("../tests/fixtures/lore-feed.xml")).unwrap();
        let mut t = Tracker::default();
        t.ingest(&entries);
        let rfc = t
            .series
            .values()
            .find(|s| s.subject.starts_with("[PATCH RFC 0/3] sstate"))
            .unwrap();
        assert!(rfc.complete());
        let consolekit = t
            .series
            .values()
            .find(|s| s.subject.starts_with("[PATCH 1/3] consolekit"))
            .unwrap();
        assert!(consolekit.complete());
        assert!(t.series.values().all(|s| !s.subject.starts_with("Re:")));
    }
}
