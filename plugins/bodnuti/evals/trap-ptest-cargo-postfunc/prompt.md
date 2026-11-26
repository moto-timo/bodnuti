---
max_turns: 10
allowed_tools: [Read, Glob, Grep, Skill]
tags: [trap]
---

This patch was posted to an OpenEmbedded mailing list on 26 November 2026. Review it as a maintainer would. List only real problems, each with the file, the line and what to change. If there are none, say so. You have the mail only, not the layer. Mail addresses and names in it are replaced by placeholders; do not report them.

````
From: Person 53de63fe <user-05cc345787@example.invalid>
Subject: [OE-core][PATCH 4/6 v2] ptest-cargo.bbclass: run do_install_ptest_cargo as a postfunc

do_install_ptest_cargo appends to run-ptest, but run-ptest is only reset
by do_install_ptest_base's cleandirs. This means that changes to its
signature (e.g. by setting 'RUST_TEST_ARGS:pn-hyperfine = "--test-threads=1"'
in local.conf) just append another block to run-ptest with an 'exit $rc'
in between, so changed test arguments aren't actually used. Run
do_install_ptest_cargo as a postfunc for do_install_ptest_base instead
so that changes are always reflected.

do_install_ptest_cargo is removed as a task.

Tested by setting RUST_TEST_ARGS in local.conf as described above. Here
is a snippet of what the double-appended run-ptest script would look
like without this fix:

|if ! /usr/lib/hyperfine/ptest/hyperfine-e8776e44d98845b7
|then
|    rc=1
|    echo "FAIL: /usr/lib/hyperfine/ptest/hyperfine-e8776e44d98845b7"
|else
|    echo "PASS: /usr/lib/hyperfine/ptest/hyperfine-e8776e44d98845b7"
|fi
|exit $rc
|
|echo ""
|echo "## starting to run rust tests ##"
|if [ -z "$rc" ]; then rc=0; fi
|if ! /usr/lib/hyperfine/ptest/execution_order_tests-0fccc0f22d97e11e --test-threads=1
|then
|    rc=1
|    echo "FAIL: /usr/lib/hyperfine/ptest/execution_order_tests-0fccc0f22d97e11e"
|else

AI-Generated: Uses Claude Opus 5.5
Signed-off-by: Person 53de63fe <user-05cc345787@example.invalid>
---
 meta/classes-recipe/ptest-cargo.bbclass | 16 +++++++---------
 1 file changed, 7 insertions(+), 9 deletions(-)

diff --git a/meta/classes-recipe/ptest-cargo.bbclass b/meta/classes-recipe/ptest-cargo.bbclass
index 73b9e84766..cc82ea320b 100644
--- a/meta/classes-recipe/ptest-cargo.bbclass
+++ b/meta/classes-recipe/ptest-cargo.bbclass
@@ -125,21 +125,19 @@ python do_install_ptest_cargo() {
             shutil.chown(os.path.join(root, f), "root", "root")
 }
 
-do_install_ptest_cargo[dirs] = "${B}"
-do_install_ptest_cargo[doc] = "Create or update the run-ptest script with rust test binaries generated"
 do_compile_ptest_cargo[dirs] = "${B}"
 do_compile_ptest_cargo[doc] = "Generate rust test binaries through cargo"
 
 addtask compile_ptest_cargo after do_compile            before do_compile_ptest_base
-addtask install_ptest_cargo after do_install_ptest_base before do_package
 
-python () {
-    if not bb.data.inherits_class('native', d) and not bb.data.inherits_class('cross', d):
-        d.setVarFlag('do_install_ptest_cargo', 'fakeroot', '1')
-        d.setVarFlag('do_install_ptest_cargo', 'umask', '022')
+# Run as part of do_install_ptest_base so its cleandirs always provides a fresh
+# run-ptest to append to. As a separate task, a signature change (e.g.
+# RUST_TEST_ARGS) re-ran only this step, appending a second block after the
+# first "exit $rc".
+do_install_ptest_base[postfuncs] += "do_install_ptest_cargo"
 
+python () {
     # Remove all '*ptest_cargo' tasks when ptest is not enabled
     if not(d.getVar('PTEST_ENABLED') == "1"):
-        for i in ['do_compile_ptest_cargo', 'do_install_ptest_cargo']:
-            bb.build.deltask(i, d)
+        bb.build.deltask('do_compile_ptest_cargo', d)
 }
-- 
2.56.1


````
