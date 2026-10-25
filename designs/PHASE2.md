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
   rather than silent.

### 3.2 Repositories that are not layers (planned)

Two projects on yocto-patches are Django web applications, not layers:
`[layerindex-web]` (git.yoctoproject.org/layerindex-web, behind
layers.openembedded.org) and `[error-report-web]`
(git.yoctoproject.org/error-report-web, behind errors.yoctoproject.org).
Both send several series a month. They are left unrouted for now, because
the layer checks find little in them:

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

This gets its own design section, settling the ruff defaults and the
vulnerability database handling, before implementation.

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
