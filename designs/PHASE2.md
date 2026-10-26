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
   against the wrong tree. A series merged before it was checked fails this
   check too, since the merge renamed or removed its files; so a failing
   series is first matched against the base's last 90 days of history, by
   patch-id and else by subject and author (§5.1), and recorded as
   `applied` if every patch is there.
3. **No tag, or a repository that is not configured,** is `unrouted` with
   the reason. The index lists unrouted series, so a missing clone is visible
   rather than silent. Each poll tries these series' tags again, so adding
   a repository to `[repos]` picks up its earlier series; series unrouted
   by the path check are not retried.

### 3.1.1 Routing refinements

Found after the first backfill:

- **Name inside the PATCH tag.** `[PATCH yocto-autobuilder2] ...` names the
  repository inside the PATCH tag, as b4's prefixes do. The words of a
  PATCH tag count as tags, as they already do for branch names.
- **Tag aliases, several repositories per tag.** A `[repos]` table may list
  `tags`: `[AUH]` is the subject tag of both patches to auto-upgrade-helper
  and the upgrade patches its bot sends for OE-Core recipes. Routing keeps
  every repository whose name or alias a tag gives, in `[repos]` order (by
  name), and the path check (step 2) picks the first in which every path
  the series modifies or deletes exists; with none, the series is unrouted
  with the first candidate's paths. The record keeps the candidates in a new
  `candidates` list, absent in older state, and `repo` the one picked.
- **Missing clone.** A configured repository without its clone under
  `cache/` used to fail its series three times. It is a configuration
  problem: the poll logs `clone missing: git clone <url> <path>` once and
  leaves the series waiting, attempts untouched, until the clone exists.
- **Branches with a slash.** meta-lts-mixins names branches by release and
  topic (`scarthgap/go`). The branch recorded for a series is the full name
  after `refs/remotes/<remote>/`, not its last component (`go`).

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

All three tiers are built: tier 1 in §5.2, tier 2 in §5.3, and tier 3, on
the decision of the applications' maintainer (who is also bodnuti's), in
§5.5.

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

### 5.0 Threads that are not series

The first backfill (two weeks, all lists) found two kinds of thread the
check cannot take as they are. Both used to fail three times and end as
`failed`.

**No patches.** A pull request is a cover letter alone (`[PATCH 00/99]
Wrynose pull request`, BitBake's `[PATCH 0/1] Pull request (cover letter
only)`), and some series are sent with their patches outside the cover
letter's thread, so each patch is a series of its own and the cover letter
has none. The fetched mbox then has no patches. That is a fact about the
thread, not a failure: it is recorded with its own status, `no-patches`,
with that reason, and shown on the index as "no patches". It is not
retried.

**Pull requests.** Most cover letters without patches are `git
request-pull` output, which names everything needed to get the patches:

```
The following changes since commit 47d7dbdd10890bbd429bc0cccbe14435c37e5af1:
are available in the Git repository at:
  https://git.openembedded.org/meta-openembedded-contrib anujm/wrynose
for you to fetch changes up to f28a144aa0a0ae05ca2ee91e1aef6aa05f6a12db:
```

Such a series is checked like any other:

1. The cover letter (not a reply quoting it) is parsed for the base and
   head commits, the repository URL and the branch. All four come from
   mail, so they are validated: both commits are full 40-digit object
   names, the URL is `https://` on `git.openembedded.org` or
   `git.yoctoproject.org` with no other characters than a path needs, and
   the branch is a plain ref name. A pull request that fails validation is
   `no-patches`, with the reason.
2. On the host, with network, `git fetch --no-tags <url>
   refs/heads/<branch>` into a ref of bodnuti's own in the target clone
   (`refs/bodnuti/pull/<root>`), over https only, and the head must be on
   the fetched branch; a branch that has since moved on is fine, one that
   no longer has the head is a failure, retried.
3. `git format-patch base..head` becomes the series' mbox. The check uses
   the pull request's base commit rather than one chosen from subject tags,
   and the branch is the upstream branch that contains the base.
