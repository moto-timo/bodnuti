<!-- select: .bb .bbappend .inc PACKAGECONFIG -->
# Rules: PACKAGECONFIG

Applies to: `.bb`, `.bbappend`, `.inc` hunks that touch `PACKAGECONFIG`,
`PACKAGECONFIG[...]`, `:remove` on any variable, or that add a dependency or
an `--enable-*` / `--with-*` / `-D...` option to `DEPENDS`, `EXTRA_OECONF`,
`EXTRA_OEMESON` or `EXTRA_OECMAKE`.

A `PACKAGECONFIG[name]` entry has up to six comma-separated fields, in this
order: enable option, disable option, `DEPENDS`, `RDEPENDS`, `RRECOMMENDS`,
conflicting knobs.

### PKGCFG-1 Recipe defaults must stay overridable

- **Severity:** warning
- **Trigger:** in a `.bb` or `.inc` (not a `.bbappend`, distro or local
  config): `PACKAGECONFIG = "..."`, `PACKAGECONFIG += "..."`,
  `PACKAGECONFIG:append = "..."` used to set the recipe's own defaults, or
  any `VAR:remove = "..."` added to a recipe or OE-Core class.
- **Problem:** users, distros and bbappends change defaults by assigning
  `PACKAGECONFIG`. A hard `=` or `:append` in the recipe makes a feature hard
  to switch off, and `:remove` cannot be undone by any later assignment.
- **Fix:** set defaults with `PACKAGECONFIG ??= "..."`. Instead of removing
  a word, avoid adding it (build the value conditionally).
- **Do not report when:** the line is in a `.bbappend`, distro, machine or
  local configuration (that is where `:append`/`:remove` belong); or it is
  `PACKAGECONFIG:append:class-target = " ..."` / `:class-native` adding
  per-class defaults on top of a `??=` default, which maintainers accept.
- **Evidence:**
  - "Never use PACKAGECONFIG_append in a recipe as then they can't easily be
    disabled." (apr-util: add ldap crypto and sqlite3 to PACKAGECONFIG,
    2016-01)
  - "Remove is generally a bad idea as its very hard to counteract."
    (qemu-system-native: disable options not included in extended tarball,
    2020-03)

### PKGCFG-2 Each knob passes both an enable and a disable option

- **Severity:** warning
- **Trigger:** a new or changed `PACKAGECONFIG[x]` whose first or second
  field is empty while the build system has the matching opposite option
  (`--with-x` / `--without-x`, `--enable-x` / `--disable-x`,
  `-Dx=enabled` / `-Dx=disabled`, `-DWITH_X=ON` / `OFF`).
- **Problem:** with one side empty, the build system auto-detects. A
  feature can be quietly enabled because a dependency happens to be in the
  sysroot, and an upstream removal of the option goes unnoticed.
- **Fix:** fill both fields, e.g.
  `PACKAGECONFIG[ldap] = "--with-ldap,--without-ldap,openldap"`.
- **Do not report when:** upstream has no option for that side (the knob
  only adds dependencies, or the feature is controlled elsewhere); confirm
  from the build files or the commit message before reporting.
- **Evidence:**
  - "This needs a negative condition as well, otherwise upstream defaults
    may result in an incorrect configuration" (mesa: enable imagination
    powervr support, 2024-03)
  - "If upstream decides to remove support for any of these, we'll be able
    to catch that." (file: add PACKAGECONFIG for auto options, 2020-05)

### PKGCFG-3 Fields in the wrong position

- **Severity:** error
- **Trigger:** a `PACKAGECONFIG[x]` value where the fields do not match
  their meaning: the same option in both the enable and disable field, an
  extra comma that shifts every later field, or a build dependency in the
  fourth (`RDEPENDS`) field.
- **Problem:** the knob does nothing, or always does the same thing, or its
  library ends up a runtime dependency instead of a build dependency, so the
  build fails or the feature is silently off.
- **Fix:** count the commas; use the order enable, disable, DEPENDS,
  RDEPENDS, RRECOMMENDS, conflicts.
- **Do not report when:** the dependency in field four is genuinely runtime
  only (a plugin or tool called at run time, not linked).
- **Evidence:**
  - "You've got "pcre2=true" in both arms of the PACKAGECONFIG." (Systemd:
    Add pattern matching support to PACKAGECONFIG, 2019-03)
  - "Specifying gtk+3 after two ,,s seems weird." (libnotify: Make gtk+3
    dependency optional, 2021-05)
  - "There is a syntax problem where the comma after --enable-tests is not
    required" (proftpd: Update to 1.3.7a release, 2021-04)

### PKGCFG-4 Optional features go in PACKAGECONFIG, not hard DEPENDS or options

- **Severity:** warning
- **Trigger:** a patch to an existing recipe adds an optional library to
  `DEPENDS` together with the configure option that turns it on, or adds a
  fixed `--enable-x` / `-Dx=true` for an optional component to
  `EXTRA_OECONF` / `EXTRA_OEMESON` / `EXTRA_OECMAKE`, or makes a dependency
  only needed in some configurations unconditional.
- **Problem:** every user now builds and ships the feature and its
  dependency chain, which changes existing setups and cannot be turned off
  without a bbappend.
- **Fix:** add `PACKAGECONFIG[x] = "--enable-x,--disable-x,libx"` and, if
  the old behaviour should stay, leave it out of the default (or add it to
  `PACKAGECONFIG ??=` when it should be on by default).
- **Do not report when:** the dependency is mandatory (configure fails
  without it); the option fixes a build or turns off something broken; an
  existing `PACKAGECONFIG` knob already controls it; the commit message
  explains why the feature must always be on.
- **Evidence:**
  - "Can we have that as a PACKAGECONFIG entry, please? otherwise I think it
    will change the behavior of the existing setups." (ltp: fix rpc build
    error, 2020-12)
  - "This has to be conditional, it is only required in certain
    configurations." (linux: Modify kernel configuration to fix runqlat
    issue, 2024-07)
