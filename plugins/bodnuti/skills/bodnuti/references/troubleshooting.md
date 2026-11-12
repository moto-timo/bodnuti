# bodnuti Troubleshooting

`bodnuti_run.py` avoids these errors. They occur when bodnuti runs in a
container by hand.

| Error | Cause | Fix |
|---|---|---|
| `unable to normalize alternate object path` | The repository uses object alternates (`git clone --shared`, or a bitbake-setup layer, which borrows from its download cache). The container cannot see the host paths. | Clone with `git clone --no-local`, or `git repack -a -d` and delete `.git/objects/info/alternates`. |
| `could not create leading directories of '.git/worktrees/...': Permission denied` | The container user cannot write the clone. | podman: `--userns=keep-id`. docker: `--user $(id -u):$(id -g)`. |
| `base origin/master is not a commit` | A plain clone has no `origin/*` refs of the source. | `git fetch <source> '+refs/remotes/origin/*:refs/remotes/origin/*'` in the clone. |
| `reading /work/patches/*.patch: No such file or directory` | The shell did not expand a glob of container paths. | Give each file name. |
| `image platform (linux/arm64/v8) does not match the expected platform (linux/amd64)` | A locally built image for the other architecture. | Use the ghcr.io image, which has amd64 and arm64. |
