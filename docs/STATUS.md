# bodnuti review status

Automated review of Yocto Project and OpenEmbedded patches, explored two ways:
as a new project inside [sashiko](https://github.com/sashiko-dev/sashiko), and
as bodnuti, a standalone low-cost tool. Status as of 25 October 2026, after the
first day of the bodnuti phase 2 deployment, the first security scans, the
addition of CI and the Django application checks.

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
including `[meta-yocto]`, `[layerindex-web]` and `[error-report-web]`). The
report pages carry the bodnuti logo and
favicon, served from the reports directory under a Content-Security-Policy
that allows only same-site images and inline styles.

On the first day of the deployment, all six lists polled cleanly:

- 8 series checked (5 openembedded-core, 2 openembedded-devel,
  1 bitbake-devel), 0 failed;
- 3 yocto-patches series unrouted, all `[layerindex-web]`, as planned
  (`designs/PHASE2.md` §3.2);
- each poll takes about a second when there is nothing new to check.

### Django applications

layerindex-web and error-report-web, the Django applications behind
layers.openembedded.org and errors.yoctoproject.org, send their patches to
yocto-patches. A `[repos]` entry with `kind = "django-app"` checks them
with patchtest's mail checks, ruff, djLint and shellcheck instead of the
layer checks (`designs/PHASE2.md` §3.2, §5.2):

- ruff with a fixed rule set: the classic defaults, the Django rules and the
  security rules, less member order, assert and plain subprocess calls;
- djLint limited to template bugs (warnings) and accessibility (info),
  without formatting;
- trivy on the `requirements*.txt` files a series changes, offline against
  a database the watcher downloads daily: advisories the series adds are
  findings (CRITICAL and HIGH are errors), and advisories it removes are
  listed as fixes, which do not count as findings.

On the layerindex-web series "requirements.txt: fix djangorestframework and
GitPython vulnerabilities", trivy confirms the series fixes 45 advisories
(GitPython, djangorestframework and Pillow) and adds none. The worker runs
tier 2 since 25 October 2026: the first poll downloaded the database in
about 11 seconds, and the three layerindex-web series were checked again
with it.

The worker's checks also stopped flooding its journal: podman no longer
logs each container's events and output there, only systemd's one line per
container. The containers' memory, CPU and process limits are enforced
(the `memory`, `cpu` and `pids` cgroup controllers are delegated to the
`bodnuti` user).

On a test patch to layerindex-web, each checker reported exactly what it
added: an undefined name and a `shell=True` call (ruff), a template tag
inside an HTML comment and an image without `alt` (djLint). A series whose
tags named no configured repository is now tried again on every poll, so
adding the two applications to `[repos]` checked the three layerindex-web
series the first day had left unrouted. All three are clean: patchtest's
mail checks 63 pass, 0 fail, and no ruff or djLint findings.

### Report pages

Each finding on a series page is a block of its own, laid out after the
review cards on sashiko.dev (`designs/PHASE2.md` §6.1): a severity badge,
the tool, check id and location, the message with its line breaks, and the
lines of the patched file around it, numbered, with the finding's line
highlighted. The check records those lines in the report, since only it has
the patched tree. Fixes and tool errors take the same form, and
`report.txt` has the same blocks in plain text.

### Other repositories on yocto-patches

Four more repositories send patches to yocto-patches (about 240 messages
in three months between them), now routed by their tags
(`designs/PHASE2.md` §5.4):

- meta-security, a layer, with the layer checks;
- pseudo, in C, with a `c` kind: patchtest's mail checks, shellcheck, and
  cppcheck's errors and warnings;
- yocto-autobuilder-helper and yocto-autobuilder2, with a `python` kind:
  patchtest's mail checks, ruff (including the helper's scripts without a
  `.py` suffix and BuildBot's `master.cfg`), shellcheck, and a check that a
  touched JSON file still parses, since the autobuilder reads `config.json`
  when it starts.

