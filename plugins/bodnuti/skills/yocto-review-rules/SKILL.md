---
name: yocto-review-rules
description: Read this before reviewing, judging or commenting on any Yocto Project, OpenEmbedded or BitBake patch (a mail or diff touching .bb, .bbappend, .inc, .bbclass, .conf, a carried .patch file, meta/lib/oe or oeqa; CVE backports, version upgrades, SRC_URI, LICENSE, PACKAGECONFIG, Upstream-Status headers), and before writing such a change. Holds the rules OE-Core and meta-openembedded maintainers enforce, condensed from ten years of list review, and the BitBake behaviour that looks like a bug but is not, so a review reports real problems and no false ones. Even when the patch looks clean, check it against these rules before saying so.
allowed-tools: Read
---

# Yocto review rules

Each rule was raised by maintainers at least twice, and the author changed
the code because of it. Every rule gives its trigger, what is wrong, the
fix, when **not** to report it, and real quotes with the patch and month.

Read only the files for what the change touches, not all of them:

| Change touches | Read |
|---|---|
| any recipe line | `rules/rules-syntax.md` |
| any recipe, class or carried patch, before reporting | `rules/rules-bitbake.md` (BitBake behaviour that is not a defect) |
| `RDEPENDS`, `FILES`, `INSANE_SKIP` assignments | `rules/rules-assignments.md` |
| `PACKAGECONFIG` | `rules/rules-packageconfig.md` |
| `DEPENDS`, `RDEPENDS`, `inherit` | `rules/rules-dependencies.md` |
| `DISTRO_FEATURES`, systemd, polkit, x11 | `rules/rules-distro-features.md` |
| `SRC_URI` | `rules/rules-src-uri.md` |
| `SRCREV`, `PV`, `PE`, a version upgrade | `rules/rules-versions.md` |
| `LICENSE`, `LIC_FILES_CHKSUM` | `rules/rules-licensing.md` |
| `CVE_STATUS`, `CVE_PRODUCT` | `rules/rules-cve.md` |
| `PACKAGES`, package splits, ptest, `INSANE_SKIP` | `rules/rules-packaging.md` |
| `do_install`, paths such as `/usr`, `/etc`, staging | `rules/rules-paths.md` |
| compiler flags, `EXTRA_OE*`, `sed` on build files | `rules/rules-build-fixes.md` |
| a `.bbclass` | `rules/rules-classes.md` |
| a carried `.patch` or `.diff` file | `rules/rules-patch-headers.md` |
| Python in `meta/lib`, `scripts/`, or in classes | `rules/rules-oe-python.md` |
| oe-selftest and other `oeqa` tests | `rules/rules-oeqa.md` |
| `run-ptest`, init scripts, unit files for the target | `rules/rules-target-scripts.md` |

How to use them:

- Report a rule's problem only when its trigger is in the change and none
  of its "do not report when" cases applies. Check the layer first: a
  value may be set in an `.inc`, a class or a `.bbappend`.
- They are not style rules. Do not raise taste, wording or formatting
  because a rule is near it.
- They are not the whole list. Still review the change itself: a broken
  hunk, a wrong path, code that cannot work is a finding even when no
  rule names it, if you can show it from the patch.
- Some rules say "as of <year>": check the branch. Stable branches keep
  older policy.

The first line of each file, `<!-- select: ... -->`, is how bodnuti picks
the files per patch: file types or `path=` parts the change touches, then
text its changed lines contain. These rules build on sashiko's Yocto
guides and do not repeat them. Draft: condensed with an AI, every quote
checked against the list mail; not yet reviewed by a maintainer.
