<!-- select: .bb .bbappend .inc CVE_ -->
# Rules: CVE Metadata in Recipes

Applies to: `.bb`, `.bbappend`, `.inc` hunks that set `CVE_STATUS`,
`CVE_CHECK_IGNORE`, `CVE_CHECK_WHITELIST`, `CVE_PRODUCT` or `CVE_VERSION`.

`CVE:` tags in patch files are in `patterns/patch-files.md`; marking a real,
unfixed CVE as not affecting the recipe is Critical (`severity.md`).

### CVE-1 Exclusions use CVE_STATUS with a status and a reason

- **Severity:** warning
- **Trigger:** on master (as of 2023): `CVE_CHECK_IGNORE` or
  `CVE_CHECK_WHITELIST` added; or a `CVE_STATUS[CVE-...]` value without a
  status prefix from `CVE_CHECK_STATUSMAP` (for example `disputed:`,
  `fixed-version:`, `not-applicable-config:`, `cpe-incorrect:`); or any
  exclusion without an explanation.
- **Problem:** the old variables are replaced by `CVE_STATUS`; without a
  known prefix the status is not mapped; without a reason nobody can check
  the exclusion later.
- **Fix:** `CVE_STATUS[CVE-YYYY-NNNN] = "<status>: <why>"`. Drop the entry
  if the CVE's affected versions do not include the recipe's version.
- **Do not report when:** the patch targets a stable branch older than
  `CVE_STATUS` support, where `CVE_CHECK_IGNORE` is still the mechanism;
  check the target branch.
- **Evidence:**
  - "This should be using CVE_STATUS instead for master." (go: ignore
    CVE-2023-45283 and CVE-2023-45284, 2023-12)
  - "This is wrong format since it's missing CVE status map prefix."
    (sqlite3: set CVE_STATUS for CVE-2023-36191, 2023-09)
  - "Can you be a bit more explicit about why this is whitelisted?" (bind:
    Whitelist CVE-2019-6470, 2019-11)

### CVE-2 CVE_PRODUCT and CVE_VERSION must match NVD and follow PV

- **Severity:** warning (CVE checks silently miss or mismatch the recipe)
- **Trigger:** a `CVE_PRODUCT` vendor or product name that is not one NVD
  uses; a literal `CVE_VERSION = "..."` that duplicates information in
  `PV`.
- **Problem:** an invented vendor never matches NVD entries; a literal
  version goes stale on the next upgrade.
- **Fix:** use the vendor:product pair NVD already uses; derive
  `CVE_VERSION` from `PV` (directly or with a small helper).
- **Do not report when:** the vendor name is shown to exist in NVD; the
  literal is needed because the upstream version scheme differs and the
  patch explains it.
- **Evidence:**
  - "Please don't make up vendor names." (builder: set CVE_PRODUCT,
    2024-11)
  - "This will almost certainly become mismatched with PV on version
    updates, so please set from PV with a helper function." (ovmf: set
    CVE_PRODUCT and CVE_VERSION, 2024-03)
