//! `git request-pull` cover letters (designs/PHASE2.md §5.0).
//!
//! A pull request names the commits to check instead of carrying them as
//! patches:
//!
//! ```text
//! The following changes since commit 47d7dbdd10890bbd429bc0cccbe14435c37e5af1:
//! are available in the Git repository at:
//!   https://git.openembedded.org/meta-openembedded-contrib anujm/wrynose
//! for you to fetch changes up to f28a144aa0a0ae05ca2ee91e1aef6aa05f6a12db:
//! ```
//!
//! Everything here comes from mail and ends up in a `git fetch` command, so
//! the repository must be on a known host and every field is validated.

/// The hosts pull requests may be fetched from.
const HOSTS: &[&str] = &[
    "https://git.openembedded.org/",
    "https://git.yoctoproject.org/",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    /// The commit the changes are based on: 40 hex digits.
    pub base: String,
    /// The commit the changes end at: 40 hex digits.
    pub head: String,
    /// An `https://` URL on one of `HOSTS`.
    pub url: String,
    /// A plain branch name, without `refs/heads/`.
    pub branch: String,
}

/// Why a cover letter that looks like a pull request cannot be fetched.
#[derive(Debug, PartialEq, Eq)]
pub enum Invalid {
    /// Not `git request-pull` output.
    NotAPullRequest,
    /// A field is missing or not in the expected form.
    Malformed(&'static str),
    /// The repository is not on a host pull requests are fetched from.
    UntrustedHost(String),
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Invalid::NotAPullRequest => f.write_str("not a git request-pull message"),
            Invalid::Malformed(what) => write!(f, "pull request has no valid {what}"),
            Invalid::UntrustedHost(url) => write!(
                f,
                "pull request repository {url} is not on {}",
                HOSTS.join(" or ")
            ),
        }
    }
}

impl std::error::Error for Invalid {}

/// Reads a pull request from a message body.
pub fn parse(body: &str) -> Result<PullRequest, Invalid> {
    let repo_marker = "are available in the Git repository at:";
    let Some(at) = body.find(repo_marker) else {
        return Err(Invalid::NotAPullRequest);
    };
    let base = after(body, "changes since commit")
        .and_then(object_name)
        .ok_or(Invalid::Malformed("base commit"))?;
    let head = after(body, "fetch changes up to")
        .and_then(object_name)
        .ok_or(Invalid::Malformed("head commit"))?;
    // The repository line is the first non-empty one after the marker:
    // "<url> <branch>", the two separated by any whitespace (mail clients
    // turn spaces into non-breaking ones).
    let line = body[at + repo_marker.len()..]
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .ok_or(Invalid::Malformed("repository"))?;
    let mut words = line.split(char::is_whitespace).filter(|w| !w.is_empty());
    let url = words.next().ok_or(Invalid::Malformed("repository"))?;
    let branch = words.next().ok_or(Invalid::Malformed("branch"))?;
    if !valid_url(url) {
        return Err(
            if url.starts_with("https://") && !HOSTS.iter().any(|h| url.starts_with(h)) {
                Invalid::UntrustedHost(url.to_string())
            } else {
                Invalid::Malformed("repository")
            },
        );
    }
    if !valid_branch(branch) {
        return Err(Invalid::Malformed("branch"));
    }
    Ok(PullRequest {
        base,
        head,
        url: url.to_string(),
        branch: branch.to_string(),
    })
}

fn after<'a>(body: &'a str, marker: &str) -> Option<&'a str> {
    body.find(marker).map(|i| &body[i + marker.len()..])
}

/// The first word of `text` that is a full object name, within its first
/// few words: the name may wrap onto the next line.
fn object_name(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || c == ':')
        .filter(|w| !w.is_empty())
        .take(3)
        .find(|w| w.len() == 40 && w.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
}

fn valid_url(url: &str) -> bool {
    HOSTS.iter().any(|host| {
        url.strip_prefix(host).is_some_and(|path| {
            !path.is_empty()
                && !path.contains("..")
                && path
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b))
        })
    })
}

/// A branch name git would accept that cannot be read as an option or a
/// revision range.
fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch.len() <= 200
        && !branch.starts_with(['-', '/', '.'])
        && !branch.ends_with(['/', '.'])
        && !branch.ends_with(".lock")
        && !branch.contains("..")
        && !branch.contains("//")
        && branch
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/+-".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WRYNOSE: &str = "Please merge these changes in wrynose.\n\n\
        The following changes since commit\n47d7dbdd10890bbd429bc0cccbe14435c37e5af1:\n\n\
        \x20 unbound: patch CVE-2026-56444 (2026-09-15 10:14:05 +0530)\n\n\
        are available in the Git repository at:\n\n\
        \x20 https://git.openembedded.org/meta-openembedded-contrib anujm/wrynose\n\
        \x20 https://git.openembedded.org/meta-openembedded-contrib/log/?h=anujm/wrynose\n\n\
        for you to fetch changes up to f28a144aa0a0ae05ca2ee91e1aef6aa05f6a12db:\n\n\
        \x20 vboxguestdrivers: fix vboxvideo build (2026-10-06 13:52:06 +0530)\n";

    #[test]
    fn reads_a_request_pull_message() {
        assert_eq!(
            parse(WRYNOSE).unwrap(),
            PullRequest {
                base: "47d7dbdd10890bbd429bc0cccbe14435c37e5af1".into(),
                head: "f28a144aa0a0ae05ca2ee91e1aef6aa05f6a12db".into(),
                url: "https://git.openembedded.org/meta-openembedded-contrib".into(),
                branch: "anujm/wrynose".into(),
            }
        );
        // A non-breaking space between URL and branch, as some mailers send.
        let nbsp = WRYNOSE.replace("contrib anujm", "contrib\u{a0}anujm");
        assert_eq!(parse(&nbsp).unwrap().branch, "anujm/wrynose");
    }

    #[test]
    fn rejects_what_git_fetch_must_not_see() {
        assert_eq!(parse("Please pull."), Err(Invalid::NotAPullRequest));
        let host = WRYNOSE.replace("https://git.openembedded.org/", "https://evil.example/");
        assert!(matches!(parse(&host), Err(Invalid::UntrustedHost(_))));
        for bad in [
            ("anujm/wrynose", "--upload-pack=x"),
            ("anujm/wrynose", "a..b"),
            ("anujm/wrynose", "a;b"),
            ("meta-openembedded-contrib", "x/../y"),
            ("https://git.openembedded.org/", "file:///etc/"),
            ("f28a144aa0a0ae05ca2ee91e1aef6aa05f6a12db", "f28a144"),
        ] {
            let text = WRYNOSE.replace(bad.0, bad.1);
            assert!(parse(&text).is_err(), "{bad:?}");
        }
    }
}
