# Design: bodnuti phase 3, model review on open-weight models

Phases 1 and 2 report what the existing checkers find. Six of the ten
OE-Core defects in the benchmark (`docs/STATUS.md`) are beyond them. Phase 3
adds layers 2 to 4 of `DESIGN.md` §6.2: deterministic context gathering, one
model call per patch, and escalation for risky patches. This document
replaces `DESIGN.md` §6.7 and settles §8 question 5 in one respect: the model
is open-weight, behind any endpoint that serves the OpenAI-compatible API,
not a closed-model API.

Decided with the maintainer before this document:

- Layers 3 and 4 run on open-weight models only, either on a model host
  bodnuti runs (§1) or on Hetzner's Inference API (§1.1), the provider that
  already hosts the worker. No closed-model API is used.
- The model runs as its own inference service, not inside the check
  container. The check container keeps `--network none`.
- bodnuti speaks one protocol to it: the OpenAI-compatible
  `/v1/chat/completions` that llama.cpp server, Ollama and vLLM all serve.
  Which server and which model are configuration, chosen by §8.

## 1. Where it runs

The worker (`DEPLOY.md`) has 2-4 shared vCPUs and 4-8 GB of RAM. A model in
the 20-30B range needs about 18-24 GB at 4-bit quantization, so it cannot
share the worker. Inference runs on a second host, the **model host**, on
the private network the worker and web host already share.

```
worker 10.1.0.2                               model host 10.1.0.4
┌──────────────────────────────┐              ┌──────────────────────────┐
│ bodnuti poll (host process)  │  HTTP, only  │ llama.cpp server /       │
│   ├─ podman run --network    │ ───────────▶ │ Ollama / vLLM            │
│   │  none: checks + layer 2  │  this route  │ loopback + private NIC   │
│   └─ layer 3/4 requests      │              │ no public inbound port   │
└──────────────────────────────┘              └──────────────────────────┘
```

- Only the `bodnuti poll` host process talks to the model host. Nothing that
  runs patch content has a route to it.
- The model host accepts TCP only from `10.1.0.2` on its inference port, in
  the Hetzner firewall and in its own nftables, and has no outbound access
  except package and model downloads during setup.
- The server binds to the private address, never `0.0.0.0` on the public
  interface. Ollama in particular has no authentication of its own.

The model host's size is decided by measurement (§8): a CPU host if a
mixture-of-experts model with few active parameters keeps up with §2, a GPU
host if not. Layer 4 (§6) may need the GPU even if layer 3 does not.

### 1.1 Hosted open-weight endpoints

A hosted service that serves open-weight models behind the same
OpenAI-compatible API needs no model host. bodnuti treats each one as one
more `endpoint` (§7), so moving between them, or to the model host, changes
configuration only.

**FriendliAI serverless** is the endpoint for §8's measurement and, if the
numbers and the cost hold, for production. As of 25 November 2026:

| | |
|---|---|
| Base URL | `https://api.friendli.ai/serverless/v1`, bearer token |
| Models measured | `zai-org/GLM-5.3-Flash` and `google/gemma-4-31B-it` (dense, Gemma terms); both with reasoning that can be switched off, structured output, and 262k-1M tokens of context |
| Price per MTok, input / output | GLM-5.3-Flash $0.15 / $0.50 (cached input $0.03); gemma-4-31B-it $0.14 / $0.40 |
| Data | zero data retention by default: request and response content is not kept beyond processing, and is not used for training |

Neither Qwen model of the earlier plan is served; FriendliAI offered these
two instead, as the models already running. The larger
`zai-org/GLM-5.3` ($1.26 / $3.96) is the candidate for layer 4.

Billing is per token, so it is measured against `DESIGN.md` §6.3's budget
rather than a host's monthly price. At about 12k input and 1k output tokens
per patch, either model costs about $0.002 a patch, or $10-12 a month for
5,100 patches: the same as the Haiku 5.5 estimate there. Reasoning output
adds to that, and §8 measures it. GLM-5.3 for layer 4 on 15% of patches
would add about $15-35 a month, depending on how many tool turns it takes.

