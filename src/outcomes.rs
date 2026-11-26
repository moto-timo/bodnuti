//! Live verification of model review against what the lists decide
//! (designs/PHASE3.md §11). Draft.
//!
//! For each reviewed patch, once the list has decided: a later revision
//! from the same author with the same summary that changed code confirms
//! the findings near a line it changed; a patch that landed confirms
//! nothing, unless a commit with `Fixes:` naming it changed a file a
//! finding was in. No model calls: only git on the worker's clones.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::escalate::{is_sha, release};
use crate::git;
use crate::report::Report;
use crate::review::{Outcome, Review, Store};
use crate::watch::{Config, ListState};

/// A finding this close to a changed line counts as confirmed.
const NEAR: u32 = 3;
/// After this long without a revision or landing, a patch is dropped.
const UNDECIDED_SECS: u64 = 60 * 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A later revision changed code in the patch.
    Revised,
    /// A later revision with no code change: a resend.
    Resent,
    Merged,
    MergedFixed,
    Undecided,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub kind: Kind,
    /// Unix seconds.
    pub decided: u64,
    /// `model prompt min_confidence` of the review.
    pub config: String,
    pub findings: usize,
    pub confirmed: usize,
}

/// One month's scores for one configuration (§11.2), as counts so months
/// add up; the rates are derived on display.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub decided: usize,
    pub findings: usize,
    pub confirmed: usize,
    pub revised: usize,
    pub revised_caught: usize,
    pub merged: usize,
    pub merged_findings: usize,
}

/// A patch of a checked series: where to find its revisions.
struct Sent {
    last_seen: String,
    root: String,
    commit: String,
}

/// Every checked patch of a list, by author and summary.
type Revisions = BTreeMap<(String, String), Vec<Sent>>;