Four recent autobuilder series and a recent pseudo series check clean
(two other pseudo series no longer apply to today's tree, which already
has them); test patches that break
`config.json`, use an undefined name in a shebang script, or dereference a
null pointer in pseudo report exactly those. Building pseudo with compiler
warnings is a later tier.

The backfill's last unrouted series led to four routing refinements
(`designs/PHASE2.md` §3.1.1): a repository named inside the PATCH tag
(`[PATCH yocto-autobuilder2]`) routes; a tag may name several repositories,
the series' paths choosing among them, so `[AUH]` reaches auto-upgrade-helper
for the tool's own patches and openembedded-core for its bot's upgrades; a
missing clone is logged as configuration and spends no attempts; and a
branch with a slash, as in meta-lts-mixins' `scarthgap/go`, keeps its full
name. meta-lts-mixins, meta-darwin, auto-upgrade-helper and the two
yocto-metrics repositories are added to the configuration.

Deploying found one more problem, now fixed: lore answers a search with no
matches with 404, so the quiet lists (yocto-docs, yocto-meta-virtualization)
failed every poll until an empty result was read as no new messages.

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

## bodnuti CI and security

bodnuti reads untrusted mail, so its dependencies and image are scanned and
its parsers fuzzed:

- `make audit` runs cargo-audit on `Cargo.lock`, trivy on `Cargo.lock` and
  `requirements.txt` (the image's Python pins, moved out of the
  Containerfile so scanners can read them), and gitleaks on the git
  history.
- `make audit-image` scans a built image with trivy, failing on HIGH or
  CRITICAL findings that have a fix. `DEPLOY.md` runs it before an image
  ships.
- `make fuzz` runs three cargo-fuzz targets: mbox parsing and everything
  read from a parsed patch, the lore feed and grouping it into series, and
  subject tags. For this, the mail and lore parsers are now a small
  library the binary uses. Two minutes per target, about 5.2 million
  inputs, found no crash.
- Dependabot checks the Cargo and pip pins and the GitHub Actions weekly.

GitHub Actions runs on every push and pull request: fmt, clippy and the
tests; a build on rust-version 1.89; `make audit`; the image build, a smoke
test of its tools and `make audit-image`; and a manylinux wheel, installed
and run. The scans and the image also run daily, the fuzz targets weekly
(five minutes each). trivy and gitleaks are pinned release binaries checked
against their checksums, not third-party actions.

The first scans on 25 October 2026 found one real problem, now fixed:
quick-xml 0.37.5, which parses the lore feed, had two high-severity
denial-of-service advisories (RUSTSEC-2026-0194 and RUSTSEC-2026-0195). It
is now on 0.42. The other results:

- no other Rust or Python dependency findings;
- no secrets in the bodnuti or bodnuti.dev history;
- 75 HIGH, 0 CRITICAL findings in the image's Debian trixie packages, none
  with a Debian fix yet. Rebuilding the image picks the fixes up.

The worker runs these fixes and upgrades since 25 October 2026.

Dependabot's first six pull requests were version updates, not security
fixes. The two action updates were merged. The toml 1.1, quick-xml 0.42
and ureq 3 updates needed code or feature changes and were done by hand,
and a proposed bump of the rust-version toolchain was declined.

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
- Decide on tier 3 of the Django application checks, Django's own `check`
  and `makemigrations --check`, after tiers 1 and 2 have run for a while
  (`designs/PHASE2.md` §3.2).
- Decide on a license policy for `cargo deny`.
- Suppress the noisiest oelint rules (`oelint.vars.dependsordered`, the
  info-level `suggestedvar` rules).
- Build and score bodnuti's single-call LLM layer (phase 3) against the six
  defects the deterministic checks miss.
- Raise the sashiko project idea with the sashiko maintainers and with OE and
  Yocto maintainers before any upstream pull request. Nothing would be emailed
  to the lists without their agreement.
