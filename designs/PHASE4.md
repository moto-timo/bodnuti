# Design draft: bodnuti phase 4, what only a model can see

Status: draft, for the maintainer to decide. Only §2 is built (branch
phase4-checks).

Phase 3 (`PHASE3.md`) sends each patch to the model once for review and
once more for each finding (the check pass). The clean retest of
27 November 2026 measured it on 13 patches of 26 November with 6 known
defects, every finding read by hand (`docs/STATUS.md`): Claude Code with
the plugin's rules, GLM-5.3 and gpt-oss-20b on a 16 GB GPU each found 4,
with 1, 2 and 4 false findings. All three found the python3-pillow test
bugs. The misses fall into two kinds:

- **Rule violations** a small model does not look for once the rules are
  out of layer 3: a package whose `LICENSE`, `FILES` and `PACKAGES`
  lines disagree (linux-firmware awinic and shikra), a ptest that needs
  a Perl module its package lacks (curl), a commit message that says the
  opposite of the change (ptest-cargo). With the rules in layer 3,
  gemma4 26B checked headers and missed the pillow bugs (`PHASE3.md`
  §5.1), so the rules cannot simply go back.
- **Defects a fixed rule can find**: a Backport URL with a truncated
  commit hash (curl). A model found it once; a check finds it every
  time, for nothing.

sashiko's Yocto workflow (moto-timo/sashiko, `yoctoproject_patch_review`)
answers the first kind with one focused stage per concern, each with only
its own guide: goal, implementation, commit hygiene, recipe metadata,
patch files, Python, reproducibility, then verification. It costs about
557k tokens a patch: every stage resends the patch, and tool loops resend
the whole conversation each turn. Phase 4 takes the focus, not the cost.

## 0. The rule: the model reviews only what no check can see

The model reports only what needs the code or its behaviour understood:
logic, tests, dependencies, whether the message matches the diff. A
header's form, a missing field and style are never the model's: if
maintainers act on such a pattern it becomes a check (§2), and if they
do not, nobody reports it.

The clean retest bears this out. Every real defect a model found needed
the code read (the pillow imports, curl's ptest module, ptest-cargo's
message, awinic's licenses); the two findings master shows to be false
were format a fixed rule could test (zip's dateless `Inactive-Upstream`,
openssh's hunks without context).

- The check pass also asks whether a fixed rule could find the finding,
  and drops it if so. Such findings are kept in the log as candidates
  for checks, not shown.
- The review rules that are only about form (most of
  `rules-patch-headers.md`) leave the check pass, rule by rule, each
  becoming a check or nothing.

## 1. A focused pass

Layer 3 stays as it is: the code review, without the review rules. After
it, one more request for the same patch, only when the patch touches
recipe metadata:

| Pass | Runs when the patch changes | Request carries |
|---|---|---|
| Recipe metadata | a `.bb`, `.bbappend`, `.inc` or `.bbclass` | the patch, the touched files, the packaging, licensing and dependency rules its lines select |

- The pass asks only its own question (do the recipe's licenses,
  packages and dependencies agree with what it builds and installs), in
  the layer 3 schema, with the same validation.
- A patch-header pass was planned as well; by §0 it would look mostly for
  form, so it is dropped.
- Its findings join layer 3's and go through the check pass, which already
  carries the rules.
- No tools and no tool loop: the request is built once, under the same
  context budget as layer 3 (`fit`).
- With `samples`, the pass is sampled as layer 3 is.
- Configuration: `passes = ["recipe-metadata"]` in `[review]`; unset,
  none.

Cost: one more call per recipe patch, of about layer 3's size. Hosted at
GLM-5.3's prices, about $0.02 more a patch with one sample; on a local
GPU, only time.

## 2. Checks learned from model findings

A model finding becomes a deterministic check only when master shows
that maintainers act on it. Scanning the 3,499 patch files of
openembedded-core and meta-openembedded master:

- **A truncated Backport hash** (built, `metadata.rs`): an added
  `Upstream-Status: Backport` URL whose GitHub, GitLab or cgit commit
  hash is longer than 12 and shorter than 40 characters. One match in
  master.
- **`Inactive-Upstream` without a date** (dropped): 36 statuses in
  master give no `[lastcommit: ...]` or `[lastrelease: ...]`, among them
  zip's own patch, which the model had reported.
- **Hunks without context lines** (dropped): 17 accepted patch files
  have them, oe-core's tar CVE-2026-5704 regression fix among them.

## 3. Prior feedback (later)

sashiko also has a stage that reads the review comments on v1 of a series
and checks whether v2 addresses them. bodnuti already pairs a v2 with its
v1 for live verification (`PHASE3.md` §11). Left for after §1 and §2.

## 4. The Claude Code plugin

Everything in phase 4 works through the plugin as well. The `bodnuti`
skill runs bodnuti's checks in the released container image, so §2 comes
with the next release. The rule of §0 and the pass of §1 go
into the `yocto-review-rules` skill: it tells Claude to leave form to
the checks and which files to read for a recipe change, as it does now
by topic, so the rules are one place for both.

## 5. Repeats across samples

With three samples, gpt-oss-20b showed five findings for the two
python3-pillow import bugs: some samples give no line and quote other
lines than the kept finding, so neither the 3-line rule nor a shared
quoted line matches them. Phase 3's table counts defects, not findings,
so it is unaffected. Phase 4 should merge such findings, for example by
the line the check pass quotes when it confirms them, and measure the
change with the rest.

## 6. Measurement

The same protocol as the clean retest (`RETEST-SETUP.md` in the
presentation repository): one bodnuti version for every setup, three
samples, every finding read by hand. On the fresh set of 27 November,
from which no rule or pass was derived, phase 3 against phase 3 with §0 and
the pass, for gpt-oss-20b on the 16 GB GPU and for GLM-5.3. Each is
kept if it finds defects the code review misses without adding false
findings the check pass lets through.

## 7. Questions

1. Is one pass the right cut, or should commit hygiene (message against
   diff) be a second?
2. Should the check of §2 go to patchtest upstream as well?
3. Does a hosted model still need three samples, or is one enough once
   the pass is in? Measure before choosing the watcher's setting.
4. Can a pass run on a smaller model than layer 3? A pass asks a narrower
   question over a shorter request (the patch and its rules, no classes
   or docs), so it may be enough, and it would make a laptop's integrated
   GPU keep up: gpt-oss-20b ran about seven times slower there than on
   the 16 GB card. Speed follows the parameters active per token, so
   gpt-oss-20b (3.6B active) is already among the fast ones. This needs
   a model per pass in `[review]`; measure it against one model for
   everything.
