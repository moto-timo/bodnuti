<!-- select: .bb .bbappend .inc LICENSE LIC_FILES_CHKSUM License-Update -->
# Rules: LICENSE and LIC_FILES_CHKSUM

Applies to: `.bb`, `.inc` hunks that add or change `LICENSE`,
`LICENSE:<pkg>` or `LIC_FILES_CHKSUM`, and new recipes.

### LIC-1 LICENSE uses SPDX identifiers

- **Severity:** warning
- **Trigger:** a new or changed `LICENSE` with an old or ambiguous name:
  `GPLv2`, `GPLv3+`, `LGPLv3+`, `LGPLv2.1`, `BSD`, `Apache-2`,
  `BSD-0-Clause`.
- **Problem:** old names are no longer valid in OE-Core (as of 2023), and
  `BSD` or `GPLv2` does not say which variant applies.
- **Fix:** SPDX identifiers: `GPL-2.0-only` or `GPL-2.0-or-later` (read the
  source headers to choose), `LGPL-3.0-or-later`, `BSD-2-Clause` or
  `BSD-3-Clause` (read the license text), `Apache-2.0`, `0BSD`.
- **Do not report when:** the name is a custom license the layer ships in
  its own license directory, or the line is unchanged by the patch.
- **Evidence:**
  - "This LICENSE value is deprecated. Please use spdx identifiers if
    possible/applicable, like GPL-2.0-only or GPL-2.0-or-later" (testdisk:
    add recipe, 2025-04)
  - ""LGPLv3+" isn't a valid license in recipes anymore." (zeromq: consider
    license exception over LGPL-3.0, 2023-06)
  - "Change "BSD" to "BSD-2-Clause"." (valijson: Add recipe, 2018-10)

### LIC-2 LICENSE must match the source

- **Severity:** error (licensing)
- **Trigger:** `LICENSE` does not cover the licenses visible in the patch:
  `LIC_FILES_CHKSUM` lists files for other licenses (`COPYING.LGPL`,
  `LICENSE.BSD`, a bundled submodule's license) that `LICENSE` omits; a
  source header quoted in the patch contradicts `LICENSE`; or
  `LICENSE = "CLOSED"` for software that has an open-source license file.
- **Problem:** license compliance tooling and `INCOMPATIBLE_LICENSE`
  decisions use `LICENSE`; a wrong value ships code under terms nobody
  checked. `CLOSED` also disables the checksum check.
- **Fix:** list every license (`&` for combined, `|` for a choice), use
  `LICENSE:<pkg>` for per-package differences, and add a recipe comment for
  parts that are not shipped.
- **Do not report when:** per-package `LICENSE:<pkg>` values cover the
  extra licenses; a recipe comment explains that the extra code is not built
  or shipped; the software is really proprietary and lives in a vendor
  layer.
- **Evidence:**
  - "with all the license files mentioned below the LICENSE setting looks
    wrong to me." (Add new recipe for squashfs-tools-ng, 2020-10)
  - "the recipe says MIT license, but the source claims to be GPL-2.0-only"
    (meta-selftest: add selftest-hello-mod recipe, 2025-06)
  - "if its wrongly marked at closed please fix it in v2." (opencl-icd-loader:
    Initial recipe for OpenCL ICD loader, 2018-10)

### LIC-3 LIC_FILES_CHKSUM points at the project's own license text

- **Severity:** warning
- **Trigger:** a recipe with upstream source whose `LIC_FILES_CHKSUM` points
  only at `${COMMON_LICENSE_DIR}/...`; or one that checksums a whole source
  file (`file://src/foo.c;md5=...`) to capture a license header.
- **Problem:** a generic text proves nothing about the project's license;
  checksumming a whole source file breaks the build on every unrelated edit
  to that file.
- **Fix:** point at the project's `COPYING`/`LICENSE` file; for a header in
  a source file, add `;beginline=N;endline=M` around the header only.
- **Do not report when:** the recipe has no upstream source (only
  `file://` files from the layer) and uses
  `${COMMON_LICENSE_DIR}/MIT` — that is the usual form for such recipes.
- **Evidence:**
  - "NAK LIC_FILES_CHKSUM should point to some files in the project"
    (assimp: Add recipe for Open Asset Import Library, 2016-05)
  - "Any change to debuginfod/debuginfod-client.c will trigger a build error
    because of license changes" (elfutils: split libdebuginfod into its own
    package, 2021-01)

### LIC-4 A changed license checksum needs a License-Update explanation

- **Severity:** warning
- **Trigger:** an upgrade changes the `md5=` (or file) in
  `LIC_FILES_CHKSUM`, and the commit message has no `License-Update:` line,
  or that line only repeats the new `LICENSE` value.
- **Problem:** reviewers cannot tell whether the license changed or only the
  copyright years did.
- **Fix:** add `License-Update: <what changed in the file>` to the commit
  message (for example "copyright years updated"), and update `LICENSE` if
  the terms changed.
- **Do not report when:** the commit message already explains the change;
  the recipe is new.
- **Evidence:**
  - "why did this checksum change ? can you add that to commit msg via
    "License-Update" tag ?" (python3-pycparser: upgrade 2.20 -> 2.21,
    2021-12)
  - "The license-update notice should be an explanation of what changed in
    the COPYING file, not just repeat the LICENSE change." (erofs-utils:
    upgrade 1.4 -> 1.5, 2022-06)
