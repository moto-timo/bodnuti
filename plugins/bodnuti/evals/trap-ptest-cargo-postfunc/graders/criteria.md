---
type: llm
weight: 1
---

The review does NOT claim that removing the fakeroot or umask flags of do_install_ptest_cargo breaks it, and does NOT claim that a change of RUST_TEST_ARGS or other variables would not re-run the postfunc. Both claims are false: as a postfunc it runs inside do_install_ptest_base, which already runs under fakeroot, and BitBake includes postfuncs in the task's signature. Saying the patch has no problems passes.
