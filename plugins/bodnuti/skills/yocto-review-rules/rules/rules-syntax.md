<!-- select: .bb .bbappend .inc -->
# Rules: Broken Values in Recipe Assignments

Applies to: any `.bb`, `.bbappend`, `.inc` or `.conf` hunk that adds or edits a
quoted value, especially a multi-line one (`SRC_URI`, `RDEPENDS`, `FILES`,
`EXTRA_OECONF`, `DESCRIPTION`, `PACKAGECONFIG[...]`).

These are the most common defects maintainers find by eye, and the author
would have seen them with a single `bitbake -p`. See `overrides.md` for
operator spacing; this file covers broken quoting and entries.

### SYN-1 Unterminated quotes, missing continuations, run-together entries

- **Severity:** error (parse failure, or a fetch of a non-existent file)
- **Trigger:** in the post-patch value:
  - an opening `"` with no closing `"` (look at the last line of a
    multi-line value), or a stray extra `"` in the middle of a list;
  - a line inside a multi-line value that does not end in ` \`, when the
    next line still belongs to the value;
  - two `SRC_URI` entries with no whitespace between them (a `+=` or
    continuation that joins `file://a` to the end of the previous URL);
  - the same main tarball or repository listed twice in `SRC_URI`, or one
    URL carrying the same parameter twice (two `branch=`, two `protocol=`).
- **Problem:** BitBake fails to parse the file, or the fetcher sees one
  mangled URL, or the same source is unpacked twice.
- **Fix:** close the quote, add the backslash, separate the entries, keep one
  copy of each URL and parameter.
- **Do not report when:** the closing quote or backslash is on a context or
  unchanged line outside the hunk you are reading; check the whole value
  first. A value that spans lines inside `${@...}` Python can be valid
  without backslashes only if the Python itself is in one logical line;
  check before reporting.
- **Evidence:**
  - "this is an unterminated string, bitbake would have told you during
    parsing." (ydotool: Add new package, 2024-03)
  - "I think this is a whitespace problem in SRC_URI where file from another
    url is being added to the string above." (coreutils: add ptest, 2019-11)
  - "This seems to have 2 branches in the URI." (nlohmann-json: Upgrade
    3.11.3 -> 3.12.0, 2025-04)
