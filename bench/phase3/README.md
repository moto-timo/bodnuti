# Phase 3 measurement

`score.py` measures a layer 3 configuration as `designs/PHASE3.md` §8
describes; it was written before layers 3 and 4 were built into bodnuti,
and knows prompts v1 to v3, not what bodnuti now sends (v2.4, `PROMPT`
in src/review.rs). It builds the layer 2 context for each benchmark
commit the way §4 describes, makes one request per patch (§5) to an
OpenAI-compatible endpoint, validates the answer (§5.3), and writes a
summary.

The other scripts:

- `docs_index.py` builds the docs index bodnuti's `docs` setting reads,
  from yocto-docs, bitbake and, with `--oe-core`, openembedded-core's
  bitbake.conf.
- `train_data.py` and `train_lora.py` build a fine-tuning set from the
  lists' review history and train a LoRA on it
  (`designs/TRAINING.md`; stopped, §8).
- `mine.py`, `history.py` and `lessons.py` built the held-out set, the
  replay of the lists' reviews and the v3 rules (below).

It is a measurement tool, not bodnuti's implementation: it reads git
objects directly, with no sandbox, so run it only on a trusted
openembedded-core clone. It needs Python 3.9 or later and nothing else.

## Setup

```sh
git clone https://git.openembedded.org/openembedded-core
git clone -b yoctoproject https://github.com/moto-timo/sashiko
install -m 600 /dev/null ~/.config/bodnuti/review.key
$EDITOR ~/.config/bodnuti/review.key   # the token from experiments.hetzner.com
```

## Run

Check the requests first, with no endpoint calls:

```sh
python3 bench/phase3/score.py --dry-run \
    --repo openembedded-core \
    --benchmark sashiko/projects/yoctoproject/benchmarks/benchmark_tiny.json \
    --guides sashiko/projects/yoctoproject/prompts \
    --model x --out results
```

Then both models on Hetzner's Inference API, with 25 clean commits for the
false-positive rate:

```sh
python3 bench/phase3/score.py \
    --repo openembedded-core \
    --benchmark sashiko/projects/yoctoproject/benchmarks/benchmark_tiny.json \
    --guides sashiko/projects/yoctoproject/prompts \
    --key-file ~/.config/bodnuti/review.key \
    --model Qwen/Qwen3.6-35B-A3B-FP8 --model Qwen3.8-27B \
    --clean 25 --out results/$(date +%F)
```

`results/<date>/summary.md` has a table per model and every finding under
the problem it was meant to find. `results/<date>/<model>/<commit>.json`
keeps the raw answer, tokens and wall time per patch.

The default endpoint is Hetzner's (`https://inference.hetzner.com/v1`).
Its experiment ends on 8 December 2026. `GET /v1/models` lists the model
names it accepts; they change during the experiment.

Options worth trying, one at a time:

- `--prompt v2`: each finding must quote the lines that show it, and is
  dropped if the quote is not in the request; the prompt adds OE-Core
  facts reviewers often get wrong, and asks for only what the patch
  introduces or leaves incomplete.
- `--min-confidence high`: drop findings the model rates lower, as
  bodnuti's `min_confidence` does. Dropped findings stay in the JSON.
- `--reasoning-effort none|low|high`: the OpenAI-style reasoning control,
  which FriendliAI's GLM models honour. GLM-5.3-Flash ignores
  `--thinking off`, and at its default effort can spend all 8192 output
  tokens reasoning and return no answer.
- `--thinking on` and `--thinking off`: Qwen3's reasoning mode. Reasoning
  costs output tokens and time, and may find more.
- No `--guides`: how much the sashiko review guides add.
- `--all`: also the four defects the deterministic checks catch, to see
  whether the model would duplicate them.

## A held-out set

The six benchmark defects are what the prompts are tuned on. `heldout.json`
holds 30 more, mined from OE-Core's `Fixes:` trailers with `mine.py` and
with three cosmetic ones removed; score a frozen configuration on it once,
with `--benchmark bench/phase3/heldout.json --all`. For clean commits not
used in tuning, add `--clean-rev origin/master~100`.

```sh
python3 bench/phase3/mine.py --repo openembedded-core \
    --exclude sashiko/projects/yoctoproject/benchmarks/benchmark_tiny.json > mined.json
```

Each entry's problem description is the fixing commit's message, so the
judge compares findings with what the fix says.

## Replaying the lists' reviews

`history.py` pairs patches resent as v2 or v3 with their v1, applies each
v1 to the branch as it was when it was sent (kept under `refs/bench/v1/`
in the clone), and writes entries whose problem description is the v2
changelog: the judge then asks whether the model raised what the
reviewers raised. `history-2026.json` holds 19 such pairs from
openembedded-core, June to October 2026, after removing pairs whose v2
changed nothing in the code; its commits exist only in the clone that
built it, so rebuild with `history.py` and curate the same way.

`lessons.py` collects code changelogs of resent patches from an earlier
period and has a model distil the rules reviewers keep applying;
`lessons-2023-2025.md` is its output for openembedded-core and
openembedded-devel, 2023 to 2025. `--prompt v3 --lessons <file>` adds
them to the v2 prompt.

## Reading the results

- **Found (judge)** asks a model whether any finding names the known
  defect. The judge is the model under test unless `--judge-model` says
  otherwise, so read the findings and confirm each "yes" and "no" by hand.
  The §8 bar is decided on the confirmed count.
- **In fixed file** is whether a finding cites a file the upstream fix
  changed. It is a hint, not a score: the right finding can sit in the file
  the bad commit changed.
- **Findings on clean commits** count as false positives. A few may be
  real issues that were merged anyway; note them, they are worth knowing.
- Wall time on a shared API says nothing about a self-run model host;
  measure it again there (§8).

The six defects are the benchmark the prompts will be tuned against. Before
trusting a number, freeze the prompts and run a held-out set chosen after
that (§8).
