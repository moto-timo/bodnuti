# Phase 3 measurement

`score.py` measures a layer 3 configuration as `designs/PHASE3.md` §8
describes, before layers 3 and 4 are built into bodnuti. It builds the
layer 2 context for each benchmark commit the way §4 describes, makes one
request per patch (§5) to an OpenAI-compatible endpoint, validates the
answer (§5.3), and writes a summary.

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

- `--reasoning-effort none|low|high`: the OpenAI-style reasoning control,
  which FriendliAI's GLM models honour. GLM-5.3-Flash ignores
  `--thinking off`, and at its default effort can spend all 8192 output
  tokens reasoning and return no answer.
- `--thinking on` and `--thinking off`: Qwen3's reasoning mode. Reasoning
  costs output tokens and time, and may find more.
- No `--guides`: how much the sashiko review guides add.
- `--all`: also the four defects the deterministic checks catch, to see
  whether the model would duplicate them.

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
