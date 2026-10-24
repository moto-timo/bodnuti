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
   It runs **patchtest**, **oelint-adv** and a shell linter on the series and
   emits findings as JSON plus a human-readable text report.
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
providers, SQLite, axum, and email routing. Phase 1 needs none of them, and
phase 3 (§6) replaces the multi-stage engine with a single call per patch.

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
  ┌──────────────┐        ┌──────────────────┐        ┌──────────────┐
  │ patchtest    │        │ oelint-adv       │        │ shellcheck   │
  │ per patch    │        │ base vs. head    │        │ base vs. head│
  └──────┬───────┘        └────────┬─────────┘        └──────┬───────┘
         └─────────────────────────┼─────────────────────────┘
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
  shell.rs       extract shell from tasks and scripts, run shellcheck, parse, delta
  report.rs      text rendering
```

Dependencies: `clap`, `serde`/`serde_json`, `anyhow`, `tempfile`. All external
tools (git, b4, patchtest, oelint-adv) are subprocesses. There is no async
runtime: the work is sequential subprocess calls, so tokio adds nothing yet.

### 3.2 Finding model

```rust
pub enum Tool { Patchtest, Oelint, Shellcheck, Llm }  // Llm: phase 3 only
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

### 3.5 Shell

Shell task functions and shipped scripts run under `/bin/sh`. That is dash
on many build hosts and BusyBox ash on many targets, and bashisms that pass
on the author's bash host break there. These are a common class of real
OE defect.

1. Collect shell the series touches:
   - shell task functions (`do_*`, `pkg_*`) in changed `.bb`, `.bbappend`,
     `.inc` and `.bbclass` files, extracted with a small BitBake-aware
     scanner;
   - changed files under `files/` or a recipe directory that have a shell
     shebang.
2. Replace `${VAR}` BitBake expansions with a placeholder so shellcheck does
   not report them as unset shell variables.
3. Run `shellcheck --shell=sh --format=json`, on base and head, using the
   same delta rule as oelint (§3.4).

The scanner is heuristic and does not parse BitBake fully. A task it cannot
extract is skipped and logged, never reported as a finding.

### 3.6 Execution environment

A `Containerfile` (Debian/Ubuntu base) bundles:
the bodnuti binary, git, b4, shellcheck, Python with oelint-adv, the patchtest
requirements (`meta/lib/patchtest/requirements.txt`), and the bitbake host
dependencies. Repos and build directories live on a mounted cache volume. On
macOS, the documented path is `podman run … bodnuti check <msgid>`. Native
Linux can run the binary directly.

### 3.7 Untrusted input

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

1. **Deterministic checks (this document):** `bodnuti check`, container, JSON
   and text output.
2. **Watch and report:** poll lore (`openembedded-core`,
   `openembedded-devel`, `yocto-patches`, `yocto-meta-virtualization`,
   `yocto-docs`, `bitbake-devel`) by Message-ID and keep state in SQLite.
   Skip a resend whose patches all have a `git patch-id --stable` already
   checked. Publish reports on a static page (bodnuti.dev). Email replies
   stay opt-in per list, start in dry-run, and need the list owners'
   agreement.
3. **LLM review at a low token rate:** see §6.

## 6. Phase 3: LLM review at a low token rate

### 6.1 Why not sashiko's pipeline

Sashiko's `yoctoproject` workflow was measured on OE-Core patches at about
340k input and 49k output tokens per patch: roughly $2.20 at Opus 5.5 API
rates at default effort, and an estimated $1.40 at low effort. The input
comes from fan-out:

- 5-8 analysis stages run in parallel;
- each stage resends the system prompt, the diff and the prefetched source;
- each stage takes several tool-call turns, and every turn resends the whole
  context;
- pre-screen, planning, verification, up to 10 post-verification stages, a
  report and a summary come on top.

The OE lists carry about 5,100 patches a month (sampled from lore, October
2026, excluding replies and cover letters). At sashiko's rate that is
$7,000-11,000 a month at API rates.

### 6.2 Layers

Each layer runs only when the layers before it leave something to judge.

| Layer | What it does | Tokens per patch |
|---|---|---|
| 0. Skip duplicates (phase 2) | Skip patches whose `patch-id` was already checked; treat AUH upgrade patches as a known pattern | 0 |
| 1. Deterministic checks (phase 1) | patchtest, oelint-adv, shellcheck; new findings only | 0 |
| 2. Context gathering | Collect what an agent would fetch with tools: the touched recipe whole, its `.inc` and `.bbappend` files, the classes it inherits, and `git grep` hits in the layer for each changed function and variable name, under a fixed size cap | 0 |
| 3. One review call | One request per patch, no tools, structured JSON output. The system prompt and the OE guides come first and are cached. Then come the diff, the layer 2 context, and the layer 1 findings stated as facts the model must not repeat | ~15k in, ~1.5k out |
| 4. Escalation | Only for patches touching classes, `lib/oe`, `bitbake.conf` or a CVE fix, or when layer 3 reports low confidence: one Opus call with at most 5 tool turns | ~60k in, ~5k out, on ~10-20% of patches |

