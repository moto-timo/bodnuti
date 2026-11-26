<!-- select: .bb .bbappend .inc SRC_URI -->
# Rules: SRC_URI

Applies to: `.bb`, `.bbappend`, `.inc` hunks that change `SRC_URI`,
`PYPI_SRC_URI`, `S`, or add a patch to `SRC_URI` behind a condition.

Patch headers (`Upstream-Status`, `CVE:`) are in `patterns/patch-files.md`;
broken quoting in `rules-syntax.md`; `SRCREV` in `rules-versions.md`.

### SRC-1 No GitHub-generated archive tarballs

- **Severity:** error (as of 2025 the `src-uri-bad` QA check rejects them;
  earlier it warned)
- **Trigger:** a `SRC_URI` entry of the form
  `https://github.com/<owner>/<repo>/archive/...` (including
  `/archive/refs/tags/...`).
- **Problem:** GitHub generates these tarballs on demand and their bytes
  and checksums can change, so fetches start failing on checksum mismatch.
- **Fix:** use a release asset uploaded by the maintainers
  (`.../releases/download/<tag>/<file>`), or fetch with `git://...;
  protocol=https;branch=...` and a commit hash in `SRCREV`.
- **Do not report when:** the URL is a `/releases/download/` asset, or a
  `git://` URL.
- **Evidence:**
  - "github /archive/ URLs produce unstable tarballs." (libcheck: Upgrade
    to 0.12.0, 2017-10)
  - "Archived tags in Github are unfortunately not immutable, so their use
    is discouraged (and it also throws a fatal QA check)." (testdisk: add
    recipe, 2025-04)

### SRC-2 Use BPN and PV, not PN or literal names and versions

- **Severity:** error for `PN` (breaks native, nativesdk and multilib
  builds); warning for a literal version
- **Trigger:** `${PN}` inside `SRC_URI`, `S`, or a source/install path; or a
  literal copy of the recipe's version or name in `SRC_URI`
  (`.../tinyproxy-1.8.4.tar.gz` in `tinyproxy_1.8.4.bb`).
- **Problem:** `PN` becomes `foo-native`, `nativesdk-foo` or `lib32-foo` in
  the other variants, so the URL or path is wrong there. A literal version
  is missed at the next upgrade and fetches the old source.
- **Fix:** `${BPN}`, `${PV}`, `${BP}` (= `${BPN}-${PV}`).
- **Do not report when:** `${PN}` names a package (`FILES:${PN}`,
  `RDEPENDS:${PN}`, `PACKAGES`) — that is correct; the upstream file name
  uses a different version format than `PV` (computed elsewhere). Never
  report `${BPN}-${PV}` versus `${BP}`: that is style.
- **Evidence:**
  - "In native or nativesdk builds PN will be ell-native or nativesdk-ell,
    so PN here should be BPN" (ell: add recipe, 2019-04)
  - "It should be BPN instead of PN, otherwise this will break with
    multilib." (stm32flash: add recipe, 2018-05)
  - "Use ${PV} for 2nd 1.8.4 as well." (tinyproxy: Add recipe, 2017-04)

### SRC-3 Do not apply patches conditionally

- **Severity:** warning
- **Trigger:** a patch added to `SRC_URI` only under an override or
  condition: `SRC_URI:append:libc-musl = " file://x.patch"`,
  `${@bb.utils.contains('DISTRO_FEATURES'|'PACKAGECONFIG', ..., 'file://x.patch', ...)}`.
- **Problem:** the source differs between configurations, the patch rots
  untested in the others, and upgrades break in ways nobody sees.
- **Fix:** make the patch correct for every configuration and apply it
  always; or turn the behaviour into a real build option (and send it
  upstream).
- **Do not report when:** the patch cannot be made configuration
  independent and the commit message says why. Maintainers have accepted
  test-only fixes scoped to one backend (one 2023 reviewer asked for that).
- **Evidence:**
  - "Conditionally applying patches is rarely a good move. Why not always
    apply this?" (bootchart2: switch to add patch from change source in
    do_install, 2019-09)
  - "Conditional patches are terrible for maintainability. Please make it a
    proper meson option, and submit upstream." (systemd: make predictable
    name mac policy opt-out, 2024-04)
