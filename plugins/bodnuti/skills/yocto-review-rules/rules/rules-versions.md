<!-- select: .bb .bbappend .inc SRCREV PV PE -->
# Rules: SRCREV, PV and Upgrades

Applies to: `.bb`, `.inc` hunks that change `SRCREV`, `PV`, `PE`, the version
in a recipe file name, a `git://` `SRC_URI`, or that come from
`devtool upgrade` / AUH (subject `upgrade X -> Y`).

### VER-1 SRCREV is a commit hash, not a tag, branch or AUTOREV

- **Severity:** error (builds without network access fail; the result is
  not reproducible)
- **Trigger:** `SRCREV = "v${PV}"` or another tag name, `rev=<tag>` in a
  `git://` URL with no hash, or `SRCREV = "${AUTOREV}"` in a recipe meant
  for a release layer.
- **Problem:** resolving a tag or AUTOREV needs the network at parse time,
  and tags can move.
- **Fix:** set `SRCREV` to the full commit hash. As of 2025, maintainers
  also ask for `;tag=${PV}` (or the matching tag) in the `git://` URL
  beside the hash, so the fetcher checks they agree.
- **Do not report when:** `tag=` appears together with a hash `SRCREV`;
  `AUTOREV` is under `:class-devupstream` or in a recipe clearly marked as
  a development tracker.
- **Evidence:**
  - "please set SRCREV to git hash, so that fetcher won't try to access
    remote repo to translate tag name to SRCREV." (ne10: add recipe,
    2016-02)
  - "Using ${AUTOREV} makes it impossible to use this recipe disconnected
    from the internet" (CANopenNode: recipe for building the CANopenSocket
    tools, 2016-07)
  - "Drop rev pointing to a tag. We need a hardcoded SHA" (rdma-core: add
    new recipe, 2018-04)

### VER-2 PV, the file name and SRCREV must agree

- **Severity:** error (the recipe builds a different version than it claims)
- **Trigger:** a git recipe whose `PV` (or file name) changes while
  `SRCREV` does not; a `PV = "..."` that differs from the version in the
  `_<version>.bb` file name; a `-devupstream` or `+git` PV whose base
  version is not the version the commit belongs to.
- **Fix:** update `SRCREV` with `PV`; keep one version, carried by the file
  name, and do not override it with a different `PV`.
- **Do not report when:** `PV` derives from the file name with a `+git`
  suffix; the recipe is `_git.bb` and sets `PV` itself.
- **Evidence:**
  - "PV is updated but SRCCREV is not, so this still fetches the 1.1.0
    version." (blktrace: 1.1.0+gitX -> 1.2.0+gitX, 2018-01)
  - "recipe says 2.0.3 but here its overridden to 5.10.0, we need to confirm
    one version" (python3-piccata: added, 2023-08)

### VER-3 A version that sorts lower needs a PE bump

- **Severity:** warning (package feeds stop offering the upgrade)
- **Trigger:** a change where the new `PV` sorts lower than the old one,
  for example from a snapshot-number version (`131`) to a release
  (`1.7.5`), or from a `+git` version above a future release to that
  release.
- **Fix:** increase `PE` (`PE = "1"`), or choose a `PV` that keeps sorting
  upwards.
- **Do not report when:** `PE` is already bumped in the same patch, or the
  new version sorts higher (compare as dpkg/opkg would, digit groups
  numerically).
- **Evidence:**
  - "131 > 1.7.5, so you'll need to add PE="1" to ensure upgrades happen."
    (lz4: upgrade to v1.7.5, 2017-02)
  - "perhaps we should leave room for sorting 2.1.0 higher than whatever we
    do now" (luajit: Upgrade to 2.1.0-beta3, 2020-02)

### VER-4 Remove devtool upgrade leftovers

- **Severity:** warning
- **Trigger:** an upgrade that adds a patch named like
  `0001-Committing-changes-from-do_patch.patch`, or keeps the comment block
  `devtool upgrade` writes above `LIC_FILES_CHKSUM` (ending
  "(example: 'License-Update: copyright years updated.')").
- **Problem:** the patch is an artefact of devtool, not an upstream change;
  the comment is instructions to the author, not recipe content.
- **Fix:** drop the generated patch and the comment; put the license
  explanation in the commit message (see LIC-4).
- **Do not report when:** a patch with a similar name has real content and a
  proper header.
- **Evidence:**
  - "These two are 'devtool upgrade' noise, and should be removed."
    (libmodule-build-perl: upgrade 0.4229 -> 0.4231, 2020-02)
  - "above cruft should be deleted." (python3-scons{-native}: upgrade
    4.1.0 -> 4.2.0, 2021-08)
