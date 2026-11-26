<!-- select: .bb .bbappend .inc PACKAGES FILES INSANE_SKIP INHIBIT_PACKAGE_STRIP ptest RPROVIDES RREPLACES RCONFLICTS -->
# Rules: Package Splits, QA Skips and ptest

Applies to: `.bb`, `.bbappend`, `.inc` hunks that change `PACKAGES`,
`PROVIDES`, `RRECOMMENDS`/`RDEPENDS` between a recipe's own packages,
`INSANE_SKIP`, `INHIBIT_PACKAGE_STRIP` or other `INHIBIT_*`, or that add
ptest support (`inherit ptest`, `do_install_ptest`, `${PN}-ptest`).

General packaging rules are in `patterns/packaging.md`; `=` versus `+=` on
`FILES`/`RDEPENDS` is in `rules-assignments.md`.

### PKG-1 Splitting a package must keep what the old package installed

- **Severity:** warning
- **Trigger:** a patch moves files of an existing package into new
  subpackages and either (a) makes the old package pull them back only with
  `RRECOMMENDS`, (b) makes it `RDEPENDS` on more than it used to ship, or
  (c) adds the new package names to `PROVIDES`.
- **Problem:** images built with `BAD_RECOMMENDATIONS` or
  no-recommends lose files they had; images that installed the old package
  gain files they did not have. `PROVIDES` names build-time recipe targets,
  not runtime packages.
- **Fix:** make the old package pull in, with `RDEPENDS`, exactly the
  subpackages that hold what it used to ship; leave `PROVIDES` alone.
- **Do not report when:** the recipe is new; the files moved were not in
  the old package; the commit message states the change in content and
  why.
- **Evidence:**
  - "For an image or configuration that turns off rrecommends ..., this is
    not backwards compatible" (sudo: split into smaller tools package,
    2020-12)
  - "This breaks backwards compatibility. The iproute2 package today does
    not bring in all the sub-packages." (iproute2: split ip to individual
    package, 2020-12)
  - "Why do we need to add all the individual package splits to PROVIDES?"
    (iproute2: split ip to individual package, 2020-12)

### PKG-2 Do not silence QA or stripping without a cause

- **Severity:** warning
- **Trigger:** a new `INSANE_SKIP` entry (`ldflags`, `dev-so`, `dev-elf`,
  `already-stripped`, `32bit-time`, ...) or `INHIBIT_PACKAGE_STRIP = "1"`,
  with no comment naming why the check cannot be met.
- **Problem:** the QA check usually points at a real defect: `ldflags`
  means the build ignored `LDFLAGS` (no hardening, no hash style);
  disabling stripping ships larger binaries.
- **Fix:** fix the cause (pass `LDFLAGS` to the build, package the file in
  the right package); if a skip stays, make it per package
  (`INSANE_SKIP:${PN} += "..."`) with a comment.
- **Do not report when:** a comment gives a cause that cannot be fixed in
  the recipe (prebuilt binaries, firmware blobs, plugins loaded by a fixed
  `.so` name); the skip is unchanged by the patch.
- **Evidence:**
  - "This one should be taken seriosly - what step made it necessary?"
    (lmdb: add recipe, 2021-07)
  - "it means we can not have stripped binaries and it will increase the
    size unnecessarily" (libcamera: fix packaging and installation, 2020-07)

### PKG-3 Test-only build changes are gated on ptest

- **Severity:** warning
- **Trigger:** a patch adding ptest support also makes test-only changes
  unconditional: a configure option that builds tests or test plugins, test
  dependencies in `DEPENDS`, or test files or extra libraries in `${PN}`.
- **Problem:** every build and image pays for the tests, even when ptest is
  off.
- **Fix:** gate them on ptest: a `PACKAGECONFIG` knob enabled by
  `bb.utils.contains('DISTRO_FEATURES', 'ptest', ...)`, or a test on
  `PTEST_ENABLED`; put test files in `${PN}-ptest`.
- **Do not report when:** the change is already behind `PTEST_ENABLED` or
  the `ptest` distro feature; it is inside `do_compile_ptest` /
  `do_install_ptest` (these only run with ptest enabled); upstream builds
  the tests unconditionally and they are not installed into `${PN}`.
- **Evidence:**
  - "This should stay as PACKAGECONFIG, and be enabled subject to ptest in
    DISTRO_FEATURES." (libsdl2: Add tests package and ptest support,
    2025-08)
  - "could it be added conditionally, only in case ptest is enabled?"
    (libxslt: add ptest support, 2025-10)
  - "This will inflate main image sizes just to make tests work" (elfutils:
    fix ptest failures, 2019-05)