**Hetzner's Inference API** (`https://inference.hetzner.com/v1`) serves
`Qwen/Qwen3.6-35B-A3B-FP8` and `Qwen3.8-27B` free, with no content stored,
but as an experiment that ends on 8 December 2026. It is an alternative
for measurement until then; nothing is built that depends on it.

**The model host** (§1) is the fallback, if a hosted endpoint's price,
terms or availability change.

Using a hosted endpoint needs, unlike the model host:

- outbound TCP 443 from the worker to the endpoint's host, added to the
  egress rules in `DEPLOY.md`;
- an API token, read from a file (`api_key_file`, §7) that only the
  `bodnuti` user can read, never from `watch.toml` or the command line.

A request carries the patch and its context. List patches are public, but
`bodnuti check --review` on an unsent series sends private work, so a
developer chooses its endpoint knowing the provider's data terms.

## 2. Throughput budget

About 5,100 patches a month is about 170 a day, or one every 8.5 minutes on
average. Layer 0 removes resends whose patch-id was already reviewed. The
budget for layers 3 and 4 together:

| | Target | Hard limit |
|---|---|---|
| Wall time per patch, average | 4 min | 8 min |
| Patches waiting for review | under one day's mail | 3 days, then the oldest are skipped and marked as such |

Headroom covers bursts (a 233-patch series such as the Home Assistant
upgrade) and backfills. A patch that exceeds the hard limit is reported as
not reviewed, never silently dropped, the same way a tool error is
(`finding::ToolError`).

The fixed part of every prompt (§5.1) comes first so the server can reuse
its KV cache across requests: `cache_prompt` in llama.cpp server, and
Ollama's and vLLM's prefix caching. This saves most of the prompt
processing, which dominates wall time on CPU.

## 3. Pipeline

Model review runs after a series' deterministic report is published, not
before it. A report never waits for the model.

1. Phase 2 checks the series and publishes as today.
2. The same check container, still with no network, also writes the layer 2
   context for each patch (§4) to its output directory.
3. `bodnuti poll` queues each patch that layer 0 does not skip in
   `state/review/queue/`.
4. A review step, in the same one-shot run after checking, takes patches off
   the queue oldest first until its time slice (`review_minutes`, §7) runs
   out, and calls layer 3 and, where §6 says so, layer 4.
5. Results go to `state/review/<series>/<patch>.json`; the next `publish`
   adds them to the report.

A crash loses at most the patch in flight, which stays queued.

## 4. Layer 2: context gathering

Runs inside the check container, on the worktree after the series is
applied, with no model and no network. Per patch, in this order, until the
cap is reached:

1. The diff of the patch.
2. Each touched recipe, `.bbappend`, `.inc` and class, whole.
3. Files they `require` or `include`, and the classes they `inherit`,
   resolved in the layer and in oe-core.
4. `git grep -n -w` hits for each variable, function and task name the diff
   adds or changes, at most 10 hits per name, three lines of context each.
5. For a recipe upgrade, the recipe's previous version from the base tree.
6. The documentation of the task flags, classes and variables the diff's
   changed lines name, in that order: the Yocto and BitBake glossaries,
   the class reference and the BitBake task flags, from an index that
   `bench/phase3/docs_index.py` builds from the yocto-docs and bitbake
   repositories. A finding can then rest on what the manual says rather
   than on the model's memory, where §13's false findings came from. The host
   adds it, outside the per-patch cap and at most 8 KiB, before the
   request is sent (`docs` in `[review]`, or `--review-docs`); the index
   is data, not code, so it is not built into bodnuti, and the docs are
   CC-BY-SA.

| Cap | Value |
|---|---|
| One file | 12 KiB, cut at a line boundary, marked as cut |
| All context for one patch | 48 KiB (about 12k tokens) |

