# bodnuti

Automated review of Yocto Project and OpenEmbedded patches. Phase 1 runs the
checks OpenEmbedded already has, and reports only what a patch series adds:

- **patchtest** (OE-Core): commit message, shortlog, Signed-off-by, and the
  `Upstream-Status`, `Signed-off-by` and `CVE:` headers of added `.patch` files;
- **oelint-adv**: recipe, class, include and configuration style and
  correctness rules;
- **shellcheck**: shell in task functions and shipped scripts, run as
  `/bin/sh`, which is dash on many hosts;
- **leftover files**: patch files a series drops from `SRC_URI` but leaves in
  the layer;
- **documentation** (yocto-docs, BitBake manual): the Sphinx build,
  sphinx-lint, and yocto-docs' check-glossaries and check-confusables.

Each checker runs on the tree before and after the series, so existing
problems are not blamed on the author. See [`designs/DESIGN.md`](designs/DESIGN.md)
for the design and [`docs/STATUS.md`](docs/STATUS.md) for the current state.

## Usage

```
bodnuti check --repo <layer checkout> <Message-ID | lore URL | mbox files...>
```

- A Message-ID or lore URL is fetched with `b4 mbox`. mbox files can be one b4
  mbox or one `git format-patch` file per patch.
- The base is `--base`, else a branch named by a subject tag such as
  `[scarthgap]`, else `origin/HEAD`.
- patchtest comes from an openembedded-core checkout: `--oecore`, defaulting to
  `--repo`.
- `--json` prints the full report as JSON.

Exit status: 0 clean, 1 findings, 2 a checker could not run or bad input.

### With pip

`pyproject.toml` packages the binary as a Python wheel with
[maturin](https://www.maturin.rs/). The wheel pulls in oelint-adv, b4 and
patchtest's requirements; the `docs` extra adds Sphinx and sphinx-lint.

```
pip install maturin
maturin build --release
python3 -m venv ~/.venvs/bodnuti
~/.venvs/bodnuti/bin/pip install 'target/wheels/bodnuti-<version>-<platform>.whl[docs]'
~/.venvs/bodnuti/bin/bodnuti check --repo <layer> <Message-ID>
```

The venv does not need to be activated: bodnuti uses the `python3`,
`oelint-adv` and `b4` installed beside it. b4 depends on pygit2, which may
have no prebuilt wheel for the newest Python release; use the release before
it if pip tries to compile pygit2.

### In a container

The image carries oelint-adv, b4, shellcheck, patchtest's Python
requirements, and Sphinx with the extensions yocto-docs uses. Mount a clone
of the layer that bodnuti may write worktrees into, not your working
checkout. It must not be a `git clone --shared` of a host repository: its
object alternates point at host paths the container cannot see. For layers
other than openembedded-core, also mount an openembedded-core checkout for
patchtest and set `BODNUTI_OECORE` to it:

```
docker build -t bodnuti -f Containerfile .
docker run --rm \
    -v ~/cache/openembedded-core:/repo \
    bodnuti check --repo /repo '<20261023192932.159420-1-peter.marko@siemens.com>'
```

### On the host

Install oelint-adv, b4, patchtest's requirements
(`meta/lib/patchtest/requirements.txt`) and, for documentation, the packages
in yocto-docs' `documentation/Pipfile` plus sphinx-lint, in one venv. Point
bodnuti at it with `--python`, `--oelint` and `--b4` (or `BODNUTI_PYTHON`,
`BODNUTI_OELINT`, `BODNUTI_B4`). shellcheck runs from its container image by default; set
`--shellcheck shellcheck` to use a local binary.