fn report(state: &Path, list: &str, root: &str) -> Option<Report> {
    let path = state
        .join("series")
        .join(list)
        .join(root)
        .join("report.json");
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn revisions(state: &Path, list: &str, ls: &ListState) -> Revisions {
    // ponytail: reads every report of the list each run; keep an index in
    // state if lists grow past a few thousand checked series.
    let mut out = Revisions::new();
    for (root, record) in &ls.series {
        let Some(r) = report(state, list, root) else {
            continue;
        };
        for p in &r.series.patches {
            let Some(a) = r.applied.iter().find(|a| a.index == p.index) else {
                continue;
            };
            out.entry((p.from.clone().unwrap_or_default(), p.summary().to_string()))
                .or_default()
                .push(Sent {
                    last_seen: record.tracked.last_seen.clone(),
                    root: root.clone(),
                    commit: a.commit.clone(),
                });
        }
    }
    out
}

/// The old-side line ranges `git diff -U0 a b` changed in `file`; None if
/// either commit is gone.
fn changed(repo: &Path, a: &str, b: &str, file: &str) -> Option<Vec<(u32, u32)>> {
    let file = crate::context::inside(file)?;
    let out = crate::context::git_out(
        repo,
        &[
            "--literal-pathspecs",
            "diff",
            "-U0",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            a,
            b,
            "--",
            &file,
        ],
    )
    .ok()?;
    Some(hunks(&out))
}

/// `@@ -start,count +... @@` as an inclusive old-side range; a pure
/// insertion is the line it follows.
fn hunks(diff: &str) -> Vec<(u32, u32)> {
    diff.lines()
        .filter_map(|l| {
            let old = l.strip_prefix("@@ -")?.split(' ').next()?;
            let (start, count) = match old.split_once(',') {
                Some((s, c)) => (s.parse::<u32>().ok()?, c.parse::<u32>().ok()?),
                None => (old.parse().ok()?, 1),
            };
            Some((start, start + count.max(1) - 1))
        })
        .collect()
}

fn near(line: Option<u32>, ranges: &[(u32, u32)]) -> bool {
    match line {
        Some(l) => ranges.iter().any(|&(a, b)| l + NEAR >= a && l <= b + NEAR),
        // A finding on the whole file counts if the file changed at all.
        None => !ranges.is_empty(),
    }
}

/// Files changed by commits whose `Fixes:` names `landed`, on `branch`.
fn fixed_files(repo: &Path, landed: &str, branch: &str) -> Option<Vec<String>> {
    if !is_sha(landed) || branch.contains(['\0', '\n']) || branch.starts_with('-') {
        return None;
    }
    let grep = format!("^Fixes: *{}", &landed[..12]);
    let fixes = git::run(
        repo,
        &[
            "log",
            "-i",
            "--format=%H",
            "--grep",
            &grep,
            &format!("{landed}..origin/{branch}"),
        ],
    )
    .ok()?;
    let mut files = Vec::new();
    for fix in fixes.lines() {
        let names = git::run(repo, &["show", "--format=", "--name-only", fix]).ok()?;
        files.extend(names.lines().map(str::to_string));
    }
    (!files.is_empty()).then_some(files)
}

fn config_of(r: &Review) -> String {
    format!("{} {} {}", r.model, r.prompt, r.min_confidence)
}

fn month(secs: u64) -> String {
    let t = crate::lore::utc(secs);
    format!("{}-{}", &t[..4], &t[4..6])
}

/// Decides what it can and writes `state/review/outcomes/`.
pub fn run(config: &Config, now: u64) -> Result<()> {
    let dir = config.state.join("review").join("outcomes");
    let path = dir.join("patches.json");
    let mut decided: BTreeMap<String, Decision> = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let store = Store::new(&config.state);
    for list in &config.lists {
        let ls = ListState::load(&config.state, &list.name)?;
        let mut index: Option<Revisions> = None;
        let roots = std::fs::read_dir(config.state.join("review").join(&list.name));
        for root in roots.into_iter().flatten().flatten() {
            let root = root.file_name().to_string_lossy().into_owned();
            let (Some(record), Some(rep)) = (
                ls.series.get(&root),
                report(&config.state, &list.name, &root),
            ) else {
                continue;
            };
            let Some(repo) = &record.repo else { continue };
            let clone = config.cache.join(repo);
            for review in store.results(&list.name, &root).values() {
                let commit = rep
                    .applied
                    .iter()
                    .find(|a| a.index == review.index)
                    .map(|a| a.commit.clone())
                    .unwrap_or_default();
                if review.outcome != Outcome::Reviewed {
                    release(&clone, &commit); // nothing to verify
                    continue;
                }
                let key = format!("{}/{root}/{}", list.name, review.index);
                let landed = record.applied.get(&review.index);
                let in_files = |files: &[String]| {
                    review
                        .findings
                        .iter()
                        .filter(|f| f.finding.file.as_ref().is_some_and(|x| files.contains(x)))
                        .count()
                };
                if let Some(d) = decided.get_mut(&key) {
                    // A merged patch can still be fixed later (§11.1).
                    if d.kind == Kind::Merged
                        && now.saturating_sub(d.decided) < UNDECIDED_SECS
                        && let Some(l) = landed
                        && let Some(files) = fixed_files(&clone, &l.commit, &l.branch)
                    {
                        d.kind = Kind::MergedFixed;
                        d.confirmed = in_files(&files);
                    }
                    continue;
                }
                let patch = rep.series.patches.iter().find(|p| p.index == review.index);
                let mut d = Decision {
                    kind: Kind::Undecided,
                    decided: now,
                    config: config_of(review),
                    findings: review.findings.len(),
                    confirmed: 0,
                };
                if landed.is_some() {
                    d.kind = Kind::Merged;
                } else if let Some(p) = patch
                    && let Some(v2) = index
                        .get_or_insert_with(|| revisions(&config.state, &list.name, &ls))
                        .get(&(p.from.clone().unwrap_or_default(), p.summary().to_string()))
                        .and_then(|sent| {
                            sent.iter()
                                .filter(|s| {
                                    s.root != root && s.last_seen > record.tracked.last_seen
                                })
                                .min_by(|a, b| a.last_seen.cmp(&b.last_seen))
                        })
                {
                    let files = p.files();
                    let mut ranges = BTreeMap::new();
                    for f in &files {
                        match changed(&clone, &commit, &v2.commit, f) {
                            Some(r) => {
                                ranges.insert(f.clone(), r);
                            }
                            None => break,
                        }
                    }
                    if ranges.len() < files.len() {
                        continue; // a commit is gone: cannot decide
                    }
                    // ponytail: v1 and v2 may sit on different bases, so the
                    // interdiff can carry base changes; trend measure only.
                    if ranges.values().all(Vec::is_empty) {
                        d.kind = Kind::Resent;
                    } else {
                        d.kind = Kind::Revised;
                        d.confirmed = review
                            .findings
                            .iter()
                            .filter(|f| {
                                f.finding
                                    .file
                                    .as_ref()
                                    .and_then(|x| ranges.get(x))
                                    .is_some_and(|r| near(f.finding.line, r))
                            })
                            .count();
                    }
                } else if crate::lore::parse_time(&record.tracked.last_seen)
                    .is_none_or(|t| now.saturating_sub(t) < UNDECIDED_SECS)
                {
                    continue;
                }
                release(&clone, &commit);
                decided.insert(key, d);
            }
        }
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&path, serde_json::to_vec_pretty(&decided)?)?;
    let mut months: BTreeMap<String, BTreeMap<String, Score>> = BTreeMap::new();
    for d in decided.values().filter(|d| d.kind != Kind::Undecided) {
        let s = months
            .entry(month(d.decided))
            .or_default()
            .entry(d.config.clone())
            .or_default();
        s.decided += 1;
        s.findings += d.findings;
        s.confirmed += d.confirmed;
        match d.kind {
            Kind::Revised => {
                s.revised += 1;
                s.revised_caught += usize::from(d.confirmed > 0);
            }
            Kind::Merged => {
                s.merged += 1;
                s.merged_findings += d.findings;
            }
            _ => {}
        }
    }
    for (m, scores) in &months {
        std::fs::write(
            dir.join(format!("{m}.json")),
            serde_json::to_vec_pretty(scores)?,
        )?;
    }
    if let Some(scores) = months.get(&month(now)) {
        for (c, s) in scores {
            println!(
                "review outcomes {}: {c}: {} decided, {}/{} findings confirmed, \
                 {}/{} revisions caught, {} findings on {} merged",
                month(now),
                s.decided,
                s.confirmed,
                s.findings,
                s.revised_caught,
                s.revised,
                s.merged_findings,
                s.merged
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunks_are_old_side_ranges_and_findings_near_them_count() {
        let diff =
            "diff --git a/f b/f\n@@ -3 +3 @@\n-a\n+b\n@@ -10,4 +10,0 @@\n@@ -20,0 +21,2 @@\n";
        let h = hunks(diff);
        assert_eq!(h, vec![(3, 3), (10, 13), (20, 20)]);
        assert!(near(Some(6), &h));
        assert!(near(Some(23), &h));
        assert!(!near(Some(24), &h));
        assert!(!near(Some(1), &[(10, 13)]));
        assert!(near(None, &h));
        assert!(!near(None, &[]));
    }

    #[test]
    fn months_are_utc() {
        assert_eq!(month(0), "1970-01");
        assert_eq!(month(1_795_000_000), "2026-11");
    }
}