4. The usual steps follow: a pull request whose every patch was already
   checked as posted is a duplicate, applied matching works by patch-id,
   and only patches that are new or changed cost a check.

The ref is deleted after the mbox is written.

**Root not on lore.** A patch whose parent the watcher never saw is named
after that parent (§3), which is usually the cover letter, but can be a
message lore does not have, such as an `In-Reply-To` pointing at mail on
another list. `b4 mbox <root>` then fails with 404. The watcher knows
every Message-ID it saw in the series, so when fetching the root fails, it
fetches the thread through each of those in turn, and saves the first that
works as the series' mbox.

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

### 5.5 django-app checks, tier 3: Django's own checks

The design for tier 3 of §3.2: `manage.py check` (Django's system checks:
models, fields, relations, settings) and `makemigrations --check --dry-run`
(a model change without its migration), before and after the series, so
only what the series adds is reported.

**Measured** on layerindex-web and error-report-web (master, 25 October
2026): both checks run offline in about a second each, against an
in-memory sqlite database, and both are clean. On error-report-web, a probe
adding a model field without a migration gives "Migrations for 'Post': +
Add field ... to build", and one adding a relation to a missing model gives
`fields.E300` and `fields.E307`.

**Dependencies.** Each application gets a venv in the image,
`/opt/django/<repository>/`, installed from a copy of its
`requirements.txt` kept in bodnuti (`django/<repository>.txt`), so the
pins are reviewed and Dependabot updates them. Database client libraries
(`mysqlclient`) are left out: the checks use sqlite. In the image the venvs
take about 143 MB (layerindex-web) and 69 MB (error-report-web). The copies
are the applications' pins on purpose, vulnerable ones included, so
bodnuti's own scans (`make audit`, `make audit-image`) skip them and
Dependabot does not update them: tier 2 reports the applications'
advisories on their own series. They are refreshed from the applications'
master branches by hand.

**Settings.** bodnuti does not use the application's database settings. It
reads the settings module from `manage.py`'s `DJANGO_SETTINGS_MODULE`
default (`settings`, `project.settings`), and runs Django with a module of
its own that imports it and then replaces `DATABASES` with in-memory sqlite
and sets a `SECRET_KEY` when the application expects one from the
environment.

**Findings.** Each system check message is a finding, its id the check's
(`django.fields.E300`), an error for `E` and `C` levels and a warning for
`W`; `makemigrations` reporting changes is one `django.migrations` error
listing them. A venv missing, or settings that fail to import, is a tool
error. The checks are project-wide, so findings are not limited to touched
files; before and after the series decide.

**Limits.** These commands import the series' settings, models and apps,
which run code from the series: the same trust as the yocto-docs Sphinx
build, in the same offline, read-only container. The venv follows the
copies in bodnuti, not each series: a series that changes the
application's requirements is checked against the previous ones.

### 5.4 Other repositories on yocto-patches

The first backfill left four more repositories' series unrouted. Measured
over three months of yocto-patches (messages tagged with the name):

| Repository | What it is | Messages | Kind |
|---|---|---|---|
| meta-security | a layer | 92 | `layer` |
| pseudo | C (282 `.c` files), shell tests, `configure` | 62 | `c` |
| yocto-autobuilder-helper | Python scripts, mostly without `.py`, bash, `config.json` | 66 | `python` |
| yocto-autobuilder2 | the BuildBot master (`master.cfg` is Python), a TypeScript view | 20 | `python` |

meta-security needs only a clone and a `[repos]` entry. The others get two
new kinds, which select checkers as `django-app` does (§5.2):

**`python`.** patchtest's mbox suite; ruff with the django-app rule set
(the Django rules find nothing outside Django), on touched `.py` files,
files whose first line is a `python` shebang, and `master.cfg`, which ruff
checks when named; shellcheck; and a JSON check: a touched `.json` file
must still parse, because the autobuilder reads `config.json` at start.

