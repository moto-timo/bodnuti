#!/usr/bin/env python3
"""Build a fine-tuning dataset for layer 3 from a list's review history.

designs/TRAINING.md. Reads a public-inbox mirror of a list (one message
per commit, blob "m"), pairs each single patch with its next version from
the same author and summary, and takes the answer from the reviewers'
inline replies to v1: a comment counts when it follows a quoted added
line of the patch and v2, applied to the branch as it was when v1 was
sent, changed code within NEAR lines of it. So the targets are what a
maintainer wrote and the author acted on, not the author's changelog.
Patches merged unchanged from the repository's history are the clean
examples, with an empty answer.

Each example is bodnuti's own layer 3 request for the patch, written by
`bodnuti check --review-requests` (no model is called), plus the target
answer: one JSON line per example in OUT/dataset.jsonl.

Mail is untrusted: it only reaches git am in a throwaway worktree with
hooks off, and bodnuti check, never a shell. Standard library only.
"""

from __future__ import annotations

import argparse
import email
import email.policy
import email.utils
import json
import random
import re
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

VERSION = re.compile(r"\[[^\]]*PATCH[^\]]*?\bv(\d+)\b[^\]]*\]", re.I)
COUNTER = re.compile(r"\b(\d+)/(\d+)\]")
ACK = re.compile(r"(?i)^(thanks|thank you|lgtm|looks good|reviewed-by|acked-by|tested-by)")
BRANCH_TAG = re.compile(r"(?i)\[[^\]]*(kirkstone|scarthgap|styhead|walnascar|whinlatter|wrynose|blacksail|dunfell|hardknott|honister|langdale|mickledore|nanbield)")
# Metadata files, in any layer layout (meta/, meta-oe/, meta-python/, ...).
METADATA = ["*.bb", "*.bbappend", "*.inc", "*.bbclass", "*.conf"]
MAX_FINDINGS = 3
NEAR = 5