Layer 3 has no pre-screen, no planning call and no per-stage fan-out. The
guides it needs are chosen deterministically from the file types the patch
touches (recipe, class, Python, shell, patch file, docs).

### 6.3 Cost

About 5,100 patches a month, at the API rates cached on 2026-10-06.

| Layer 3 model | Per patch | Per month | With Batch API (-50%) |
|---|---|---|---|
| Claude Haiku 5.5 ($0.10 / $0.50 per MTok) | ~$0.002 | ~$12 | ~$6 |
| Claude Sonnet 5.5 ($2 / $10) | ~$0.045 | ~$230 | ~$115 |
| Claude Opus 5.5, low effort ($4 / $20) | ~$0.12 | ~$600 | ~$300 |
| + layer 4, Opus 5.5 on 15% of patches | +~$0.05 on average | +~$260 | +~$130 |

The token counts per patch are estimates until §6.5 measures them. The Batch
API is acceptable because a web report can arrive hours after the patch.

### 6.4 What this gives up

- **No verification pass.** Sashiko's verification stages exist to filter
  false positives. Without them, layer 3 output will be noisier. A second
  short call per finding ("is this real, given this context?") is the
  cheap substitute, if measurement shows it is needed.
- **Weaker cross-file reasoning.** Missed callers and missed sibling
  recipes are only found if layer 2's grep brought the right files in.
  Layer 4's tool turns are the fallback for risky patches.

### 6.5 Measurement before building

Score layers 1-3, on each candidate model, against sashiko's
`benchmarks/benchmark_yoctoproject_tiny.json`: ten OE-Core defects, each
with an upstream `Fixes:` commit. Record the detection rate, the false
positives per patch, and the tokens per patch. Compare them with sashiko's
`yoctoproject` result on the same corpus. Pick the cheapest configuration
whose detection rate is within reach of sashiko's. Then confirm it on a
held-out set chosen after the prompts are frozen, so the result is not tuned
to the corpus.

### 6.6 Untrusted input in prompts

Patch text, commit messages and file contents go into the prompt, and any of
them can contain instructions aimed at the model. Layer 3 has no tools, so
the worst case is a wrong or missing finding, never an action. Layer 4's
tools are read-only git queries confined to the worktree. Both layers put
untrusted content inside clearly delimited blocks, and the system prompt says
that text inside them is data. Findings are validated against the JSON
schema, and a finding that cites a file or line not present in the patch or
the layer 2 context is dropped.

### 6.7 Implementation

Phase 3 code calls the Claude API directly (Rust, raw HTTP to
`/v1/messages`, since there is no official Rust SDK). It does not reuse
sashiko's engine.

## 7. Testing

- Parser unit tests on captured patchtest and oelint-adv output: PASS, FAIL,
  SKIP, a traceback-only error, and the empty-output case.
- A delta unit test: findings that moved lines are not reported, and
  duplicates are counted as a multiset.
- One end-to-end fixture: a small oe-core series with a known missing
  `Upstream-Status`, run in the container (CI only, `#[ignore]` locally).

## 8. Open questions

1. **License:** Apache-2.0 (matches sashiko) or MIT? patchtest is GPL-2.0-only.
   bodnuti calls it as a subprocess and does not link it, so any license works.
2. **oelint-adv rule profile:** which IDs to suppress for oe-core by default?
   Proposal: start from a run over the last ~200 merged oe-core commits and
   suppress any ID whose findings maintainers merged anyway.
3. **meta-patchtest guest mode:** upstream's "guest" mode runs patchtest in a
   QEMU image. Is the container (§3.6) isolation enough for the tinfoil
   step, or should phase 2 adopt the guest image?
4. **Branch detection:** subject tags cover `[scarthgap]`-style stable patches.
   Is a fallback to `git am --3way` on master plus a "did not apply" finding
   enough, or does phase 1 need sashiko-style baseline search?
5. **Layer 3 model:** Haiku 5.5 for every patch, with Opus 5.5 escalation,
   or Sonnet 5.5 for every patch? §6.5 decides; the default is the cheapest
   configuration that measurement supports.
6. **Escalation rule:** is a fixed path list (classes, `lib/oe`,
   `bitbake.conf`, CVE fixes) plus layer 3's own confidence enough, or does
   escalation need its own measurement?
