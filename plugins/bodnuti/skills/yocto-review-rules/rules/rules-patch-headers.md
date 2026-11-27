<!-- select: .patch .diff -->
Applies to: `.patch` and `.diff` files added or changed in a layer (usually beside a recipe, under `files/` or `<recipe>/`).

# Patch File Headers

These rules add to `patterns/patch-files.md`, which lists the allowed
`Upstream-Status` values and the required `Signed-off-by:` and `CVE:` tags.
Read that first. The rules below are the mistakes maintainers keep sending
back. Each comes from at least two real review comments.

Check only the header of the patch file: the text between `Subject:` (or the
top of the file) and the `---` line or the first `diff`/`Index:` line. Report
only on patch files the series adds or whose header it changes.

## PH1. Upstream-Status missing or misspelled

- **Severity:** error
- **Trigger:** a new patch file whose header has no line starting exactly
  with `Upstream-Status:`. This includes near misses: `Upstream status:`,
  `Upstream Status:`, `Upstream-status`, or the status written only in the
  commit message of the series.
- **Problem:** the tag is machine-read (patchtest, the patch status
  reports). A misspelled tag counts as missing.
- **Fix:** add `Upstream-Status: <Value> [...]` to the patch file header,
  spelled exactly, with one of the values in `patch-files.md`.
- **Do not report when:** the correctly spelled tag is present anywhere in
  the header, including after the commit text and before `---`; the
  series only deletes the patch file; the header is unchanged and the tag
  was already missing before (mark it pre-existing instead).
- **Evidence:**
  - "This is a machine-readable tag, so please use the correct form Upstream-Status: Pending." — mesa-demos: OpenVG demos (2016-05)
  - "Upstream-Status, you need the hyphen." — createrepo-c: Fix setup of logging (2018-12)

## PH2. Submitted without a link to the submission

- **Severity:** warning
- **Trigger:** `Upstream-Status: Submitted` with no `[...]`, or with a
  bracket that names only a mailing list address (`[foo-devel@lists...]`)
  or a project name, not a URL.
- **Problem:** maintainers need to follow the submission to see whether it
  was merged, changed or rejected. A list address does not identify the
  message.
- **Fix:** `Upstream-Status: Submitted [<URL>]`, pointing at the pull or
  merge request, the bug, or the message in a web mail archive.
- **Do not report when:** the bracket holds a URL to a specific pull
  request, merge request, bug or archived message.
- **Evidence:**
  - "Submitted [where] please. If it's to a mailing list, link to the message on some web archive." — linux-firmware: replace mkdir by install (2022-05)
  - "Please add a link to the actual submission post in the mailing list archive, so we can track what happened to it." — xf86-video-intel: CFL-S PCI IDs (2018-01)

## PH3. Pending although the patch was already submitted

- **Severity:** warning
- **Trigger:** `Upstream-Status: Pending` while the patch header or the
  commit message of the series links to an upstream pull request, merge
  request, bug, or mailing list post for this change, or says it was sent
  upstream.
- **Problem:** the status is wrong. `Submitted` does not mean upstream
  accepted it, only that it was sent.
- **Fix:** change it to `Submitted [<URL of the submission>]`.
- **Do not report when:** the link is to a bug report or discussion that
  does not carry this patch, or nothing in the series shows a submission.
  Do not guess that a submission exists.
- **Evidence:**
  - "This should say 'Submitted [where]', as you have indeed submitted the patch and upstream have seen it." — kea: upgrade to v2.4.0 (2023-08)
  - "As you have submitted upstream, please change to 'Submitted' with a link" — gpgme: fix python setuptools invalid version (2023-02)

## PH4. Backport that is not an upstream commit, or has no location

- **Severity:** warning
- **Trigger:** `Upstream-Status: Backport` where the header or bracket shows
  the change came from somewhere other than upstream's own repository:
  another distribution (Debian, Fedora, RHEL, SUSE, Arch, Alpine), a bug
  tracker attachment, an unmerged or rejected pull request, or a branch
  that is not upstream's development branch. Also `Backport` with no
  bracket, or with a bare commit hash and no URL.
