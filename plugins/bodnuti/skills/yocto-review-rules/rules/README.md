# Review rules for Yocto Project and OpenEmbedded patches

Condensed from maintainers' inline review comments on the
openembedded-core and openembedded-devel lists, 2016 to 2025, that the
author acted on in the next version of the patch. Every rule is backed
by at least two comments, quoted with the patch and month, and says
when not to report it. Style-only objections were left out on purpose.
The exception is `rules-bitbake.md`: BitBake behaviour that models
reported as defects on real patches, each case checked by hand, so a
reviewer checks a finding against it before reporting.

Plain markdown, for any reviewer or AI tool: point it at this
directory, or at the files for what a patch touches. bodnuti builds them
in and selects them per
patch by the first line of each file, `<!-- select: ... -->`: file
types or `path=` parts the diff changes, then text its changed lines
contain. They build on sashiko's Yocto guides (review-core.md,
false-positive-guide.md, patterns/*.md) and do not repeat them.

The quotes are short excerpts from public list mail, cited by patch and
month. A file without the select line is never sent (this README).

Draft: written with an AI from the comments, every quote checked
against the source; not yet reviewed by a maintainer. A `rules/`
directory in bodnuti's guides directory replaces these, for trying
changes without a rebuild.
