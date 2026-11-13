# Working on bodnuti

bodnuti runs deterministic checks on Yocto Project and OpenEmbedded patch
series from the mailing lists and publishes the results at
<https://bodnuti.dev/reports/>. `README.md` covers using it,
`docs/DEPLOY.md` the worker that runs it, `docs/STATUS.md` where the
project stands, and `designs/` the design, numbered by section (§3.3, §5.7).

## Layout

- `src/lib.rs` exports what the fuzz targets and tests use: `lore` (mail
  and the lore Atom feed), `series` (a series and its patches),
  `track` (grouping messages into series) and `request_pull`.
- The binary, in `src/main.rs`, adds the rest. The watcher polls lore
  (`watch`), routes each series to a repository, fetches it with b4 and
  checks it in a container (`runner`), matches it to the commits it became
  (`applied`, `ci`, `autobuilder`), and writes the static site
  (`publish`, `report`).
- Each checker has its own module (`oelint`, `patchtest`, `shell`,
  `srcuri`, `cppcheck`, `ruff`, `djlint`, `django`) and reports
  `finding::Finding`s; a finding counts against a series only when it is
  new in a file the series touches.
- `tests/fixtures/` holds real mail; `fuzz/` fuzzes the mail parsers.

## Rules

- Mail, lore and everything fetched from a list is untrusted input:
  validate it, and never pass it to a shell or let it name a path outside
  the work directory.
- Design a non-trivial feature first, as a section of `designs/`, and ask
  before choosing an architecture.
- Do not run the benchmarks unless asked.
- Edition 2024, MSRV 1.89. Before each commit, all three must pass, as
  CI runs them:

  ```sh
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo test
  ```

## Commits

- One self-contained change per commit, each building and passing tests.
- A short message: a `area: summary` subject, then a paragraph on the
  cause, the effect and the fix. Wrap at 72 columns; no backticks.
- A fix to an earlier commit names it: `Fixes: <12-char sha> ("subject")`.
- Every commit carries a `Signed-off-by:` from the person submitting it,
  certifying the [Developer Certificate of Origin](https://developercertificate.org/)
  in their own name. An agent never adds one, for its user or anyone
  else: it commits unsigned, and the person signs off after reviewing the
  commit, with `git commit --amend -s --no-edit` or
  `git rebase --signoff`.
- Trailers go in this order: `Fixes:`, `Co-Authored-By:`, then
  `Signed-off-by:` last.
- Each change a user or operator would notice adds a release note in
  `changes/` (see `changes/README.md`): short, starting with the area, such
  as `**check:**`. A fix to a feature not yet released goes into that
  feature's fragment instead.

## Protected refs

`main` and the `v*` tags are protected by rulesets for SLSA source
provenance: never force-push, and never delete or move a tag. A broken
release is fixed by the next one.

## Releasing

1. Set the version in `Cargo.toml`, run `cargo build` and
   `cargo update -p bodnuti --manifest-path fuzz/Cargo.toml`, and run
   `towncrier build --version X.Y.Z` to collect `changes/` into `NEWS.md`.
   Commit as `release: bodnuti X.Y.Z`.
2. Push, and run the Release workflow by hand on `main`: a dry run that
   builds everything but publishes nothing.
3. Tag `vX.Y.Z` with `git tag -a` and push the tag; the Release workflow
   publishes the wheels, the image and the SBOMs, with provenance.
4. Set the next `-dev` version in its own commit.