**`c`.** patchtest's mbox suite; shellcheck; and cppcheck on touched `.c`
and `.h` files with `--enable=warning`, keeping its `error` and `warning`
findings and dropping style, performance and portability. On pseudo's four
main sources it finds eight existing problems in about four seconds; as
everywhere, only what a series adds is reported. cppcheck comes from
Debian's package in the image.

Built as designed; cppcheck's "could not parse" findings (`syntaxError`
and the like) are dropped, because pseudo's `ports/*/guts/*.c` are function
bodies spliced into generated wrappers and are a syntax error on their own.

**Not now.** Building pseudo with compiler warnings: it needs a C toolchain
and sqlite headers in the image and runs the series' `configure` and
Makefile. It can follow as a later tier, like Django's own checks. Linting
yocto-autobuilder2's TypeScript view.

### 5.6 Series that depend on staged patches

Some maintainers stage the patches they have taken on a branch of their
own before they reach `master`, such as meta-openembedded's `master-next`.
A series that builds on staged patches does not apply to `master`, and is
reported as a `bodnuti.apply` error. On 26 October, `[meta-oe][PATCH 6/8] tbb: Skip test_resumable_tasks
in ptest under QEMU` changed `tbb/files/run-ptest` from the version an
earlier series (`[meta-oe][PATCH 3/6] tbb: Skip test_scheduler_mix in
ptest`, the day before) creates. That series was on `master-next` and not
on `master`, so 6/8 failed with "Failed to merge in the changes", although
it applies cleanly on top of the patches it was written for. The error is
accurate for `master`, but tells the author and the reviewer nothing they
can act on, and every checker that needs the patched tree is skipped, for
the whole series.

**The fallback.** A repository can name a staging branch:

```toml
[repos.meta-openembedded]
url = "https://git.openembedded.org/meta-openembedded"
staging = "master-next"
```

When the series targets the repository's default branch (no subject tag
names another branch, §5) and does not apply there, the check tries once
more on the staging branch, and checks the series there if it applies.
Series for stable branches get no fallback: stable releases are staged in
other repositories, and their staging can follow if a series on a stable
branch needs it.

**Which commit on the staging branch.** A staging branch often carries the
series under check itself, as the tbb patch was on `master-next` by the
time it was checked (`01e8e0510e`). Applying it on the branch's tip then
fails as already applied, or checks nothing. So the base is chosen with
the patch-id matching of §5.1:

1. Match each patch of the series against the staging branch's commits
   since it left the default branch (`git log <default>..<staging>`), by
   `git patch-id --stable`.
2. If any matches, the base is the parent of the earliest matching commit:
   the staging branch as it was just before the series was taken, which
   holds the patches it depends on.
3. Otherwise the base is the staging branch's tip.

If the series does not apply at that base either, the result is the
`bodnuti.apply` error for the default branch, as now: the staging branch
did not explain it.

**One base for every checker.** The fallback is decided before any checker
runs. patchtest applies the series itself, and the before and after runs
of the other checkers must see the same base, so the check first tries
`git am` in a throwaway workspace on the default branch, and only if a
patch fails tries the staging base. Everything then runs on the chosen
base. The extra cost is one `git am` attempt on a series that already
failed to apply, and none on one that applies.

**The report.** The base line names the branch it came from:
`base: <commit> (master-next)`. An info finding, `bodnuti.base`, says
why:

```
INFO    bodnuti bodnuti.base
    does not apply to master (533d0a63ac2a): patch 6/8: Failed to merge
    in the changes.
    Checked on master-next (<commit>), which carries the patches it
    builds on.
```

It is info, not a warning: depending on staged patches is how the
maintainer works, not a fault of the series. The series' findings are the
usual ones, compared before and after the series on that base.

**What stays keyed to the default branch.** The series still targets
`master`: the branch recorded for duplicates (`state/patch-ids/`) and the
landed-patch matching (§5.1) stay `master`, so a later resend to `master`,
or the patches reaching `master`, are recognised as before. The record and
`report.json` gain the staging branch the check used, for the report and
the index.

