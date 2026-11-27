<!-- select: .bb .bbappend .inc .bbclass DEPENDS inherit -->
# Rules: Build and Runtime Dependencies

Applies to: `.bb`, `.bbappend`, `.inc` hunks that change `DEPENDS`,
`RDEPENDS`, `RRECOMMENDS` or `inherit`, especially with `pkgconfig`,
`python*`, `gettext`, `cmake`, `glibc*`, `libtirpc`, `libxcrypt` or `bash`.

General rules (native vs target, shlibs, `PACKAGECONFIG` for optional
dependencies) are in `patterns/packaging.md`.

### DEP-1 Build tools: use the -native recipe, and the class where one exists

- **Severity:** error when a target recipe is used for a build-host tool;
  warning when a class should replace the DEPENDS
- **Trigger:** `DEPENDS` gains `pkgconfig`, `pkgconfig-native`,
  `python3-setuptools` (target), `python3-native`, `gettext`,
  `gettext-native`, or `cmake-native`, or a recipe exports its own
  `PYTHON=` path to the native interpreter.
- **Problem:** a target package does not provide a tool that runs on the
  build host. Hand-written native dependencies miss what the class also
  sets up (environment, `USE_NLS` handling, the minimal gettext), and full
  `gettext` is slow to build.
- **Fix:** `inherit pkgconfig`, `inherit python3native`,
  `inherit setuptools3` (or the matching Python build class),
  `inherit gettext`, `inherit cmake`; otherwise depend on the `-native`
  recipe.
- **Do not report when:** a target `python3` is needed to build against
  libpython (C extensions, embedding); the recipe needs gettext programs the
  class does not provide and says so. Do not report a dependency that is
  merely redundant with an inherited class as more than low priority.
- **Evidence:**
  - "If setuptools is a build dependency then surely you mean
    python3-setuptools-native." (python3-pygobject: move python3-setuptools
    from RDEPENDS to DEPENDS, 2019-08)
  - "Would 'inherit gettext' do the trick here? Depending on full gettext is
    undesired as it is slow to build" (core: recipes: fix some native tools
    dependencies, 2020-01)
  - "I think inherit pkgconfig should be the way to go here"
    (netcat-openbsd: fix implicit dependency on pkg-config, 2017-02)

### DEP-2 No Python 2

- **Severity:** error (Python 2 recipes are gone from OE-Core, as of 2020)
- **Trigger:** `inherit pythonnative`, `DEPENDS` on `python-native` or
  `python`, or new code paths for Python 2.
- **Fix:** `inherit python3native` / depend on `python3-native`; drop Python
  2 support.
- **Do not report when:** the line is on an old stable branch that still
  ships Python 2 (check the target branch).
- **Evidence:**
  - "this would need py2 and we do not have py2 anymore in core" (Add
    nbdkit, 2020-04)
  - "drop the support for python2, this is long gone." (opencv: upgrade
    4.8.0 -> 4.9.0, 2024-02)

### DEP-3 Do not hard-code glibc; use the virtual provider

- **Severity:** error (musl builds fail)
- **Trigger:** `RDEPENDS`/`DEPENDS` naming `glibc`, `glibc-dev`,
  `glibc-utils`, `glibc-gconv-*` or `locale-base-*` without a
  `:libc-glibc` override; a direct dependency on `libtirpc` or `libxcrypt`
  where a virtual exists.
- **Problem:** those packages do not exist with musl, so the dependency
  fails there.
- **Fix:** put glibc-only runtime dependencies under
  `RDEPENDS:${PN}:append:libc-glibc = " ..."`; depend on `virtual/librpc`,
  `virtual/crypt`, `virtual/libiconv`; for an on-target toolchain use
  `packagegroup-core-standalone-sdk-target` rather than picking parts.
- **Do not report when:** the recipe is glibc-only (`COMPATIBLE_HOST` or a
  libc override excludes musl), or the line already carries a libc override.
- **Evidence:**
  - "Using glibc here breaks musl builds." (coreutils: add ptest, 2019-11)
  - "you perhaps want virtual/crypt instead of libxcrypt, secondly it fails
    to build with musl" (minifi: Add recipe for c++ version, 2020-08)
  - "the dependency has a `PROVIDES = "virtual/librpc"`, so I think
    depending on virtual/librpc should be the safest way" (ltp: fix rpc
    build error, 2020-12)

### DEP-4 Do not make a main package depend on bash

- **Severity:** warning
- **Trigger:** `bash` added to `RDEPENDS:${PN}` (the main package), or to a
  `-ptest` package when no shipped script needs bash.
- **Problem:** small images use BusyBox ash; pulling in bash for one script
  grows every image that installs the package.
- **Fix:** make the script POSIX `sh` (see `patterns/shell-tasks.md`), or
  move the bash script into its own subpackage that depends on bash.
- **Do not report when:** the shipped script really uses bash features and
  is in its own subpackage; a ptest script uses bash and the dependency is
  on `${PN}-ptest`.
- **Evidence:**
  - "We do not want iptables to depend on bash. Either modify the
    iptables-apply script ... or move it ... to a separate package"
    (iptables: fix invalid symbolic link for ip6tables-apply, 2020-06)
  - "Is bash needed or would busybox' ash be enough?" (postgresql: Allow
    successful run of postgresql-setup, 2018-11)
  - "I don't see any scripts that use bash" (libcheck: add ptest support,
    2025-11)