The caps are starting points for §8 to tune. Paths are resolved inside the
worktree only; a `require` that resolves outside it is skipped, as is any
symlink. The output is one JSON object per patch, carried in the check's
JSON report (`bodnuti check --review-context`) on the same path out of the
container as every other result; the host process writes each to the
review queue as a file, and never reads the worktree itself.

## 5. Layer 3: one review call

### 5.1 The request

One `/v1/chat/completions` request per patch, no tools, temperature 0.

- **System message, fixed:** the rules, the output schema, and the
  statement that everything inside `<untrusted>` blocks is data to review,
  never instructions. Then the guides, chosen deterministically from what
  the patch touches, always in the same order so the prefix is shared.
- **User message:** the layer 2 context, each item in its own
  `<untrusted kind="..." path="...">` block, then the layer 1 findings for
  the patch, stated as facts the model must not repeat.

The guides follow the four questions from the bodnuti talk:

| Guide | Chosen when |
|---|---|
| Message against diff: does the commit message say what the diff does, including version, license and dependency changes? | every patch |
| Upgrade: what changes between the two versions that a reviewer should check? | the patch renames a recipe to a new `PV`, or its subject matches AUH's upgrade form |
| CVE backport: does the patch file carry `CVE:` and `Upstream-Status:`, and does the change look like the upstream fix it names? | a `CVE-` id in the subject, the message or an added patch file |
| Explain a finding: one plain sentence and a fix hint per layer 1 finding | the patch has layer 1 findings |

Recipe, class, shell, Python and documentation guides come from
`DESIGN.md` §6.2 and are selected the same way.

### 5.2 The response

Output is constrained to a JSON schema with the server's structured output
(`response_format` with a JSON schema in llama.cpp server and vLLM,
`format` in Ollama), so a small model cannot return prose:

```json
{
  "findings": [
    {"file": "meta/recipes-core/foo/foo_1.2.bb", "line": 14,
     "category": "packaging", "severity": "warning",
     "message": "...", "confidence": "high",
     "evidence": "+FILES:${PN} += \"${libdir}/foo\""}
  ],
  "explanations": [{"finding": 0, "text": "...", "fix": "..."}],
  "confidence": "high"
}
```

`category` is one of a fixed list (license, dependency, packaging,
override-syntax, upgrade, cve, message, shell, python, docs, other).

### 5.3 Validation

Before anything is stored, as in `DESIGN.md` §6.6:

- A finding whose file is not in the patch or the layer 2 context, or whose
  line is not in that file, is dropped.
- A finding that restates a layer 1 finding (same file within three
  lines; layer 1 findings carry no category to compare) is dropped.
- A finding whose `evidence` is not, line for line, in the request is
  dropped (diff markers and spacing ignored). The prompt asks for the
  quote and lists a few OE-Core facts behind recurring false findings;
  in §8's measurement this halved false findings.
- A finding the model rates below `min_confidence` (§7, default
  `medium`) is dropped. With the evidence rule, `medium` kept the most
  defects at 0.04 false findings per clean patch; `high` lost defects.
- Messages are cut at 500 characters. Reports escape them like all other
  text.
- A response that fails the schema after one retry is a tool error for that
  patch.

## 6. Layer 4: escalation

A patch is escalated when it touches `meta/classes*/`, `meta/lib/oe/`,
`meta/conf/bitbake.conf` or a CVE fix, or when layer 3 returns
`"confidence": "low"`.

Escalation uses a larger open-weight model on the same endpoint, through the
same API, configured separately (§7). It gets the layer 3 request, layer 3's
findings as unverified leads, and up to five turns of read-only tools:

| Tool | Does |
|---|---|
| `grep` | `git grep -n -w` in the worktree, at most 50 hits |
| `show` | a file from the worktree or the base tree, up to the per-file cap |
| `log` | `git log --oneline -n 20` for a path |