def git(repo: Path, *args: str, stdin: bytes | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(["git", "-C", str(repo), *args], input=stdin, capture_output=True)


def summary(subject: str) -> str:
    return re.sub(r"^(\s*(Re:\s*)?\[[^\]]*\]\s*)+", "", subject).strip()


def added_lines(body: str) -> dict[str, list[tuple[str, int]]]:
    """Each added line of a patch mail, stripped, to its (file, new line)."""
    out: dict[str, list[tuple[str, int]]] = {}
    path, n = None, None
    for line in body.splitlines():
        if line.startswith("+++ b/"):
            path, n = line[6:], None
        elif line.startswith("@@ ") and path:
            m = re.match(r"@@ -\S+ \+(\d+)", line)
            n = int(m.group(1)) if m else None
        elif n is not None:
            if line.startswith("+"):
                key = line[1:].strip()
                if len(key) >= 4:
                    out.setdefault(key, []).append((path, n))
                n += 1
            elif line.startswith(" ") or line == "":
                n += 1
    return out


def tidy(msg: str) -> str | None:
    """A comment without its sign-off name, or None for one the patch cannot show."""
    if re.search(r"(?i)autobuilder|\bfails? on (the )?a[bh]\b", msg):
        return None  # a build result, not something a reviewer reads from the patch
    return re.sub(r"(?<=[.!?)])\s+[A-Z][a-z]{1,11}\.?$", "", msg)


def comments(body: str, lines: dict) -> list[tuple[str, int, str]]:
    """(file, line, text) of each comment a reply makes below a quoted added line."""
    out, at, text = [], None, []

    def flush():
        msg = tidy(" ".join(" ".join(text).split())) or ""
        if at and len(msg) >= 20 and not ACK.match(msg):
            out.append((*at, msg[:600]))
        text.clear()

    for line in body.splitlines():
        if line.rstrip() == "--":
            break  # signature
        if line.startswith(">"):
            if text:
                flush()
                at = None
            q = line[1:]
            if q.startswith(">"):
                at = None  # an earlier message's quote
                continue
            q = q[1:] if q.startswith(" ") else q
            if q.startswith("+") and not q.startswith("+++"):
                hits = lines.get(q[1:].strip(), [])
                at = hits[0] if len(hits) == 1 else None
            elif q.strip():
                at = None
        elif at:
            text.append(line)
    flush()
    return out


def messages(mirror: Path):
    """(commit, subject, from, date, body, raw, msg) of each mail, oldest first."""
    log = git(mirror, "log", "--reverse", "--format=%H", "--all").stdout.decode().split()
    batch = subprocess.Popen(
        ["git", "-C", str(mirror), "cat-file", "--batch"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    )
    for h in log:
        batch.stdin.write(f"{h}:m\n".encode())
        batch.stdin.flush()
        header = batch.stdout.readline().split()
        if len(header) < 3 or header[1] != b"blob":
            continue
        raw = batch.stdout.read(int(header[2]))
        batch.stdout.read(1)
        try:
            msg = email.message_from_bytes(raw, policy=email.policy.default)
            subject = " ".join(str(msg["subject"] or "").split())
        except Exception:
            continue
        if "PATCH" not in subject:
            continue
        try:
            date = email.utils.parsedate_to_datetime(str(msg["date"])).isoformat()
            addr = email.utils.parseaddr(str(msg["from"]))[1].lower()
            part = msg.get_body(preferencelist=("plain",))
            body = part.get_content() if part else ""
        except Exception:
            continue
        yield h, subject, addr, date, body, raw, msg


def msgid(value) -> str:
    return str(value or "").strip().strip("<>")


def pairs(mirror: Path, since: str, until: str):
    """(v1 raw, v1 date, v2 raw, reviewers' comments, meta) for revised single patches."""
    groups: dict[tuple[str, str], list] = {}
    replies: dict[str, list[tuple[str, str]]] = {}
    for h, subject, addr, date, body, raw, msg in messages(mirror):
        if not (since <= date < until) or BRANCH_TAG.search(subject):
            continue
        if subject.lower().startswith("re:"):
            if re.search(r"(?m)^> ?\+", body):  # ponytail: only inline reviews are kept in memory
                replies.setdefault(msgid(msg["in-reply-to"]), []).append((addr, body))
            continue
        c = COUNTER.search(subject)
        if c and c.group(2) != "1":
            continue  # ponytail: single patches only; series need the earlier patches applied
        v = VERSION.search(subject)
        groups.setdefault((addr, summary(subject)), []).append(
            (date, int(v.group(1)) if v else 1, raw, body, subject, msgid(msg["message-id"]))
        )
    for (addr, summ), mails in groups.items():
        mails.sort()
        for a, b in zip(mails, mails[1:]):
            if b[1] <= a[1]:
                continue
            lines = added_lines(a[3])
            said = [c for who, body in replies.get(a[5], []) if who != addr for c in comments(body, lines)]
            if said:
                yield a[2], a[0], b[2], said, {"summary": summ, "v1": a[4], "v2": b[4]}


class Tree:
    """A throwaway worktree of the repository for applying mail."""

    def __init__(self, repo: Path):
        self.repo = repo
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / "wt"
        git(repo, "worktree", "add", "-q", "--detach", str(self.path), "HEAD")

    def apply(self, base: str, raw: bytes) -> str | None:
        git(self.path, "am", "--abort")
        if git(self.path, "checkout", "-q", "-f", "--detach", base).returncode:
            return None
        r = git(self.path, "-c", "user.name=t", "-c", "user.email=t@t", "-c",
                "core.hooksPath=/dev/null", "am", "-q", "--3way", stdin=raw)
        if r.returncode:
            git(self.path, "am", "--abort")
            return None
        head = git(self.path, "rev-parse", "HEAD").stdout.decode().strip()
        return head if head != base else None

    def close(self):
        git(self.repo, "worktree", "remove", "--force", str(self.path))
        self.tmp.cleanup()


def hunks(repo: Path, c1: str, c2: str) -> list[tuple[str, int]]:
    """(file, v1 line) of each change between two applied versions."""
    files = git(repo, "show", "--format=", "--name-only", c1).stdout.decode().split()
    out = []
    diff = git(repo, "diff", "-U0", "--no-color", "--no-ext-diff", "--no-textconv", c1, c2, "--", *files)
    current = None
    for line in diff.stdout.decode(errors="replace").splitlines():
        if line.startswith("+++ b/"):
            current = line[6:]
        elif line.startswith("@@ -") and current:
            start = int(line[4:].split(" ")[0].split(",")[0])
            out.append((current, max(start, 1)))
    return out


def request(bodnuti: Path, repo: Path, base: str, raw: bytes, docs: Path | None = None) -> list | None:
    with tempfile.TemporaryDirectory() as tmp:
        mbx = Path(tmp) / "p.mbx"
        mbx.write_bytes(b"From mboxrd@z Thu Jan  1 00:00:00 1970\n" + raw)
        out = Path(tmp) / "req"
        subprocess.run(
            [str(bodnuti), "check", "--repo", str(repo), "--base", base,
             "--review-requests", str(out), *(["--review-docs", str(docs)] if docs else []), str(mbx)],
            capture_output=True, timeout=600,
        )
        f = out / "1.json"
        return json.loads(f.read_text()) if f.is_file() else None


def answer(findings: list[dict]) -> str:
    return json.dumps({"findings": findings, "confidence": "high"})


def defect_example(args, tree: Tree, v1raw, date, v2raw, said, meta):
    base = git(args.repo, "rev-list", "-1", f"--before={date}", args.rev).stdout.decode().strip()
    if not base:
        return None
    c1 = tree.apply(base, v1raw)
    c2 = tree.apply(base, v2raw) if c1 else None
    if not c2:
        return None
    changed = hunks(args.repo, c1, c2)
    found = []
    for path, line, msg in said:
        if not any(p == path and abs(n - line) <= NEAR for p, n in changed):
            continue  # the author did not change the code there
        text = git(args.repo, "show", f"{c1}:{path}").stdout.decode(errors="replace").splitlines()
        if line > len(text) or not text[line - 1].strip():
            continue
        found.append({"file": path, "line": line, "category": "other", "severity": "warning",
                      "message": msg, "confidence": "high", "evidence": text[line - 1]})
    found = found[:MAX_FINDINGS]
    if not found:
        return None
    req = request(args.bodnuti, args.repo, base, v1raw, args.docs)
    if not req:
        return None
    return {"messages": req + [{"role": "assistant", "content": answer(found)}],
            "meta": {"kind": "defect", "date": date, **meta}}


def clean_commits(args, n: int) -> list[str]:
    fixed = set(re.findall(r"(?im)^Fixes:\s*([0-9a-f]{8,12})",
                           git(args.repo, "log", "--format=%B", "-E", "--grep=^Fixes:", args.rev).stdout.decode()))
    log = git(args.repo, "log", "--no-merges", f"--since={args.since}", f"--until={args.until}",
              "--format=%H", args.rev, "--", *METADATA).stdout.decode().split()
    log = [c for c in log if not any(c.startswith(f) for f in fixed)]
    random.Random(1).shuffle(log)
    return log[:n]


def clean_example(args, commit: str):
    raw = git(args.repo, "format-patch", "-1", "--stdout", commit).stdout
    if not raw or len(raw) > 200_000:
        return None
    raw = raw.split(b"\n", 1)[1]  # drop format-patch's From line; request() adds one
    req = request(args.bodnuti, args.repo, commit + "^", raw, args.docs)
    if not req:
        return None
    date = git(args.repo, "log", "-1", "--format=%aI", commit).stdout.decode().strip()
    return {"messages": req + [{"role": "assistant", "content": answer([])}],
            "meta": {"kind": "clean", "date": date, "commit": commit}}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--mirror", type=Path, required=True, help="public-inbox git mirror of the list")
    ap.add_argument("--repo", type=Path, required=True, help="clone of the list's repository")
    ap.add_argument("--rev", default="origin/master")
    ap.add_argument("--bodnuti", type=Path, required=True)
    ap.add_argument("--docs", type=Path, help="docs_index.py's index, as review uses it")
    ap.add_argument("--since", default="2016-01-01")
    ap.add_argument("--until", default="2026-01-01")
    ap.add_argument("--clean-ratio", type=float, default=3.0)
    ap.add_argument("--jobs", type=int, default=6)
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()
    args.bodnuti = args.bodnuti.resolve()
    args.docs = args.docs.resolve() if args.docs else None
    args.out.mkdir(parents=True, exist_ok=True)
    data = args.out / "dataset.jsonl"
    done = sum(1 for _ in data.open()) if data.exists() else 0
    if done:
        print(f"{data} has {done} examples; delete it to start over", file=sys.stderr)
        return 1

    found = list(pairs(args.mirror, args.since, args.until))
    print(f"{len(found)} v1/v2 pairs with inline review comments", file=sys.stderr)
    defects = []
    tree = Tree(args.repo)
    try:
        for i, p in enumerate(found):
            ex = defect_example(args, tree, *p)
            if ex:
                defects.append(ex)
                with data.open("a") as f:
                    f.write(json.dumps(ex) + "\n")
            if i % 50 == 0:
                print(f"pairs {i}/{len(found)}: {len(defects)} examples", file=sys.stderr, flush=True)
    finally:
        tree.close()

    commits = clean_commits(args, int(len(defects) * args.clean_ratio))
    with ThreadPoolExecutor(args.jobs) as pool:
        for i, ex in enumerate(pool.map(lambda c: clean_example(args, c), commits)):
            if ex:
                with data.open("a") as f:
                    f.write(json.dumps(ex) + "\n")
            if i % 100 == 0:
                print(f"clean {i}/{len(commits)}", file=sys.stderr, flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
