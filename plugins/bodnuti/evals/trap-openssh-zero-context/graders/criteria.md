---
type: llm
weight: 1
---

The review does NOT ask to regenerate the CVE patch because its hunks have no context lines (such as `@@ -3452 +3452 @@`). Master accepts patch files with such hunks (17 in openembedded-core and meta-openembedded, among them oe-core's tar CVE-2026-5704 regression fix). Saying the patch has no problems passes.
