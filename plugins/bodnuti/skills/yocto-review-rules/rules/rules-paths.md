<!-- select: .bb .bbappend .inc .bbclass /usr /etc /lib /bin STAGING -->
# Rules: Paths in Recipes

Applies to: `.bb`, `.bbappend`, `.inc` hunks with `do_install`, `FILES`,
`EXTRA_OE*`, `export`, `*FLAGS`, or `sed`/`install`/`mv`/`ln` commands that
contain a literal `/usr`, `/etc`, `/var`, `/lib`, `/bin`, `/sbin`, or refer
to `${D}`, `${WORKDIR}`, `STAGING_*` or `*_native` variables.

### PATH-1 Use the path variables, not literal FHS paths

- **Severity:** warning (wrong with usrmerge, multilib, or a distro that
  changes the paths)
- **Trigger:** a literal path in an install command, `FILES`, or an option
  passed to the build, where a variable exists:

  | Literal | Variable |
  |---|---|
  | `/usr` | `${prefix}` |
  | `/usr/bin`, `/usr/sbin`, `/bin`, `/sbin` | `${bindir}`, `${sbindir}`, `${base_bindir}`, `${base_sbindir}` |
  | `/usr/lib`, `/lib` | `${libdir}`, `${base_libdir}`; `${nonarch_libdir}` / `${nonarch_base_libdir}` for arch-independent `lib` paths (firmware, udev, tmpfiles.d) |
  | `/usr/include` | `${includedir}` |
  | `/usr/share` | `${datadir}` |
  | `/etc` | `${sysconfdir}` |
  | `/var` | `${localstatedir}` |
  | `${systemd_unitdir}/system` | `${systemd_system_unitdir}` |
  | `${WORKDIR}/build`, `${S}/../build` | `${B}` |

- **Fix:** replace the literal with the variable, in `FILES` and in
  `${D}${...}` install paths.
- **Do not report when:** the literal is part of a `sed` match that must
  equal upstream's text; it is `/dev`, `/proc`, `/sys` or `/tmp`; it is in a
  comment; it is inside file content that a later `sed` rewrites. Report a
  literal path only when the patch adds it.
- **Evidence:**
  - "Don't hardcode /var here, use ${localstatedir}." (sudo: provide sudo
    paths through EXTRA_OECONF, 2017-12)
  - "Please use ${sysconfdir} here instead of hardcoding /etc"
    (dediprog-flasher: enable native builds, 2025-05)
  - "Replace ${systemd_unitdir}/system with ${systemd_system_unitdir}"
    (weston-init: Add possibility to run weston as non-root user, 2019-09)

### PATH-2 Point the build at the sysroot, not at ${D} or native paths

- **Severity:** warning
- **Trigger:** an option or export that tells the build where a dependency's
  headers or libraries are, written as `${RECIPE_SYSROOT}/usr`,
  `${STAGING_DIR_TARGET}/usr`, `${D}/...`, or, in a target recipe,
  `${libdir_native}` and other `*_native` variables.
- **Problem:** `/usr` is hard-coded; `${D}` is this recipe's own install
  tree, empty at compile time; `*_native` variables point at the build
  host's layout, not the target's.
- **Fix:** `${STAGING_EXECPREFIXDIR}`, `${STAGING_INCDIR}`,
  `${STAGING_LIBDIR}`, `${STAGING_DIR_TARGET}${prefix}`; or `inherit
  pkgconfig` and let the build find the dependency itself.
- **Do not report when:** `${D}` is used as the install destination
  (`DESTDIR=${D}` in `do_install`); the recipe is a native recipe using
  `*_native` variables.
- **Evidence:**
  - "Hardcoding /usr may not be best here. Can you figure out a bitbake
    variable to use here ?" (iperf3: Upgrade to 3.6, 2018-07)
  - "perhaps use STAGING_EXECPREFIXDIR" (libnet-ssleay-perl: Fix configure
    issue, 2018-08)
  - "Why libdir_native (_native suffix also in other variables) when this is
    target recipe?" (netdata: add new recipe, 2016-04)
