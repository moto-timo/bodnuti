<!-- select: .bb .bbappend .inc RDEPENDS RRECOMMENDS FILES INSANE_SKIP -->
# Rules: Assignments That Replace Instead of Add

Applies to: `.bb`, `.bbappend`, `.inc` hunks that assign `RDEPENDS`,
`RRECOMMENDS`, `FILES`, `INSANE_SKIP`, `DEPENDS`, `*FLAGS`, `EXTRA_OE*` or
any variable with an override suffix (`:class-native`, `:class-target`,
`:arm`, `:libc-musl`, `:${PN}-ptest`, ...).

Operator semantics are in `overrides.md`. These rules are the two mistakes
maintainers keep finding that follow from them.

### ASSIGN-1 `=` wipes values set by a class, an .inc or the defaults

- **Severity:** error for `RDEPENDS:${PN}-ptest` (the ptest package stops
  pulling in the main package); warning otherwise
- **Trigger:** the patch uses plain `=` on a variable that already has a
  value from somewhere else:
  - `RDEPENDS:${PN}-ptest = "..."` in a recipe that inherits `ptest`;
  - `RDEPENDS:${PN} = "..."` / `FILES:${PN} = "..."` in a recipe whose
    `.inc` or classes also set it, or that changes an existing `+=` to `=`;
  - `FILES:${PN} = "..."` or `FILES:${PN}-dev = "..."` (the defaults from
    `bitbake.conf` are replaced, so files they covered become unshipped);
  - `INSANE_SKIP = "..."` or `INSANE_SKIP:${PN} = "..."`.
- **Problem:** the earlier value is silently discarded: dependencies that a
  class or `.inc` added disappear, default packaging rules are lost.
- **Fix:** use `+=` (or `:append = " ..."` where ordering requires it).
- **Do not report when:** no class, `.inc`, `bitbake.conf` default or earlier
  line sets that variable for that package (for example `FILES:${PN}-tools`
  for a package the recipe itself adds to `PACKAGES`, or `RDEPENDS:${PN}` in
  a new standalone recipe whose classes do not set it). Check every
  `require`, `include` and `inherit` before reporting. Do not report a
  deliberate replacement the commit message or a comment explains.
- **Evidence:**
  - "Won't that need a += there to preserve the class definition?" (Remove
    manual RDEPENDS from PN-ptest to PN package, 2019-06)
  - "This should be "+=" - otherwise it won't install the main package when
    you build a ptest image" (python3-python-dotenv: new recipe, 2025-11)
  - "Other RDEPENDS are overwritten. So why not use '+=' ?" (bluez5: add dbus
    to RDEPENDS, 2022-10)

### ASSIGN-2 `VAR:override += "x"` replaces VAR, it does not add to it

- **Severity:** error when the base value carries needed items (for
  example `DEPENDS`, `TARGET_CPPFLAGS`); warning otherwise
- **Trigger:** `VAR:<override> += "..."` or `VAR:<override> = "..."` where
  the author's intent (commit message, the `+=`) is to add to `VAR` for that
  override: `DEPENDS:class-target += "..."`, `TARGET_CPPFLAGS:arm += "..."`,
  `RRECOMMENDS:${PN}:class-target += "..."`.
- **Problem:** `+=` appends to the separate variable `VAR:<override>`. When
  the override is active, that variable replaces `VAR` entirely, so the base
  value is lost for that configuration.
- **Fix:** `VAR:append:<override> = " x"` (note the leading space).
- **Do not report when:** replacing the whole value for that override is the
  intent (a complete per-class `PACKAGECONFIG:class-native = "..."`
  default), or `VAR:<override>` was already assigned a complete value
  earlier and the `+=` extends that.
- **Evidence:**
  - "The above doesn't += to TARGET_CPPFLAGS, it _replaces_ TARGET_CPPFLAGS.
    This is probably not what you want." (libucontext: use hard/soft float,
    2025-06)
  - "This should be DEPENDS:append:class-target = " libstd-rs"" (ripgrep:
    limit libstd-rs DEPENDS to class-target, 2025-12)
