#!/usr/bin/env python3
"""Mine benchmark entries for designs/PHASE3.md §8 from Fixes: trailers.

Each OE-Core commit with a trailer such as

    Fixes: 1234abcd5678 ("foo: do something")

names an earlier commit that carried a defect. The earlier commit is the
patch to review; the fixing commit's message describes the defect, for the
judge. Output has the shape of sashiko's benchmark_tiny.json.

Like score.py it reads git directly, so run it only on a trusted clone.
Standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

FIXES_RE = re.compile(r"^Fixes:\s*([0-9a-f]{8,40})\b", re.M | re.I)
TRAILER_RE = re.compile(r"^[A-Za-z-]+:\s", re.M)


def git(repo: Path, *args: str) -> str:
    r = subprocess.run(
        ["git", "-C", str(repo), *args], capture_output=True, text=True, errors="replace"
    )
    return r.stdout if r.returncode == 0 else ""


def description(message: str) -> str:
    """The fixing commit's subject and body, trailers dropped."""
    lines = []
    for line in message.strip().splitlines():
        if TRAILER_RE.match(line) and line.split(":", 1)[0].lower() in {
            "fixes", "signed-off-by", "reviewed-by", "acked-by", "tested-by",
            "reported-by", "suggested-by", "cc", "co-authored-by", "link",
        }:
            continue
        lines.append(line)
    text = " ".join(" ".join(lines).split())
    return f"A later commit fixed this patch. Its message: {text}"[:1500]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--repo", type=Path, required=True)
    ap.add_argument("--rev", default="origin/master")
    ap.add_argument("--since", default="2024-01-01")
    ap.add_argument("--max-lines", type=int, default=600, help="skip larger defect diffs")
    ap.add_argument("--exclude", type=Path, help="a benchmark whose commits to leave out")
    ap.add_argument("-n", type=int, default=40)
    args = ap.parse_args()

    exclude = set()
    if args.exclude:
        exclude = {e["Commit"][:12] for e in json.loads(args.exclude.read_text())}

    log = git(
        args.repo, "log", "--no-merges", f"--since={args.since}", "-E",
        "--grep=^Fixes: *[0-9a-f]{8}", "--format=%H%x00%B%x01", args.rev,
    )
    entries, seen = [], set()
    for record in log.split("\x01"):
        if "\x00" not in record:
            continue
        fix, message = record.strip().split("\x00", 1)
        targets = FIXES_RE.findall(message)
        if len(targets) != 1:
            continue  # one defect per entry, so the judge's question is clear
        commit = git(args.repo, "rev-parse", "--verify", "-q", f"{targets[0]}^{{commit}}").strip()
        if not commit or commit[:12] in exclude or commit in seen:
            continue
        if len(git(args.repo, "rev-list", "--parents", "-n1", commit).split()) != 2:
            continue  # a merge or a root commit
        ancestor = subprocess.run(
            ["git", "-C", str(args.repo), "merge-base", "--is-ancestor", commit, args.rev]
        )
        if ancestor.returncode != 0:
            continue
        files = git(args.repo, "show", "--format=", "--name-only", commit).split()
        if not any(f.startswith("meta/") for f in files):
            continue
        size = len(git(args.repo, "show", "--format=", commit).splitlines())
        if size > args.max_lines:
            continue
        subject = git(args.repo, "log", "-1", "--format=%s", commit).strip()
        if subject.lower().startswith("revert"):
            continue
        seen.add(commit)
        entries.append({
            "Commit": commit,
            "Fixed-by": fix,
            "subsystem": subject.split(":", 1)[0],
            "problem_description": description(message),
        })
        if len(entries) >= args.n:
            break
    json.dump(entries, sys.stdout, indent=1)
    print(file=sys.stderr, end=f"{len(entries)} entries\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
