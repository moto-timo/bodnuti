#!/usr/bin/env python3
"""Build benchmark entries from v1/v2 pairs on a mailing list.

For each patch sent as v2 (or later) with a changelog that names a code
change, find its v1 by the same author, apply v1 to the branch as it was
when v1 was sent, and write an entry whose problem description is the v2
changelog. score.py then reviews v1 and its judge asks whether a finding
names what the reviewers had changed: whether the model comes to the same
conclusion the list did.

Each applied v1 is kept under refs/bench/v1/ in the clone so score.py can
read it. Mail is untrusted: it only reaches git am, never a shell, and
the worktree is a temporary directory. Run on a trusted clone. Standard
library only.
"""

from __future__ import annotations

import argparse
import html
import json
import re
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request
from pathlib import Path

UA = {"User-Agent": "bodnuti-bench"}
LORE = "https://lore.kernel.org"
# Changelogs about only the message or the submission, not the code.
NOT_CODE = re.compile(
    r"(?i)commit message|subject|shortlog|typo in (the )?(commit|message|log)|"
    r"signed-off|sign[- ]off|reword|rebase|resend|upstream-status|cc |link to v1"
)
VERSION = re.compile(r"\[[^\]]*PATCH[^\]]*?\bv(\d+)\b[^\]]*\]", re.I)


def get(url: str) -> str:
    time.sleep(0.5)  # be gentle with lore
    req = urllib.request.Request(url, headers=UA)
    return urllib.request.urlopen(req, timeout=60).read().decode(errors="replace")


def search(listname: str, query: str) -> list[tuple[str, str, str]]:
    """(title, url, updated) of each message matching query, newest first."""
    out = []
    for offset in (0, 200, 400):
        q = urllib.parse.quote(query)
        atom = get(f"{LORE}/{listname}/?q={q}&x=A&o={offset}")
        entries = re.findall(r"<entry>(.*?)</entry>", atom, re.S)
        for e in entries:
            t = re.search(r"<title>(.*?)</title>", e, re.S)
            u = re.search(r'<link\s+href="(.*?)"', e, re.S)
            d = re.search(r"<updated>(.*?)</updated>", e)
            if t and u and d:
                out.append((html.unescape(t.group(1)), u.group(1), d.group(1)))
        if len(entries) < 200:
            break
    return out


def summary(title: str) -> str:
    """The subject without list and PATCH tags."""
    return re.sub(r"^(\s*(Re:\s*)?\[[^\]]*\]\s*)+", "", title).strip()


def changelog(raw: str) -> str:
    body = raw.split("\n\n", 1)[-1].split("\ndiff --git")[0]
    after = body.split("\n---\n", 1)[1] if "\n---\n" in body else ""
    after = re.split(r"\n [^\n]+\|\s+\d+", after)[0].strip()
    if re.search(r"(?i)\bv\d+\b|changes", after):
        return after
    return ""


def git(repo: Path, *args: str, stdin: bytes | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["git", "-C", str(repo), *args], input=stdin, capture_output=True
    )


def apply_v1(repo: Path, rev: str, when: str, raw: bytes, ref: str) -> str | None:
    """v1 applied to rev as it was at `when`; its commit, kept under ref."""
    base = git(repo, "rev-list", "-1", f"--before={when}", rev).stdout.decode().strip()
    if not base:
        return None
    with tempfile.TemporaryDirectory() as tmp:
        tree = Path(tmp) / "wt"
        if git(repo, "worktree", "add", "-q", "--detach", str(tree), base).returncode:
            return None
        try:
            env_ok = git(
                tree, "-c", "user.name=bench", "-c", "user.email=bench@localhost",
                "am", "-q", "--3way", stdin=raw,
            )
            if env_ok.returncode:
                git(tree, "am", "--abort")
                return None
            head = git(tree, "rev-parse", "HEAD").stdout.decode().strip()
            if head == base:
                return None
            git(repo, "update-ref", ref, head)
            return head
        finally:
            git(repo, "worktree", "remove", "--force", str(tree))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--repo", type=Path, required=True)
    ap.add_argument("--list", default="openembedded-core")
    ap.add_argument("--rev", default="origin/master")
    ap.add_argument("--since", default="2026-06-01")
    ap.add_argument("--until", default="2026-11-01")
    ap.add_argument("-n", type=int, default=30)
    args = ap.parse_args()

    found = []
    for v in (2, 3):
        found += search(args.list, f's:"PATCH v{v}" d:{args.since}..{args.until}')
    entries, seen = [], set()
    for title, url, updated in found:
        if title.startswith("Re:") or re.search(r"\b0+/\d+\]", title):
            continue  # replies and cover letters
        if re.search(r"(?i)\[[^\]]*(scarthgap|kirkstone|walnascar|whinlatch|wrynose)", title):
            continue  # backports: the branch's history is not origin/master
        m = VERSION.search(title)
        if not m or summary(title) in seen:
            continue
        raw = get(url.rstrip("/") + "/raw")
        note = changelog(raw)
        if not note or NOT_CODE.search(note):
            continue
        author = re.search(r"(?im)^From:.*?<([^>]+)>", raw)
        if not author:
            continue
        # v1: the same summary from the same author, without a version.
        cands = search(args.list, f's:"{summary(title)[:80]}" f:{author.group(1)}')
        v1 = [
            (t, u, d) for t, u, d in cands
            if not t.startswith("Re:") and not VERSION.search(t) and d < updated
            and summary(t) == summary(title)
        ]
        if not v1:
            continue
        t1, u1, d1 = v1[-1]
        raw1 = get(u1.rstrip("/") + "/raw").encode()
        ref = f"refs/bench/v1/{len(entries):03d}"
        commit = apply_v1(args.repo, args.rev, d1, raw1, ref)
        if not commit:
            continue
        seen.add(summary(title))
        entries.append({
            "Commit": commit,
            "Fixed-by": commit,
            "subsystem": summary(title).split(":", 1)[0],
            "problem_description": (
                "Reviewers asked for changes to this patch, and the next version "
                f"says what changed: {' '.join(note.split())[:1200]}"
            ),
            "v1": u1,
            "v2": url,
        })
        print(f"{len(entries)}: {summary(title)[:70]}", file=sys.stderr)
        if len(entries) >= args.n:
            break
    json.dump(entries, sys.stdout, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