The tools read git objects only: `git grep -F` and `cat-file` at the
commit git am made of the patch, and `git log` up to it, in the clone the
patch was checked in. No checkout, worktree, filter or textconv runs, so
nothing from the patch is executed or written; a fresh check container per
call would add a container start per turn for no extra isolation. Every
path and pattern argument is validated as mail is, and tool output goes
back as `<untrusted>` data. The model never gets a shell. The check keeps
each applied commit under `refs/bodnuti/review/` until §11 has decided
the patch, so git gc does not drop it while it waits in the queue.

Tool turns go without structured output, which not every server combines
with tools; the answer is parsed from the text, and the last turn offers
no tools, so the model must answer. A failed or unparsable escalation is
tried once more. An escalation starts only if ten minutes of the run's
time slice are left.

Only the files the patch changes count for the path rule, not its
context. Layer 4's output replaces layer 3's for that patch, with the same
schema and validation; the result says why it was escalated. If no model is configured for layer 4, or the patch would not
finish inside the §2 limit, the patch keeps its layer 3 result and is
marked as not escalated.

## 7. Configuration

`watch.toml`, and the same keys under `[review]` for `bodnuti check`:

```toml
[review]
endpoint = "http://10.1.0.4:8080/v1"   # OpenAI-compatible; or
                                       # https://inference.hetzner.com/v1
api_key_file = "~/.config/bodnuti/review.key"   # mode 0600; unset: no
                                                # Authorization header
model = "..."                          # layer 3, chosen by §8
escalation_model = "..."               # layer 4; unset: no escalation
context_tokens = 32768                 # must match the server's setting
review_minutes = 30                    # time slice per poll run
timeout_seconds = 480                  # one request
guides = "..."                         # sashiko's yoctoproject prompts;
                                       # unset: the fixed rules only
thinking = false                       # enable_thinking; unset: the
                                       # model's default
reasoning_effort = "none"              # none, low, ...: FriendliAI's GLM
                                       # honours this, not enable_thinking
min_confidence = "medium"              # drop model findings rated lower
```

`guides`, `thinking` and `reasoning_effort` were added while building
layer 3: the guides are part of the configuration §8 measured, and
reasoning is one of its settings. GLM-5.3-Flash ignores `enable_thinking`
and, at its default effort, can spend all of `max_tokens` reasoning and
return no answer; `reasoning_effort` limits it.

An HTTP 429 from the endpoint leaves the patch queued and ends the review
step for this run; it is not a tool error.

Review is off when `[review]` is absent, so a deployment without a model
host is unchanged.

`bodnuti check --review` runs layers 2 and 3 on a local series before it is
sent, against the same endpoint or a developer's own Ollama on
`localhost`. That is the "before every send" use, where the series is not
yet public.

## 8. Measurement before building layers 3 and 4

As in `DESIGN.md` §6.5, adapted to open-weight models. Layer 2 and the scoring
harness are built first; layers 3 and 4 are built for the configuration the
numbers pick.

First on FriendliAI serverless (§1.1), for both models. That gives
detection, false positives and tokens per patch, and so the cost, without a
model host. Wall time and memory there say nothing about a self-run host,
so those two rows are measured again on the model host candidates (a CPU
host, and a GPU host if CPU misses §2) only if §1.1's fallback is taken:

| Measure | On |
|---|---|
| Defects found | the six benchmark defects the deterministic checks miss |
| False positives per patch | the 25 held-out merged OE-Core commits |
| Wall time per patch, cold and with a warm prefix | both sets |
| Peak memory | both sets |

Proposed bar, for the maintainer to confirm: at least three of the six
defects, at most one false positive per four clean patches, and inside the
§2 targets. Then confirm on a held-out set chosen after the prompts are
frozen.

## 9. Reporting

