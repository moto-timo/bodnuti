<!-- select: .bbclass -->
Applies to: `.bbclass` files (`classes/`, `classes-recipe/`, `classes-global/`); CL5 also to new variables read in `meta/lib/`.

# Classes

A class runs for every recipe that inherits it, in every configuration.
Read `patterns/overrides.md`, `patterns/shell-tasks.md` and
`patterns/python-metadata.md` first; bashisms, `getVar` returning `None` and
callers of a changed helper are covered there. Python code inside a class
is also subject to `rules-oe-python.md`.

## CL1. Hard-coded library or prefix paths

- **Severity:** warning (an error for multilib, `usrmerge` or 64-bit
  `lib64` builds the change reaches)
- **Trigger:** a class adds a literal `/usr/lib`, `/lib`, `/usr/lib64`,
  `/usr/bin` or `/usr/share` to a path built under `${D}`, a sysroot
  (`RECIPE_SYSROOT`, `RECIPE_SYSROOT_NATIVE`, `STAGING_DIR_*`) or `IMAGE_ROOTFS`.
- **Problem:** the real directory depends on `libdir`, `baselib`,
  multilib and `usrmerge`. The literal path misses files in other layouts.
- **Fix:** use the path variables: `${libdir}`, `${base_libdir}`,
  `${nonarch_libdir}`, `${bindir}`, `${datadir}`, or the native/staging
  equivalents.
- **Do not report when:** the path is fixed by the target ABI or another
  standard (`/proc`, `/sys`, `/dev`, `/etc`, the dynamic loader path); the
  code deliberately uses the architecture-independent directory and says
  so; the literal is in a comment.
- **Evidence:**
  - "Also, can we avoid hardcoding /usr/lib? That probably won't work for multilibs" — python_hatchling: remove empty python sysroot dirs (2023-05)
  - "should this be ${nonarch_libdir}, or does it use /usr/lib64 or /usr/libx32 correctly?" — npm: change install directory (2018-10)

## CL2. Override syntax used where it does not apply

- **Severity:** error
- **Trigger:** either
  - an override on a variable flag: `do_task:class-target[vardeps] += ...`,
    `VAR:append[doc]`, `VAR:arm[depends]`; or
  - `:` used as a plain separator in a new variable name that is not an
    override, typically keyed by data such as a file name:
    `d.getVar(f"FIT_DTB_COMPATIBLE:{dtb_name}")`.
- **Problem:** as of 2022, BitBake does not support overrides on flags;
  at best the flag lands on a name nothing reads and has no effect. `:` in a name is
  parsed as override syntax; a name keyed by arbitrary data is not one.
- **Fix:** set the flag on the plain variable, inside a condition if it
  must vary (an anonymous Python block or `${@...}`). Use `_` or a varflag
  (`VAR[key]`) to key a variable by data.
- **Do not report when:** the suffix is a real override or package name:
  `RDEPENDS:${PN}`, `FILES:%s` % pkg, `d.getVar("SPDX_IMAGE_PURPOSE:%s" %
  imagetype)` where the class documents per-type values the same way;
  the flag is set on an override-free name.
- **Evidence:**
  - "You can't use overrides with flags like that, I'm surprised that even parses!" — meson.bbclass: add exe_wrapper helper (2022-03)
  - ": is used for bitbake overrides, and this is not one of them." — fitimage: custom compatible string (2025-09)

## CL3. Dependency for an optional feature added unconditionally

- **Severity:** warning
- **Trigger:** a class adds a cross-recipe task dependency
  (`do_x[depends] += "recipe:do_y"`, `d.appendVarFlag('do_x', 'depends', ...)`,
  `[recrdeptask]`) for a feature that is only sometimes enabled (signing,
  key generation, SPDX, a `MACHINE_FEATURES` or `DISTRO_FEATURES` item),
  without checking that the feature is on.
- **Problem:** builds with the feature off still pull in, or fail on,
  the dependency; it can create dependency loops.
- **Fix:** add the dependency only when the controlling variable is set
  (check it in anonymous Python, or add it from the class that implements
  the feature).
- **Do not report when:** the dependency sits inside a check of the
  controlling variable already; the task always needs the dependency.
- **Evidence:**
  - "This should also check for FIT_GENERATE_KEYS=1 before adding the dependency, right ?" — u-boot: ensure keys are generated before FIT image (2025-05)
  - "The SPDX dependencies are generally added in creates-spdx-sdk-3.0.bbclass so as not to cause problems when SPDX is disabled." — populate_sdk_ext: SPDX tasks (2025-10)

## CL4. Acting on a path that may not exist

- **Severity:** warning (an error when the task fails for a common image or
  configuration)
- **Trigger:** class code removes, moves, copies, touches or opens a path
  that comes from a user-set variable or an optional output (an exclude
  list, a multiconfig file, a `.siginfo`, a kernel image variant) without
  checking it exists; or the patch drops an existing `[ -e ... ] ||` or
  `os.path.exists` guard.
- **Problem:** the task fails with an exception or a shell error for
  configurations where the path is absent.
- **Fix:** check for the path first, and skip or warn when it is missing,
  unless its absence is a real error that deserves a clear `bb.fatal`.
- **Do not report when:** the same task created the path, or the task's
  `[dirs]`/`[cleandirs]` guarantees it; a missing file is meant to be fatal
  and the message says so; the command already tolerates absence
  (`rm -f`, `mkdir -p`, `bb.utils.remove`).
- **Evidence:**
  - "We need to check if full_path exists here, else it may try modifying things that don't exist." — do_image: IMAGE_ROOTFS_EXCLUDE_PATH (2018-01)
  - "was there some reason you removed the test for if the siginfo file exists before touching it?" — sstate: touching files inside pseudo (2021-11)

## CL5. A new variable that duplicates an existing one

- **Severity:** warning
- **Trigger:** a class (or `meta/lib` code) introduces a new user-facing
  variable (`NEW_VAR ?= ...`, `d.getVar("NEW_VAR")`) whose job an existing
  variable or feature list already does: an existing `*_EXCLUDE*` or
  `*_IGNORE*` list, `MACHINE_FEATURES`, `DISTRO_FEATURES`, `IMAGE_FEATURES`.
- **Problem:** maintainers reject parallel knobs for one behaviour; they
  have to be documented and kept in sync forever.
- **Fix:** use the existing variable, or explain in the commit message why
  it cannot be used.
- **Do not report when:** you cannot name the existing variable and show
  it covers the same case; the new variable is internal to the class and
  not meant to be set by users.
- **Evidence:**
  - "Why do you need a new variable? Isn't this what OEQA_REPRODUCIBLE_EXCLUDED_PACKAGES is used for?" — reproducible: per-layer non-repro packages (2025-06)
  - "Apart from the lack of documentation, why not just look at the MACHINE_FEATURES?" — image-live.bbclass: enable/disable isohybrid (2025-06)
