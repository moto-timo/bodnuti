# bodnuti review status

Automated review of Yocto Project and OpenEmbedded patches, explored two ways:
as a new project inside [sashiko](https://github.com/sashiko-dev/sashiko), and
as bodnuti, a standalone low-cost tool. Status as of 25 November 2026, after
the first weeks of the bodnuti phase 2 deployment, the first security scans,
the addition of CI, the Django application checks, the releases up to 0.5.0,
the fixes the live reports called for, patchtest's metadata checks, a
Claude Code plugin and the SLSA source policy, and the first measurement
of phase 3 model review on open-weight models. The bodnuti repository is
public.

Prices are Anthropic API list rates for Claude Opus 5.5, Sonnet 5.5 and
Haiku 5.5, and FriendliAI serverless list rates for the open-weight
models.

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
- checks for lines over 200 characters, for LIC_FILES_CHKSUM changes
  without a `License-Update:` line and for added CVE_CHECK_IGNORE, from
  patchtest's metadata suite;
- for yocto-docs and the BitBake manual, the Sphinx build, sphinx-lint,
  check-glossaries and check-confusables.

It runs on the host, from a container image, or from a Python wheel
(`pip install`), and uses no model tokens. A Claude Code plugin, in
`plugins/bodnuti`, runs it on a local series, patch files or a series on
lore; the repository is its marketplace
(`/plugin marketplace add moto-timo/bodnuti`).

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
`report.txt` has the same blocks in plain text. The index lists one series
per line, as sashiko.dev's does: every column but the subject has a fixed
width, long names are cut short, and the page is up to 90rem wide. The
series indexes reload themselves every five minutes, so a page left open
shows new series and results.

Matching a landed patch by subject and author now uses the author git am
records, from an in-body `From:` line when the sender is not the author: a
meta-lts-mixins backport of an OE-Core fix, sent by someone else, matched
only once it did. And a patch that landed before its series was checked is
now matched when the series is checked: the scans read only commits that
arrive later, so after the two-week backfill openembedded-core showed 1
landed series in a week while opensbi, curl 8.23.0 and createrepo-c, all on
master, showed none. With the fix deployed and the upstream scan positions
reset, the next poll matched about 145 backfilled series as applied
(about 67 on openembedded-core, 65 on openembedded-devel, 12 on
yocto-patches).

oelint-adv's layout rules (spacing, ordering, suggested variables) no
longer bury the findings that matter. Measured on the 397 published series,
they gave about 470 of the 919 findings. Their findings are now style: kept
apart, collapsed under each patch behind "Show N style findings", and not
counted in the result or the exit status (`designs/PHASE2.md` §6.1.1). The
style rules OE-Core does not follow, once suppressed, are shown that way
too; only `homepageping` and `bbclass.underscores`, whose result is not
about the series, stay suppressed.

The third tier runs Django's own checks, `manage.py check` and
`makemigrations --check`, before and after each series, offline against
in-memory sqlite, in a venv per application built into the image from
copies of their requirements (`designs/PHASE2.md` §5.5). On
error-report-web, a patch adding a model field without its migration
reports exactly that; layerindex-web's checks run clean. The copies keep
the applications' own pins, so Dependabot alerts on them are fixed in the
application, not here. layerindex-web master has since pinned Pillow
13.0.0, GitPython 3.2.1 and djangorestframework 3.18.2, and 0.4.0 took
those pins, closing the 30 alerts on the copy. layerindex-web then moved to
soupsieve 4.1, and the copy took its pins again on 22 November 2026,
closing the last four alerts.

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
name. meta-lts-mixins, meta-darwin, meta-rockchip, auto-upgrade-helper and the two
yocto-metrics repositories are added to the configuration.

Later unrouted series added meta-selinux, meta-tensorflow, meta-zephyr and
wic (the image tool split out of OE-Core, `kind = "python"`). A ten-patch
meta-lts-mixins linux-firmware series showed that the path check must also
skip files an earlier patch of the series added or renamed. And
`maintainers.inc` showed that `conf/distro/` and `conf/machine/` files must
be linted as distro and machine configuration, not through a stub recipe.

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

### Fixes from the live reports

Reading the published reports on 26 October turned up these, all fixed:

- **Series built on staged patches.** A meta-oe series built on patches
  staged before `master` failed to apply, and every checker that needs the
  patched tree was skipped. A series that does not apply to its branch is
  now checked on the branch's staging branch, named per repository: OE-Core
  stages `master` on `mathieu/master-next` in openembedded-core-contrib,
  meta-openembedded `master` on `yoe/mut` and `scarthgap` on
  `anujm/scarthgap` in meta-openembedded-contrib (`designs/PHASE2.md`
  §5.6). The check uses the commit just before the series when the staging
  branch already carries it, and records the target branch commit the
  staging branch was forked from, since staging branches are rebased at
  any time. Re-checking the 44 openembedded-devel series whose reports
  ended in `bodnuti.apply`, three now apply; 31 of the others had since
  landed, so a re-check could not tell.
- **A revision sent as a reply.** A `[PATCH v2]` answering patch 11/12 of an
  OE-Core series was folded into the v1 series and never checked; and as
  it changed only the author, its patch-id was v1's, so it would have
  counted as a resend. Such a reply is now a series of its own, a resend
  must match the mail (author, summary, commit message) as well as the
  diff, and `bodnuti check` given a Message-ID checks that revision. On
  the worker the v2 checked clean (patchtest 10 pass, 0 fail). On the site
  it sits under the patch it revises, and the series' row shows the series
  as it stands (§6.3).
- **A resend to another repository.** A `[meta-networking]` series first
  sent to openembedded-core, and checked against OE-Core there, was then
  taken for a resend when sent to openembedded-devel, and never checked
  against meta-openembedded. A resend now has to match the repository as
  well as the branch, and a resend's row shows its original's result
  ("resend: clean") instead of only "resend".
- **Python read as shell.** librsvg's `do_compile_ptest_cargo:prepend`
  extends a Python task of ptest-cargo.bbclass, and shellcheck failed to
  parse it, leaving the recipe's shell unchecked. A function whose body is
  BitBake Python is now skipped like a `python` one; seven functions in
  OE-Core and meta-openembedded are.
- **A patch sent as an attachment.** An Outlook mail carried its patch as a
  base64 attachment and was reported as having no patches. It is still not
  checked, but the reason now names the attached file and asks for
  `git send-email`.

A scan of every published report with findings (128 of 426 series, on all
lists, against local clones of all 18 repositories) found three more:

- **Series checked after they landed.** Of the 65 landed series that
  showed findings, 59 had been checked after their patches landed, by
  re-checks and backfills against the branch as it was: their patches
  failed to apply to a tree that already had them, or every finding in an
  upgraded recipe looked new (56 for one meta-python series, 250 for a
  linux-firmware upgrade). bodnuti now looks for landed patches before
  every check: a series whose patches all landed is marked applied without
  a check, and one partly landed is checked against the branch just before
  the first of them, which its page notes. Re-checked on the worker, 51 of
  the 59 became "already applied", one clean, and seven, partly landed,
  show only what their patches add: mail checks, and lines the patches
  wrote.
- **A file several patches touch.** It was checked once per patch, so each
  of its findings was counted as often, and the copies the base could not
  cancel showed as new: a `[wrynose]` python3 upgrade reported 14 findings
  its recipe already had, a pjproject series 27, 24 of them copies. Five
  reports had such copies; each file is checked once now.
- **Staging coverage.** Of 15 genuine "Failed to merge" errors, re-run with
  today's branches and staging, two now apply to the branch, four apply on
  the configured staging branches, and two on OE-Core's stable staging
  branches, `stable/<release>-nut` in openembedded-core-contrib, which the
  deployment now names; layerindex-web and error-report-web stage on their
  own `master-next`. Of the rest, three `[scarthgap]` series built on what
  `stable/scarthgap-nut` held in mid-October and has since dropped, and four
  look like genuine conflicts.

### Builds and CI of patches under review

Two more sources of evidence about a series are built, shown on its page
apart from its result:

- **GitHub CI of the Django applications** (§5.8). The maintainer pushes
  patches under review to `master-next` on GitHub (both layerindex-web
  repositories and error-report-web), where CI and Safety run. Each finished
  run's commit is fetched, its patches on top of `master` are matched to
  checked series by patch-id, and the run is recorded on each. A run that
  skipped its own steps (Safety without a key, in upstream's layerindex-web)
  shows as skipped. A failure is a series' own only once two runs failed
  newly carrying it. CodeQL alerts on `master` are shown on the series whose
  patch wrote the alert's line, found by blame, with a read-only token on
  the worker. On the worker, five series carry 4 to 12 runs each.
- **The Yocto Project AutoBuilder** (§5.7), for OE-Core and BitBake, from
  `a-full` builds. Each build's trees come from poky-ci-archive's tag for
  it, so staging branches can rebase; its commits on top of their branch are
  matched to checked series, and its failed steps are read from their
  `errors` logs. A failed fetch is infrastructure; a failure in a recipe the
  series changes is the series' at once; any other once two builds agree.
  Against the live AutoBuilder, an OE-Core strace upgrade was found in four
  `a-full` builds on `mathieu/master-next`, the one failure among them a
  `cvelistV5` fetch, classed as infrastructure. Error reports are only
  linked until error-report-web has a JSON read API. meta-openembedded is
  traced too, from Khem Raj's `meta-oe` builder (30 builds in nine days, of
  `yoe/mut` and `anujm/<release>`): nothing archives its trees, so each
  poll pins the staging branch tips the clone fetched under
  `refs/bodnuti/`, and a build's commit is read from there or from its own
  branch. On the worker the AutoBuilder evidence shows on 35 series, from
  130 `a-full` builds so far. meta-virtualization's `meta-virt` builds
  are children of `a-full` that build its own branches, and it stages
  nothing now, so there is nothing of its own to trace.

Building the AutoBuilder evidence found that patch-ids were computed from
the raw mail, so a patch the list re-encoded as quoted-printable matched no
commit: not upstream, not as a resend, not in a build or a CI run. They are
read from the decoded mail now; the first poll recorded every checked
series' patch-ids again, and resetting the upstream scan positions rescanned
the branches with them, matching 131 patches. Landed moved only from 55% to
56% of all series (240 of 423), so the bug was a small part of the low
counts; of series older than three days, about 61% have landed, and most of
the rest look still in review.

### More fixes from the reports

- **Series sent unthreaded.** A wrynose initramfs series came as eight
  threads, each patch without `In-Reply-To`, and each patch was checked
  alone, so 2/7 could not apply. Messages with no parent, numbered in one
  series, from one sender (the lore feed's author), with the same tags,
  within ten minutes, are now one series, fetched from all their threads
  (§3.3); a member arriving later reopens it. The initramfs series is one
  row, clean.
- **A new revision's cover letter in reply to the old one.** OE-Core's SPDX
  v5 cover letter replied to v1's, and the series was checked as v1's five
  patches. It starts its own series now; v5's six patches apply.
- **Multipart mail.** Patches sent as `multipart/alternative` (the Sweego
  mailer, the groups.io web poster) made patchtest run no tests and gave
  wrong patch-ids; their text part is decoded now, and patchtest found a
  missing commit message in two bitbake-devel series.
- **Empty shell functions** (`do_compile () {` then `}` in rust's recipe)
  are valid BitBake but not shell; they get a no-op now, and the recipe is
  checked.
- **Staging in two places.** OE-Core and BitBake stage `master` both on
  `mathieu/master-next` in their contrib repositories and on their own
  `master-next`; `staging` takes a list, tried in order.
- **GitHub CI runs that tested nothing**, such as upstream layerindex-web's
  Safety without a key, show as skipped, not passed.

## bodnuti CI and security

bodnuti reads untrusted mail, so its dependencies and image are scanned and
its parsers fuzzed:

- `make audit` runs cargo-deny on `Cargo.lock` (advisories; a license
  policy in `deny.toml` that allows only the permissive licenses bodnuti's
  crates use; crates.io as the only source), trivy on `requirements.txt`
  (the image's Python pins, moved out of the Containerfile so scanners can
  read them), and gitleaks on the git history.
- A Main image workflow publishes `ghcr.io/moto-timo/bodnuti:main` (and
  `:main-<commit>`) for amd64 and arm64 on each push that changes the
  image, with build provenance, SPDX SBOMs and a keyless cosign signature,
  as releases have. The package is public, and the worker pulls and
  verifies it, taking its host binary from the same image, instead of
  building locally. The image builds with Rust 1.99 and carries
  shellcheck 0.11.0.
- `make audit-image` scans a built image with trivy, failing on HIGH or
  CRITICAL findings that have a fix. `DEPLOY.md` runs it before an image
  ships.
- `make fuzz` runs three cargo-fuzz targets: mbox parsing and everything
  read from a parsed patch, the lore feed and grouping it into series, and
  subject tags. For this, the mail and lore parsers are now a small
  library the binary uses. Two minutes per target, about 5.2 million
  inputs, found no crash.
- Dependabot checks the Cargo and pip pins, the GitHub Actions and the
  Containerfile's base images weekly. trivy is pinned twice, in the
  Containerfile and in `.github/install-scanners.sh`, and Dependabot bumps
  only the first.
- The image upgrades its Debian packages when it is built, so it takes
  Debian's security fixes without waiting for a new debian:trixie-slim.

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
  with a Debian fix yet. Each image build now upgrades the packages and
  picks the fixes up.

The worker runs these fixes and upgrades since 25 October 2026.

Dependabot's first six pull requests were version updates, not security
fixes. The two action updates were merged. The toml 1.1, quick-xml 0.42
and ureq 3 updates needed code or feature changes and were done by hand,
and a proposed bump of the rust-version toolchain was declined.

## bodnuti releases

Release notes are kept with towncrier: each change a user or operator would
notice adds a fragment in `changes/` (`changes/README.md`), and a release
collects them into `NEWS.md`.

bodnuti 0.1.0 is released (`v0.1.0`, 25 October 2026:
<https://github.com/moto-timo/bodnuti/releases/tag/v0.1.0>, with x86_64 and
aarch64 wheels, and `ghcr.io/moto-timo/bodnuti:0.1.0` for amd64 and arm64).
Its notes collect the
whole history in 40 entries (30 features, 4 security, 3 documentation, 3
other), fixes to unreleased features folded into the feature they fix. A
Release workflow builds from a `v*` tag: it checks the tag against
`Cargo.toml` and `NEWS.md`, builds x86_64 and aarch64 wheels and the amd64
and arm64 image, pushes the image to `ghcr.io/moto-timo/bodnuti`, and
creates the GitHub release with the wheels and the version's notes. Its
trial run caught the aarch64 wheel failing on manylinux2014's compiler,
fixed by building it for manylinux_2_28.

bodnuti 0.2.0 followed the same day (`v0.2.0`:
<https://github.com/moto-timo/bodnuti/releases/tag/v0.2.0>), the first
release under MIT or Apache-2.0, at your option, with `SECURITY.md` asking
for vulnerabilities to be reported through GitHub's private vulnerability
reporting. It adds Django's own checks, oelint-adv style findings kept
apart, the front-page summary and the landed-patch fix. Its wheels and
image carry SLSA build provenance and attested SPDX 2.3 SBOMs (the crates
from cargo-sbom, the image per platform from trivy, also attached to the
release), and the image a keyless cosign signature; all of them verify
from the published artifacts. `AGENTS.md` lists the release steps.

bodnuti 0.3.0 (26 October 2026) adds checking on staging branches, the
signed `:main` image, unthreaded series put together again, and the GitHub
CI and AutoBuilder evidence, with the fixes from the live reports. bodnuti
0.4.0 (7 November 2026) lints distro and machine configuration as such,
stops linting a recipe's payload, reloads the series indexes, and takes
trivy 0.76.2, Debian's security fixes and layerindex-web's new pins.
bodnuti 0.5.0 (22 November 2026) adds patchtest's metadata checks and the
Claude Code plugin, takes oelint-adv 9.14.0 and djLint 1.54.0, and is the
first release made at SLSA Source Level 3.

The repository also produces SLSA source provenance, set up with the SLSA
source tool: rulesets keep `main` and every tag from being deleted or
rewritten, and a workflow writes signed source provenance and a
verification summary to git notes on each push. Every source control
except two-party review is met, and the repository's policy,
slsa-framework/source-policies#27, merged on 22 November 2026 with a
Level 3 target for `main`. main verified at Level 1 at first, for two
reasons found that day:

- Provenance is written only for the tip of each push. Rebase-merging a
  pull request of several commits (#18, #19) left the others without it
  and broke the chain each commit's provenance continues. Pull requests are
  now merged with a merge commit (rebase merging is off, and the workflow
  passes `allow-merge-commits`), and the chain is whole again from #25.
- Every level from 1 up needs the `SLSA_SOURCE_SCS_VSA` control, which
  sourcetool v0.7.3 and earlier set only from a verification summary the
  commit already has, and on a push it has none yet. So no commit could
  rise above the Level 1 fallback. slsa-framework/source-tool#479 fixes
  this, released in sourcetool v0.7.4 the same day.

With sourcetool v0.7.4, main verifies at SLSA Source Level 3 from the
#29 merge commit (`667c450`) on, in the workflow and with sourcetool run
locally. The workflow is on slsa-framework/actions v0.2.1, set to run
sourcetool v0.7.4, and Dependabot follows the actions' releases.

The `v0.2.0` tag has no tag provenance: the workflow's pinned release ran
sourcetool v0.6.2, which fails on tags. The `v0.3.0` and `v0.4.0` tags
carry tag provenance, and sourcetool verifies all three at Level 1, the
level main had when they were made.

bodnuti.dev's front page replaces the placeholder: what bodnuti checks, the
watched lists, links to the reports, source, releases and security policy,
and a live "This week" block, per list the last seven days' series, those
with findings and those landed, and the latest series. `bodnuti publish`
writes it as `reports/summary.html`, and Caddy inserts it as it is
(`readFile`, never as a template, since it holds mail subjects;
`designs/PHASE2.md` §6.2). `readFile` needs a newer Caddy than Debian
trixie's 2.6.2; the web host runs 2.11.7 from Caddy's repository, and the
front page is live.

A CI check, the News workflow, fails a pull request that adds no fragment,
unless it carries the `no news` label; Dependabot's are exempt.

## bodnuti phase 3: model review, first measurement

Phase 3 ([`designs/PHASE3.md`](../designs/PHASE3.md)) adds one model call
per patch on open-weight models. Layers 2 (context gathering in the check
container) and 3 (the call, its validation and the report section) are
built, along with `bodnuti check --review`. Layer 4 (escalation to a
larger model with read-only git tools), live verification against what
the lists decide (`PHASE3.md` §11) and the check pass (§13.1) are built as
drafts. Model findings are shown apart, marked unverified, and never count
in the result.

Measured on 25 November 2026 with `bench/phase3/score.py` on FriendliAI's
serverless API: the six benchmark defects the deterministic checks miss,
and the 25 most recent merged OE-Core commits as clean patches. One
request per patch, temperature 0, JSON-schema output, sashiko's review
guides, layer 2 context of 8-21k tokens.

| Model, reasoning | Defects, judge | Defects, confirmed | Findings per clean patch | Tokens out | Per patch | Per month |
|---|---:|---:|---:|---:|---:|---:|
| gemma-4-31B-it, off | 4 / 6 | 3-4 / 6 | **0.28** | ~90 | ~$0.002 | ~$11 |
| gemma-4-31B-it, on | 5 / 6 | 4-5 / 6 | 0.36 | ~5,100 | ~$0.004 | ~$21 |
| GLM-5.3-Flash, effort none | 4 / 6 | not read | 1.52 | ~600 | ~$0.002 | ~$11 |
| GLM-5.3-Flash, effort low | 4 / 6 | **5 / 6** | 0.36 | ~680 | ~$0.002 | ~$11 |
| GLM-5.3, effort none | 6 / 6 | not read | 1.56 | ~370 | ~$0.017 | ~$89 |
| GLM-5.3, effort low | 5 / 6 | **5 / 6** | 0.48 | ~500 | ~$0.018 | ~$91 |

Costs are FriendliAI list prices without its prefix cache, which served
most of a repeated system prompt in testing; per month is 5,100 patches.
Wall time was 2-6 s a patch, 23 s for gemma with reasoning.

How to read this:

- "Confirmed" is a first reading by the agent that ran the measurement,
  still to be checked by a maintainer, of whether a finding names the
  known defect. The judge (the model under test) errs both ways: it
  counted a piglit finding about wayland-protocols, and an ICU finding
  about the fetch URL, as the defects, and rejected GLM-5.3-Flash's
  rootfs finding, which names it exactly. Rows marked "not read" use the
  judge only.
- Most findings on clean patches are false. Of the 37 from the four
  setups read in full, one is a real but minor issue (a refreshed
  consolekit patch file without `Signed-off-by:`), about nine are
  plausible but unconfirmed (systemd ukify runtime dependencies,
  glib-networking's second test, grub's regenerated configure,
  consolekit's udev on a sysvinit distro), and the rest are wrong:
  `bb.utils.contains` on several words (it checks that all are set), a
  dependency already in `DEPENDS`, a `PACKAGES =+` ordering misread,
  `SOURCE_DATE_EPOCH` or `TUNE_PKGARCH` "unset" when bitbake.conf always
  sets them, a selftest leftover the framework removes.
- No setup meets the proposed bar (at least 3 of 6 defects, at most 0.25
  false findings per clean patch). GLM-5.3-Flash with low reasoning is
  the best trade so far: 5 of 6 confirmed, 0.36 false findings per clean
  patch, about $0.002 a patch. gemma-4-31B-it without reasoning is
  quietest (0.28) but finds only 3-4.
- A larger model does not by itself reduce false findings: GLM-5.3 with
  reasoning off reported 39 findings on 25 clean patches. Reasoning does:
  GLM-5.3 went from 1.56 to 0.48 with `effort low`, keeping 5 of 6.
- GLM-5.3-Flash ignores `enable_thinking`. At its default effort it spent
  all 8,192 output tokens reasoning and returned no answer, which the
  first runs scored as clean patches; `reasoning_effort` controls it, and
  bodnuti and the harness now send it. Even at `none` it once looped to
  the token limit.
- Six defects are a small set, and the prompts were not tuned. Before
  trusting a number, tune on these and confirm on a held-out set.

### Improving accuracy

The same day, four changes were tried against the false findings, each
scored on sets not used to choose it:

- **Keep only high-confidence findings** (`min_confidence`, now in
  `[review]`). On 25 older clean commits picked afterwards: GLM-5.3 at
  low reasoning 0.00 false findings per patch, GLM-5.3-Flash 0.16, both
  still finding 4 of the 6 tuning defects.
- **Prompt v2** (`bench/phase3`, `--prompt v2`): each finding quotes
  the lines that show it and is dropped if the quote is not in the
  request; a short list of OE-Core facts behind the recurring false
  findings; only what the patch introduces or leaves incomplete. On the
  tuning set it halved false findings without a filter (Flash 0.36 to
  0.16, GLM-5.3 0.48 to 0.16).
- **Only changed lines** was rejected on the stored results: false
  findings already sit near the changes, and it would lose the ICU
  defect, whose line the version change made wrong without touching it.
- **Rules learned from the lists:** `lessons.py` distilled 30 review
  rules from 508 changelogs of resent patches (2023-2025);
  `--prompt v3` adds them. It lost to v2 (below): the rules made the
  model raise Upstream-Status and style points, so it stays in the
  bench.

Two harder test sets were added. `heldout.json` has 30 defects mined
from OE-Core's `Fixes:` trailers; `history-2026.json` has 19 patches from
2026 whose v2 changed code after review, so the question is whether the
model raises what the reviewers raised. Prompts v2 and v3, scored on the
held-out sets:

| Setup | Filter | Mined defects (judge) | False findings, 25 held-out clean |
|---|---|---:|---:|
| v1, GLM-5.3 low | high | 5 / 30 | 0.00 |
| v2, GLM-5.3 low | medium and high | **8 / 30** | **0.04** |
| v2, GLM-5.3 low | high | 3 / 30 | 0.00 |
| v1, GLM-5.3-Flash low | high | 4 / 30 | 0.16 |
| v2, GLM-5.3-Flash low | medium and high | 5 / 30 | 0.20 |
| v3, GLM-5.3 low | medium and high | 8 / 30 | 0.28 |

On the 2026 replay, GLM-5.3 low with medium and high findings: v2 9 of
19 by the judge, v3 6. Read by hand, about 4-5 of v2's 9 really raise
what the reviewers raised.

With v2's evidence rule a high-only filter costs defects, so v2 wants
`min_confidence = "medium"`. Defects found after merging are much harder
than the six: many need knowledge the diff does not carry (firmware file
lists, boot behaviour), and a few judge verdicts are generous. bodnuti
checks the quotes and defaults to `min_confidence = "medium"`.

Since then the request grew past score.py's v2 (`PROMPT` in
src/review.rs): v2.1 adds the series context, v2.2 the docs index (each
variable's glossary entry and bitbake.conf value) and sashiko's
false-positive guide, v2.3 the review rules, and v2.4 moves the rules to
the check pass. With the rules in layer 3, gemma4 26B checked patch
headers field by field and missed the python3-pillow test bugs it finds
without them; the rules now judge findings instead of steering the
search.

Fine-tuning on the lists' v1/v2 history (`designs/TRAINING.md`) was
tried once and stopped: the adapter learned to report nothing. The
history goes into the request instead, as the review rules.

Measured on 27 November 2026, after fixing what made local models look
worse (temperature 0, requests that filled the context, one answer per
patch, a generic chat template): 13 patches of 26 November with 8 known
defects, three samples, every finding read by hand. Claude Code with the
plugin's rules found 5 with no false finding shown; GLM-5.3 on
FriendliAI 5, with 1 false, about $0.22 a patch (about $0.03 with one
sample); gpt-oss-20b on a 16 GB GPU 4, with 4 false, about 6 minutes a
patch; the LoRA 12B none. Qwen3.6-35B-A3B is still being measured.

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
| bodnuti, GLM-5.3-Flash (low reasoning) on FriendliAI | ~$0.002 | ~$11 | measured, phase 3 benchmark |
| bodnuti, gemma-4-31B-it on FriendliAI | ~$0.002 | ~$11 | measured, phase 3 benchmark |
| bodnuti, GLM-5.3 (low reasoning) on FriendliAI | ~$0.018 | ~$91 | measured, phase 3 benchmark |

The Batch API halves the bodnuti figures, which suits a web report that can
arrive hours after the patch. On a Claude subscription, one five-hour usage
window held about eight sashiko reviews.

## Next steps

- Add `a-quick` to `[autobuilder] builders` once `a-full` and `meta-oe`
  have run for a while, and watch the first AutoBuilder failures tied to a
  series, by recipe and by two builds.
- Read error reports through a JSON API in error-report-web once it has
  one.
- Watch for the cases only unit-tested so far: a BitBake series rescued by
  its staging branches, a late member reopening an unthreaded series, and a
  CodeQL alert on a line a checked series wrote.
- Drop the SLSA Source workflow's `version` once slsa-framework/actions
  defaults to sourcetool v0.7.4 or later.
- Watch the new metadata checks (long lines, LIC_FILES_CHKSUM without
  `License-Update:`, CVE_CHECK_IGNORE) on the live reports for false
  alarms.
- Score the three unreviewed benchmark entries, and measure sashiko at low
  effort.
- Sort the 37 extra findings into real issues and false alarms.
- Refresh `django/<application>/requirements.txt` from the applications'
  master branches when their pins change, so Django's checks run against
  current dependencies.
- Phase 3: finish the comparison on the 40 patches of 26 November and a
  fresh set (`PHASE3.md` §13.2): GLM-5.3 hosted against gpt-oss-20b and
  Qwen3.6-35B-A3B on one 16 GB GPU, with docs and the check pass; then
  pick the model for the watcher, and have a maintainer read the review
  rules.
- Measure layer 4 (GLM-5.3-Flash with GLM-5.3 escalation, built as a
  draft) on the held-out sets before turning on `escalation_model`.
- Turn on `[review]` in the watcher, so live verification (§11) starts
  collecting outcomes; read a sample by hand after the first month.
- Raise the sashiko project idea with the sashiko maintainers and with OE and
  Yocto maintainers before any upstream pull request. Nothing would be emailed
  to the lists without their agreement.
