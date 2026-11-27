<!-- select: .bb .bbappend .inc .bbclass .patch .diff -->
# Rules: BitBake Behaviour That Is Not a Defect

Applies to: any recipe, class or carried patch. Unlike the other files,
these are not maintainers' objections: they are how BitBake and the OE
build work, written down because models reviewing the patches of
26 November 2026 reported each of them as a defect. Every one was
false. Check a finding against them before reporting it.

### BB-1 Local files in SRC_URI are found, applied and need no checksum

- **Not a defect:** a `file://` entry with no `do_patch` code, no
  `SRC_URI[sha256sum]`, or no proof in the patch that the file exists.
- **Why:** `do_patch` applies every `file://*.patch` and `*.diff` in
  `SRC_URI` by itself, unless the entry says `apply=no`. Checksums are
  only for remote fetches (`http://`, `https://`, `ftp://`); `file://`
  needs none. The fetcher looks in `FILESPATH` (the recipe directory,
  its `${BPN}`, `${BP}` and `files/` subdirectories), so a file there
  needs no other mention.
- **Report only when:** the series adds the `SRC_URI` entry but no file
  of that name, and none exists in the tree; or the file name in
  `SRC_URI` differs from the one the series adds.
- Seen as false findings: inetutils CVE patch "no instruction to apply
  it", sqlite3 CVE patch "no SRC_URI[sha256sum]", apt `triehash` "not
  shown to exist".

### BB-2 Variables from bitbake.conf and the classes are always set

- **Not a defect:** `TARGET_SYS`, `HOST_SYS`, `BUILD_SYS`, `TARGET_ARCH`,
  `S`, `B`, `D`, `WORKDIR`, `STAGING_DIR_HOST`, `bindir`, `libdir` and the
  like used in a recipe that does not set them.
- **Why:** `meta/conf/bitbake.conf` defines them for every recipe;
  classes such as `autotools`, `cmake`, `meson` and `cargo` define
  their own. The documentation item in the request names the variables
  the patch uses.
- **Report only when:** the variable is not in `bitbake.conf`, the
  glossary, an inherited class or an included file, and nothing in the
  layer sets it.
- Seen as a false finding: pseudo "`TARGET_SYS` is not defined".

### BB-3 Inline Python and prefuncs work in shell tasks

- **Not a defect:** `${@...}` inside a shell function or task, or a
  Python function listed in `[prefuncs]` or `[postfuncs]`.
- **Why:** BitBake expands `${@...}` and `${VAR}` before the shell runs
  the function, so the shell sees the result. `[prefuncs]` and
  `[postfuncs]` run Python and shell functions alike.
- **Report only when:** the Python expression itself is wrong, or uses a
  shell variable (`$foo`, set at run time) that BitBake cannot know.
- Seen as false findings: xen-rpi-u-boot-scr `${@oe.kernel.map_uboot_arch(d)}`
  "not expanded in shell", ptest-cargo "a python function is not run as
  a postfunc".

### BB-4 PACKAGECONFIG carries its own dependencies

- **Not a defect:** enabling a `PACKAGECONFIG` option with no matching
  `DEPENDS` line.
- **Why:** the third field of `PACKAGECONFIG[name] = "enable,disable,
  depends,rdepends,rrecommends,conflicts"` is added to `DEPENDS` when the
  option is on.
- **Report only when:** the `PACKAGECONFIG[name]` line has no depends
  field and the option needs a library from another recipe.
- Seen as a false finding: gettext enabling `libunistring` for musl;
  `PACKAGECONFIG[libunistring]` already lists `libunistring`.

### BB-5 A carried patch is a diff inside a diff

- **Not a defect:** a line in a `.patch` file that seems to lack its
  `-` or `+`, or to add a duplicate.
- **Why:** in a series that adds or changes a `.patch` file, each line
  has two prefix columns: the first is the series' own (`+` added to the
  `.patch` file, `-` removed from it, a space unchanged), the second the
  carried patch's (`+` it adds, `-` it removes, a space for context). So
  `+-requires = ...` is a removal the series adds to the carried patch,
  and ` +requires = ...` an addition the carried patch already made.
- **Report only when:** reading both columns, the carried patch's hunk
  is still wrong: its counts do not match, or the result is broken.
- Seen as a false finding: charset-normalizer pyproject patch "adds a
  second `requires` key"; the carried patch replaces the line.