- **Problem:** a backport is a commit merged upstream. Anything else is
  `Submitted`, `Pending`, `Denied` or `Inactive-Upstream`. A bare hash does
  not say which repository holds it.
- **Fix:** for a real upstream commit, `Backport [<URL of the commit in
  upstream's git web UI>]`. Otherwise pick the status that is true, and
  keep the reference to where the patch came from in the header.
- **Do not report when:** the URL points at a commit in upstream's own
  repository or its official GitHub/GitLab project (`.../commit/<sha>`,
  `?id=<sha>`, `;h=<sha>`). Never compare that hash with the
  `From <hash> Mon Sep 17 00:00:00 2001` line at the top of the patch: that
  line is the commit `git format-patch` was run on, usually a local
  cherry-pick, so it differs from upstream's hash in a correct backport.
- **Evidence:**
  - "Another distribution patching a recipe does not make it a backport, backports have to be from upstream." — expect: Fix segfaults (2017-09)
  - "This doesn't say where the commit can be found. It's better to point to the git web ui" — debugedit: fix build failure (2025-02)

## PH5. Inappropriate on a fix upstream would want

- **Severity:** warning
- **Trigger:** `Upstream-Status: Inappropriate` with no reason in brackets,
  or with a reason (`[OE specific]`, `[configuration]`, `[musl specific]`)
  that does not fit the change: the patch fixes a compiler error, a crash,
  a missing include, a portability problem (musl, a newer GCC), a race, a
  test timeout, or a build-system bug any upstream user could hit.
- **Problem:** carrying a patch forever costs every upgrade. Maintainers
  ask for generic fixes to go upstream and want the reason for
  `Inappropriate` to be true and explained.
- **Fix:** submit upstream and use `Submitted [<URL>]` (or `Pending` until
  then); if it is truly OE-only, say why in the header.
- **Do not report when:** the patch only makes sense inside OE: it points
  at OE sysroot or install paths, stops a build looking at the build host,
  adapts test output for ptest, removes a check that cross-compiling cannot
  pass, or the header explains why upstream would not take it.
- **Evidence:**
  - "At the very least you need to explain why this is OE specific and doesn't affect upstream." — systemd: use the absolute path in service file (2021-01)
  - "High CPU load is not at all an oe-specific problem." — openssl: extend check_cwm test timeout (2025-10)

## PH6. Pending on a change only OE would carry

- **Severity:** warning
- **Trigger:** `Upstream-Status: Pending` where the bracket or header text
  itself says the change is specific to OE/Yocto/ptest ("Pending [specific
  to oe setup]"), or the patch only adapts upstream output or paths for OE
  (for example ptest result format, OE-only native tool behaviour).
- **Problem:** `Pending` means it will be sent upstream. If it never will
  be, the status is wrong.
- **Fix:** `Upstream-Status: Inappropriate [oe specific]` (or another
  accurate reason).
- **Do not report when:** the change would be useful to upstream users
  (then PH5 applies in reverse); the header plans an upstream submission.
- **Evidence:**
  - "You choose the wrong status. It should be Inappropriate." — gstreamer: enable ptest support (2018-02)
  - "I doubt there's any intention to get upstream to change the output of their test harness to suit us." — icu: use automake ptest output format (2025-10)

## PH7. Dead upstream not marked Inactive-Upstream

- **Severity:** warning
- **Trigger:** the header says upstream is unmaintained, has had no release
  or commit for years, or cannot accept patches, while the status is
  `Pending`, `Inappropriate` or `Backport`.
- **Problem:** OE has a status for this case.
- **Fix:** `Upstream-Status: Inactive-Upstream [lastcommit: <when>]` and/or
  `[lastrelease: <when>]`, keeping any reference to where the patch came
  from.
- **Do not report when:** upstream is slow but active; the patch is a real
  backport of an upstream commit; the status already is
  `Inactive-Upstream`, with a free-text reason or none: master carries 36
  such patch files without the `[lastcommit:]`/`[lastrelease:]` form, so
  a missing date is not a finding.
- **Evidence:**
  - "There is a separate status for this situation: Inactive-Upstream. Please use it." — expect: Replace tclsh with tclsh8 (2024-12)
  - "I think for the status "Backport", or maybe "Inactive-Upstream" would be better (while still keeping the reference)" — zip: fix a buffer overflow (2025-05)

