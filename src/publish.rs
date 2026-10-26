//! `bodnuti publish`: the watcher's state as static HTML (designs/PHASE2.md
//! §6).
//!
//! ```text
//! site/
//!   index.html                 every list, newest series first
//!   <list>/index.html          one list
//!   <list>/<root>/index.html   one series: its patches and findings
//!   <list>/<root>/report.txt   the text report, as `bodnuti check` prints it
//! ```
//!
//! Everything that comes from mail (subjects, names, finding messages) is
//! escaped. The pages have no JavaScript.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::finding::{Finding, Severity};
use crate::lore;
use crate::report::{Report, location, numbered, tool_name};
use crate::series::{counters, subject_tags};
use crate::watch::{Config, ListState, SeriesRecord, Status};

/// Seconds in a month, for `retain_months`. The cutoff is approximate; a
/// series a day either side of it makes no difference to the site.
const MONTH_SECS: u64 = 30 * 86_400;

const LORE: &str = "https://lore.kernel.org";

/// The check id of an apply failure (`workspace::NotApplied`).
const APPLY_CHECK: &str = "bodnuti.apply";

/// One series on the index.
struct Row<'a> {
    list: &'a str,
    record: &'a SeriesRecord,
    report: Option<Report>,
    /// Revisions of its patches sent as replies, oldest first
    /// (designs/PHASE2.md §6.3).
    revisions: Vec<Revision<'a>>,
    /// For a resend, its original's result: class and label.
    original: Option<(&'static str, String)>,
}

/// A revision of one patch of a series, sent as a reply: checked as a
/// series of its own, shown under the patch it revises.
struct Revision<'a> {
    /// The number of the patch it revises.
    patch: u32,
    row: Row<'a>,
}

/// The series a revision's own page links back to.
struct Revised {
    page: Option<String>,
    patch: u32,
    total: u32,
    subject: String,
}

/// What a series' row says in the result column.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// Not every patch has arrived, or the series has not settled.
    Waiting,
    Clean,
    Findings {
        errors: usize,
        total: usize,
    },
    /// A checker could not run, or there is no report.
    NotChecked,
    Unrouted,
    /// Every patch was checked before, in another series.
    Resend,
    /// The thread has no patches, such as a pull request.
    NoPatches,
    /// A patch did not apply because the base already had it, so the
    /// patches from it on were not checked.
    AlreadyApplied,
    Failed,
}

impl Outcome {
    fn label(&self) -> String {
        match self {
            Outcome::Waiting => "waiting".into(),
            Outcome::Clean => "clean".into(),
            Outcome::Findings { errors: 0, total } => plural(*total, "finding"),
            Outcome::Findings { errors, total } => {
                format!(
                    "{}, {}",
                    plural(*total, "finding"),
                    plural(*errors, "error")
                )
            }
            Outcome::AlreadyApplied => "already applied".into(),
            Outcome::NotChecked => "not checked".into(),
            Outcome::Unrouted => "unrouted".into(),
            Outcome::Resend => "resend".into(),
            Outcome::NoPatches => "no patches".into(),
            Outcome::Failed => "failed".into(),
        }
    }

    fn class(&self) -> &'static str {
        match self {
            Outcome::Clean => "clean",
            Outcome::Findings { errors: 0, .. } => "warn",
            Outcome::Findings { .. } | Outcome::Failed => "error",
            _ => "muted",
        }
    }
}

