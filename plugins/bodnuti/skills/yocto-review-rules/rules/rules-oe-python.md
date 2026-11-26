<!-- select: .py -->
Applies to: Python in `meta/lib/` and `scripts/` (`.py` files and Python scripts such as devtool, wic, oe-selftest), and Python code inside `.bbclass`, `.bb` and `.inc` files (`python do_x()`, `def` functions, anonymous `python ()`, `${@...}`).

# Python in OE

Read `patterns/python-metadata.md` first: it covers `getVar` returning
`None`, parse-time exceptions and finding callers of a changed helper.

## PY1. A boolean variable tested as a plain string

- **Severity:** error for a truthiness test (the feature is always on);
  warning for a string comparison
- **Trigger:** a variable meant as a yes/no switch (`*_ENABLE`, `*_ADD_*`,
  a documented `"0"`/`"1"` value) read with `d.getVar()` and then used
  directly in `if`, `and`, `or`, or compared to one literal
  (`== "1"`, `== "yes"`).
- **Problem:** `"0"` and `"no"` are non-empty strings, so a truthiness test
  treats them as true. A comparison to `"1"` rejects `"yes"` and `"true"`.
- **Fix:** `bb.utils.to_boolean(d.getVar("X"))` or
  `oe.types.boolean(d.getVar("X"))`; in shell, compare against the value
  the variable is documented to take.
- **Do not report when:** the variable's meaning is "set to any value"
  (empty means off) and that is how it is documented; the comparison
  matches how the same variable is already tested elsewhere in OE-Core and
  the patch does not change that.
- **Evidence:**
  - "I think something like bb.utils.to_boolean() should be used, otherwise the non-empty string will always evaluate as True." — kernel-fit-image: compatible string for built-in DTBs (2025-10)
  - "May I suggest: have_mirror_tarballs = oe.types.boolean(d.getVar('BB_GENERATE_MIRROR_TARBALLS')) instead?" — archiver.bbclass: fix BB_GENERATE_MIRROR_TARBALLS checking (2024-08)

## PY2. Substring match where a prefix, suffix or exact match is meant

- **Severity:** warning
- **Trigger:** classifying names or paths with `"x" in name`,
  `str.replace()` or an unanchored `re.sub()`/`re.search()`, where the
  intent is a suffix (`-native`, a file extension), a prefix (a parent
  directory) or a whole word. For example `"-native" in recipe_name`,
  `re.sub("dtbo?", "dts", f)`, `srcdirname in fname`.
- **Problem:** the test also matches names that contain the text in the
  middle (`foo-native-bar`, `test-dtbo.dts`, any path that mentions the
  directory name), giving wrong results for some recipes.
- **Fix:** `endswith()`, `startswith()` on an absolute path,
  `os.path.commonpath`, an anchored regex (`r"\.dtbo?$"`), or a split into
  words.
- **Do not report when:** the code really means "contains"; the value is
  already a single known token; the regex is anchored.
- **Evidence:**
  - "I would have expected if dep.recipe.name.endswith("-native"): here. Otherwise this is very prone to false positives" — create-spdx: Don't collect natives sources (2021-09)
  - "it will substitute any part of the filename and not just the extension suffix. E.g. "test-dtbo.dts" -> "test-dts.dts"." — devicetree.bbclass: selection of dts files (2023-04)

## PY3. Shell command strings where a list or direct file I/O works

- **Severity:** warning
- **Trigger:** new code that runs a command through a shell string with
  interpolated values (`os.system`, `os.popen`, `subprocess.*(...,
  shell=True)`, `oe.utils.getstatusoutput`, a selftest `runCmd("echo %s >> %s")`),
  a hand-written wrapper around `subprocess.Popen` that redoes what
  `subprocess.run()`/`check_output()` does, or `echo` with `\n` or other
  escapes in such a string.
- **Problem:** values with spaces, quotes or newlines break the command;
  `echo` escape handling differs between shells; reimplemented wrappers
  drift from the standard library's error handling.
- **Fix:** `subprocess.run([...], check=True)` or `subprocess.check_output([...], cwd=...)`
  with an argument list; write files with `open()`; use `printf` if a
  shell is unavoidable.
- **Do not report when:** the command needs a pipeline or redirection and
  every interpolated value is constant or passed through `shlex.quote`;
  the call is the selftest `bitbake()` helper; the call site is unchanged
  by the patch.
- **Evidence:**
  - "These two functions are basically a bad reimplementation of subproccess.run(). Don't copy-paste, just use modern API." — image-live: EFI-bootable ISO images (2023-11)
  - "This causes some shell's echos to output a literal \n (two characters, '\' and 'n'). The solution should be to use printf" — wic: Fix cp's target path for ext* (2019-02)
