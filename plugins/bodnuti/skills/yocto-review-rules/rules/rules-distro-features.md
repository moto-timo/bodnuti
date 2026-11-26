<!-- select: .bb .bbappend .inc .bbclass DISTRO_FEATURES systemd -->
# Rules: DISTRO_FEATURES and systemd

Applies to: `.bb`, `.bbappend`, `.inc` hunks that add `systemd`, `polkit`,
`pam`, X11, Wayland or OpenGL recipes to `DEPENDS`/`RDEPENDS` or a
packagegroup, set `REQUIRED_DISTRO_FEATURES`, install `.service` files, or
test `VIRTUAL-RUNTIME_init_manager`.

### DISTRO-1 A hard dependency on a feature-gated recipe needs features_check

- **Severity:** error (world builds and distros without the feature fail
  with "Nothing PROVIDES ...")
- **Trigger:** a recipe unconditionally depends (`DEPENDS`, `RDEPENDS`, or a
  packagegroup's `RDEPENDS`) on a recipe that only builds with a distro
  feature, such as `systemd` or `polkit`, and has no matching
  `REQUIRED_DISTRO_FEATURES`; or it sets `REQUIRED_DISTRO_FEATURES` without
  `inherit features_check` (`distro_features_check` before 2020).
- **Problem:** in a distro without the feature the dependency is skipped,
  so this recipe fails to parse or build instead of being skipped cleanly.
  `REQUIRED_DISTRO_FEATURES` alone does nothing without the class.
- **Fix:** `inherit features_check` and
  `REQUIRED_DISTRO_FEATURES = "systemd"`, or move the dependency into a
  `PACKAGECONFIG` knob whose default follows `DISTRO_FEATURES`, or wrap it
  in `bb.utils.contains('DISTRO_FEATURES', ...)`.
- **Do not report when:** the recipe, its `.inc` or a class already sets
  `REQUIRED_DISTRO_FEATURES` / `ANY_OF_DISTRO_FEATURES` with the class
  inherited; the dependency is inside a `PACKAGECONFIG` entry or a
  `bb.utils.contains` on `DISTRO_FEATURES`; the dependency is not actually
  feature-gated (check the depended-on recipe before reporting).
- **Evidence:**
  - "We have to also check for systemd in REQUIRED_DISTRO_FEATURE or else it
    can cause errors" (python3-pystemd: add new recipe, 2019-08)
  - "depending on systemd is allowed only with REQUIRED_DISTRO_FEATURES =
    "systemd"" (mtd-utils: add optional systemd service for ubihealthd,
    2025-07)
  - "the above IIRC doesn't work without distro_features_check inherit"
    (gstreamer1.0-python: rework gobject-introspection handling, 2019-04)

### DISTRO-2 systemd support follows DISTRO_FEATURES, best via the systemd class

- **Severity:** warning
- **Trigger:** a recipe that does not `inherit systemd` installs unit files
  unconditionally in `do_install`; or systemd configure options are chosen
  by testing `VIRTUAL-RUNTIME_init_manager` instead of `DISTRO_FEATURES`.
- **Problem:** distros without systemd get unit files and systemd-enabled
  binaries they cannot use, or the options disagree with what the distro
  supports.
- **Fix:** `inherit systemd` with `SYSTEMD_SERVICE:${PN}`, which handles
  installation per distro; key configure options and the matching
  `PACKAGECONFIG` default on `bb.utils.contains('DISTRO_FEATURES',
  'systemd', ...)`.
- **Do not report when:** the recipe inherits `systemd` (the class handles
  distros without systemd), or the install is already wrapped in a
  `DISTRO_FEATURES` test.
- **Evidence:**
  - "we should be checking for systemd in DISTRO_FEATURES here." (open-lldp:
    Add initial version 1.0.1, 2020-02)
  - "it should inherit systemd and then conditionally enable this
    packageconfig if systemd is in DISTRO_FEATURES" (ktls-utils: Initial
    commit, 2025-04)