impl Row<'_> {
    fn outcome(&self) -> Outcome {
        match self.record.status {
            Status::Seen | Status::Complete => Outcome::Waiting,
            Status::Unrouted => Outcome::Unrouted,
            Status::Duplicate => Outcome::Resend,
            Status::NoPatches => Outcome::NoPatches,
            Status::Failed => Outcome::Failed,
            Status::Checked | Status::Applied => match &self.report {
                // Found upstream before it was checked (§3.1).
                None if self.record.status == Status::Applied => Outcome::AlreadyApplied,
                None => Outcome::NotChecked,
                Some(r) => {
                    // The series as it stands: each revised patch's findings
                    // are its latest revision's (§6.3).
                    let latest = self.latest();
                    let reports =
                        std::iter::once(r).chain(latest.values().filter_map(|v| v.report.as_ref()));
                    if reports.clone().any(|r| !r.tool_errors.is_empty()) {
                        return Outcome::NotChecked;
                    }
                    let mut hidden = 0;
                    let mut shown: Vec<&Finding> = Vec::new();
                    for f in &r.findings {
                        if f.patch.is_some_and(|k| latest.contains_key(&k)) {
                            continue;
                        }
                        if already_applied(self.record, f) {
                            hidden += 1;
                        } else {
                            shown.push(f);
                        }
                    }
                    for v in latest.values() {
                        if let Some(rep) = &v.report {
                            shown.extend(
                                rep.findings
                                    .iter()
                                    .filter(|f| !already_applied(v.record, f)),
                            );
                        }
                    }
                    match shown.len() {
                        0 if hidden == 0 => Outcome::Clean,
                        0 => Outcome::AlreadyApplied,
                        total => Outcome::Findings {
                            errors: shown
                                .iter()
                                .filter(|f| f.severity == Severity::Error)
                                .count(),
                            total,
                        },
                    }
                }
            },
        }
    }

    /// The result column's class and label: a resend shows the result of
    /// the series its patches were checked in.
    fn result(&self) -> (&'static str, String) {
        let outcome = self.outcome();
        match (&outcome, &self.original) {
            (Outcome::Resend, Some((class, label))) => (class, format!("resend: {label}")),
            _ => (outcome.class(), outcome.label()),
        }
    }

    /// The series a resend's patches were checked in, `<list>/<root>`.
    fn duplicate_of(&self) -> Option<&str> {
        if self.record.status != Status::Duplicate {
            return None;
        }
        self.record.duplicate_of.as_deref().or_else(|| {
            // Older state names it only at the end of the reason.
            self.record
                .reason
                .as_deref()?
                .rsplit_once(" in ")
                .map(|(_, s)| s)
        })
    }

    /// The latest checked revision of each revised patch.
    fn latest(&self) -> BTreeMap<u32, &Row<'_>> {
        let mut latest = BTreeMap::new();
        for rev in &self.revisions {
            if rev.row.report.is_some() {
                latest.insert(rev.patch, &rev.row);
            }
        }
        latest
    }

    /// `11/12 revised in v2`, for each revised patch; empty without any.
    fn revised_note(&self) -> String {
        let mut last: BTreeMap<u32, &Row> = BTreeMap::new();
        for rev in &self.revisions {
            last.insert(rev.patch, &rev.row);
        }
        last.iter()
            .map(|(k, row)| {
                let version = counters(&subject_tags(&row.record.tracked.subject)).0;
                let waiting = if row.report.is_some() {
                    ""
                } else {
                    ", waiting"
                };
                format!(
                    "{k}/{} revised in v{version}{waiting}",
                    self.record.tracked.total.max(1)
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn author(&self) -> Option<&str> {
        let series = &self.report.as_ref()?.series;
        series
            .cover
            .iter()
            .chain(&series.patches)
            .find_map(|p| p.author())
    }

    /// `k/N applied`, `applied` or nothing.
    fn applied(&self) -> String {
        let landed = &self.record.applied;
        if landed.is_empty() {
            return String::new();
        }
        let probably = if landed.values().any(|l| !l.exact) {
            "probably "
        } else {
            ""
        };
        if self.record.status == Status::Applied {
            format!("{probably}applied")
        } else {
            format!(
                "{probably}{}/{} applied",
                landed.len(),
                self.record.tracked.total.max(1)
            )
        }
    }

    /// The series page, relative to the site root. Roots that cannot be
    /// file names have no page.
    fn page(&self) -> Option<String> {
        lore::safe_id(&self.record.tracked.root)
            .then(|| format!("{}/{}/", self.list, self.record.tracked.root))
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// An apply failure on a patch that has since landed on the branch it was
/// checked against: the base already had it, so it could not apply again.
fn already_applied(record: &SeriesRecord, f: &Finding) -> bool {
    f.check == APPLY_CHECK && f.patch.is_some_and(|k| record.applied.contains_key(&k))
}

/// Renders the site into `state/site/`, replacing the previous one.
pub fn render(config: &Config, states: &BTreeMap<String, ListState>, now: u64) -> Result<PathBuf> {
    let cutoff = now.saturating_sub(config.retain_months * MONTH_SECS);
    let mut rows = Vec::new();
    for (list, state) in states {
        for record in state.series.values() {
            let seen = lore::parse_time(&record.tracked.last_seen).unwrap_or(0);
            if seen < cutoff {
                continue;
            }
            let report = read_report(&config.state, list, record)?;
            rows.push(Row {
                list,
                record,
                report,
                revisions: Vec::new(),
                original: None,
            });
        }
    }
    let mut rows = nest_revisions(rows);
    let results: BTreeMap<String, (&'static str, String)> = rows
        .iter()
        .map(|r| {
            let o = r.outcome();
            (
                format!("{}/{}", r.list, r.record.tracked.root),
                (o.class(), o.label()),
            )
        })
        .collect();
    for row in &mut rows {
        row.original = row.duplicate_of().and_then(|k| results.get(k)).cloned();
    }

    let site = config.state.join("site");
    let tmp = config.state.join(".site.tmp");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    let generated = timestamp(&lore::utc(now));
    write(&tmp.join("logo.svg"), LOGO)?;
    let lists: Vec<&str> = states.keys().map(String::as_str).collect();
    write(
        &tmp.join("summary.html"),
        &summary(&rows, &lists, now, &generated),
    )?;
    write(
        &tmp.join("index.html"),
        &index_page(&rows, None, &generated),
    )?;
    for list in states.keys() {
        let own: Vec<&Row> = rows.iter().filter(|r| r.list == list).collect();
        let page = index_page_of(&own, Some(list), &generated);
        write(&tmp.join(list).join("index.html"), &page)?;
    }
    let write_series = |row: &Row, revised: Option<&Revised>| -> Result<()> {
        let Some(page) = row.page() else {
            return Ok(());
        };
        let dir = tmp.join(&page);
        write(
            &dir.join("index.html"),
            &series_page(config, row, revised, &generated),
        )?;
        let text = series_dir(&config.state, row).join("report.txt");
        if text.exists() {
            std::fs::copy(&text, dir.join("report.txt"))
                .with_context(|| format!("copying {}", text.display()))?;
        }
        Ok(())
    };
    for row in &rows {
        write_series(row, None)?;
        for rev in &row.revisions {
            let revised = Revised {
                page: row.page(),
                patch: rev.patch,
                total: row.record.tracked.total.max(1),
                subject: row.record.tracked.subject.clone(),
            };
            write_series(&rev.row, Some(&revised))?;
        }
    }
    if site.exists() {
        std::fs::remove_dir_all(&site)?;
    }
    std::fs::rename(&tmp, &site)?;
    Ok(site)
}

/// Moves each revision sent as a reply under the series it revises, when
/// that series is on the site and its report names the patch; a revision
/// whose series is not stays a row of its own. Newest first.
fn nest_revisions(rows: Vec<Row<'_>>) -> Vec<Row<'_>> {
    let (mut revisions, mut rows): (Vec<Row>, Vec<Row>) = rows
        .into_iter()
        .partition(|r| r.record.tracked.revises.is_some());
    revisions.sort_by(|a, b| a.record.tracked.last_seen.cmp(&b.record.tracked.last_seen));
    for rev in revisions {
        let Some(of) = rev.record.tracked.revises.as_ref() else {
            continue;
        };
        let found = rows.iter().enumerate().find_map(|(i, p)| {
            if p.list != rev.list || p.record.tracked.root != of.series {
                return None;
            }
            let patch = p
                .report
                .as_ref()?
                .series
                .patches
                .iter()
                .find(|x| x.message_id.as_deref() == Some(of.message.as_str()))?;
            Some((i, patch.index))
        });
        match found {
            Some((i, patch)) => rows[i].revisions.push(Revision { patch, row: rev }),
            None => rows.push(rev),
        }
    }
    rows.sort_by(|a, b| b.record.tracked.last_seen.cmp(&a.record.tracked.last_seen));
    rows
}

fn series_dir(state: &Path, row: &Row) -> PathBuf {
    state
        .join("series")
        .join(row.list)
        .join(&row.record.tracked.root)
}

fn read_report(state: &Path, list: &str, record: &SeriesRecord) -> Result<Option<Report>> {
    if !matches!(record.status, Status::Checked | Status::Applied)
        || !lore::safe_id(&record.tracked.root)
    {
        return Ok(None);
    }
    let path = state
        .join("series")
        .join(list)
        .join(&record.tracked.root)
        .join("report.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .with_context(|| format!("parsing {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// Where the site is served on bodnuti.dev, for the summary's links.
const SITE_PATH: &str = "/reports/";

/// How far back the summary looks, in days.
const SUMMARY_DAYS: u64 = 7;

/// How many of the latest series the summary lists.
const SUMMARY_LATEST: usize = 5;

/// `summary.html`, a fragment for bodnuti.dev's front page, which Caddy
/// inserts as it is with `readFile` (designs/PHASE2.md §6.2): per list, the
/// series of the last week, how many have findings and how many landed, and
/// the latest series. `rows` are newest first.
fn summary(rows: &[Row], lists: &[&str], now: u64, generated: &str) -> String {
    let since = now.saturating_sub(SUMMARY_DAYS * 86_400);
    let recent: Vec<&Row> = rows
        .iter()
        .filter(|r| lore::parse_time(&r.record.tracked.last_seen).unwrap_or(0) >= since)
        .collect();
    let mut out = String::from("<section class=\"bodnuti-summary\">\n");
    let _ = writeln!(
        out,
        "<table>\n<tr><th>List</th><th>Series, last {SUMMARY_DAYS} days</th>\
         <th>With findings</th><th>Landed</th></tr>"
    );
    for list in lists {
        let own: Vec<&&Row> = recent.iter().filter(|r| r.list == *list).collect();
        let findings = own
            .iter()
            .filter(|r| matches!(r.outcome(), Outcome::Findings { .. }))
            .count();
        let landed = own.iter().filter(|r| !r.record.applied.is_empty()).count();
        let name = escape(list);
        let _ = writeln!(
            out,
            "<tr><td><a href=\"{SITE_PATH}{name}/\">{name}</a></td><td>{}</td>\
             <td>{findings}</td><td>{landed}</td></tr>",
            own.len()
        );
    }
    out.push_str("</table>\n<ul class=\"latest\">\n");
    for row in rows.iter().take(SUMMARY_LATEST) {
        let subject = escape(&row.record.tracked.subject);
        let subject = match row.page() {
            Some(page) => format!("<a href=\"{SITE_PATH}{}\">{subject}</a>", escape(&page)),
            None => subject,
        };
        let (class, label) = row.result();
        let _ = writeln!(
            out,
            "<li><span class=\"when\">{}</span> {subject} \
             <span class=\"result {class}\">{label}</span></li>",
            timestamp(&row.record.tracked.last_seen),
        );
    }
    let _ = writeln!(
        out,
        "</ul>\n<p class=\"updated\">Updated {generated} UTC. \
         <a href=\"{SITE_PATH}\">All series</a>.</p>\n</section>"
    );
    out
}

/// Escapes text for HTML element content and quoted attribute values.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            // Braces too: the summary goes into bodnuti.dev's front page
            // through a Caddy template, and mail must never read as one.
            '{' => out.push_str("&#123;"),
            '}' => out.push_str("&#125;"),
            c => out.push(c),
        }
    }
    out
}

/// `YYYYMMDDhhmmss` or lore's RFC 3339 form to `YYYY-MM-DD hh:mm`.
fn timestamp(t: &str) -> String {
    let digits: String = t.chars().filter(char::is_ascii_digit).collect();
    match (digits.get(0..4), digits.get(4..6), digits.get(6..8)) {
        (Some(y), Some(m), Some(d)) => {
            let hh = digits.get(8..10).unwrap_or("00");
            let mm = digits.get(10..12).unwrap_or("00");
            format!("{y}-{m}-{d} {hh}:{mm}")
        }
        _ => escape(t),
    }
}

/// A link to a message on lore. Message-IDs are percent-encoded, since
/// they come from mail.
fn lore_url(list: &str, message_id: &str) -> String {
    let mut encoded = String::new();
    for b in message_id.bytes() {
        if b.is_ascii_alphanumeric() || b"._-+@=".contains(&b) {
            encoded.push(b as char);
        } else {
            let _ = write!(encoded, "%{b:02X}");
        }
    }
    format!("{LORE}/{list}/{encoded}/")
}

/// A link to an upstream commit, for the cgit hosts the layers live on.
fn commit_url(config: &Config, repo: Option<&str>, commit: &str) -> Option<String> {
    let url = config.repos.get(repo?)?.url.trim_end_matches('/');
    let cgit = [
        "https://git.openembedded.org/",
        "https://git.yoctoproject.org/",
    ];
    cgit.iter()
        .any(|host| url.starts_with(host))
        .then(|| format!("{url}/commit/?id={commit}"))
}

/// The bodnuti.dev logo, shipped with the site so the pages need nothing
/// from outside it, and work when opened from `state/site/`.
const LOGO: &str = include_str!("../assets/logo.svg");

const STYLE: &str = r#"
  :root { --bg: #fafaf7; --fg: #1d1d1f; --muted: #6b6b70; --accent: #1f4e8c;
          --ok: #2d7a3a; --warn: #9a6a00; --error: #b3261e; --line: #e2e2dc;
          --card: #ffffff; --code: #f3f3ee; --here: #fbecc8; }
  @media (prefers-color-scheme: dark) {
    :root { --bg: #111214; --fg: #e8e8ea; --muted: #9a9aa0; --accent: #7fa8e0;
            --ok: #7cc68a; --warn: #e0b660; --error: #f08a80; --line: #2a2b2f;
            --card: #17181b; --code: #0c0d0f; --here: #3a3220; }
  }
  * { box-sizing: border-box; }
  body { margin: 0; background: var(--bg); color: var(--fg);
         font: 16px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; padding: 2rem 1rem; }
  main { max-width: 90rem; margin: 0 auto; }
  h1 { font: 600 1.8rem/1.1 ui-monospace, "SF Mono", Menlo, monospace; margin: 0 0 .25rem; }
  h1 a { color: inherit; text-decoration: none; display: inline-flex; align-items: center; gap: .5rem; }
  h2 { font-size: 1.1rem; margin: 1.5rem 0 .5rem; overflow-wrap: anywhere; }
  h3 { font-size: 1rem; font-weight: 500; margin: 1.25rem 0 .4rem; overflow-wrap: anywhere; }
  .stitch { border: 0; border-top: 3px dashed var(--accent); margin: 1rem 0 1.5rem; width: 6rem; }
  .muted, .tag { color: var(--muted); }
  a { color: var(--accent); }
  table { border-collapse: collapse; width: 100%; font-size: .92rem; }
  th, td { text-align: left; vertical-align: top; padding: .35rem .5rem; border-bottom: 1px solid var(--line); }
  th { color: var(--muted); font-weight: 500; }
  td.subject { overflow-wrap: anywhere; }
  td.when, td.list, td.result, td.applied { white-space: nowrap; }
  table.index { table-layout: fixed; font-size: .85rem; }
  table.index th, table.index td { padding: .3rem .5rem; }
  table.index tr:hover td { background: var(--card); }
  table.index th.when { width: 8.5rem; } table.index th.list { width: 11rem; }
  table.index th.author { width: 10rem; } table.index th.result { width: 9rem; }
  table.index th.applied { width: 8rem; }
  table.index td.list, table.index td.author { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .clean { color: var(--ok); } .warn { color: var(--warn); } .error { color: var(--error); }
  code, pre { font: .88rem/1.45 ui-monospace, "SF Mono", Menlo, monospace; }
  .superseded { opacity: .6; }
  .revision { border-left: 3px solid var(--line); padding-left: .75rem; margin: .5rem 0; }
  .finding { border: 1px solid var(--line); border-left: 4px solid var(--muted);
             border-radius: 4px; padding: .5rem .75rem; margin: .6rem 0; background: var(--card); }
  .finding.sev-error { border-left-color: var(--error); }
  .finding.sev-warn { border-left-color: var(--warn); }
  .finding.sev-fixed { border-left-color: var(--ok); }
  .finding-head { display: flex; flex-wrap: wrap; gap: .3rem .6rem; align-items: baseline; }
  .badge { font: 600 .7rem/1.7 ui-monospace, "SF Mono", Menlo, monospace; text-transform: uppercase;
           padding: 0 .45rem; border-radius: 3px; color: var(--bg); background: var(--muted); }
  .sev-error .badge { background: var(--error); } .sev-warn .badge { background: var(--warn); }
  .sev-fixed .badge { background: var(--ok); }
  .tool { font-weight: 600; } .where { color: var(--muted); overflow-wrap: anywhere; }
  .finding .message { white-space: pre-wrap; overflow-wrap: anywhere; margin-top: .35rem; }
  pre.excerpt { background: var(--code); border: 1px solid var(--line); border-radius: 4px;
                padding: .35rem 0; margin: .5rem 0 0; overflow-x: auto; font-size: .8rem; }
  .excerpt .line { display: block; padding: 0 .6rem; white-space: pre; }
  .excerpt .here { background: var(--here); }
  details.style { margin: .4rem 0 .8rem; }
  details.style > summary { cursor: pointer; color: var(--muted); font-size: .9rem; }
  details.style[open] > summary { margin-bottom: .2rem; }
  .excerpt .ln { display: inline-block; min-width: 3.5em; padding-right: .8rem; text-align: right;
                 color: var(--muted); user-select: none; }
  dl { display: grid; grid-template-columns: max-content 1fr; gap: .2rem 1rem; }
  dt { color: var(--muted); } dd { margin: 0; overflow-wrap: anywhere; }
  footer { margin-top: 2.5rem; font-size: .85rem; color: var(--muted); }
  @media (max-width: 40rem) { .wide { display: none; }
    table.index { table-layout: auto; } table.index th { width: auto !important; }
    table.index td.when { white-space: normal; } }
"#;

/// The page frame. `root` is the relative path back to the site root.
fn page(title: &str, root: &str, body: &str, generated: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n<link rel=\"icon\" href=\"{root}logo.svg\" type=\"image/svg+xml\">\n\
         <style>{STYLE}</style>\n</head>\n<body>\n<main>\n\
         <h1><a href=\"{root}\"><img src=\"{root}logo.svg\" alt=\"\" width=\"40\" height=\"40\"> bodnuti</a></h1>\n\
         <p class=\"tag\">Deterministic checks of Yocto Project &amp; OpenEmbedded patches.</p>\n\
         <hr class=\"stitch\">\n{body}\
         <footer>Generated {generated} UTC. \
         Independent project; not affiliated with or endorsed by the Yocto Project or the Linux Foundation.\
         </footer>\n</main>\n</body>\n</html>\n"
    )
}

fn index_page(rows: &[Row], list: Option<&str>, generated: &str) -> String {
    let refs: Vec<&Row> = rows.iter().collect();
    index_page_of(&refs, list, generated)
}

/// The series table, for every list (`list` is `None`) or for one.
fn index_page_of(rows: &[&Row], list: Option<&str>, generated: &str) -> String {
    let root = if list.is_some() { "../" } else { "./" };
    let mut body = String::new();
    match list {
        Some(name) => {
            let name = escape(name);
            let _ = writeln!(
                body,
                "<h2>{name}</h2>\n<p class=\"muted\">Series on \
                 <a href=\"{LORE}/{name}/\">{name}</a>, newest first. \
                 <a href=\"../\">All lists</a>.</p>"
            );
        }
        None => body
            .push_str("<p class=\"muted\">Patch series on the watched lists, newest first.</p>\n"),
    }
    if rows.is_empty() {
        body.push_str("<p>No series yet.</p>\n");
        return page("bodnuti", root, &body, generated);
    }
    // Every column but Series has a fixed width, as on sashiko.dev, so the
    // subject gets the rest of the row instead of wrapping.
    body.push_str("<table class=\"index\">\n<tr><th class=\"when\">Last message</th>");
    if list.is_none() {
        body.push_str("<th class=\"wide list\">List</th>");
    }
    body.push_str(
        "<th class=\"subject\">Series</th><th class=\"wide author\">Author</th>\
         <th class=\"result\">Result</th><th class=\"wide applied\">Upstream</th></tr>\n",
    );
    for row in rows {
        let _ = write!(
            body,
            "<tr><td class=\"when\">{}</td>",
            timestamp(&row.record.tracked.last_seen)
        );
        if list.is_none() {
            let name = escape(row.list);
            let _ = write!(
                body,
                "<td class=\"wide list\" title=\"{name}\"><a href=\"{name}/\">{name}</a></td>"
            );
        }
        let subject = escape(&row.record.tracked.subject);
        let prefix = if list.is_some() { "../" } else { "" };
        let subject = match row.page() {
            Some(page) => format!("<a href=\"{prefix}{}\">{subject}</a>", escape(&page)),
            None => subject,
        };
        let note = row.revised_note();
        let subject = if note.is_empty() {
            subject
        } else {
            format!("{subject} <span class=\"muted\">({})</span>", escape(&note))
        };
        let (class, label) = row.result();
        let author = escape(row.author().unwrap_or(""));
        let _ = writeln!(
            body,
            "<td class=\"subject\">{subject}</td><td class=\"wide author\" title=\"{author}\">{author}</td>\
             <td class=\"result {class}\">{label}</td><td class=\"wide applied\">{}</td></tr>",
            row.applied()
        );
    }
    body.push_str("</table>\n");
    let title = match list {
        Some(name) => format!("bodnuti: {}", escape(name)),
        None => "bodnuti".into(),
    };
    page(&title, root, &body, generated)
}

fn series_page(config: &Config, row: &Row, revised: Option<&Revised>, generated: &str) -> String {
    let record = row.record;
    let list = escape(row.list);
    let subject = escape(&record.tracked.subject);
    let mut body = format!("<h2>{subject}</h2>\n<dl>\n");
    let mut item = |key: &str, value: String| {
        let _ = writeln!(body, "<dt>{key}</dt><dd>{value}</dd>");
    };
    item("List", format!("<a href=\"../\">{list}</a>"));
    if let Some(of) = revised {
        let subject = escape(&of.subject);
        let series = match &of.page {
            Some(page) => format!("<a href=\"../../{}\">{subject}</a>", escape(page)),
            None => subject,
        };
        item(
            "Revises",
            format!("patch {}/{} of {series}", of.patch, of.total),
        );
    }
    let note = row.revised_note();
    if !note.is_empty() {
        item("Revisions", escape(&note));
    }
    item(
        "Thread",
        format!(
            "<a href=\"{}\">lore</a>",
            escape(&lore_url(row.list, &record.tracked.root))
        ),
    );
    if let Some(author) = row.author() {
        item("Author", escape(author));
    }
    item("Last message", timestamp(&record.tracked.last_seen));
    let style = row.report.as_ref().map_or(0, |r| r.style.len());
    let style = if style == 0 {
        String::new()
    } else {
        format!(
            " <span class=\"muted\">· {}</span>",
            plural(style, "style finding")
        )
    };
    let (class, label) = row.result();
    let label = match row.duplicate_of().filter(|_| row.original.is_some()) {
        Some(key) if lore::safe_id(key.split_once('/').map_or("", |(_, r)| r)) => {
            format!("<a href=\"../../{}/\">{label}</a>", escape(key))
        }
        _ => label,
    };
    item(
        "Result",
        format!("<span class=\"{class}\">{label}</span>{style}"),
    );
    if let Some(repo) = &record.repo {
        let branch = record.branch.as_deref().unwrap_or("");
        item("Checked against", escape(&format!("{repo} {branch}")));
    }
    if let Some(report) = &row.report {
        let staged = report
            .staged
            .as_ref()
            .map(|s| format!(" ({})", escape(&s.branch)))
            .unwrap_or_default();
        item(
            "Base",
            format!(
                "<code>{}</code>{staged}",
                escape(&report.base[..report.base.len().min(12)])
            ),
        );
        if let Some(s) = &report.staged {
            item("Staging", escape(&s.note()));
        }
    }
    if let Some(reason) = &record.reason {
        item("Note", escape(reason));
    }
    let applied = row.applied();
    if !applied.is_empty() {
        item("Upstream", applied);
    }
    if !record.ci.is_empty() {
        let passed = record
            .ci
            .iter()
            .filter(|r| r.conclusion == "success")
            .count();
        let failed = record
            .ci
            .iter()
            .filter(|r| r.conclusion == "failure")
            .count();
        item(
            "GitHub CI",
            format!(
                "{}: {passed} passed, {failed} failed",
                plural(record.ci.len(), "run")
            ),
        );
    }
    body.push_str("</dl>\n");
    if let Some(report) = &row.report {
        body.push_str(&findings_html(
            config,
            record,
            row.list,
            report,
            &row.revisions,
        ));
        body.push_str("<p class=\"muted\"><a href=\"report.txt\">Text report</a></p>\n");
    }
    body.push_str(&ci_html(record));
    let title = format!("bodnuti: {subject}");
    page(&title, "../../", &body, generated)
}

/// The GitHub CI runs that carried the series (designs/PHASE2.md §5.8),
/// newest first. A run tests every patch on the branch at once, so a
/// failure is the series' own only when two runs tied it to the series.
fn ci_html(record: &SeriesRecord) -> String {
    if record.ci.is_empty() {
        return String::new();
    }
    let mut runs: Vec<&crate::ci::CiRun> = record.ci.iter().collect();
    runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let mut out = String::from(
        "<h3>GitHub CI</h3>\n<p class=\"muted\">Runs on master-next that carried this series. \
         Each tests every patch on the branch; a failure is shown as this series' only when two \
         runs failed newly carrying it.</p>\n<ul class=\"ci\">\n",
    );
    for r in runs {
        let class = match r.conclusion.as_str() {
            "success" => "clean",
            "failure" if r.tied => "error",
            "failure" => "warn",
            _ => "muted",
        };
        let patches = r
            .patches
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let whose = match (r.conclusion.as_str(), r.tied) {
            ("failure", true) => " · failed newly with this series in two runs".to_string(),
            ("failure", false) => format!(" · with {} series on the branch", r.series),
            _ => String::new(),
        };
        let _ = writeln!(
            out,
            "<li>{} <a href=\"{}\">{} {}</a>: <span class=\"{class}\">{}</span> \
             <span class=\"muted\">(patches {patches}{whose})</span></li>",
            timestamp(&r.created_at),
            escape(&r.url),
            escape(&r.repo),
            escape(&r.workflow),
            escape(&r.conclusion),
        );
    }
    out.push_str("</ul>\n");
    out
}

/// The series' patches, each with its findings and where it landed.
/// A revision under the patch it revises: its subject, linked to its own
/// page, when it was sent, its result and its findings.
fn revision_html(rev: &Revision) -> String {
    let row = &rev.row;
    let subject = escape(&row.record.tracked.subject);
    let subject = match row.page() {
        Some(_) => format!(
            "<a href=\"../{}/\">{subject}</a>",
            escape(&row.record.tracked.root)
        ),
        None => subject,
    };
    let outcome = row.outcome();
    let mut out = format!(
        "<div class=\"revision\">\n<p>Revised in {subject}, {}: <span class=\"{}\">{}</span></p>\n",
        timestamp(&row.record.tracked.last_seen),
        outcome.class(),
        outcome.label()
    );
    if let Some(report) = &row.report {
        let findings: Vec<&Finding> = report.findings.iter().collect();
        out.push_str(&finding_list(row.record, report, &findings));
    }
    out.push_str("</div>\n");
    out
}

fn findings_html(
    config: &Config,
    record: &SeriesRecord,
    list: &str,
    report: &Report,
    revisions: &[Revision],
) -> String {
    let mut out = String::new();
    // The cover letter is the thread link above.
    for patch in &report.series.patches {
        let subject = escape(&patch.subject);
        let title = match &patch.message_id {
            Some(id) => format!("<a href=\"{}\">{subject}</a>", escape(&lore_url(list, id))),
            None => subject,
        };
        let _ = writeln!(out, "<h3>{title}</h3>");
        if let Some(landed) = record.applied.get(&patch.index) {
            let short = escape(&landed.commit[..landed.commit.len().min(12)]);
            let commit = match commit_url(config, record.repo.as_deref(), &landed.commit) {
                Some(url) => format!("<a href=\"{}\"><code>{short}</code></a>", escape(&url)),
                None => format!("<code>{short}</code>"),
            };
            let how = if landed.exact {
                "applied"
            } else {
                "probably applied"
            };
            let _ = writeln!(
                out,
                "<p class=\"clean\">{how} to {} as {commit}</p>",
                escape(&landed.branch)
            );
        }
        let findings: Vec<&Finding> = report
            .findings
            .iter()
            .filter(|f| f.patch == Some(patch.index))
            .collect();
        let revised: Vec<&Revision> = revisions
            .iter()
            .filter(|r| r.patch == patch.index)
            .collect();
        if revised.is_empty() {
            out.push_str(&finding_list(record, report, &findings));
        } else {
            // The history in order: what this revision found, then each
            // later revision (§6.3).
            if !findings.is_empty() {
                out.push_str("<div class=\"superseded\">\n<p class=\"muted\">Superseded by a later revision:</p>\n");
                out.push_str(&finding_list(record, report, &findings));
                out.push_str("</div>\n");
            }
            for rev in revised {
                out.push_str(&revision_html(rev));
            }
        }
        out.push_str(&style_list(record, report, Some(patch.index)));
    }
    let series: Vec<&Finding> = report
        .findings
        .iter()
        .filter(|f| f.patch.is_none())
        .collect();
    let series_style = style_list(record, report, None);
    if !series.is_empty() || !series_style.is_empty() {
        out.push_str("<h3>Series</h3>\n");
        out.push_str(&finding_list(record, report, &series));
        out.push_str(&series_style);
    }
    if !report.fixed.is_empty() {
        out.push_str("<h3>Fixes</h3>\n");
        for f in &report.fixed {
            // A fix's line is in the tree before the series; no excerpt.
            out.push_str(&finding_block(report, f, "fixed", "sev-fixed", false));
        }
    }
    if !report.tool_errors.is_empty() {
        out.push_str("<h3>Not checked</h3>\n");
        for e in &report.tool_errors {
            let _ = writeln!(
                out,
                "<div class=\"finding toolerror\">\n<div class=\"finding-head\">\
                 <span class=\"badge\">not run</span> <span class=\"tool\">{}</span></div>\n\
                 <div class=\"message\">{}</div>\n</div>",
                tool_name(e.tool),
                escape(e.message.trim())
            );
        }
    }
    if let Some(c) = &report.patchtest {
        let _ = writeln!(
            out,
            "<p class=\"muted\">patchtest: {} pass, {} fail, {} skip</p>",
            c.pass, c.fail, c.skip
        );
    }
    out
}

fn finding_list(record: &SeriesRecord, report: &Report, findings: &[&Finding]) -> String {
    let mut out = String::new();
    for f in findings {
        if already_applied(record, f) {
            out.push_str(
                "<p class=\"muted\">Did not apply to the base, which already has it.</p>\n",
            );
            continue;
        }
        // Classes of their own: the page's .error and .warn colour text,
        // which would colour the whole block.
        let class = match f.severity {
            Severity::Error => "sev-error",
            Severity::Warning => "sev-warn",
            Severity::Info => "sev-info",
        };
        let badge = format!("{:?}", f.severity).to_lowercase();
        out.push_str(&finding_block(report, f, &badge, class, true));
    }
    out
}

/// A patch's style findings (`patch` None: the series'), collapsed in a
/// `<details>` element that opens without JavaScript (designs/PHASE2.md
/// §6.1.1). Empty when there are none.
fn style_list(record: &SeriesRecord, report: &Report, patch: Option<u32>) -> String {
    let style: Vec<&Finding> = report.style.iter().filter(|f| f.patch == patch).collect();
    if style.is_empty() {
        return String::new();
    }
    format!(
        "<details class=\"style\"><summary>Show {}</summary>\n{}</details>\n",
        plural(style.len(), "style finding"),
        finding_list(record, report, &style)
    )
}

/// One finding as a block (designs/PHASE2.md §6.1): a header with the
/// badge, tool, check id and location; the message with its line breaks;
/// and, with `excerpt`, the recorded lines of the patched file around it.
fn finding_block(report: &Report, f: &Finding, badge: &str, class: &str, excerpt: bool) -> String {
    let mut out = format!("<div class=\"finding {class}\">\n<div class=\"finding-head\">");
    let _ = write!(
        out,
        "<span class=\"badge\">{}</span> <span class=\"tool\">{}</span> <code class=\"check\">{}</code>",
        escape(badge),
        tool_name(f.tool),
        escape(&f.check)
    );
    if let Some(place) = location(f) {
        let _ = write!(out, " <span class=\"where\">{}</span>", escape(&place));
    }
    out.push_str("</div>\n");
    let _ = writeln!(
        out,
        "<div class=\"message\">{}</div>",
        escape(f.message.trim())
    );
    if let Some(e) = report.excerpt(f).filter(|_| excerpt) {
        out.push_str("<pre class=\"excerpt\">");
        for (number, text, here) in numbered(e, f.line) {
            let class = if here { "line here" } else { "line" };
            let _ = write!(
                out,
                "<span class=\"{class}\"><span class=\"ln\">{number}</span>{}</span>",
                escape(text)
            );
        }
        out.push_str("</pre>\n");
    }
    out.push_str("</div>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Tool;
    use crate::series::{Patch, Series};
    use crate::track::Tracked;
    use crate::watch::Landed;

    fn record(status: Status, applied: &[u32]) -> SeriesRecord {
        let json = serde_json::json!({
            "root": "1@x",
            "subject": "[PATCH 1/1] <script>alert(1)</script>",
            "total": 1,
            "seen": [1],
            "last_seen": "2026-10-23T10:28:30Z",
            "status": status,
            "attempts": 0,
        });
        let mut r: SeriesRecord = serde_json::from_value(json).unwrap();
        for &k in applied {
            r.applied.insert(
                k,
                Landed {
                    commit: "abc".into(),
                    branch: "master".into(),
                    exact: true,
                },
            );
        }
        r
    }

    fn report(findings: Vec<Finding>) -> Report {
        Report {
            series: Series {
                cover: None,
                patches: vec![Patch {
                    subject: "[PATCH 1/1] x".into(),
                    message_id: Some("1@x".into()),
                    tags: vec![],
                    version: 1,
                    index: 1,
                    total: 1,
                    from: Some("\"Ann <b>Dev</b>\" <ann@x>".into()),
                    raw: vec![],
                }],
            },
            base: "c9b000059a37241c5b97426f0aadc012b13bd20f".into(),
            applied: vec![],
            findings,
            tool_errors: vec![],
            patchtest: None,
            fixed: vec![],
            style: vec![],
            excerpts: Default::default(),
            staged: None,
        }
    }

    fn apply_failure() -> Finding {
        Finding {
            tool: Tool::Bodnuti,
            check: APPLY_CHECK.into(),
            severity: Severity::Error,
            patch: Some(1),
            file: None,
            line: None,
            message: "does not apply".into(),
        }
    }

    #[test]
    fn escapes_mail_text() {
        assert_eq!(
            escape("<a href=\"x\">&'"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
        assert_eq!(
            escape("{{env \"HOME\"}}"),
            "&#123;&#123;env &quot;HOME&quot;&#125;&#125;"
        );
        let rec = record(Status::Checked, &[]);
        let row = Row {
            list: "openembedded-core",
            record: &rec,
            report: Some(report(vec![])),
            revisions: Vec::new(),
            original: None,
        };
        let html = index_page(&[row], None, "now");
        assert!(!html.contains("<script>"), "{html}");
        assert!(!html.contains("<b>"), "{html}");
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn style_findings_are_collapsed() {
        let mut f = apply_failure();
        f.tool = Tool::Oelint;
        f.check = "oelint.vars.dependsordered".into();
        let mut r = report(vec![]);
        r.style = vec![f.clone(), f];
        let rec = record(Status::Checked, &[]);
        let html = style_list(&rec, &r, Some(1));
        assert!(
            html.starts_with("<details class=\"style\"><summary>Show 2 style findings</summary>"),
            "{html}"
        );
        assert!(style_list(&rec, &r, Some(2)).is_empty());
    }

    #[test]
    fn a_finding_is_a_block_with_its_excerpt() {
        let mut f = apply_failure();
        f.tool = Tool::Shellcheck;
        f.check = "SC3037".into();
        f.severity = Severity::Warning;
        f.file = Some("a.sh".into());
        f.line = Some(2);
        f.message = "echo flags <b>are</b> undefined\nin POSIX sh".into();
        let mut r = report(vec![f]);
        r.excerpts.insert(
            crate::report::excerpt_key("a.sh", 2),
            crate::report::Excerpt {
                first: 1,
                lines: vec!["#!/bin/sh".into(), "echo -e '<script>'".into()],
            },
        );
        let rec = record(Status::Checked, &[]);
        let html = finding_list(&rec, &r, &r.findings.iter().collect::<Vec<_>>());
        assert!(
            html.starts_with("<div class=\"finding sev-warn\">"),
            "{html}"
        );
        assert!(
            html.contains("<span class=\"badge\">warning</span>"),
            "{html}"
        );
        assert!(
            html.contains("<span class=\"where\">a.sh:2</span>"),
            "{html}"
        );
        assert!(
            html.contains("echo flags &lt;b&gt;are&lt;/b&gt; undefined\nin POSIX sh"),
            "{html}"
        );
        assert!(html.contains(
            "<span class=\"line here\"><span class=\"ln\">2</span>echo -e &#39;&lt;script&gt;&#39;</span>"
        ), "{html}");
        assert!(
            !html.contains("<script>") && !html.contains("<b>"),
            "{html}"
        );
    }

    #[test]
    fn an_apply_failure_of_a_landed_patch_is_not_a_finding() {
        let landed = record(Status::Applied, &[1]);
        let row = Row {
            list: "l",
            record: &landed,
            report: Some(report(vec![apply_failure()])),
            revisions: Vec::new(),
            original: None,
        };
        assert_eq!(row.outcome(), Outcome::AlreadyApplied);
        assert_eq!(row.applied(), "applied");

        let pending = record(Status::Checked, &[]);
        let row = Row {
            list: "l",
            record: &pending,
            report: Some(report(vec![apply_failure()])),
            revisions: Vec::new(),
            original: None,
        };
        assert_eq!(
            row.outcome(),
            Outcome::Findings {
                errors: 1,
                total: 1
            }
        );
    }

    #[test]
    fn labels_count_findings() {
        let one = Outcome::Findings {
            errors: 1,
            total: 1,
        };
        assert_eq!(one.label(), "1 finding, 1 error");
        let two = Outcome::Findings {
            errors: 0,
            total: 2,
        };
        assert_eq!(two.label(), "2 findings");
    }

    #[test]
    fn outcomes_follow_the_status() {
        for (status, outcome) in [
            (Status::Seen, Outcome::Waiting),
            (Status::Unrouted, Outcome::Unrouted),
            (Status::Duplicate, Outcome::Resend),
            (Status::Failed, Outcome::Failed),
            (Status::Checked, Outcome::NotChecked),
        ] {
            let rec = record(status, &[]);
            let row = Row {
                list: "l",
                record: &rec,
                report: None,
                revisions: Vec::new(),
                original: None,
            };
            assert_eq!(row.outcome(), outcome, "{status:?}");
        }
    }

    #[test]
    fn links_and_times() {
        assert_eq!(
            lore_url("openembedded-core", "a/b@x"),
            "https://lore.kernel.org/openembedded-core/a%2Fb@x/"
        );
        assert_eq!(timestamp("2026-10-23T10:28:30Z"), "2026-10-23 10:28");
        assert_eq!(timestamp("20261024000000"), "2026-10-24 00:00");
    }

    #[test]
    fn renders_and_retains_recent_series() {
        let dir = tempfile::tempdir().unwrap();
        let text = format!(
            "image = \"i\"\nstate = \"{0}\"\ncache = \"{0}\"\nretain_months = 1\n\
             [[list]]\nname = \"openembedded-core\"\nrepo = \"openembedded-core\"\n\
             [repos]\nopenembedded-core = \"https://git.openembedded.org/openembedded-core\"\n",
            dir.path().display()
        );
        let path = dir.path().join("watch.toml");
        std::fs::write(&path, text).unwrap();
        let config = Config::load(&path).unwrap();

        let mut state = ListState::default();
        let mut recent = record(Status::Seen, &[]);
        recent.tracked = Tracked {
            root: "new@x".into(),
            ..recent.tracked
        };
        let mut old = record(Status::Seen, &[]);
        old.tracked = Tracked {
            root: "old@x".into(),
            last_seen: "2026-01-01T00:00:00Z".into(),
            ..old.tracked
        };
        state.series.insert("new@x".into(), recent);
        state.series.insert("old@x".into(), old);
        let states = BTreeMap::from([("openembedded-core".to_string(), state)]);

        let now = lore::parse_time("2026-10-24T00:00:00Z").unwrap();
        let site = render(&config, &states, now).unwrap();
        assert!(site.join("index.html").exists());
        assert!(site.join("logo.svg").exists());
        let summary = std::fs::read_to_string(site.join("summary.html")).unwrap();
        assert!(
            summary.starts_with("<section class=\"bodnuti-summary\">"),
            "{summary}"
        );
        assert!(
            summary.contains(
                "<td><a href=\"/reports/openembedded-core/\">openembedded-core</a></td><td>1</td>"
            ),
            "{summary}"
        );
        assert!(
            summary.contains("href=\"/reports/openembedded-core/new@x/\""),
            "{summary}"
        );
        assert!(!summary.contains("<script>"), "{summary}");
        assert!(site.join("openembedded-core/index.html").exists());
        assert!(site.join("openembedded-core/new@x/index.html").exists());
        assert!(!site.join("openembedded-core/old@x").exists());
        // A second run replaces the site.
        render(&config, &states, now).unwrap();
        assert!(!dir.path().join(".site.tmp").exists());
    }

    #[test]
    fn lists_the_ci_runs_that_carried_a_series() {
        let run = |id: u64, conclusion: &str, tied: bool| crate::ci::CiRun {
            repo: "moto-timo/layerindex-web".into(),
            workflow: "CI".into(),
            id,
            url: format!("https://github.com/moto-timo/layerindex-web/actions/runs/{id}"),
            conclusion: conclusion.into(),
            created_at: format!("2026-10-2{id}T02:00:00Z"),
            patches: vec![1, 2],
            series: 3,
            tied,
        };
        let mut rec = record(Status::Checked, &[]);
        rec.ci = vec![
            run(4, "success", false),
            run(5, "failure", false),
            run(6, "failure", true),
        ];
        let html = ci_html(&rec);
        // Newest first; an untied failure is the branch's, a tied one this series'.
        let first = html.find("runs/6").unwrap();
        assert!(first < html.find("runs/5").unwrap(), "{html}");
        assert!(
            html.contains("<span class=\"error\">failure</span>"),
            "{html}"
        );
        assert!(
            html.contains("<span class=\"warn\">failure</span>"),
            "{html}"
        );
        assert!(html.contains("with 3 series on the branch"), "{html}");
        assert!(
            html.contains("failed newly with this series in two runs"),
            "{html}"
        );
        assert!(
            html.contains("<span class=\"clean\">success</span>"),
            "{html}"
        );
    }

    #[test]
    fn a_resend_shows_its_original_result() {
        let dir = tempfile::tempdir().unwrap();
        let text = format!(
            "image = \"i\"\nstate = \"{0}\"\ncache = \"{0}\"\n\
             [[list]]\nname = \"openembedded-core\"\nrepo = \"openembedded-core\"\n\
             [repos]\nopenembedded-core = \"https://git.openembedded.org/openembedded-core\"\n",
            dir.path().display()
        );
        let path = dir.path().join("watch.toml");
        std::fs::write(&path, text).unwrap();
        let config = Config::load(&path).unwrap();
        let d = dir.path().join("series/openembedded-core/1@x");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("report.json"),
            serde_json::to_vec(&report(Vec::new())).unwrap(),
        )
        .unwrap();
        let mut resend = record(Status::Duplicate, &[]);
        resend.tracked = Tracked {
            root: "2@x".into(),
            last_seen: "2026-10-23T12:00:00Z".into(),
            ..resend.tracked
        };
        // Older state names the original only in the reason.
        resend.reason =
            Some("every patch was already checked for master in openembedded-core/1@x".into());
        let mut state = ListState::default();
        state
            .series
            .insert("1@x".into(), record(Status::Checked, &[]));
        state.series.insert("2@x".into(), resend);
        let states = BTreeMap::from([("openembedded-core".to_string(), state)]);
        let now = lore::parse_time("2026-10-24T00:00:00Z").unwrap();
        let site = render(&config, &states, now).unwrap();
        let index = std::fs::read_to_string(site.join("openembedded-core/index.html")).unwrap();
        assert!(
            index.contains("<td class=\"result clean\">resend: clean</td>"),
            "{index}"
        );
        let page = std::fs::read_to_string(site.join("openembedded-core/2@x/index.html")).unwrap();
        assert!(
            page.contains("<a href=\"../../openembedded-core/1@x/\">resend: clean</a>"),
            "{page}"
        );
    }

    #[test]
    fn nests_a_revision_under_the_patch_it_revises() {
        let dir = tempfile::tempdir().unwrap();
        let text = format!(
            "image = \"i\"\nstate = \"{0}\"\ncache = \"{0}\"\n\
             [[list]]\nname = \"openembedded-core\"\nrepo = \"openembedded-core\"\n\
             [repos]\nopenembedded-core = \"https://git.openembedded.org/openembedded-core\"\n",
            dir.path().display()
        );
        let path = dir.path().join("watch.toml");
        std::fs::write(&path, text).unwrap();
        let config = Config::load(&path).unwrap();
        let save = |root: &str, report: &Report| {
            let d = dir.path().join("series/openembedded-core").join(root);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("report.json"), serde_json::to_vec(report).unwrap()).unwrap();
        };
        // v1: its only finding is the AUH author of patch 1.
        let auh = Finding {
            tool: Tool::Patchtest,
            check: "test_mbox.test_non_auh_upgrade".into(),
            severity: Severity::Error,
            patch: Some(1),
            file: None,
            line: None,
            message: "Commit message contains AUH email address".into(),
        };
        save("1@x", &report(vec![auh]));
        let v1 = record(Status::Checked, &[]);
        // v2 of patch 1, sent as a reply to it: clean.
        save("v2@x", &report(Vec::new()));
        let mut v2 = record(Status::Checked, &[]);
        v2.tracked = Tracked {
            root: "v2@x".into(),
            subject: "[PATCH v2] x".into(),
            last_seen: "2026-10-23T11:00:00Z".into(),
            revises: Some(crate::track::Revises {
                series: "1@x".into(),
                message: "1@x".into(),
            }),
            ..v2.tracked
        };
        let mut state = ListState::default();
        state.series.insert("1@x".into(), v1);
        state.series.insert("v2@x".into(), v2);
        let states = BTreeMap::from([("openembedded-core".to_string(), state)]);
        let now = lore::parse_time("2026-10-24T00:00:00Z").unwrap();
        let site = render(&config, &states, now).unwrap();

        let index = std::fs::read_to_string(site.join("openembedded-core/index.html")).unwrap();
        // One row, for the series as it stands.
        assert_eq!(
            index.matches("<td class=\"subject\">").count(),
            1,
            "{index}"
        );
        assert!(index.contains("(1/1 revised in v2)"), "{index}");
        assert!(
            index.contains("<td class=\"result clean\">clean</td>"),
            "{index}"
        );
        let page = std::fs::read_to_string(site.join("openembedded-core/1@x/index.html")).unwrap();
        assert!(page.contains("Superseded by a later revision"), "{page}");
        assert!(
            page.contains("<a href=\"../v2@x/\">[PATCH v2] x</a>"),
            "{page}"
        );
        let v2page =
            std::fs::read_to_string(site.join("openembedded-core/v2@x/index.html")).unwrap();
        assert!(
            v2page.contains(
                "<dt>Revises</dt><dd>patch 1/1 of <a href=\"../../openembedded-core/1@x/\">"
            ),
            "{v2page}"
        );
    }
}