**Where it runs.** The choice is made in `bodnuti check`, offline in the
container like the rest of the check, given `--staging <ref>` by the
runner from the configuration; run by hand, `bodnuti check --staging
origin/master-next` does the same. The staging branch comes from the same
upstream as the default branch and is fetched with it, so the check reads
nothing it did not already trust; and if the clone has no such ref, there
is no fallback.

**Which repositories.** meta-openembedded has `master-next`. Whether
OE-Core, BitBake and the other watched repositories stage patches the same
way, on which branch, is checked when this is built, together with how
often their series fail to apply on `master`; each is named in the
configuration only then.

### 5.7 AutoBuilder builds and error reports

Patches under review are built and tested on the Yocto Project AutoBuilder
(<https://autobuilder.yoctoproject.org/valkyrie/>) before they reach
`master`: OE-Core's maintainers push stacks of them to staging branches
(`master-next`; `mathieu/master-next` and `mathieu/master-next-tests` in
openembedded-core-contrib) and start a full build (`a-full`), which
triggers about 60 builds across machines, distros, oe-selftest and ptest.
Their failures are sent to errors.yoctoproject.org. That is evidence about
a series that no static check can give: whether it builds and passes the
tests everywhere. A staging branch rebases at any time, but every build
was of a stack of patches on top of a `master` commit, and that is enough
to trace a build to the series in it.

**What is there.** Measured on 26 October:

- **Builds.** Buildbot's REST API (`/valkyrie/api/v2`) is public, read-only
  JSON. Each top-level build's properties give, per layer, the repository,
  branch and exact commit it built (`repo_oecore`, `branch_oecore`,
  `commit_oecore`, and the same for `meta-openembedded`,
  `meta-virtualization`, BitBake and the rest), the owner and the reason
  ("test mathieu/master-next"). Its child builds are found through
  `/buildsets?parent_buildid=<id>`, each with a result (success, warnings,
  failure, cancelled), and each failed step has `stdio`, `errors` and
  `warnings` logs. Of the last 60 `a-full` builds, 31 tested OE-Core
  staging branches (21 `mathieu/master-next`, 5 `master-next`, 5
  `mathieu/master-next-tests`), 10 were nightly builds of `master`, and the
  rest were stable `-nut` and personal branches. meta-openembedded and
  meta-virtualization were on `master` or a stable branch in all 60;
  staging builds of theirs, if there are any, use builders this sample did
  not cover, and are to be found before this is built.
- **The tested trees.** poky-ci-archive keeps a tag per build for OE-Core,
  BitBake, meta-yocto and poky
  (`oecore/autobuilder.yoctoproject.org/valkyrie/a-full-4999`), so a
  staging commit stays reachable after its branch rebases. It keeps
  nothing for meta-openembedded or meta-virtualization.
- **The stacks.** a-full-4999 (`mathieu/master-next`) built 71 commits on
  top of `master`, from about 20 authors; a-full-5001, a day later, built
  70 of the same 71 by patch-id. Consecutive builds of one branch differ
  by a few patches.
- **Error reports.** Each failure on errors.yoctoproject.org records the
  branch (`mathieu/master-next`), the commit (`unknown` for AutoBuilder
  reports), the recipe, task and log, and a link back to the AutoBuilder
  build that sent it (`#/builders/74/builds/4827`). There is no JSON read
  API: the pages are HTML, and the AutoBuilder feed
  (`Errors/Latest/Autobuilder/feed`) carries only the latest 10 failures,
  with their whole logs (9 MB). error-report-web is maintained by
  bodnuti's maintainer, so a small read API (failures of one AutoBuilder
  build, as JSON) is a change that can be made there rather than scraped
  around.

**From a build to a series.** For each finished top-level build of a
staging branch:

