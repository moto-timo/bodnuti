<!-- select: .bb .bbappend .inc FLAGS EXTRA_OE 's/ "s/ -->
# Rules: Build Fixes and Workarounds

Applies to: `.bb`, `.bbappend`, `.inc` hunks that add compiler flags
(`CFLAGS`, `CXXFLAGS`, `TARGET_CFLAGS`, `LDFLAGS`, `ARM_INSTRUCTION_SET`),
architecture, class or libc overrides, or `sed`/regex commands in tasks or
`UPSTREAM_CHECK_*REGEX` — typically patches titled "fix build", "fix ... on
<arch>", "fix ... with musl/clang/gcc-N".

### BUILD-1 Fix the cause, not with flags that hide it

- **Severity:** warning
- **Trigger:** a build failure "fixed" by adding `-Wno-error`, `-std=gnu*`,
  `-fpermissive`, `-DDISABLE_ASM`-style switches, or `-I${S}/...` /
  `-I${B}/...` include paths to compiler flags.
- **Problem:** the warning or bug stays in the code, performance can drop
  (assembly disabled), and the workaround outlives its reason.
- **Fix:** use the build system's own option if it has one
  (`--disable-werror`); otherwise patch the source (send it upstream, with
  `Upstream-Status`), or use an existing upstream fix.
- **Do not report when:** the flag is narrow (`-Wno-error=<one warning>`),
  carries a comment or commit-message reason, and no upstream fix exists;
  the flag is the build system's documented way to select a feature.
- **Evidence:**
  - "is there configure option --disable-werror, we should use that if
    available." (open-lldp: Add initial version 1.0.1, 2020-02)
  - "This will build suboptimally. It would be better to fix the issue in
    Makefile to use compatible options." (cryptopp: add a recipe for version
    8.9.0, 2024-11)
  - "There is a patch attached ... perhaps this can be used and above flags
    avoided." (bvi: add new package, 2025-05)

### BUILD-2 Scope a workaround to exactly the configurations that need it

- **Severity:** error when the change breaks other configurations; warning
  otherwise
- **Trigger:** a fix for one architecture, CPU feature, libc or class
  applied to all of them (no override); or an override that misses
  variants with the same problem (`armv7a` but not `armv7ve`) or covers
  variants that do not have it (`arm` when only Thumb-1 cores fail); or a
  CPU feature (crypto, NEON) forced for a whole architecture family.
- **Problem:** other configurations get a needless or wrong change; forced
  CPU features produce binaries that crash on cores without them.
- **Fix:** use the narrowest matching override (`:powerpc`, `:armv5`,
  `:class-native`, `:libc-musl`), or test `TUNE_FEATURES` instead of the
  architecture name.
- **Do not report when:** the change is correct for every configuration —
  then unconditional is preferred (a 2020 reviewer asked to drop a
  `:class-native` scope for that reason).
- **Evidence:**
  - "If this is ppc-specific shouldn't it have a ppc override?" (llvm: fix
    link error for powerpc, 2019-04)
  - "you should only force the ARM instruction set for ARM cores which don't
    support Thumb2" (multipath-tools: fix ARM build failure, 2017-02)
  - "This cannot be enforced, as it would break those AArch64 derivatives,
    which does not have Crypto extension." (abseil-cpp: add recipe, 2020-02)

### BUILD-3 Change build files with a patch, not sed

- **Severity:** warning
- **Trigger:** `sed -i` on the source's build files (`configure.ac`,
  `configure`, `Makefile*`, `CMakeLists.txt`, `meson.build`) in a task.
- **Problem:** a sed that stops matching after an upgrade fails silently,
  and the intent is hard to read.
- **Fix:** a patch with an `Upstream-Status` header, ideally sent upstream;
  or pass the value as a make/configure variable instead of editing the
  file.
- **Do not report when:** the sed rewrites installed files in `${D}`
  (shebangs, build paths for reproducibility); it substitutes a value only
  known at build time and a patch could not carry it.
- **Evidence:**
  - "Ad hoc patching with sed should be avoided if possible because it's
    difficult to understand and is prone to silent breakage" (ghostscript:
    avoid neon on unsupported machines, 2023-09)
  - "Can we do this sed operation as a patch and apply it unconditionally"
    (alsa-lib: enabling ptest for alsa-lib component, 2024-10)
  - "Can't you just pass BINDIR=${base_sbindir} ... instead of patching the
    makefile?" (mdadm: fix do_package failed, 2019-09)

### BUILD-4 Anchor patterns so they match only what is meant

- **Severity:** warning
- **Trigger:** a new `sed`, `grep` or `UPSTREAM_CHECK_*REGEX` pattern that
  can match more than intended: no end-of-word or end-of-line anchor
  (`ttyS1` also matches `ttyS10`), a `+` quantifier on a literal version
  digit, a `python[0-9.]*` that also rewrites correct `python3` shebangs.
- **Problem:** the wrong lines are edited or deleted, or the version check
  picks the wrong release.
- **Fix:** anchor with `^`, `$`, `\b` or explicit separators; match exactly
  the token that must change.
- **Do not report when:** the input is known to hold only the intended
  matches and the patch shows it.
- **Evidence:**
  - "This doesn't check for the end of a word, so it'd match a partial as
    well." (sysvinit-inittab: fix getty device removal, 2016-11)
  - "Change "1+" to "1" or you will match, e.g., "11" and "111"."
    (babeltrace: only check latest git tag version for 1.x.x, 2020-03)
  - "An anchored expression would be better." (kernel-devsrc: fix incorrect
    python shebang replacement, 2025-10)
