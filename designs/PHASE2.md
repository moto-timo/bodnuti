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
```

patchtest always comes from `cache/openembedded-core`. `yocto-patches`
carries several repositories, so it is left out until there is a rule for
choosing one (§8).

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

## 4. State

Files, not SQLite. The volume is about 170 patches a day, every record is
written once and read rarely, and files can be inspected with `ls` and `cat`
and copied with rsync. A database can replace them if the volume or the
queries grow.

```
state/
  lock
  lists/<list>.json              newest timestamp seen, last poll time
  series/<list>/<root-msgid>/
    series.json                  subject, n/N, message-ids seen, first/last seen, status
    <root-msgid>.mbx             what b4 fetched
    report.json                  bodnuti check --json
    report.txt                   bodnuti check
  patch-ids/<patch-id>           path of the series that first had this patch
```

Message-IDs are used as directory names only after they pass a strict
filter (letters, digits and `.-_+@=`, no `/`, no leading `.`), because they
come from untrusted mail. A Message-ID that fails is logged and skipped.

Series status moves forward only:

```
seen -> complete -> fetched -> checked
                         \-> failed (with the reason)
```

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

## 6. Publishing

`bodnuti publish` renders, from `state/`:

- `index.html`: the most recent series across all lists, newest first, with
  list, subject, author, result (clean, findings, not checked) and a link;
- `<list>/index.html`: the same for one list;
- `<list>/<root-msgid>/index.html`: the text report, the series' patches and
  links to lore.

Everything from mail (subjects, author names, messages in findings) is
HTML-escaped. The pages are plain HTML and CSS with no JavaScript, styled
like the bodnuti.dev landing page. Output goes to `state/site/`, then:

```
rsync -a --delete state/site/ bodnuti-web:reports/
```

over the restricted deploy key from `DEPLOY.md` (`rrsync` on the web host).
Pages are regenerated in full each time; at a few thousand series this takes
well under a second.

## 7. Testing

- Unit tests for the Atom parser (on a captured feed), series grouping
  (cover letter, no cover letter, `vN` resend, replies), the Message-ID
  filter, and status transitions.
- A test of `poll` against a local directory of captured feeds and mboxes,
  with the podman calls replaced by a stub command, so CI needs no network
  and no container runtime.
- A first live run on the worker with `publish` writing locally only, before
  the rsync key is set up.

## 8. Open questions

1. `yocto-patches` carries meta-yocto, poky helpers and other repositories.
   Route by subject tag (`[meta-yocto]`), by the paths a patch touches, or
   leave the list out?
2. Retention: keep every report forever (small: a few kilobytes each), or
   only the last N months on the published site?
3. Should the index mark a series that a maintainer has already applied?
   That needs matching upstream commits to patch-ids, and is probably phase 2b.
