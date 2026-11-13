# bodnuti Plugin for Claude Code

Review Yocto Project and OpenEmbedded patch series with
[bodnuti](https://github.com/moto-timo/bodnuti) before you send them, or check
a series posted on lore. bodnuti runs patchtest, oelint-adv, shellcheck, the
leftover-file check, the line-length, license and CVE_CHECK_IGNORE checks and
the documentation checks, and reports only what the series adds.

## Installation

```
/plugin marketplace add moto-timo/bodnuti
/plugin install bodnuti@bodnuti
```

## Requirements

- podman or docker. The script uses podman when both are installed.
- git and Python 3 (standard library only).
- Network: `ghcr.io` to pull the image (about 850 MB), and `lore.kernel.org`
  to check a series by Message-ID.

## Script

`skills/bodnuti/scripts/bodnuti_run.py`:

```
bodnuti_run.py --repo <layer> --range origin/master..HEAD
bodnuti_run.py --repo <layer> --mbox 0001-*.patch
bodnuti_run.py --repo <layer> --message-id <Message-ID>
```

- `--tag`: `latest` (default) is the newest release; `main` is bodnuti's main
  branch; or a release such as `0.3.0`. All tags have amd64 and arm64 images.
- `--oecore`: an openembedded-core checkout, for patchtest on other layers.
- `--kind`, `--base`, `--staging`, `--json` are passed to `bodnuti check`.
- With `--range`, the base defaults to the start of the range.

The script copies the layer to a standalone clone in a temporary directory,
so your checkout is never changed. Exit status: 0 clean, 1 findings, 2 a
checker could not run.

## Tests

Run from the plugin directory. They run no container.

```
python3 -m unittest discover -s skills/bodnuti/tests
```

## License

MIT OR Apache-2.0, the same as bodnuti. See `LICENSE-MIT` and
`LICENSE-APACHE` at the top of the repository.
