# Release notes

<!-- towncrier release notes start -->

## bodnuti 0.2.0 (2026-10-25)

### Features

- **check:** oelint-adv layout rules (spacing, ordering, suggested variables) are reported as style, kept apart from findings and not counted in the result or the exit status; the style rules OE-Core does not follow, once suppressed, are reported this way too.
- **kinds:** Django applications also get Django's own checks, `manage.py check` and `makemigrations --check`, run offline against sqlite with the image's venv for each application; a model change without its migration is an error.
- **publish:** Style findings are collapsed under each patch behind a "Show N style findings" toggle, which needs no JavaScript, and the result notes how many there are.
- **publish:** `bodnuti publish` also writes `summary.html`, the last week per list and the latest series, for bodnuti.dev's front page.

### Bug fixes

- **poll:** Patches that landed before their series was checked, as a backfill checks them, are now matched upstream; they were missed, since the scans read only later commits.

### Security

- Report pages escape `{` and `}` as well, so text from mail can never act as a template where a page is served through one.
- **packaging:** Release wheels and the container image carry SLSA build provenance and an attested SPDX 2.3 SBOM (GitHub artifact attestations), and the image a keyless cosign signature; the SBOMs are also attached to the release, and the README says how to verify all of them.

### Documentation

- SECURITY.md says how to report a vulnerability privately, through GitHub's private vulnerability reporting, and what is in scope.

### Other changes

- **packaging:** `make audit` runs cargo-deny: besides advisories, every linked crate must have a permissive license (`deny.toml`) and come from crates.io.
- **packaging:** bodnuti is licensed under MIT or Apache-2.0, at your option (`LICENSE-MIT`, `LICENSE-APACHE`).


## bodnuti 0.1.0 (2026-10-25)

### Features

- **check:** A patch file left in a layer after the series drops it from `SRC_URI` is reported.
- **check:** A series that does not apply is reported as a finding, `bodnuti.apply`, naming the patch and git's reason, rather than as a failure of the check.
- **check:** OE-Core's patchtest mbox and patch suites run on every series: commit message, shortlog, Signed-off-by, Upstream-Status and CVE tags. Its mailing-list test is skipped for repositories other than OE-Core, and quoted-printable mail is decoded first, so it no longer fails good patches.
- **check:** Series to yocto-docs and the BitBake manual get the projects' own documentation checks: a Sphinx build with warnings as errors, sphinx-lint, check-glossaries and check-confusables.
- **check:** The report is text by default and JSON with `--json`; the exit status is 0 when clean, 1 with findings and 2 when a checker could not run.
- **check:** `bodnuti check` checks a patch series, given as mbox files or as a lore Message-ID fetched with b4, by applying it to a throwaway worktree of the target layer and running each checker before and after it, so only what the series adds is reported. The base is the branch a subject tag names (`[scarthgap]`, `[PATCH wrynose]`), else the default branch; a recipe upgrade that git sees as a delete and an add is compared as one file, and one revision of a series is kept when a thread carries several.
- **check:** bodnuti finds patchtest's Python, oelint-adv and b4 installed beside its own binary, so a virtualenv works without being activated.
- **check:** oelint-adv checks the recipes, classes and include files a series touches, for the release its base branch names, with the rules OE-Core does not follow suppressed.
- **check:** shellcheck checks the shell functions in recipes and classes (dash-compatible `/bin/sh`) and shell scripts with a shebang.
- **kinds:** A repository has a kind that selects its checks: `layer` (the default), `django-app`, `python` or `c`, set in `[repos]` or with `bodnuti check --kind`.
- **kinds:** C projects (pseudo) are checked with patchtest's mail checks, shellcheck and cppcheck's errors and warnings.
- **kinds:** Django applications (layerindex-web, error-report-web) are checked with patchtest's mail checks, ruff (bugs, Django and security rules), djLint (template bugs and accessibility) and shellcheck.
- **kinds:** For Django applications, trivy scans the `requirements*.txt` files a series changes, offline against a database the watcher downloads daily: advisories the series adds are findings, and advisories it fixes are listed as fixes, which do not count as findings.
- **kinds:** Python tools (yocto-autobuilder-helper, yocto-autobuilder2, auto-upgrade-helper, yocto-metrics) are checked with patchtest's mail checks, ruff (including scripts without `.py` and BuildBot's `master.cfg`), shellcheck, and a check that touched JSON files still parse.
- **packaging:** A container image (Debian trixie) carries bodnuti and every checker, runs as an unprivileged user, and works read-only with a tmpfs on `/tmp`.
- **packaging:** bodnuti is also packaged as a Python wheel (maturin), installing the binary into the same environment as the Python tools it runs.
- **poll:** A `[repos]` entry may list subject tags besides its name (`tags = ["AUH"]`), and a tag may name several repositories; the paths a series changes pick among them.
- **poll:** A `git request-pull` cover letter is checked like a series: its branch is fetched over https from git.openembedded.org or git.yoctoproject.org and base..head is checked against the pull request's own base.
- **poll:** A configured repository without its clone is reported as `clone missing: git clone <url> <path>`, and its series wait without spending their attempts.
- **poll:** A list maps to one repository, or, for yocto-patches, a series is routed by its subject tag (also inside the PATCH tag, `[PATCH yocto-autobuilder2]`) and then checked against the paths it changes. A series whose tags name no configured repository is tried again on every poll, so adding the repository picks it up.
- **poll:** A series whose root message lore does not have is fetched through another of its messages.
- **poll:** A thread with no patches, such as a cover letter whose patches were sent outside it, is recorded as "no patches" instead of failing.
- **poll:** Complete series are fetched with b4 and checked in the container without network, one at a time; a resend whose patches were all checked for the same branch is recorded as a duplicate instead.
- **poll:** Each poll matches checked patches to upstream commits on their branch, by patch-id and else by subject and author, and marks a series applied once all its patches have landed. A series merged before it was checked is recognised as applied rather than as unrouted.
- **poll:** The default runner keeps podman's container events and output out of the journal, leaving bodnuti's own line per series.
- **poll:** `bodnuti poll --backfill-hours N` checks a list's series from before the watcher started, once; series already known are not checked again.
- **poll:** `bodnuti poll` watches lore lists for new patch series, groups their messages, waits for a series to complete or settle, and keeps its state between runs in one JSON file per list.
- **publish:** Each finding is shown as a block, on the page and in `report.txt`: severity, tool, check and location, the message with its line breaks, and the lines of the patched file around it.
- **publish:** `bodnuti publish` renders the watcher's state as static HTML, an index of recent series per list and a page per series with its patches, findings, fixes and where each patch landed, and copies it to the web host with rsync. `retain_months` sets how far back the site goes.

