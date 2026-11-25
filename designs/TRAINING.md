# Design draft: fine-tuning layer 3 on the lists' review history

Status: draft, for the maintainer to decide. Nothing here is built.

Layer 3 (`PHASE3.md` §5) asks a general open-weight model to review an
OE patch. Measured in `docs/STATUS.md`, it finds most of the six benchmark
defects but only 4-5 of 30 mined from OE-Core's `Fixes:` trailers, and
needs a confidence filter and an evidence check to keep false findings
down. The lists hold what a model lacks: years of patches that reviewers
sent back, and the v2 that fixed them. This design trains a LoRA adapter
on that history.

## 1. What a training example is

One example is the layer 3 request for a patch, exactly as bodnuti builds
it (§5.1: system prompt, layer 2 context, layer 1 findings), and the answer
a good reviewer would have given, in the §5.2 schema. Training on the
same request bodnuti sends means the adapter needs no new code path: it is
one more `model` on an OpenAI-compatible endpoint (§7).

| Kind | Source | Target answer |
|---|---|---|
| Defect | a v1 patch whose v2 changed the code, with a changelog or interdiff that says why | findings naming what v2 fixed, on v1's files and lines |
| Clean | a v1 patch merged unchanged (matched by patch-id, as `applied` already does) | `{"findings": [], "confidence": "high"}` |

Clean examples are what teaches restraint, the main weakness measured so
far. They are plentiful; the ratio is a tuning knob, starting at three
clean to one defect.

## 2. Data

- **Source:** the public-inbox git mirrors on lore.kernel.org
  (`git clone https://lore.kernel.org/<list>/0`), read once, not scraped.
  Lists: openembedded-core, openembedded-devel, bitbake-devel,
  yocto-patches. Rough volume: at least 400 v2 messages a year on each of
  the first two, about 10,000 revised patches over ten years.
- **Pairing v1 with v2:** `track` already groups messages into series and
  links revisions by subject and author; the same code, run over the
  archive, pairs each patch of a v2 with its v1.
- **What changed:** apply v1 and v2 to the same base in a worktree
  (`workspace`) and take the interdiff; read the changelog under `---`.
- **Filtering out non-defects:** drop pairs where only the commit
  message, subject or `Signed-off-by:` changed, rebases with an empty
  interdiff, AUH version bumps, and series of CVE backports whose v2 only
  rewords. About a third of changelogs name a concrete defect (sample of
  60, `openembedded-core`, September-October 2026).
- **Labelling:** a teacher model (GLM-5.3 with high reasoning) turns the
  changelog and interdiff into findings on v1, constrained to the §5.2
  schema; each label is checked as §5.3 checks answers (file and line
  exist in v1), and a sample is checked by a person before training.

All archive content is untrusted input, as everywhere in bodnuti: paths
stay inside the work directory and nothing reaches a shell.

## 3. Split and measurement

- Train on patches sent before 2026; test on 2026, which also keeps the
  test set after the base model's training cutoff where possible.
- Report on the same three sets as the prompt work: the six tuning
  defects, `bench/phase3/heldout.json`, and held-out clean commits, plus a
  2026 v1/v2 test set of review catches.
- Compare with the best untrained configuration (prompt v2, `STATUS.md`);
  the adapter is worth serving only if it beats it on the §8 bar.

## 4. Model, training and serving

- **Base model:** open weights whose licence allows fine-tuning and
  serving an adapter: gemma-4-31B-it (Gemma terms), GLM-5.3-Flash if its
  weights are published under terms that allow it, or a Qwen3 model.
  Same family as measured, so the gain is the adapter's.
- **Training:** LoRA, a few GPU-hours on a rented GPU; the data and the
  scripts live in `bench/`, outside the bodnuti binary.
- **Serving:** a separate endpoint for the trained model, so the hosted
  serverless models stay as the fallback and a comparison: an
  OpenAI-compatible endpoint that loads the adapter:
  FriendliAI dedicated endpoints (to confirm with FriendliAI: LoRA
  support, price), or the model host of `PHASE3.md` §1 with vLLM and
  `--enable-lora`. Serverless per-token pricing (~$11 a month) does not
  apply to a dedicated endpoint, which bills for GPU time.

## 5. A prompt learned from the same history

Before any training, the same changelogs can teach the prompt:
`bench/phase3/lessons.py` collects code changelogs of resent patches
from a period before the test period and has a model distil the rules
reviewers keep applying; `score.py --prompt v3 --lessons` adds them to
the system prompt. It needs no GPU and no new endpoint, and its result
is the baseline a trained adapter has to beat.

## 6. Open questions for the maintainer

1. **Consent:** the archives are public, but training on contributors'
   and reviewers' text is a community question. Ask the OE and Yocto
   maintainers first, as for email replies? Publish the adapter and the
   dataset, or keep them private?
2. **Cost:** a dedicated endpoint or a GPU model host costs far more per
   month than serverless. Is that worth a better layer 3, or should
   training wait until the prompt work has plateaued?
3. **Base model:** which family, given licence and where it can be served.
4. **Scope:** only openembedded-core first, or all four lists?
