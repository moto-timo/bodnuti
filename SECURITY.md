# Security policy

## Reporting a vulnerability

Report a vulnerability in bodnuti privately, through GitHub's private
vulnerability reporting: on the repository's **Security** tab, choose
**Report a vulnerability**
(<https://github.com/moto-timo/bodnuti/security/advisories/new>). Please do
not open a public issue for it.

Include what you can of: the affected command or component (`check`,
`poll`, `publish`, the container image), the version (`bodnuti --version`
or the image tag), how to reproduce it, and its impact. You will get an
acknowledgement within a week, and the fix will be released with an
advisory crediting you, unless you would rather not be named.

## Scope

bodnuti reads mail that anyone can send, so these matter most:

- the parsers of mail, the lore feed and pull requests (`series`, `lore`,
  `request_pull`), and anything they let through to git, b4 or a checker;
- escaping in the published report pages;
- the checks' sandbox: the container runs without network, read-only and
  without capabilities; a way for a patch to reach the network, the host or
  the worker's other series is in scope.

A problem in a tool bodnuti runs (patchtest, oelint-adv, shellcheck, ruff,
djLint, trivy, cppcheck) belongs to that project, unless bodnuti makes it
exploitable. bodnuti is independent of the Yocto Project: a vulnerability in
the Yocto Project or OpenEmbedded itself goes to their security team, as
their `SECURITY.md` files describe.

## Supported versions

Only the latest release gets security fixes.
