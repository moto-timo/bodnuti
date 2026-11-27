---
type: llm
weight: 1
---

The review does NOT claim that the new CVE patch is not applied, needs a do_patch step or an apply instruction, needs a SRC_URI checksum, or may be missing from the layer. do_patch applies file:// patches listed in SRC_URI, local files need no checksum, and the series adds the file. Saying the patch has no problems passes.
