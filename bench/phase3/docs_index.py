#!/usr/bin/env python3
"""Index the Yocto and BitBake documentation for layer 2 context.

Reads a yocto-docs and a bitbake checkout and writes one JSON file:
the glossary entry of each variable, the section of each class, and the
description of each BitBake variable flag, and with --oe-core the value
meta/conf/bitbake.conf gives each variable for every recipe, as plain text with the RST
roles stripped, each capped at CAP characters. Layer 2 can then add the
entries for the variables, classes and flags a patch touches, so a
finding can quote the documentation instead of the model's memory.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path

CAP = 1500
TERM = re.compile(r"^   :term:`([^`]+)`\s*$")
CLASS = re.compile(r"^\.\. _ref-classes-([\w.+-]+):\s*$")
FLAG = re.compile(r"^-\s+``\[(\w+)\]``:\s*(.*)$")
ASSIGN = re.compile(r'^([A-Za-z_][A-Za-z0-9_]*(?::[\w${}-]+)*)\s*(\?\?=|\?=|:=|=)\s*"(.*)"\s*$')


def plain(text: str) -> str:
    """RST to plain text: roles to their text, literals unquoted, one line."""
    text = re.sub(r":ref:`ref-classes-([\w.+-]+)`", r"\1", text)
    text = re.sub(r":\w+:`([^`<]*?)\s*<[^>]*>`", r"\1", text)
    text = re.sub(r":\w+:`([^`]*)`", r"\1", text)
    text = re.sub(r"``([^`]*)``", r"\1", text)
    text = re.sub(r"^\s*\.\. \w+::.*$", "", text, flags=re.M)
    return " ".join(text.split())[:CAP]


def glossary(path: Path) -> dict[str, str]:
    out, name, body = {}, None, []
    for line in path.read_text().splitlines() + ["   :term:`END`"]:
        m = TERM.match(line)
        if m:
            if name and body:
                out[name] = plain("\n".join(body))
            name, body = m.group(1), []
        elif name and (not line.strip() or line.startswith("      ")):
            body.append(line)
        elif name and line.strip():
            if body:
                out[name] = plain("\n".join(body))
            name, body = None, []
    out.pop("END", None)
    return out


def classes(path: Path) -> dict[str, str]:
    out, name, body = {}, None, []
    for line in path.read_text().splitlines() + [".. _ref-classes-END:"]:
        m = CLASS.match(line)
        if m:
            if name:
                out[name] = plain("\n".join(body[2:]))  # drop the title and its underline
            name, body = m.group(1), []
        elif name:
            if line.strip() or body:
                body.append(line)
    out.pop("END", None)
    return out


def flags(path: Path) -> dict[str, str]:
    out, name, body = {}, None, []
    for line in path.read_text().splitlines() + ["END"]:
        m = FLAG.match(line)
        if m or (name and line.strip() and not line.startswith(" ")):
            if name:
                out[name] = plain("\n".join(body))
            name, body = (m.group(1), [m.group(2)]) if m else (None, [])
        elif name:
            body.append(line)
    return out


def defaults(path: Path) -> dict[str, str]:
    """Each variable's one-line assignments in bitbake.conf, overrides included."""
    out: dict[str, list[str]] = {}
    for line in path.read_text().splitlines():
        m = ASSIGN.match(line)
        if m:
            out.setdefault(m.group(1).split(":")[0], []).append(f'{m.group(1)} {m.group(2)} "{m.group(3)}"')
    return {k: "; ".join(v) for k, v in out.items()}


def head(repo: Path) -> str:
    return subprocess.run(["git", "-C", str(repo), "rev-parse", "HEAD"],
                          capture_output=True, text=True).stdout.strip()


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--yocto-docs", type=Path, required=True)
    ap.add_argument("--bitbake", type=Path, required=True)
    ap.add_argument("--oe-core", type=Path, help="openembedded-core checkout, for bitbake.conf")
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()
    ref = args.yocto_docs / "documentation/ref-manual"
    bb = args.bitbake / "doc/bitbake-user-manual"
    variables = glossary(bb / "bitbake-user-manual-ref-variables.rst")
    for name, text in glossary(ref / "variables.rst").items():
        # many Yocto entries start "See the BitBake manual": keep both
        variables[name] = (variables[name] + " | Yocto: " + text)[:CAP] if name in variables else text
    source = {"yocto-docs": head(args.yocto_docs), "bitbake": head(args.bitbake)}
    if args.oe_core:
        # Set for every recipe, so never "undefined" in a recipe that does not set it.
        for name, text in defaults(args.oe_core / "meta/conf/bitbake.conf").items():
            note = "Set for every recipe in meta/conf/bitbake.conf: " + text
            variables[name] = (note + " | " + variables[name])[:CAP] if name in variables else note[:CAP]
        source["openembedded-core"] = head(args.oe_core)
    index = {
        "source": source,
        "variables": variables,
        "classes": classes(ref / "classes.rst"),
        "flags": flags(bb / "bitbake-user-manual-metadata.rst"),
    }
    args.out.write_text(json.dumps(index, indent=1, sort_keys=True))
    print({k: len(v) for k, v in index.items() if k != "source"})


def demo() -> None:
    import tempfile
    d = Path(tempfile.mkdtemp())
    (d / "v.rst").write_text(".. glossary::\n\n   :term:`SRC_URI`\n      The list of source files, see\n      :term:`PREMIRRORS` and ``file://``.\n\n   :term:`S`\n      The source dir.\n")
    (d / "c.rst").write_text(".. _ref-classes-cargo:\n\n``cargo``\n=========\n\nBuilds Rust with :ref:`ref-classes-rust`.\n\n.. _ref-classes-ptest:\n\n``ptest``\n=========\n\nRuns tests.\n")
    (d / "f.rst").write_text("-  ``[postfuncs]``: List of functions to call after\n   the task.\n-  ``[umask]``: The umask.\n\nOther text.\n")
    assert glossary(d / "v.rst") == {"SRC_URI": "The list of source files, see PREMIRRORS and file://.", "S": "The source dir."}
    assert classes(d / "c.rst") == {"cargo": "Builds Rust with rust.", "ptest": "Runs tests."}
    assert flags(d / "f.rst") == {"postfuncs": "List of functions to call after the task.", "umask": "The umask."}
    (d / "b.conf").write_text('USRBINPATH = "${bindir}"\nUSRBINPATH:class-native = "/usr/bin"\nexport PATH\n')
    assert defaults(d / "b.conf") == {"USRBINPATH": 'USRBINPATH = "${bindir}"; USRBINPATH:class-native = "/usr/bin"'}


if __name__ == "__main__":
    demo()
    main()
