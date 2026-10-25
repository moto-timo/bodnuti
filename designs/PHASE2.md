# Design: bodnuti phase 2, watching the lists

Phase 1 checks one series on request. Phase 2 finds new series on the
OpenEmbedded and Yocto Project lists by itself, checks each one in the
sandbox phase 1 already uses, and publishes the reports as static pages on
bodnuti.dev. Nothing is emailed; that stays a later, opt-in step that needs
the list maintainers' agreement.

Decided with the maintainer before this document:

- The watcher runs natively on the worker host as the `bodnuti` user, not in
  a container. It never runs patch code. Each check runs in its own
  `podman run --network none --read-only` container, as in `DEPLOY.md`.
- New messages are found by polling lore.kernel.org's search, not by
  mirroring the public-inbox git repositories or using Patchwork.

## 1. Commands

Two new subcommands, both one-shot, run by a systemd user timer:

| Command | Does |
|---|---|
| `bodnuti poll` | Query lore for new patch messages, group them into series, and check every series that is complete. |
| `bodnuti publish` | Render the reports as static HTML and rsync them to the web host. |

One-shot runs keep no state in memory between runs, so a crash, a reboot or
an upgrade loses nothing; all state is on disk (§4). A lock file
(`flock` on `state/lock`) stops two runs from overlapping.

The host needs `git`, `podman` and `rsync`. It does not need b4, Python or
any checker: those stay in the image. The host binary is copied out of the
same image (`podman cp`), which is built on Debian trixie like the worker, so
host and container always run the same bodnuti version.

## 2. Configuration

`~/.config/bodnuti/watch.toml`:

```toml
image = "localhost/bodnuti"
state = "~/state"
cache = "~/cache"          # layer clones, as in DEPLOY.md
poll_minutes = 5
settle_minutes = 15         # wait this long after a series' last message
retain_months = 6           # how far back the published site goes
publish_to = "bodnuti-web:./"         # rsync destination; unset: state/site/ only

[[list]]
name = "openembedded-core"  # lore inbox name
repo = "openembedded-core"  # clone under cache/

[[list]]
name = "openembedded-devel"
repo = "meta-openembedded"

[[list]]
name = "bitbake-devel"
repo = "bitbake"

[[list]]
name = "yocto-docs"
repo = "yocto-docs"

[[list]]
name = "yocto-meta-virtualization"
repo = "meta-virtualization"

[[list]]
name = "yocto-patches"
route = "subject-tag"       # §3.1; no fixed repo

# Clones under cache/, with the upstream each is fetched from. A list's repo,
# or a subject tag on yocto-patches, must name one of these.
[repos]
openembedded-core = "https://git.openembedded.org/openembedded-core"
meta-openembedded = "https://git.openembedded.org/meta-openembedded"
bitbake = "https://git.openembedded.org/bitbake"
yocto-docs = "https://git.yoctoproject.org/yocto-docs"
meta-virtualization = "https://git.yoctoproject.org/meta-virtualization"
meta-yocto = "https://git.yoctoproject.org/meta-yocto"
layerindex-web = "https://git.yoctoproject.org/layerindex-web"
pseudo = "https://git.yoctoproject.org/pseudo"
yocto-autobuilder-helper = "https://git.yoctoproject.org/yocto-autobuilder-helper"
```

patchtest always comes from `cache/openembedded-core`.

## 3. Discovery

Each poll asks lore, per list:

```
https://lore.kernel.org/<list>/?q=s:PATCH+dt:<since>..&x=A
```