## PH8. Authorship changed or sign-offs forged or removed

- **Severity:** error
- **Trigger:** in a patch file the series adds or refreshes:
  - a `-Signed-off-by:` line removed from an existing patch header, or the
    `From:` author changed from the original author to the submitter;
  - a `From:` or `Signed-off-by:` with a placeholder identity
    (`@example.com`, "kas User", "OE Builder", "root@localhost");
  - in a patch file the series adds (not one it only refreshes), the
    only `Signed-off-by:` belongs to the upstream author, and none
    belongs to the person adding the patch to the layer.
- **Problem:** `Signed-off-by:` certifies the Developer Certificate of
  Origin personally. Nobody may add one in someone else's name, remove
  someone's, or take over authorship of another person's patch.
- **Fix:** keep the original `From:` and existing sign-offs; add only your
  own `Signed-off-by:` below them.
- **Do not report when:** the extra sign-offs are copied verbatim from the
  upstream commit being backported (`git format-patch` output); the
  submitter is the original author; a carried patch is only refreshed
  (context, offsets or paths updated in an upgrade): that needs no new
  `Signed-off-by:`.
- **Evidence:**
  - "Please do not, ever, add sign-off-by that has someone else's name." — readline: backport a patch (2025-08)
  - "Please do not change the author of the patch to yourself." — kea: fix multilib conflict (2025-10)

## PH9. Generic fix left as Pending

- **Severity:** warning (a maintainer request; report at most once per
  series)
- **Trigger:** a new patch file with `Upstream-Status: Pending` whose change
  is a generic fix: a build fix for a newer compiler or libc, a bug fix,
  an install or multilib fix in upstream's build system.
- **Problem:** as of 2023–2024, OE-Core maintainers ask authors to submit
  such patches upstream before or while sending them to OE, not to add
  more `Pending` patches.
- **Fix:** send it upstream, then use `Submitted [<URL>]`.
- **Do not report when:** the patch is OE-specific (see PH6); the series is
  a stable-branch backport of an already accepted master patch; the
  patch file is only refreshed, not new.
- **Evidence:**
  - "Please no pending patches. Submit here: https://bugs.kde.org/..." — valgrind: Upgrade to 3.23.0 (2024-05)
  - "Please send this upstream first" — xcb-proto: Fix install conflict when enable multilib (2023-03)

## PH10. Patch carries changes unrelated to its purpose

- **Severity:** warning
- **Trigger:** hunks in the patch that change only wording, comments,
  whitespace, formatting or verbosity, and are not needed for the fix the
  header describes; or removed lines (such as a deleted comment or a
  commented-out line) the header does not explain.
- **Problem:** every extra hunk must be rebased on each upgrade and may
  fail to apply. Unexplained removals look like debugging leftovers.
- **Fix:** drop the unrelated hunks, or explain them in the patch header.
- **Do not report when:** the patch is a verbatim backport of an upstream
  commit; the hunk is needed for the fix to build or apply.
- **Evidence:**
  - "This needs to be separately justified. Do we really need those extra tweaks" — icu: use automake ptest output format (2025-10)
  - "Without an explanation in the long log, I can't tell if this comment was removed intentionally" — bzip2: Add test suite (2020-04)

## PH11. Header information placed below the `---` line

- **Severity:** warning
- **Trigger:** `Upstream-Status:`, `CVE:`, `Signed-off-by:` or reference
  links (where the patch came from, bug URLs) placed after the `---`
  separator, among the diffstat, instead of in the header above it.
- **Problem:** text below `---` is dropped when devtool or git rebases and
  regenerates the patch, so the provenance is lost.
- **Fix:** move these lines above `---`, after the commit message.
- **Do not report when:** the text below `---` is only a diffstat or a
  version note that is not needed after merging.
- **Evidence:**
  - "keep these notes above the "---" divider in the patch file so that they're not lost if devtool ... is used to rebase" — readline: backport a patch (2025-10)
  - "The point being that without the git history, we can still identify who sent a patch and whether it was a backport" — python3: fix build on softfloat mips (2019-06)
