//! New messages on a lore.kernel.org list, from its search Atom feed.
//!
//! `https://lore.kernel.org/<list>/?q=s:PATCH+dt:<since>..&x=A` returns, per
//! message, its subject, its URL (which carries the Message-ID) and the URL of
//! the message it replies to. That is everything grouping needs; message
//! bodies are fetched later, per series, with b4.

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::Event;

const LORE: &str = "https://lore.kernel.org";
const USER_AGENT: &str = concat!(
    "bodnuti/",
    env!("CARGO_PKG_VERSION"),
    " (+https://bodnuti.dev)"
);

/// One feed entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub message_id: String,
    pub subject: String,
    /// Message-ID of the parent message, if any.
    pub in_reply_to: Option<String>,
    /// RFC 3339 timestamp, as lore gives it.
    pub updated: String,
}

/// Fetches the patch messages on `list` since `since` (`YYYYMMDDhhmmss`,
/// UTC), newest first. lore returns at most 200 entries per page.
// ponytail: one page only; poll more often than 200 patches arrive per list.
pub fn fetch(list: &str, since: &str) -> Result<Vec<Entry>> {
    let url = format!("{LORE}/{list}/?q=s:PATCH+dt:{since}..&x=A");
    let body = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .call()
        .with_context(|| format!("fetching {url}"))?
        .into_string()
        .context("reading the lore feed")?;
    parse(&body)
}

/// Parses an Atom feed. Entries without a usable Message-ID are skipped.
pub fn parse(xml: &str) -> Result<Vec<Entry>> {
    let mut reader = Reader::from_str(xml);
    let mut entries = Vec::new();
    let mut current: Option<(Option<String>, String, Option<String>, String)> = None;
    let mut field: Option<&'static str> = None;
    loop {
        match reader.read_event().context("lore feed is not valid XML")? {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.name();
                match (name.as_ref(), current.as_mut()) {
                    (b"entry", _) => current = Some((None, String::new(), None, String::new())),
                    (b"title", Some(_)) => field = Some("title"),
                    (b"updated", Some(_)) => field = Some("updated"),
                    (b"link", Some(c)) if c.0.is_none() => {
                        c.0 = href(&e).as_deref().and_then(message_id)
                    }
                    (b"thr:in-reply-to", Some(c)) => c.2 = href(&e).as_deref().and_then(message_id),
                    _ => {}
                }
            }
            Event::Text(t) => {
                if let (Some(f), Some(c)) = (field, current.as_mut()) {
                    let text = t.unescape().context("bad text in lore feed")?;
                    match f {
                        "title" => c.1.push_str(&text),
                        _ => c.3.push_str(&text),
                    }
                }
            }
            Event::End(e) => match e.name().as_ref() {
                b"title" | b"updated" => field = None,
                b"entry" => {
                    if let Some((Some(message_id), subject, in_reply_to, updated)) = current.take()
                    {
                        let subject = subject.split_whitespace().collect::<Vec<_>>().join(" ");
                        entries.push(Entry {
                            message_id,
                            subject,
                            in_reply_to,
                            updated,
                        });
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(entries)
}

fn href(e: &quick_xml::events::BytesStart) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == b"href")
        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

/// `https://lore.kernel.org/<list>/<message-id>/` to `<message-id>`,
/// percent-decoded.
fn message_id(url: &str) -> Option<String> {
    let path = url.strip_prefix(LORE)?.trim_end_matches('/');
    let encoded = path.rsplit('/').next()?;
    let decoded = percent_decode(encoded)?;
    (!decoded.is_empty()).then_some(decoded)
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The UTC time `hours` ago as `YYYYMMDDhhmmss`, the form lore's `dt:`
/// prefix takes.
pub fn hours_ago(hours: u64) -> String {
    utc(now().saturating_sub(hours * 3600))
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Parses lore's `YYYY-MM-DDThh:mm:ssZ` timestamps to seconds since the
/// epoch. Other RFC 3339 forms are not used by lore and are rejected.
pub fn parse_time(t: &str) -> Option<u64> {
    let b = t.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[19] != b'Z' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| t.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    // Howard Hinnant's days-from-civil algorithm.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

/// Formats seconds since the epoch as `YYYYMMDDhhmmss`, UTC.
pub fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's days-to-civil algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Whether a Message-ID can be used as a file name. Message-IDs come from
/// untrusted mail; anything outside this set is rejected, not rewritten.
pub fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-+@=".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = include_str!("../tests/fixtures/lore-feed.xml");

    #[test]
    fn parses_a_real_feed() {
        let entries = parse(FEED).unwrap();
        assert_eq!(entries.len(), 12);
        let cover = entries
            .iter()
            .find(|e| e.subject == "[PATCH RFC 0/3] sstate: Replay warnings and errors when restoring from sstate")
            .unwrap();
        assert!(cover.in_reply_to.is_none());
        assert!(cover.message_id.contains('@'), "{}", cover.message_id);
        let member = entries
            .iter()
            .find(|e| e.subject.starts_with("[PATCH 2/3] pulseaudio"))
            .unwrap();
        assert!(member.in_reply_to.is_some());
        assert!(entries.iter().all(|e| e.updated.starts_with("2026-")));
    }

    #[test]
    fn message_ids_come_from_urls() {
        assert_eq!(
            message_id(
                "https://lore.kernel.org/openembedded-core/20261022154937.1328672-1-a@b.com/"
            ),
            Some("20261022154937.1328672-1-a@b.com".into())
        );
        assert_eq!(
            message_id("https://lore.kernel.org/x/abc%2Fdef%40host/").as_deref(),
            Some("abc/def@host")
        );
        assert_eq!(message_id("https://example.com/x/abc@host/"), None);
    }

    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(utc(0), "19700101000000");
        assert_eq!(utc(1_792_800_000), "20261024000000");
        assert_eq!(utc(951_782_400 + 3661), "20000229010101");
    }

    #[test]
    fn parses_lore_timestamps() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-10-24T00:00:00Z"), Some(1_792_800_000));
        let t = parse_time("2000-02-29T01:01:01Z").unwrap();
        assert_eq!(utc(t), "20000229010101");
        assert_eq!(parse_time("2026-10-24 00:00:00"), None);
    }

    #[test]
    fn unsafe_message_ids_are_rejected() {
        assert!(safe_id(
            "20261022154937.1328672-1-khem.raj@oss.qualcomm.com"
        ));
        assert!(!safe_id("abc/def@host"));
        assert!(!safe_id("../../etc/passwd@x"));
        assert!(!safe_id(".hidden@x"));
        assert!(!safe_id(""));
    }
}