`x=A` returns an Atom feed. Each entry gives the subject, the Message-ID (in
the entry's link), the parent message (`thr:in-reply-to`), and a timestamp.
`<since>` is the newest timestamp seen on the previous poll, minus a
ten-minute overlap so a message indexed late is not missed; entries already
seen are skipped by Message-ID. One request per list per poll is six requests
every five minutes. A feed returns at most 200 entries; when it is full, the
poll pages with `o=` until it reaches `<since>`.

Replies (`Re:` and similar) and cover letters are not patches, but a cover
letter identifies a series, so entries are kept or dropped by subject:

| Entry | Use |
|---|---|
| `[... PATCH ... 0/N]` | series root |
| `[... PATCH ... k/N]` with `in-reply-to` | belongs to the series of its root |
| `[... PATCH ...]` with no counter | a one-patch series, its own root |
| reply | ignored |

The root of a message is found by following `in-reply-to` through entries
already seen. A `vN` resend is a new series: it has a new root.

### 3.1 Routing yocto-patches

yocto-patches carries patches for about twenty repositories. Their subject
tags name the repository (`[layerindex-web]`, `[pseudo]`,
`[yocto-autobuilder-helper]`), sometimes followed by a branch tag
(`[meta-lts-mixins][scarthgap/go]`). A sample of three months' subjects found
seven of about 160 with no repository tag.

1. **Tag.** The first subject tag that names a configured repository (the
   `[repos]` table) selects it. Remaining tags go to `bodnuti check` as usual,
   so a branch tag still selects the base.
2. **Paths.** Before checking, every path the series modifies or deletes
   must exist at the base commit of the selected repository
   (`git cat-file -e <base>:<path>`). Added files are not checked: they do
   not exist yet in any repository. A series whose paths do not match is
   recorded as `unrouted` with the mismatching paths, rather than checked
   against the wrong tree.
3. **No tag, or a repository that is not configured,** is `unrouted` with
   the reason. The index lists unrouted series, so a missing clone is visible
   rather than silent. Each poll tries these series' tags again, so adding
   a repository to `[repos]` picks up its earlier series; series unrouted
   by the path check are not retried.

### 3.2 Repositories that are not layers

Two projects on yocto-patches are Django web applications, not layers:
`[layerindex-web]` (git.yoctoproject.org/layerindex-web, behind
layers.openembedded.org) and `[error-report-web]`
(git.yoctoproject.org/error-report-web, behind errors.yoctoproject.org).
Both send several series a month. The layer checks find little in them:

- oelint-adv and the documentation checks have nothing to read: no
  recipes, no Sphinx tree;
- shellcheck only sees recipe shell tasks and shipped `.sh` scripts;
- of patchtest, only the mbox suite (Signed-off-by, subject format,
  sign-off on the shortlog) applies; the patch suite expects a layer.

Handling them needs a repository kind that selects the checkers, for
example `kind = "django-app"` beside the URL in `[repos]`, so routing stays
as in §3.1 and only the checks differ. The checks come in three tiers,
each run before and after the series like every other checker, so only
what the series adds is reported.

**Tier 1: static checks.** Nothing from the application is installed or
run:

- patchtest's mbox suite only;
- ruff on the touched `.py` files, with its pyflakes and pycodestyle rules,
  the Django rules (`DJ`, from flake8-django: `null=True` on string fields,
  a model without `__str__`, misordered model members) and the security
  rules (`S`, from bandit);
- djLint on touched Django templates;
- shellcheck on touched shell scripts, as now.

ruff and djLint are added to the image's `requirements.txt`. Which ruff
rules are enforced follows each project's own configuration where it has
one, and a small default set where it does not, so a first run does not
bury every series in style findings.

**Tier 2: dependency pins.** When a series changes `requirements*.txt`,
the pins before and after it are scanned for known vulnerabilities (trivy
or pip-audit), and a pin the series adds or keeps that has a published
fix is reported. Series that bump pins for security fixes are common (for
example "requirements.txt: fix djangorestframework and GitPython
vulnerabilities"), so this also confirms such a series fixes what it says.
The checks run without network, so the vulnerability database is
downloaded in the fetch step, which has network, and mounted read-only.

**Tier 3: Django's own checks.** `manage.py check` and `makemigrations
--check --dry-run`, the second catching a model change without its
migration. They need the application's dependencies, so each application
gets a venv built into the image from upstream's `requirements.txt` on its
default branch. Known limits:

- the venv lags upstream, and other branches may pin other versions;
- a series that changes `requirements.txt` is still checked against the
  old dependencies;
- these commands run the series' own code (settings, models, apps). That
  is the trust level the yocto-docs Sphinx build already has, which runs
  the series' `conf.py`, and it runs in the same offline, read-only
  container.

Tiers 1 and 2 are built with the repository kind. Tier 3 waits for a
decision by the layerindex-web and error-report-web maintainer (who is
also bodnuti's), after tiers 1 and 2 have run for a while.

Tiers 1 and 2 are designed in §5.2 and §5.3, and built.

## 4. State

Files, not SQLite. The volume is about 170 patches a day, every record is
written once and read rarely, and files can be inspected with `ls` and `cat`
and copied with rsync. A database can replace them if the volume or the
queries grow.

```
state/
  lock
  lists/<list>.json              cursor, thread map, and every series on the list
                                 (subject, n/N, patches seen, last seen, status)
  series/<list>/<root-msgid>/
    <root-msgid>.mbx             what b4 fetched
    report.json                  bodnuti check --json
    report.txt                   bodnuti check
  patch-ids/<patch-id>           path of the series that first had this patch
```

One file per list keeps a poll to one read and one atomic write (temporary
file, then rename) per list; at about a thousand series per list per month it
stays in the low megabytes.

Message-IDs are used as directory names only after they pass a strict
filter (letters, digits and `.-_+@=`, no `/`, no leading `.`), because they
come from untrusted mail. A Message-ID that fails is logged and skipped.

Series status moves forward only:

```
seen -> complete -> fetched -> checked -> applied
                \-> unrouted   \-> failed (with the reason)
```

`applied` is set per patch, and on the series once every patch is applied
(§5.1).

A series is **complete** when all N patches of `k/N` have been seen, or when
`settle_minutes` have passed since its last message (some series are sent
without a cover letter or lose a message).

## 5. Checking

For each complete series, in order of last message:

1. Fetch: `podman run ... --entrypoint b4 <image> mbox -o /out <root>`,
   network on, only the series directory mounted.
2. Compute `git patch-id --stable` for each patch in the mbox, on the host.
   If every patch-id is already in `state/patch-ids/`, the series is a resend
   of an already checked series: record a link to it and stop.
3. Check: `podman run --network none --read-only ... <image> check --json
   --repo /repo /in/<root>.mbx`, with the list's clone as `/repo` and
   OE-Core as `/oecore`. Write `report.json`, then render `report.txt` with
   `--json` off (or from the JSON, §6).
4. Record the patch-ids.

A check that exits 2 (a checker could not run) is `checked` with its tool
errors shown in the report; a fetch or podman failure is `failed` and retried
on the next poll, up to three times.

Checks run one at a time. At 15 to 45 seconds each, one worker clears a day's
traffic in under an hour; a parallel queue can come later if bursts need it.

### 5.1 Matching applied patches

After the clones are fetched (the hourly timer from `DEPLOY.md`), each poll
scans the upstream commits that arrived since the previous scan, on master
and on every branch a checked series targeted:

```
git rev-list --no-merges <last-scanned>..origin/<branch>
git show <commit> | git patch-id --stable      # for each new commit
```

A commit whose patch-id is in `state/patch-ids/` marks that patch as applied,
with the commit, branch and date. `state/upstream/<repo>/<branch>` holds the
last commit scanned. The first scan of a branch starts at the commit that
was the tip when the first series for it was seen, so history is not
rescanned.

A maintainer often edits a patch before applying it: rewording the commit
message does not change its patch-id, but changing the diff does. Such a
commit is not matched by patch-id. As a fallback, a commit whose subject and
author equal those of a checked, unapplied patch is marked **probably
applied**, and shown as such. patch-id matches are exact; subject matches are
labelled as guesses.

### 5.2 django-app checks, tier 1

The design for tier 1 of §3.2. Tiers 2 and 3 are not designed yet.

**Repository kind.** A `[repos]` entry is either a URL, as now, which is a
layer, or a table with a kind:

```toml
[repos]
openembedded-core = "https://git.openembedded.org/openembedded-core"
layerindex-web = { url = "https://git.yoctoproject.org/layerindex-web", kind = "django-app" }
error-report-web = { url = "https://git.yoctoproject.org/error-report-web", kind = "django-app" }
```

The kind is an enum (`layer`, `django-app`), not free text, and the runner
passes it to the check as `bodnuti check --kind django-app`. `--kind`
defaults to `layer`, so a check by hand behaves as before. Routing (§3.1)
does not change: the subject tag still selects the repository, and the
path check still guards against the wrong tree.

**Checkers by kind.**

| Checker | layer | django-app |
|---|---|---|
| patchtest | mbox and patch suites | mbox suite only (`test_mbox*.py`); the patch suite expects a layer |
| oelint-adv, docs, leftover-file check | yes | no |
| shellcheck | recipes, classes, scripts | scripts |
| ruff | no | touched `.py` files |
| djLint | no | touched templates (`.html` under a `templates/` directory) |

patchtest still comes from the OE-Core clone mounted as `/oecore`, as for
any repository that is not OE-Core.

**ruff rules.** bodnuti passes its own `--select`, so both applications are
checked the same way whatever their `ruff.toml` selects; the rest of the
project's configuration (excluded paths, per-file ignores, target version)
still applies. Style is left to the projects' own tooling.

- the classic defaults: `E4`, `E7`, `E9`, `F` (error-report-web's
  `ruff.toml` already selects exactly these);
- the Django rules except `DJ012` (member order, which is style);
- the security rules except `S101` (assert, used throughout the tests),
  `S603` and `S607` (any subprocess call), which carry little signal in
  these code bases.

On the applications' master branches at 20 October 2026, that set finds
357 existing problems in layerindex-web and 6 in error-report-web.
They do not matter for patches, because only what a series adds is
reported, but layerindex-web's `ruff.toml` selects no rules at all, so
ruff's current broad defaults would report 1,504.

**djLint rules.** djLint runs with `--profile=django`, and bodnuti keeps
the rules that find template bugs or accessibility problems, not
formatting:

- template bugs: `H019`, `H025`, `H037`, `H041`, `H053`, `T027`, `T034`,
  `T038` to `T045`. For example `T045`, a template tag inside an HTML
  comment, which still runs, has 42 existing cases in layerindex-web;
- accessibility: `H005`, `H006`, `H013`, `H016`, `H043`, `H045` to `H051`,
  `H054` (missing `lang`, `alt` and image sizes, button types, ARIA
  mistakes, nested interactive elements).

**Findings.** Two new tools, `ruff` and `djlint`, with each rule's code as
the check id. ruff's JSON output gives file, line and message; djLint's
`--linter-output-format` is set to one parseable line per finding. Both are
pinned in `requirements.txt` (ruff 0.17.2, djLint 1.47.0) and so in the
image.

**Not in tier 1.** Dependency pin scanning (tier 2, §5.3) and Django's own
checks (tier 3).

### 5.3 django-app checks, tier 2: dependency pins

The design for tier 2 of §3.2: when a series changes a `requirements*.txt`
file, report the known vulnerabilities its pins add, and the ones it fixes.

**Scanner.** trivy, already used for bodnuti's own scans (`make audit`).
It reads `requirements.txt` pins (`name==version`) and matches them against
its vulnerability database, which covers the GitHub and OSV advisories for
PyPI. The image gets a pinned trivy binary, copied from
`docker.io/aquasec/trivy:<version>` like shellcheck from its image. Lines
that are not exact pins (ranges, `-i` index URLs, `-r` includes) are
ignored by trivy, so only exact pins are checked.

**Database without network.** The database is about 1.6 GB on disk and
downloads in seconds. The check container has no network, so:

1. Once a day, the watcher refreshes `cache/trivy/` with
   `trivy fs --download-db-only --cache-dir /trivy` in a container with network
   (like the b4 fetch), when `cache/trivy/db/metadata.json` is older than 24
   hours. A failed refresh keeps the old database and is logged; checks go
   on with it.
2. Checks mount `cache/trivy/` read-only and scan with `--cache-dir /trivy
   --cache-backend memory --skip-db-update --skip-java-db-update
   --offline-scan`, which writes nothing and makes no request. Tested on
   layerindex-web's master `requirements.txt` with a read-only database: 45
   advisories, among them five CRITICAL in Pillow 12.3.0, fixed in 13.0.0.

**Before and after.** Each touched `requirements*.txt` is scanned at the
base and after the series. As with every checker, the advisories the series
adds are findings. A finding's message names the package and pinned
version, so moving a pin to another version that is still affected reports
the advisory again:

- tool `trivy`, check id the advisory (`CVE-...` or `GHSA-...`), file the
  requirements file;
- severity: CRITICAL and HIGH are errors, MEDIUM a warning, LOW and unknown
  info;
- message: `Pillow 12.3.0: <advisory title> (fixed in 13.0.0)`.

**Fixed advisories.** The advisories the series removes are what a "fix
vulnerabilities" series claims, so they are reported too, but not as
findings: a fix is not a problem, and must not turn a clean result into
"N findings". The report gains a `fixed` list (package, old and new
version, advisory ids), shown on the series page as "Fixes" and in the
text report. A series that only fixes is clean.

**Scope.** django-app repositories only, where requirements files are the
dependency manifests. Layers pin Python packages in recipes, which this does
not read.

**Not in tier 2.** Unpinned or range requirements, `pyproject.toml` and lock
files, which neither application uses; licence checks.

## 6. Publishing

`bodnuti publish` renders, from `state/`:

- `index.html`: the series of the last `retain_months` months across all
  lists, newest first, with list, subject, author, result (clean, findings,
  not checked, unrouted), applied state and a link;
- `<list>/index.html`: the same for one list;
- `<list>/<root-msgid>/index.html`: the text report, the series' patches and
  links to lore.

Everything from mail (subjects, author names, messages in findings) is
HTML-escaped. The pages are plain HTML and CSS with no JavaScript, styled
like the bodnuti.dev landing page. Output goes to `state/site/`, then:

```
rsync -a --delete state/site/ bodnuti-web:./
```

over the restricted deploy key from `DEPLOY.md` (`rrsync` on the web host).
Pages are regenerated in full each time; at a few thousand series this takes
well under a second. Series older than `retain_months` drop off the site at
the next publish (`--delete`), but stay in `state/`.

## 7. Testing

- Unit tests for the Atom parser (on a captured feed), series grouping
  (cover letter, no cover letter, `vN` resend, replies), the Message-ID
  filter, and status transitions.
- A test of `poll` against a local directory of captured feeds and mboxes,
  with the podman calls replaced by a stub command, so CI needs no network
  and no container runtime.
- A first live run on the worker with `publish` writing locally only, before
  the rsync key is set up.

## 8. Decisions

Settled with the maintainer after the first draft:

1. yocto-patches is routed by subject tag, then checked against the paths
   the series touches (§3.1).
2. The published site keeps the last `retain_months` months; `state/` keeps
   everything.
3. Applied patches are matched to upstream commits by patch-id now, with a
   labelled subject fallback (§5.1), rather than in a later phase.