### Security

- Checks run in the container without network, read-only, as an unprivileged user with every capability dropped and limits on memory, CPUs and processes; only fetching a series, and the daily vulnerability database download, have network.
- Everything taken from mail is validated before use: Message-IDs before they become file names, and a pull request's repository (https on git.openembedded.org or git.yoctoproject.org only), branch and commits before they reach `git fetch`. The documentation build refuses series that change files the build would run.
- Report pages escape everything from mail, run no JavaScript, and are served under a Content-Security-Policy that allows only same-site images and inline styles; the documented deploy key can only write the reports directory.
- quick-xml, which parses the lore feed, is on 0.42, past RUSTSEC-2026-0194 and RUSTSEC-2026-0195.

### Documentation

- `designs/DESIGN.md` and `designs/PHASE2.md` describe the checks, the watcher and the decisions behind them.
- `docs/DEPLOY.md` describes a worker: rootless Podman, the image, the layer clones, the watcher's systemd timer, backfilling, and publishing to the web host with a write-only deploy account.
- `docs/STATUS.md` reports where the project stands, with measured results.

### Other changes

- **packaging:** A `v*` tag builds x86_64 and aarch64 wheels and the amd64 and arm64 image, publishes the image to ghcr.io, and makes the GitHub release with the version's release notes.
- GitHub Actions runs fmt, clippy and the tests, a build on Rust 1.89, dependency and secret scans, the image build and scan, and the wheel on every change, the scans daily and the fuzz targets weekly. Dependabot watches the Cargo, pip and Actions dependencies.
- `make audit` and `make audit-image` scan the dependencies, the git history and the image (cargo-audit, trivy, gitleaks); `make fuzz` runs the fuzz targets for the mail, lore feed, subject and pull request parsers.
- bodnuti needs Rust 1.89 and builds with toml 1.1, quick-xml 0.42 and ureq 3; the image's Python tools are pinned in `requirements.txt`.
