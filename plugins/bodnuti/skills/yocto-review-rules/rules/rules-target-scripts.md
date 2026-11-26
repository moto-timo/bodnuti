<!-- select: .sh .service path=run-ptest path=init -->
Applies to: scripts and unit files a recipe ships to the target from its `files/` or `<recipe>/` directory: `run-ptest`, init scripts, udev and initramfs helpers, `.service` files, other shell scripts installed by `do_install`.

# Scripts Shipped to the Target

Bashisms and POSIX shell rules are in `patterns/shell-tasks.md`; they apply
here, since `/bin/sh` on the target is often BusyBox ash.

## TS1. Hard-coded library directories

- **Severity:** warning (runtime breakage for multilib, `lib64` or
  `usrmerge` builds)
- **Trigger:** a literal `/usr/lib/...`, `/lib/...` or `/usr/libexec/...`
  in a shipped script or unit file, including `cd /usr/lib/<pn>/ptest` in
  `run-ptest`.
- **Problem:** `${libdir}` and `${base_libdir}` differ by architecture,
  multilib and `usrmerge`; the script then looks in the wrong place.
- **Fix:** write a placeholder (`@libdir@`, `@base_libdir@`) and replace it
  with `sed` in `do_install`, or use paths relative to where the script
  runs. `ptest-runner` already starts `run-ptest` in its ptest directory.
- **Do not report when:** the recipe's `do_install` already substitutes
  that literal; the path is fixed regardless of layout (`/proc`, `/sys`,
  `/dev`, `/etc`, `/run`, `/var`).
- **Evidence:**
  - "hardcoding /usr/lib might not work in all cases e.g. where baselib is lib64" — c-ares: add ptest (2023-12)
  - "replace all hardcoded paths with something like @base_libdir@ and then use sed to apply all the relevant bitbake path variables." — udev-extraconf: systemd-mount (2018-04)

## TS2. run-ptest that can hide failures

- **Severity:** warning
- **Trigger:** in `run-ptest`:
  - `PASS:` printed unconditionally, or without checking any test ran;
  - a test command piped into `tee`, `sed` or `grep` and the result
    judged by the pipe's exit status (plain `sh` has no `pipefail`);
  - PASS/FAIL derived only from parsing the test's output instead of its
    exit status;
  - all output sent to `/dev/null`, so failures leave no log.
- **Problem:** a failing or empty test suite reports success, and
  failures cannot be diagnosed.
- **Fix:** take PASS/FAIL from each test's exit status, print `FAIL:` when
  it is non-zero, keep the output, and fail if no tests ran.
- **Do not report when:** the exit status is captured separately (saved
  before the pipe, or `$?` of the test itself); the upstream harness
  already prints automake-style `PASS:`/`FAIL:` lines from exit codes.
- **Evidence:**
  - "If any of $t fails but prints nothing, will this be reported as a failure?" — icu: use automake ptest output format (2025-10)
  - "Should the output be completely discarded like this? Maybe it's better to have it printed, especially if something fails?" — json-c: Add ptest (2022-05)

## TS3. Values that go stale on the next upgrade

- **Severity:** warning
- **Trigger:** a shipped script with a hard-coded upstream version string
  (`"Testsuite summary for yaml 0.2.5"`), or a `run-ptest` with a fixed
  list of test names that repeats a list kept elsewhere (`PTEST_TESTS`)
  or that could be found from the installed test directory.
- **Problem:** upgrades do not update the copy; the version goes wrong
  and new tests are silently never run.
- **Fix:** drop the version or generate it from `${PV}` in `do_install`;
  run every test in the installed test directory, or use the one list.
- **Do not report when:** the list deliberately leaves out tests and says
  why; the tests cannot be discovered from the installed files.
- **Evidence:**
  - "This hard-coded "0.2.5" version will most likely *not* be updated on future upgrades." — libyaml: add ptest support (2025-11)
  - "Also run-ptest should not hardcode the list of tests" — libsdl2: tests package and ptest support (2025-08)