1. Read its properties, and for each watched repository fetch the commit
   it built into a ref of bodnuti's own (`refs/bodnuti/ab/<build>`): from
   poky-ci-archive for OE-Core and BitBake, from the upstream branch for
   the others, which must happen soon after the build, while the branch
   still has the commit.
2. List the stack: `git log <master>..<commit>`, merges excluded, the
   `master` commit being the merge-base.
3. Match each commit to the series it came from by `git patch-id --stable`,
   against `state/patch-ids/`, as applied matching does (§5.1); a commit
   that matches no checked series is counted, not reported.
4. Record, per series, each build that carried all or part of it: the
   build, the branch, which of its patches, and the result.

**Failures and blame.** A build tests 70 patches at once, so its result is
not a result for one series, and some failures are no patch's fault: the
one failure of a-full-4999 was GitHub refusing a `git fetch` of
`cvelistV5`. Its stack differed from a-full-5001's, which passed, by one
patch (a systemd upgrade), so naive bisection would have blamed the
systemd upgrade for an outage. Failures are therefore classified first,
from the failed step's `errors` log and the error report:

- **Infrastructure:** fetch failures from upstream sites, worker disk or
  network trouble, cancelled builds. Recorded, never attributed.
- **Build or test:** a task failure (`do_compile`, `do_install`,
  `do_testimage`, a ptest), with its recipe.

A build or test failure is shown on a series only when it is tied to the
series by one of:

- **The recipe:** the failed task's recipe is one the series changes (its
  `.bb`, `.bbappend` or files directory).
- **The difference:** the failure is in this build and not in the build of
  the same branch before it, and this series is among the few patches that
  differ between their stacks.

Anything else is the stack's, not the series': the series page says only
that it was in a build that failed, with the link, so the reviewer can
look.

**On the series page.** A block per build, like a finding (§6.1): the
build (`a-full 4999, mathieu/master-next`, linked), its date, which of the
series' patches were in it, and the result. Passing builds count as much
as failing ones: "built and tested in 3 full builds, 2 passed" is news for
a reviewer. A failure tied to the series by its recipe or by the
difference shows the recipe, task and the first lines of its error, with
links to the AutoBuilder step and the error report. None of this changes
the check's findings or its counts: it is another kind of result, shown
apart, like style findings (§6.1.1).

**Polling.** One request for new top-level builds per poll (`builds` of
`a-full`, newer than the last one seen), then, per finished build, its
properties and its buildsets' builds: about ten requests per build, and a
few dozen builds a day. Error reports only for failed builds, by their
link back. Results are kept in `state/autobuilder/`, a build being done
once all its children have finished.

**Trust.** Build properties, step logs and error reports are written by
the AutoBuilder and by what it built, which includes the patches under
review: they are untrusted text, escaped like mail (§6), and nothing from
them is run. Fetching tested commits uses the same https upstreams as the
clones.

**With §5.6.** A staging branch on the AutoBuilder is a tested stack, so
the staging fallback can also try the stack of the latest build that
carried a series' predecessors, where the configured staging branch has
moved on.

**Decided.** Settled with the maintainer:

1. OE-Core and BitBake first, the repositories poky-ci-archive keeps a tag
   per build for. meta-openembedded, meta-virtualization and the other
   layers follow once their tested commits can be kept, since nothing
   archives them.
2. `a-full` builds first, then `a-quick`. The single-layer builders (such
   as `meta-virt`) come with the layers in 1.
3. A failure tied to a series by the difference alone is shown only once
   two builds agree: it is new in two builds whose stacks carry the series,
   each against the build of its branch before it. Until then it stays the
   stack's. A failure tied by the recipe is shown at once.
4. A JSON read API in error-report-web, for the failures of one AutoBuilder
   build, is being decided separately and is likely. bodnuti reads error
   reports through it once it exists; until then, the AutoBuilder's own
   step logs (`errors`, `stdio`) give the recipe, task and error, and the
   error report is only linked.

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

### 6.1 Findings as blocks

