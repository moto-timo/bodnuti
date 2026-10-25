# bodnuti review status

Automated review of Yocto Project and OpenEmbedded patches, explored two ways:
as a new project inside [sashiko](https://github.com/sashiko-dev/sashiko), and
as bodnuti, a standalone low-cost tool. Status as of 25 October 2026, when the
bodnuti phase 2 section was added.

Prices are Anthropic API list rates for Claude Opus 5.5, Sonnet 5.5 and
Haiku 5.5.

## The two approaches

| Approach | State | Where |
|---|---|---|
| **Sashiko project**: `--project yoctoproject` (and `bitbake`), with an OpenEmbedded review workflow and prompt guides. Full multi-stage LLM review, results in sashiko's web UI. | Working prototype | [moto-timo/sashiko, branch `yoctoproject`](https://github.com/moto-timo/sashiko/tree/yoctoproject) (public fork, no pull request) |
| **bodnuti standalone**: a Rust tool that runs the checks OE already has (patchtest, oelint-adv, shellcheck, the yocto-docs Sphinx checks) and will add at most one cheap LLM call per patch. | Phase 1 (deterministic checks) working; phase 2 watching six OE and Yocto lists and publishing to [bodnuti.dev/reports](https://bodnuti.dev/reports/) | this repository; see [`README.md`](../README.md) and [`designs/DESIGN.md`](../designs/DESIGN.md) |

The landing page for the eventual service is in
[moto-timo/bodnuti.dev](https://github.com/moto-timo/bodnuti.dev).

## Sashiko benchmark: first result

Ten OpenEmbedded-Core commits, each with a defect that a later upstream commit
fixed and named in a `Fixes:` trailer. Sashiko reviewed seven before the Claude
subscription usage limit ran out. An AI judge then compared its findings with
each known defect.

| Defect | Kind | Result |
|---|---|---|
| `:append` value with no leading space (ptest-packagelists) | override syntax | found |
| Typo `{$weston_modules}` in weston-start | shell | found |
| icu 78.1 version still split on `-` | Python, upgrade | found |
| Dropped patch left on disk (barebox) | patch files | found |
| Reordering result discarded (rootfs) | Python | found |
| Explicit `FILES` empties a split package (alsa-utils) | packaging | found |
| Missing target wayland dependency (piglit) | dependencies | partly found |
| Patch applied only for RISC-V (u-boot) | patch wiring | not reviewed |
| `echo -e` under dash (uboot-sign) | shell portability | not reviewed |
| API change misses a caller (kernel.bbclass) | cross-file Python | not reviewed |

How to read this:

- Seven of seven reviewed defects were caught, six exactly. The three hardest
  cases, including the cross-file one, are still unscored.
- It is noisy: 44 findings across seven patches, 37 of them not the target
  defect. Those still need sorting into real problems and false alarms.
- The result is optimistic. The review stages were chosen with this corpus in
  view, so a held-out set is needed before trusting the number.

## bodnuti phase 1: deterministic checks

`bodnuti check --repo <layer> <Message-ID>` fetches a series from lore,
applies it to a throwaway worktree, and runs each checker before and after the
series, reporting only what the series adds:

- patchtest's mbox and patch suites;
- oelint-adv, with OE-Core's unenforced style rules suppressed;
- shellcheck on shell tasks and shipped scripts;
- a check for patch files left behind after SRC_URI drops them;
- for yocto-docs and the BitBake manual, the Sphinx build, sphinx-lint,
  check-glossaries and check-confusables.

It runs on the host, from a container image, or from a Python wheel
(`pip install`), and uses no model tokens.

On the same ten OE-Core defects:

| Defect | Caught by |
|---|---|
| `:append` value with no leading space (ptest-packagelists) | oelint-adv |
| `echo -e` under dash (uboot-sign) | shellcheck |
| Typo `{$weston_modules}` in weston-start | shellcheck |
| Dropped patch left on disk (barebox) | bodnuti's leftover-file check |
| The other six | not deterministic: the job of the later LLM layer |

Noise:

- On 25 merged OE-Core commits held out from tuning, 23 come out clean.
- On the eight most recent yocto-docs commits, there are no documentation
  findings.

A run takes 4-12 s for a recipe patch and about 40 s for a documentation
patch, which builds the docs twice.

## bodnuti phase 2: watching the lists

Design: [`designs/PHASE2.md`](../designs/PHASE2.md). Deployment:
[`docs/DEPLOY.md`](DEPLOY.md) §8 and §9.

- `bodnuti poll` reads each list's lore search feed and groups messages
  into series. Once a series has all its patches and has been quiet for 15
  minutes, it is fetched with b4 and checked in the container, with no
  network.
- Each series is routed to a repository, either fixed per list or from its
  subject tag (yocto-patches), and checked against the branch its subject
  names.
- Resends whose patches were all checked before, matched by patch-id per
  branch, are recorded as duplicates and not checked again.
- Each poll also matches checked patches against the upstream branches. A
  matching patch-id is an exact match; the same subject and author is a
  probable one. A series whose patches have all landed is marked applied.
- `bodnuti publish` renders the results as static HTML (no JavaScript,
  mail text escaped) and copies them with rsync to bodnuti.dev. The
  deploy key there is write-only and confined to the reports directory.

Running on the Hetzner arm64 worker since 25 October 2026. A systemd user
timer polls six lists five minutes after each run ends, then publishes:
openembedded-core, openembedded-devel, bitbake-devel, yocto-docs,
yocto-meta-virtualization and yocto-patches (routed by subject tag,
including `[meta-yocto]`).

Before the deployment, a 48-hour openembedded-core run on a development
machine gave these results:

- 26 series checked and 0 failed;
- 3 resends recognized as duplicates;
- 7 patches matched upstream, all by patch-id, including a 6-patch wrynose
  series that landed in full.

That run found four problems, all fixed:

- a branch named inside the PATCH tag was missed;
- a series that does not apply aborted instead of reporting it;
- quoted-printable mail caused false patchtest failures;
- a v2 posted in the v1 thread mixed the two revisions.

## What it costs

The OE and Yocto lists carry about 5,100 patches a month (sampled from
lore.kernel.org, October 2026, replies and cover letters excluded): roughly
2,500 on openembedded-core, 2,400 on openembedded-devel, and a few hundred on
yocto-patches, yocto-docs and yocto-meta-virtualization.

| Approach | Per patch | Per month | Basis |
|---|---:|---:|---|
| Sashiko, default effort | ~$3.30 | ~$17,000 | measured: ~557k tokens in, ~67k out |
| Sashiko, low effort | ~$1.40–2.10 | ~$7,000–11,000 | estimate, not measured yet |
| bodnuti, Opus 5.5 single call + escalation | ~$0.17 | ~$860 | estimate |
| bodnuti, Sonnet 5.5 single call | ~$0.045 | ~$230 | estimate |
| bodnuti, Haiku 5.5 single call | ~$0.002 | ~$12 | estimate |

The Batch API halves the bodnuti figures, which suits a web report that can
arrive hours after the patch. On a Claude subscription, one five-hour usage
window held about eight sashiko reviews.

## Next steps

- Score the three unreviewed benchmark entries, and measure sashiko at low
  effort.
- Sort the 37 extra findings into real issues and false alarms.
- Check layerindex-web and error-report-web, the two Django applications on
  yocto-patches, with their own checker set (patchtest's mail checks and a
  Python linter) instead of the layer checks; until then they are unrouted
  (`designs/PHASE2.md` §3.2).
- Suppress the noisiest oelint rules (`oelint.vars.dependsordered`, the
  info-level `suggestedvar` rules).
- Build and score bodnuti's single-call LLM layer (phase 3) against the six
  defects the deterministic checks miss.
- Raise the sashiko project idea with the sashiko maintainers and with OE and
  Yocto maintainers before any upstream pull request. Nothing would be emailed
  to the lists without their agreement.
