---
name: bodnuti
description: Review Yocto Project and OpenEmbedded patch series with bodnuti (patchtest, oelint-adv, shellcheck, leftover-file, line-length, license, CVE_CHECK_IGNORE and documentation checks) in a podman or docker container. Use before sending a series to a mailing list, or to check a series posted on lore.
allowed-tools: Read, Bash(${CLAUDE_SKILL_DIR}/scripts/bodnuti_run.py *)
---

# bodnuti Patch Review

bodnuti runs the checks that OpenEmbedded already has on a patch series, and
reports only what the series adds. Run
`${CLAUDE_SKILL_DIR}/scripts/bodnuti_run.py` directly, not through `python3`,
so it needs no permission prompt; see `--help`. The script:

- uses podman, else docker;
- uses `ghcr.io/moto-timo/bodnuti:latest`, the newest release, by default.
  `--tag main` uses the image of bodnuti's main branch; `--tag 0.3.0` a fixed
  release. All tags are built for amd64 and arm64;
- copies the layer to a standalone clone in a temporary directory, so the
  user's checkout is never changed.

| Series | Command |
|---|---|
| Local commits | `bodnuti_run.py --repo <layer> --range origin/master..HEAD` |
| Patch files | `bodnuti_run.py --repo <layer> --mbox 0001-*.patch` |
| Posted on lore | `bodnuti_run.py --repo <layer> --message-id <Message-ID>` |

For a layer other than openembedded-core, add `--oecore <oe-core checkout>`
for patchtest.

Exit status: 0 clean, 1 findings, 2 a checker could not run or bad input.
Report the findings to the user with their file and line. Style notes are not
counted as findings; mention them only briefly.

## Rules

- bodnuti only reads and reports. Do not send mail or post a review.
- The first run pulls an image of about 850 MB.
- Network: `ghcr.io` to pull the image, and `lore.kernel.org` for
  `--message-id`. If the sandbox blocks a host, tell the user to add it to
  `sandbox.network.allowedDomains`. Do not retry outside the sandbox.
- `references/troubleshooting.md` explains the errors seen with this setup.
