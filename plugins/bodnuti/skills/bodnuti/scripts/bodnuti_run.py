#!/usr/bin/env python3
"""Run bodnuti, the Yocto Project patch review tool, in a container.

Checks a series from a git revision range, from mbox files, or from a lore
Message-ID. The layer is copied to a standalone clone that bodnuti may write
worktrees into, so the user's checkout is never changed.
"""

import argparse
import os
import platform
import shutil
import subprocess
import sys
import tempfile

IMAGE = "ghcr.io/moto-timo/bodnuti"


def fail(msg, code=2):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(code)


def pick_engine(requested=None):
    """podman if installed, else docker, unless one is requested."""
    for name in ([requested] if requested else ["podman", "docker"]):
        if shutil.which(name):
            return name
    fail(f"{requested or 'podman or docker'} not found; install one of them")


def run(cmd, **kw):
    p = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if p.returncode != 0:
        fail(f"{' '.join(cmd)} failed: {(p.stderr or p.stdout).strip()}")
    return p.stdout


def standalone_clone(src, dst):
    """Clone src with every object it can reach and its remote-tracking refs.

    --no-local copies through the pack protocol, so the clone has no object
    alternates: those point at host paths that the container cannot see. It
    needs a checkout: patchtest runs from the openembedded-core tree.
    """
    run(["git", "clone", "-q", "--no-local", src, dst])
    run(["git", "-C", dst, "fetch", "-q", src,
         "+refs/remotes/origin/*:refs/remotes/origin/*"])


def range_base(rev_range):
    """The commit a git format-patch range starts from: A of A..B, or the rev itself."""
    return rev_range.split("..", 1)[0] if ".." in rev_range else rev_range


def container_command(engine, image, workdir, args, env=None, pull="missing"):
    cmd = [engine, "run", "--rm", f"--pull={pull}"]
    if engine == "podman":
        # Rootless podman: keep the user's uid so bodnuti can write the clone
        cmd += ["--userns=keep-id", "-v", f"{workdir}:/work:Z"]
    else:
        cmd += ["--user", f"{os.getuid()}:{os.getgid()}", "-v", f"{workdir}:/work"]
    for k, v in (env or {}).items():
        cmd += ["-e", f"{k}={v}"]
    return cmd + [image, "check"] + args


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    src = ap.add_mutually_exclusive_group(required=True)
    src.add_argument("--range", help="git revision range of the series, e.g. origin/master..HEAD")
    src.add_argument("--mbox", nargs="+", help="b4 mbox, or git format-patch files")
    src.add_argument("--message-id", help="lore Message-ID or URL of a patch in the series")
    ap.add_argument("--repo", required=True, help="layer checkout the series targets")
    ap.add_argument("--oecore", help="openembedded-core checkout, for patchtest on other layers")
    ap.add_argument("--tag", default="latest",
                    help="image tag: latest (newest release, default), main, or a version such as 0.3.0")
    ap.add_argument("--engine", choices=["podman", "docker"], help="default: podman, else docker")
    ap.add_argument("--kind", help="bodnuti --kind: layer (default), django-app, python or c")
    ap.add_argument("--base", help="commit to apply the series to (bodnuti --base)")
    ap.add_argument("--staging", action="append", help="bodnuti --staging, may repeat")
    ap.add_argument("--json", action="store_true", help="JSON report")
    ap.add_argument("--keep", action="store_true", help="keep the work directory")
    args = ap.parse_args(argv)

    repo = os.path.abspath(args.repo)
    if run(["git", "-C", repo, "rev-parse", "--is-inside-work-tree"]).strip() != "true":
        fail(f"{repo} is not a git checkout")
    engine = pick_engine(args.engine)
    # A release tag never changes; main and latest move, so check for a newer image
    pull = "missing" if args.tag[:1].isdigit() else ("newer" if engine == "podman" else "always")

    work = tempfile.mkdtemp(prefix="bodnuti-")
    try:
        standalone_clone(repo, os.path.join(work, "repo"))
        inputs, env = [], {}
        if args.range:
            pdir = os.path.join(work, "patches")
            run(["git", "-C", repo, "format-patch", "-q", "-o", pdir, args.range])
            inputs = sorted(f"/work/patches/{f}" for f in os.listdir(pdir))
            if not inputs:
                fail(f"no commits in {args.range}")
            if not args.base:
                # Else bodnuti uses origin/HEAD of the clone: the user's
                # checkout, which may already carry the series
                args.base = run(["git", "-C", repo, "rev-parse", "--verify",
                                 range_base(args.range) + "^{commit}"]).strip()
        elif args.mbox:
            os.mkdir(os.path.join(work, "mbox"))
            for f in args.mbox:
                shutil.copy(f, os.path.join(work, "mbox", os.path.basename(f)))
            inputs = [f"/work/mbox/{os.path.basename(f)}" for f in args.mbox]
        else:
            inputs = [args.message_id]
        if args.oecore:
            standalone_clone(os.path.abspath(args.oecore), os.path.join(work, "oecore"))
            env["BODNUTI_OECORE"] = "/work/oecore"

        opts = ["--repo", "/work/repo"]
        for name in ("kind", "base"):
            if getattr(args, name):
                opts += [f"--{name}", getattr(args, name)]
        for s in args.staging or []:
            opts += ["--staging", s]
        if args.json:
            opts.append("--json")

        cmd = container_command(engine, f"{IMAGE}:{args.tag}", work, opts + inputs, env, pull)
        print(f"bodnuti: {engine} {IMAGE}:{args.tag} ({platform.machine()})", file=sys.stderr)
        # bodnuti exits 0 when clean, 1 with findings, 2 when a checker could not run
        return subprocess.run(cmd).returncode
    finally:
        if args.keep:
            print(f"bodnuti: work directory kept: {work}", file=sys.stderr)
        else:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
