# Design: bodnuti — automated patch review for Yocto Project / OpenEmbedded

## 1. Overview

bodnuti reviews patches sent to the Yocto Project and OpenEmbedded mailing
lists, in the spirit of [sashiko](https://sashiko.dev) for the Linux kernel.

Sashiko is an LLM-first reviewer. OpenEmbedded already has deterministic
checkers that catch most of the mistakes that come up in review again and
again: missing `Signed-off-by`, missing or malformed `Upstream-Status`,
undocumented `LIC_FILES_CHKSUM` changes, and recipe style problems. bodnuti
starts there. Phase 1 runs those checkers on a patch series and produces one
structured report. LLM review comes later and builds on that report.

### 1.1 Goals (phase 1)

1. `bodnuti check <input>` takes a lore Message-ID, a URL, or local mbox files.
   It runs **patchtest** and **oelint-adv** on the series and emits findings as
   JSON plus a human-readable text report.
2. Report only what the series introduces. A recipe that already had oelint
   warnings must not blame the patch author for them.
3. Reproducible execution in a Linux container. bitbake/tinfoil, which
   patchtest needs, does not run on macOS.
4. One typed `Finding` model that later stages (LLM review, email, web UI) use
   without re-parsing tool output.

### 1.2 Non-goals (phase 1)

- Watching mailing lists, a database, a web UI, or sending email.
- LLM review.
- Building recipes. The checks are static and parse-time only.
- Patchwork integration. lore and b4 are enough for input.

## 2. Lessons from sashiko

| Sashiko concept | bodnuti equivalent |
|---|---|
| `ProjectId` selects workflow and prompts | `Target` selects a layer profile (oe-core, bitbake, meta-oe, yocto-docs, …): repo URL, which checkers apply, patchtest test dir |
| lore/NNTP ingestion → `patch.rs` series model | `b4 am` → mbox per series; phase 2 adds lore polling |
| `baseline.rs`: find the commit the series applies to | Branch from the subject tag (`[scarthgap]`, `[walnascar]`, default `master`); `git am --3way` against that branch tip |
| Pre-existing vs. introduced bugs (`DESIGN_SEPARATE_PREEXISTING_BUGS.md`) | Delta mode: run oelint on base and head, report only new findings |
| Review JSON (`--agent` output) | `Report { series, findings, tool_errors }` JSON |
| `git_cmd` constructors that strip `GIT_DIR` | Same rule from day one: one place spawns git |

Deliberately **not** carried over: the multi-stage workflow engine, AI
providers, SQLite, axum, and email routing. Phase 1 needs none of them.

## 3. Architecture

```
bodnuti check <msgid|url|mbox...> [--target oe-core] [--branch master] [--json]
        │
        ▼
  ┌──────────┐   b4 am -o <tmp> <msgid>    ┌──────────────┐
  │  input   │ ──────────────────────────▶ │ Series       │ N patch files + cover
  └──────────┘                             └──────┬───────┘
                                                  ▼
  ┌──────────────────────────────────────────────────────────┐
  │ workspace: <cache>/repos/<target> mirror (git fetch)     │
  │            git worktree add <tmp>/wt <branch tip>        │
  └──────┬───────────────────────────────┬───────────────────┘
         ▼                               ▼
  ┌──────────────┐               ┌──────────────────┐
  │ patchtest    │ per patch     │ oelint-adv       │ base vs. head
  │ (subprocess) │               │ (subprocess)     │ on touched recipes
  └──────┬───────┘               └────────┬─────────┘
         └──────────────┬─────────────────┘
                        ▼
              Vec<Finding> → Report → JSON / text
```

### 3.1 Crate layout

One binary crate, small on purpose:

```
src/
  main.rs        clap CLI, wires the steps
  series.rs      fetch via b4, load mbox files, parse subject tags (branch, vN, n/m)
  git.rs         the only place that spawns git (strips GIT_DIR & friends)
  workspace.rs   mirror cache + per-run worktree, apply series with git am
  target.rs      Target enum: repo URL, applicable checkers, list address
  finding.rs     Finding, Severity, Tool, Report
  patchtest.rs   run patchtest, parse PASS/FAIL/SKIP lines
  oelint.rs      run oelint-adv, parse, delta
  report.rs      text rendering
```

Dependencies: `clap`, `serde`/`serde_json`, `anyhow`, `tempfile`. All external
tools (git, b4, patchtest, oelint-adv) are subprocesses. There is no async
runtime: the work is sequential subprocess calls, so tokio adds nothing yet.

### 3.2 Finding model

```rust
pub enum Tool { Patchtest, Oelint }
pub enum Severity { Error, Warning, Info }

pub struct Finding {
    pub tool: Tool,
    pub check: String,          // "test_upstream_status_presence_format", "oelint.var.mandatoryvar.SUMMARY"
    pub severity: Severity,
    pub patch: Option<usize>,   // 1-based index in the series; None = whole series
    pub file: Option<PathBuf>,  // repo-relative
    pub line: Option<u32>,
    pub message: String,
}

pub struct Report {
    pub series: SeriesInfo,     // subject, msgid, version, branch, base commit
    pub findings: Vec<Finding>,
    pub tool_errors: Vec<ToolError>, // a checker crashed: not the author's fault, never silently dropped
}
```

`check` stays a `String`. The check IDs belong to the upstream tools, and an
enum would go stale on every oelint-adv release.

### 3.3 patchtest

patchtest (`openembedded-core/scripts/patchtest`) is a unittest runner. Tests
live in `meta/lib/patchtest/tests/`:

- `test_mbox.py`: Signed-off-by, shortlog format and length, target mailing
  list, merge on HEAD, bugzilla format, author, AUH rules.
- `test_patch.py`: `Upstream-Status`, Signed-off-by and `CVE:` tags in added
  `.patch` files.
- `test_metadata.py`: LICENSE / LIC_FILES_CHKSUM / SUMMARY presence,
  SRC_URI leftovers, CVE_CHECK_IGNORE. These need **tinfoil**, so they need an
  initialized build directory.
- `test_python_pylint.py`: pylint delta on changed `.py` files.

Invocation, once per patch file, from inside an `oe-init-build-env` shell:

```
patchtest --patch <n.patch> --repodir <worktree> \
          --testdir <oe-core>/meta/lib/patchtest/tests --base-commit <base>
```

Output lines have the form `{PASS|FAIL|SKIP}: <description>[: <issue>] (<test.id>)`.
`FAIL` becomes a `Finding`. `PASS` and `SKIP` are counted in the report and
are not findings. A test **error** (`addError`) only prints a traceback and
writes no result line. bodnuti therefore treats "non-zero exit and no result
lines" or "fewer result lines than expected" as a `ToolError`, never as a
clean pass.

Which suite applies to which target: patchtest's suite is written for oe-core,
bitbake and docs (`test_target_mailing_list` knows those). For other layers
(meta-oe and others), phase 1 runs only `test_mbox` and `test_patch`
(`--pattern`). Those are layer-agnostic. `test_metadata` needs the layer in
`bblayers.conf` and is turned on per target later.

Cost: tinfoil parsing takes minutes on a cold cache. One persistent build
directory per (target, branch), reused across runs so bitbake's parse
cache stays warm, keeps a warm run in the tens of seconds. Runs that share a build
directory must not overlap; phase 1 is single-run, so this is noted, not
built.

### 3.4 oelint-adv

oelint-adv lints `.bb`, `.bbappend`, `.bbclass`, `.inc` and `.conf` files
statically. It does not need bitbake.

1. Collect the recipe files the series touches (`git diff --name-only base..head`).
   Expand each touched `.inc` or `.bbappend` to its recipe group, because
   oelint-adv resolves includes and appends across the files passed together.
2. Run on **base** (files that existed) and on **head**:
   `oelint-adv --quiet --relpaths --release <branch> --messageformat '{path}:{line}:{severity}:{id}:{msg}' <files>`
3. Delta: a head finding is new when its `(path, id, msg)` key does not occur
   in the base set. Line numbers are left out of the key because patches move
   lines. The key is a multiset: three base copies against four head copies
   gives one new finding.
4. Map the file of each new finding to the patch in the series that last
   touched it, for attribution.

oelint-adv is stricter than oe-core's actual review norms; for example, it
flags variable ordering that oe-core does not enforce. Phase 1 ships a
`--rules` profile per target (a list of suppressed IDs) and starts out
conservative. The aim is that maintainers trust the output, not maximum
coverage.

### 3.5 Execution environment

A `Containerfile` (Debian/Ubuntu base) bundles:
the bodnuti binary, git, b4, Python with oelint-adv, the patchtest
requirements (`meta/lib/patchtest/requirements.txt`), and the bitbake host
dependencies. Repos and build directories live on a mounted cache volume. On
macOS, the documented path is `podman run … bodnuti check <msgid>`. Native
Linux can run the binary directly.

### 3.6 Untrusted input

Patches come from arbitrary senders. Phase 1 never executes patched code:

- `git am` only writes files.
- oelint-adv parses statically.
- patchtest's `test_metadata` runs **tinfoil**, which executes inline Python
  (`${@...}`) and anonymous functions from the patched recipes. That is code
  execution from untrusted mail. So `test_metadata` runs only in the
  container, with no network (`--network none` after the fetch step), a
  read-only root, and an unprivileged user. Running it unconfined on a host is
  not supported.
- Subject text, file names, and Message-IDs go to subprocesses as argv
  elements, never through a shell.

## 4. Output

JSON (`--json`) is the `Report` serialized. The text report follows
patchtest's existing mail style, so list readers recognise it:

```
bodnuti: [PATCH v2 3/5] curl: upgrade 8.9.1 -> 8.10.0
  base: openembedded-core master @ 39b27e524f

  FAIL patchtest  Upstream-Status presence (patch 2/5)
       meta/recipes-support/curl/files/fix-build.patch: Upstream-Status is missing
  WARN oelint     oelint.vars.srcurichecksum (patch 3/5)
       meta/recipes-support/curl/curl_8.10.0.bb:12: ...

  patchtest: 41 pass, 1 fail, 6 skip   oelint: 1 new (3 pre-existing, not shown)
```

Exit codes: 0 means clean, 1 means findings, 2 means a tool error or bad
input. This lets CI and scripts gate on the result.

## 5. Phases

1. **Deterministic checks (this document):** `bodnuti check`, container, JSON and text output.
2. **Watch and report:** poll lore (`yocto-patches`, `openembedded-core`,
   `openembedded-devel`, `bitbake-devel`) by Message-ID, keep state in SQLite,
   publish reports on a static page (bodnuti.dev). Email replies stay
   opt-in per list, start in dry-run, and need the list owners' agreement.
3. **LLM review:** a second pass fed the series plus the phase 1 findings as
   facts, with OE-specific guides (Upstream-Status semantics, CVE patch
   hygiene, `PV`/`SRCREV` upgrade rules, license changes, `inherit` and class
   usage). The obvious path is to call sashiko's local review as a subprocess
   or reuse its AI provider crate, decided when phase 3 starts.

## 6. Testing

- Parser unit tests on captured patchtest and oelint-adv output: PASS, FAIL,
  SKIP, a traceback-only error, and the empty-output case.
- A delta unit test: findings that moved lines are not reported, and
  duplicates are counted as a multiset.
- One end-to-end fixture: a small oe-core series with a known missing
  `Upstream-Status`, run in the container (CI only, `#[ignore]` locally).

## 7. Open questions

1. **License:** Apache-2.0 (matches sashiko) or MIT? patchtest is GPL-2.0-only.
   bodnuti calls it as a subprocess and does not link it, so any license works.
2. **oelint-adv rule profile:** which IDs to suppress for oe-core by default?
   Proposal: start from a run over the last ~200 merged oe-core commits and
   suppress any ID whose findings maintainers merged anyway.
3. **meta-patchtest guest mode:** upstream's "guest" mode runs patchtest in a
   QEMU image. Is the container (§3.5) isolation enough for the tinfoil
   step, or should phase 2 adopt the guest image?
4. **Branch detection:** subject tags cover `[scarthgap]`-style stable patches.
   Is a fallback to `git am --3way` on master plus a "did not apply" finding
   enough, or does phase 1 need sashiko-style baseline search?