Each finding on a series page was one line of `<code>`: severity, tool,
file and line, message and check id run together, and a long message (a
patchtest explanation, an advisory title) wrapped into an unreadable run.
Each finding becomes a block of its own, laid out as sashiko.dev lays out a
review (`static/index.html` in sashiko: a bordered `review-card`, a header
of labelled values, the severity in its own colour, and the text in a
`log-inline-block`, monospace with `white-space: pre-wrap`):

```
+--------------------------------------------------------------------+
| [ERROR] oelint  oelint.vars.inconspaces       meta/foo/foo.bb:12   |
| Assignment should be enclosed in double quotes                     |
|   10 | SRC_URI = "..."                                             |
| > 12 | A:append= "x"                                               |
|   13 | ...                                                         |
+--------------------------------------------------------------------+
```

- **Header:** a severity badge (error, warning, info), the tool, the check
  id, and the file and line.
- **Message:** a block of its own with its line breaks kept.
- **Excerpt:** when the finding has a file and line, the lines around it
  (three before and after) in the file as the series leaves it, numbered,
  the finding's line marked. Only the check has the patched tree, so the
  check records the excerpts in the report (`excerpts`, keyed by file and
  line; at most seven lines, each cut at 200 characters). The text comes
  from the series and is escaped like everything else.
- **Fixes** (§5.3) are blocks in the same form with a "fixed" badge and no
  excerpt (their line is in the tree before the series). **Tool errors**
  are blocks with the tool and the error text. An apply failure on a patch
  that has landed stays the one-line "already has it" note.

Findings stay grouped under the patch they belong to. `report.txt` gets the
same blocks in plain text: a header line, the indented message, and the
numbered excerpt with `>` on the finding's line, a blank line between
blocks. The index is unchanged. Reports written before `excerpts` existed
render without excerpts.

### 6.1.1 Style findings

Measured on the 397 series published by 25 October 2026: 919 findings, 677
of them from oelint-adv, and about 470 of those from rules about layout
rather than correctness (spacing, ordering, suggested variables). They
drowned the findings that matter, and some style rules OE-Core does not
follow were suppressed outright, so their findings were lost.

oelint-adv rules are now in three groups:

- **style**, recorded but kept apart: `vars.notneededspace`,
  `var.suggestedvar`, `vars.dependsordered`, `tabs.notabs`,
  `var.bbclassextend`, `task.docstrings`, `newline.consecutive`, and the
  style rules previously suppressed (`var.order`, `var.filesoverride`,
  `var.override`, `vars.multilineident`, `vars.outofcontext`);
- **suppressed**, because their result is not about the series:
  `vars.homepageping` (fetches HOMEPAGE, fails offline) and
  `bbclass.underscores` (fires on the stub recipe for a `.inc`);
- **findings**, everything else, `vars.spacesassignment` and
  `vars.inconspaces` included.

Style findings go into the report's `style` list, compared before and
after like any other. They do not count: not in the result, not in the
exit status, and a series with only style findings is clean. The series
page shows them under each patch in a collapsed `<details>` element ("Show
N style findings"), which needs no JavaScript; the result reads "3
findings · 10 style". `report.txt` lists them under a `style` heading.

### 6.2 The front page

bodnuti.dev's front page is a static page in the bodnuti.dev repository:
what bodnuti checks, the watched lists, and links to the reports, the
repository, its releases and `SECURITY.md`. A live block on it comes from
`summary.html`, which `bodnuti publish` writes beside the reports: per list,
the series of the last seven days, how many have findings and how many have
landed, and the five latest series, with links under `/reports/`.

Caddy inserts the fragment with its `templates` directive, on the front
page only, using `readFile`, which inserts a file as it is. `include` would
render the file as a template, and the fragment holds mail subjects: a
subject with `{{ … }}` in it would run as a template action. As a second
guard, bodnuti's escaping turns `{` and `}` into character references
everywhere. Without a `summary.html` (before the first publish) the page
says so.

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
