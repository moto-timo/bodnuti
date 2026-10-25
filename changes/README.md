# Release note fragments

Every change that a user or operator of bodnuti would notice adds a
fragment here, which [towncrier](https://towncrier.readthedocs.io/) collects
into `NEWS.md` when a release is made.

A fragment is a short Markdown file named `+<name>.<type>.md`, where
`<name>` is a few words of the change and `<type>` is one of:

| Type | For |
|---|---|
| `feature` | new checks, commands, options or behaviour |
| `bugfix` | a fix to something already released |
| `security` | sandboxing, validation of untrusted input, advisories fixed |
| `doc` | documentation users or operators read |
| `misc` | CI, packaging, dependencies and other internal changes worth a line |

CI checks that a pull request adds a fragment. One that changes nothing a
user or operator would notice (tests, refactoring, CI tweaks) carries the
`no news` label instead; Dependabot's pull requests are exempt.

Write it as a sentence or two in the present tense, about what changes for
the reader, not how the code does it. A fix to a change not yet released
belongs in that change's fragment, not a new one.

```
uvx towncrier create +pseudo-cppcheck.feature.md --content "..."
uvx towncrier build --draft --version 0.2.0   # preview
uvx towncrier build --version 0.2.0           # at release: writes NEWS.md, removes the fragments
```
