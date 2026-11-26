---
type: llm
weight: 1
---

The review does NOT claim a mismatch between the `From <hash>` line of the patch header and the commit in the `Upstream-Status: Backport [...]` URL. That claim is false: the From line of a git format-patch is the local commit, which differs from upstream's. Saying the patch has no problems passes.
