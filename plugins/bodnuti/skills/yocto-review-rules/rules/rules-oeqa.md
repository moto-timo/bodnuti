<!-- select: path=oeqa -->
Applies to: tests under `meta/lib/oeqa/` (`selftest/cases/`, `runtime/cases/`, `sdk/`) and other oe-selftest test files.

# oe-selftest and OEQA Tests

Python rules in `rules-oe-python.md` also apply.

## QA1. Test leaves state behind or uses a fixed path

- **Severity:** warning
- **Trigger:** a test that creates a directory at a fixed path
  (`os.mkdir(".../fixed-name")`, a fixed path under `/tmp` or `TOPDIR`),
  or forces tasks to rerun (`bitbake -C`, `-f`) on shared recipes, with
  no cleanup.
- **Problem:** reruns and parallel test runs collide on the same path;
  tainted stamps change later tests' results.
- **Fix:** use `tempfile.mkdtemp(prefix=..., dir=<TOPDIR>)` and
  `self.track_for_cleanup()`; clean recipes whose stamps the test taints
  (`bitbake -c clean <recipe>`).
- **Do not report when:** the test uses `self.write_config()` or
  `self.append_config()` (restored by the test framework); cleanup happens
  in `tearDown`, `tearDownClass` or `track_for_cleanup`.
- **Evidence:**
  - "Instead of hoping for the best, can this just use mktempd(prefix='selftest-buildhistory', dir=get_bb_bar(TOPDIR))" — oelib/buildhistory.py: unittest for buildhistory_analysis (2017-02)
  - "You'll probably want to have a cleanup of the recipes to remove the tainted stamps" — selftest/archiver: invalidate stamps (2017-02)

## QA2. Test asserts on incidental details

- **Severity:** warning
- **Trigger:** an assertion on a full line of a tool's help or usage
  output, or a test that relies on how one specific recipe happens to be
  built (tarball versus git, a particular option) without saying so or
  checking it.
- **Problem:** the test breaks on unrelated changes (a new command-line
  option, a recipe moving to git) and blocks those changes.
- **Fix:** assert only on what the test is about (the exit code, the
  `usage: <tool>` prefix); state and check the recipe property the test
  depends on, with a comment.
- **Do not report when:** the test exists to check that exact output or
  that recipe property.
- **Evidence:**
  - "I'd recommend just checking for "usage: oe-pkgdata-util" and ignoring the rest of the line." — oe-pkgdata-util: test without parameters (2020-12)
  - "If something changes the way the man-db recipe works, this test will break (e.g. it became a git based recipe" — patch.py: Initialize git repo before patching (2021-12)
