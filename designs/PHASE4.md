# Design draft: bodnuti phase 4, focused review passes

Status: draft, for the maintainer to decide. Nothing here is built.

Phase 3 (`PHASE3.md`) sends each patch to the model once for review and
once more for each finding (the check pass). The clean retest of
27 November 2026 measured it on 13 patches of 26 November with 8 known
defects, every finding read by hand (`docs/STATUS.md`): Claude Code with
the plugin's rules and GLM-5.3 each found 5, gpt-oss-20b on a 16 GB GPU
4. All three found the python3-pillow test bugs. The misses fall into two
kinds:

- **Rule violations** a small model does not look for once the rules are
  out of layer 3: an `Inactive-Upstream` status without a date (zip), a
  package whose `LICENSE`, `FILES` and `PACKAGES` lines disagree
  (linux-firmware awinic and shikra), a commit message that says the
  opposite of the change (ptest-cargo). With the rules in layer 3,
  gemma4 26B checked headers and missed the pillow bugs (`PHASE3.md`
  §5.1), so the rules cannot simply go back.
- **Defects a fixed rule can find**: the `Inactive-Upstream` date, a
  carried patch with no context lines (openssh), a Backport URL with a
  truncated commit hash (curl). A model found each once; a check finds
  them every time, for nothing.

sashiko's Yocto workflow (moto-timo/sashiko, `yoctoproject_patch_review`)
answers the first kind with one focused stage per concern, each with only
its own guide: goal, implementation, commit hygiene, recipe metadata,
patch files, Python, reproducibility, then verification. It costs about
557k tokens a patch: every stage resends the patch, and tool loops resend
the whole conversation each turn. Phase 4 takes the focus, not the cost.

## 1. Focused passes

Layer 3 stays as it is: the code review, without the review rules. After
it, up to two more requests for the same patch, each only when the patch
touches what it is about:

| Pass | Runs when the patch changes | Request carries |
|---|---|---|
| Patch files | a `.patch` or `.diff` file | the patch, `rules-patch-headers.md`, `rules-bitbake.md` |
| Recipe metadata | a `.bb`, `.bbappend`, `.inc` or `.bbclass` | the patch, the touched files, the packaging, licensing and dependency rules its lines select |

- Each pass asks only its own question ("check the carried patch headers
  against these rules"), in the layer 3 schema, with the same validation.
- Its findings join layer 3's and go through the check pass, which already
  carries the rules.
- No tools and no tool loop: the request is built once, under the same
  context budget as layer 3 (`fit`).
- With `samples`, each pass is sampled as layer 3 is.
- Configuration: `passes = ["patch-files", "recipe-metadata"]` in
  `[review]`; unset, none.

Cost: one or two more calls per patch, of about layer 3's size. Hosted at
GLM-5.3's prices, about $0.02 to $0.04 more a patch with one sample; on
a local GPU, only time.

## 2. Checks learned from model findings

Three deterministic checks in `metadata.rs`, which already reads only the
diff and the commit message:

- **`Inactive-Upstream` without a date:** a carried patch whose
  `Upstream-Status: Inactive-Upstream` has no `[lastcommit: ...]` or
  `[lastrelease: ...]`.
- **A carried patch with no context lines:** an added `.patch` file whose
  hunks are all bare `+` and `-` lines, which GNU patch can only place
  by line number.
- **A truncated Backport hash:** an `Upstream-Status: Backport` URL whose
  commit hash is shorter than 40 hexadecimal characters.

Each is new in a file the series touches, so it counts as other checks do
(a finding against the series).

## 3. Prior feedback (later)

sashiko also has a stage that reads the review comments on v1 of a series
and checks whether v2 addresses them. bodnuti already pairs a v2 with its
v1 for live verification (`PHASE3.md` §11). Left for after §1 and §2.

## 4. Repeats across samples

With three samples, gpt-oss-20b showed five findings for the two
python3-pillow import bugs: some samples give no line and quote other
lines than the kept finding, so neither the 3-line rule nor a shared
quoted line matches them. Phase 3's table counts defects, not findings,
so it is unaffected. Phase 4 should merge such findings, for example by
the line the check pass quotes when it confirms them, and measure the
change with the rest.

## 5. Measurement

The same protocol as the clean retest (`RETEST-SETUP.md` in the
presentation repository): one bodnuti version for every setup, three
samples, every finding read by hand. On the fresh set of 27 November,
from which no rule or pass was derived, phase 3 against phase 3 with the
two passes, for gpt-oss-20b on the 16 GB GPU and for GLM-5.3. A pass is
kept if it finds defects the code review misses without adding false
findings the check pass lets through.

## 6. Questions

1. Are two passes the right cut, or should commit hygiene (message
   against diff) be a third?
2. Should the three checks of §2 go to patchtest upstream as well?
3. Does a hosted model still need three samples, or is one enough once
   the passes are in? Measure before choosing the watcher's setting.
4. Can a pass run on a smaller model than layer 3? A pass asks a narrower
   question over a shorter request (the patch and its rules, no classes
   or docs), so it may be enough, and it would make a laptop's integrated
   GPU keep up: gpt-oss-20b ran about seven times slower there than on
   the 16 GB card. Speed follows the parameters active per token, so
   gpt-oss-20b (3.6B active) is already among the fast ones. This needs
   a model per pass in `[review]`; measure it against one model for
   everything.
