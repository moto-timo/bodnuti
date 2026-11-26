---
type: llm
weight: 1
---

The review reports at least one of the two real license defects in this patch, and presents no false claim as a must-fix problem:

1. New Qualcomm topology files (the shikra ...-tplg.bin files) are added to a package whose LICENSE does not cover their licence (LICENCE.linaro / Linaro).
2. The awinic block is broken: `${PN}-awinic-aw88399 = "..."` lacks the `LICENSE:` prefix, LICENSE and FILES are set on `${PN}-awinic` while PACKAGES and RDEPENDS use `${PN}-awinic-license`, or FILES names `LICENCE.awinic` where the file is `LICENSE.awinic`.