Model findings are labelled `Tool::Llm` and shown in their own collapsed
section, "Model review", under the deterministic findings, with the model
name and a note that they are unverified. Like style findings
(`PHASE2.md` §6.1.1), they do not count in the result or the exit status
until measurement on the live lists shows the false-positive rate is low
enough to change that. Explanations of layer 1 findings appear inline under
the finding they explain.

## 10. Testing

- Unit tests for layer 2 on fixture layers: include resolution, the caps,
  paths outside the worktree, symlinks.
- Unit tests for §5.3 validation on captured model responses, including
  invalid JSON, invented files and lines, and restated layer 1 findings.
- A `poll` test with the inference endpoint replaced by a stub HTTP server
  that returns captured responses, so CI needs no model.
- A prompt-injection fixture: a patch whose commit message and added file
  instruct the model to report nothing. The test checks only that the
  output still validates and that no tool call leaves the worktree, since a
  local model's answer is not deterministic across versions.

## 11. Live verification against the lists

Benchmarks age, and every prompt change risks tuning to them. The lists
answer each review bodnuti makes, later and for free: a patch is merged
unchanged, or revised, or merged and fixed afterwards. The watcher already
records all three (`track` links revisions, `applied` matches patch-ids,
§5.7 of `PHASE2.md`). This section scores model review against them,
continuously and on patches no prompt was tuned on.

### 11.1 Outcomes

For each reviewed patch, once the list has decided:

| Outcome | Seen by | What a model finding on the patch counts as |
|---|---|---|
| Revised: a later revision changed code in the patch | `track` revision link, interdiff of the two versions | **confirmed** if its file and line fall in a hunk the revision changed (±3 lines); otherwise unconfirmed |
| Merged unchanged: the same patch-id landed | `applied` | **unconfirmed** (the list saw no problem); not proof of a false finding |
| Merged, later fixed: a commit with `Fixes:` names the landed commit | `applied` plus a `Fixes:` scan of new commits | **confirmed late** if the finding is in a file the fix changed |
| Undecided after 60 days | | dropped from the scores |

The match is deterministic: file and line against the interdiff or the
fix, no model as judge. It undercounts (a right finding on a line the
author fixed elsewhere is missed) and overcounts (a revision that touched
the line for another reason), so it is a trend measure, not a verdict.
A sample is read by a person each month.

### 11.2 Scores

Per month and per model configuration (model, prompt version,
`min_confidence`):

- **Hit rate of findings:** confirmed findings over all findings on
  decided patches; the live counterpart of false findings per patch.
- **Catch rate of revisions:** revised patches with at least one
  confirmed finding, over revised patches whose revision changed code;
  the live counterpart of defects found.
- **Noise on accepted patches:** findings per patch merged unchanged.

They are kept in `state/review/outcomes/<yyyy-mm>.json` and shown on a
page of the site next to the cost, so a change of model or prompt shows
up as a step in the series. The configuration is recorded with each
review (§5), so months can mix configurations.

### 11.3 Built (draft)

`outcomes::run` runs after review in each poll. A revision is a later
checked patch on the same list with the same author and summary; the
interdiff is `git diff -U0` between the two applied commits, so a v2 on a
newer base carries the base's changes too (a trend measure, as above).
A patch with no revision and no landing after 60 days is dropped.
Decisions are kept in `state/review/outcomes/patches.json`, and each
month's counts per configuration (`model prompt min_confidence`) in
`<yyyy-mm>.json`. The page of the site is not built yet; the poll logs
the current month.

### 11.4 What it needs

- Review on in the watcher (`[review]`), so patches are reviewed when
  they arrive.
- The interdiff of a patch and its revision: both are already fetched;
  apply both to the same base in a worktree, as the checks do.
- A `Fixes:` scan of commits `applied` already reads.
- No model calls: the cost is git work on the worker.

## 12. Open questions

