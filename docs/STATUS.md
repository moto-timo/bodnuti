# bodnuti review status

Automated review of Yocto Project and OpenEmbedded patches, explored two ways:
as a new project inside [sashiko](https://github.com/sashiko-dev/sashiko), and
as bodnuti, a standalone low-cost tool. Status as of 24 October 2026.

Prices are Anthropic API list rates for Claude Opus 5.5, Sonnet 5.5 and
Haiku 5.5.

## The two approaches

| Approach | State | Where |
|---|---|---|
| **Sashiko project**: `--project yoctoproject` (and `bitbake`), with an OpenEmbedded review workflow and prompt guides. Full multi-stage LLM review, results in sashiko's web UI. | Working prototype | [moto-timo/sashiko, branch `yoctoproject`](https://github.com/moto-timo/sashiko/tree/yoctoproject) (public fork, no pull request) |
| **bodnuti standalone**: a Rust tool that runs the checks OE already has (patchtest, oelint-adv, shellcheck, the yocto-docs Sphinx checks) and adds at most one cheap LLM call per patch. | Design only | this repository, [`designs/DESIGN.md`](../designs/DESIGN.md) |

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
- Build bodnuti phase 1 (deterministic checks), then score its single-call LLM
  layer against the same ten defects.
- Raise the sashiko project idea with the sashiko maintainers and with OE and
  Yocto maintainers before any upstream pull request. Nothing would be emailed
  to the lists without their agreement.