1. **Model host:** a CPU host is cheaper to keep running; a GPU host makes
   layer 4 practical. §8 decides, possibly a CPU host now and layer 4 later.
   Only needed if a hosted endpoint (§1.1) does not hold on cost, terms
   or availability.
2. **Layer 4 at all:** if the larger model cannot meet §2 on the chosen
   host, is layer 3 alone enough for the first release?
3. **Escalation rule:** `DESIGN.md` §8 question 6 still stands; a model
   that reports `"low"` confidence too often would escalate most patches.
4. **Model updates:** a new model version changes results. Pin the model by
   file hash in the report, and rerun §8 before changing it.
5. **Cost against §6.3:** Haiku 5.5 was estimated at $6-12 a month for
   layer 3. FriendliAI's per-token price for the two measured models lands
   in the same range (§1.1); §8 measures the real token counts, and the
   chosen configuration's cost goes into `STATUS.md`. A model host costs
   more per month, accepted only as the fallback.

## 13. Design draft: no false findings, on local hardware

Status: draft, for the maintainer to decide. Not built.

One day of real traffic (26 November 2026, 126 patches, GLM-5.3) gave
31 model findings: 7 real, 2 partly real, 22 false, read by hand
(`docs/STATUS.md`). The false ones mostly need one fact the request
lacks (the python3 manifest has no `misc` entry; `oe.kernel` exists on
master; `RUST_HOST_SYS` is the target triple), and one `git grep` or
file read settles them. A rerun with five more facts in the prompt gave
a different mix, not fewer: prompt rules alone do not fix this. The
goal is now stricter than §8's bar: **no false finding shown, and most
real ones**, on a model that runs on a laptop GPU, so that no hosted
tokens are needed.

### 13.1 A check pass

Every finding layer 3 keeps goes through one more request to the same
local model, with the tools of §6 (`grep`, `show`, `log`, read-only, at
the applied commit):

- **The question:** the finding, its quoted evidence, and "show that
  this is a defect in the patch, with the tools; answer confirmed or
  refuted, and quote the line that decides it". Refuted is the
  default: a finding that is not confirmed is not shown.
- **What it costs:** one request per finding, not per patch. At about
  0.25 findings per patch this adds a quarter of a review's time.
- **What it catches:** a claim about a file the request did not show
  (the manifest, a class, a library function) is checked against that
  file instead of guessed.

A finding refuted by the check pass is kept in `dropped` with the
deciding quote, for tuning.

Built (draft): `verify = true` in `[review]`, in `src/verify.rs`. The
check request is layer 3's, with sashiko's `false-positive-guide.md`
and `severity.md` now among the guides of every patch, plus the
finding and the tools when the applied commit is at hand (three turns).
A confirmation whose quote is in neither the request nor a tool's
output counts as refuted.

Review rules (draft): markdown files in the Claude Code plugin's
`yocto-review-rules` skill, built into bodnuti (a `rules/` directory
inside the guides directory replaces them), each rule condensed from at least two maintainer comments on
the lists that the author acted on in v2, with the quotes, a trigger and
a "do not report when" case. Each file's first line,
`<!-- select: .bb PACKAGECONFIG -->`, says which changed files and
changed lines select it, so a patch gets only the rules it touches, at
most 24 KiB. They are plain markdown on purpose: any other review tool
or agent can load the same rules.

### 13.2 Measured on a labelled set, locally

The 26 November findings, read by hand, become a test set: 40 patches
with their verdicts (the 10 real issues, the patches that drew false
findings, and clean patches), under `bench/phase3/`. A configuration
is scored by real issues found and false findings shown, on the laptop
GPU, without hosted tokens. Candidates so far, on one patch: gemma-4
26B-A4B with reasoning (about 10 minutes, 1 of 2 real, none false);
without reasoning, it and glm-4.7-flash are fast and wrong.

### 13.3 Questions

1. Is a quarter more review time per patch acceptable for showing no
   false findings?
2. Does the check pass run on every finding, or only on findings in a
   file the request did not show in full?
