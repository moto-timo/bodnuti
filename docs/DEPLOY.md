# Deploying bodnuti on a VM

How to run bodnuti as an unprivileged service on a small Debian trixie VM
(Hetzner Cloud or similar), with rootless Podman. The public site
(bodnuti.dev) stays on its own host; the worker only pushes reports to it.

Patches are untrusted input. The setup below keeps them in a rootless,
read-only container with no network while they are checked, and gives the
worker no inbound ports except SSH.

## Sizing

Measured in the container, capped at 2 CPUs:

| Workload | Time | Memory |
|---|---|---|
| OE-Core recipe patch | about 15 s | fits in 256 MB |
| yocto-docs patch (two Sphinx builds) | about 46 s | fits in 1 GB; hung at 512 MB |
| Layer clones (oe-core, meta-oe, bitbake, yocto-docs, meta-virt, meta-yocto) | | about 0.85 GB of `.git` |
| Image | | about 420 MB |

The lists carry about 170 patches a day, which one worker handles easily.

| Use | vCPU | RAM | Disk |
|---|---|---|---|
| Watch lists, run checks, publish static reports | 2 | 4 GB | 40 GB |
| Two parallel workers, documentation builds included | 4 | 8 GB | 40 GB |

Shared vCPU is enough. Arm (Hetzner CAX) should work, because the base
images and `koalaman/shellcheck:v0.10.0` publish arm64, but check that
`pygit2` and the Sphinx extensions install from wheels on aarch64.

## Firewall

Hetzner Cloud Firewall filters by IP and port, not by hostname.

Inbound:

| Port | Proto | Source |
|---|---|---|
| 2222 | TCP | your IP, if stable |

Outbound. Adding any outbound rule makes the firewall deny all other
outbound traffic; replies to allowed inbound traffic pass automatically.
Check both behaviours against Hetzner's documentation.

| Port | Proto | Destination | Why |
|---|---|---|---|
| 443 | TCP | any | lore.kernel.org (b4), git over https, container registry, Debian, later the Anthropic API |
| 80 | TCP | any | Debian mirrors, if apt sources use `http://`; drop it if they use https |
| 53 | UDP, TCP | Hetzner's recursive resolvers (see the console) | DNS |
| 123 | UDP | any, or Hetzner's NTP servers | time |
| 22 | TCP | the bodnuti.dev host only, ideally its private-network IP | rsync of reports |

Port-level rules still allow any host on 443. The checks need no network at
all, so they run with `--network none` (step 8); only the fetch step talks to
lore.kernel.org.

## 1. SSH on port 2222

Keep your current SSH session open until the new port works.

1. Add the inbound TCP 2222 rule in the Hetzner firewall. Leave port 22
   open for now.
2. Make sure you have a non-root sudo user whose `~/.ssh/authorized_keys`
   holds your key, and that you can log in as it. The next step disables
   root login.
3. Create `/etc/ssh/sshd_config.d/10-hardening.conf`:

   ```
   Port 2222
   PermitRootLogin no
   PasswordAuthentication no
   KbdInteractiveAuthentication no
   PubkeyAuthentication yes
   AllowUsers <your-admin-user>
   ```

4. Validate and restart:

   ```
   sudo sshd -t && sudo systemctl restart ssh
   ```

   If `systemctl is-active ssh.socket` reports `active`, sshd is socket
   activated and ignores `Port`. Set the port with `systemctl edit ssh.socket`
   instead: an empty `ListenStream=` line, then `ListenStream=2222`.

5. From a second terminal: `ssh -p 2222 <your-admin-user>@<host>`.
6. Once that works, remove the port 22 rule from the firewall.

## 2. The bodnuti user

```
sudo adduser --disabled-password --gecos "" bodnuti
```

No password, no sudo and no SSH login. Administer it from your own user with
`sudo machinectl shell bodnuti@`.

Do not add `bodnuti` to the `docker` group: that group is root-equivalent, so
a patch that escaped its container would own the VM. Rootless Podman avoids
this.

## 3. Rootless Podman

```
sudo apt install podman uidmap passt dbus-user-session systemd-container
```

Check that `bodnuti` has subordinate IDs:

```
grep bodnuti /etc/subuid /etc/subgid
```

If either file has no line for it:

```
sudo usermod --add-subuids 100000-165535 --add-subgids 100000-165535 bodnuti
```

Let its user services run without a login session:

```
sudo loginctl enable-linger bodnuti
```

Open a shell as `bodnuti`. Use `sudo`: without polkit only root may open
another user's session, and plain `sudo -iu` does not set the
`XDG_RUNTIME_DIR` that rootless Podman needs.

```
sudo machinectl shell bodnuti@
```

If machinectl is not available:

```
sudo -iu bodnuti
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
```

Check, as `bodnuti`:

```
podman info --format '{{.Host.Security.Rootless}}'   # true
podman run --rm docker.io/library/debian:trixie-slim id
```

## 4. The image

Build it on another machine, so the VM needs no Rust toolchain or PyPI
access. Match `--platform` to the VM: `linux/amd64` for CX and CPX,
`linux/arm64` for CAX. Building for another architecture needs QEMU
emulation, and the Rust stage is slow under it.

```
podman build --platform linux/arm64 -t bodnuti -f Containerfile .
podman save bodnuti | gzip > bodnuti.tar.gz
scp -P 2222 bodnuti.tar.gz <your-admin-user>@<host>:/tmp/
```

Load it on the VM in an interactive session, so sudo can ask for your
password:

```
chmod 644 /tmp/bodnuti.tar.gz
sudo -iu bodnuti env XDG_RUNTIME_DIR=/run/user/$(id -u bodnuti) podman load -i /tmp/bodnuti.tar.gz
rm /tmp/bodnuti.tar.gz
```

`podman images`, as `bodnuti`, then lists `localhost/bodnuti`.

## 5. Layer clones

As `bodnuti`. Use plain clones, not `git clone --shared`: a shared clone's
object alternates point at paths the container cannot see.

```
mkdir -p ~/cache ~/inbox
git clone https://git.openembedded.org/openembedded-core ~/cache/openembedded-core
git clone https://git.openembedded.org/meta-openembedded ~/cache/meta-openembedded
git clone https://git.yoctoproject.org/yocto-docs ~/cache/yocto-docs
git clone https://git.openembedded.org/bitbake ~/cache/bitbake
```

bodnuti writes temporary worktrees into these clones' `.git` and removes
them when a check ends. Nobody should work in them by hand.

## 6. Running a check

As `bodnuti`. Fetching the series needs the network; checking it does not.

```
B="podman run --rm --userns=keep-id:uid=1000,gid=1000 \
   --read-only --tmpfs /tmp:rw,size=2g \
   --cap-drop=ALL --security-opt no-new-privileges \
   --pids-limit 512 --memory 2g --cpus 2"

ID='<Message-ID>'

# fetch: network on, only the inbox mounted; writes ~/inbox/$ID.mbx
$B -v ~/inbox:/out --entrypoint b4 bodnuti mbox -o /out "$ID"

# check: no network
$B --network none -v ~/cache/openembedded-core:/repo -v ~/inbox:/in:ro \
   bodnuti check --repo /repo "/in/$ID.mbx"
```

`b4 mbox -o DIR` saves the whole thread as `DIR/<Message-ID>.mbx`, keeping
the original subjects (and so tags such as `[scarthgap]`); bodnuti drops the
replies. Any Message-ID in the thread works.

Expand file name globs against the host path (`~/inbox/...`) and pass only
the name. A glob written as `/in/...` is expanded by the host shell, where
`/in` does not exist, and reaches bodnuti as a literal `*`.

To check that a worker catches known defects, run it on two commits from the
benchmark corpus, with no network:

```
for c in 388cd4770e04 c12e013453689697a8680f1c7de3e625a0ff28ec; do
  rm -rf ~/inbox/test && mkdir -p ~/inbox/test
  git -C ~/cache/openembedded-core format-patch -q -1 $c -o ~/inbox/test
  P=$(basename ~/inbox/test/0001-*.patch)
  $B --network none -v ~/cache/openembedded-core:/repo -v ~/inbox:/in:ro \
     bodnuti check --repo /repo --base $c~1 /in/test/$P
done
```

The first reports two `oelint.vars.inconspaces` errors in
`ptest-packagelists.inc`; the second reports `SC3037` (`echo -e`) in
`uboot-sign.bbclass`. Both exit 1.

- `--userns=keep-id:uid=1000,gid=1000` maps the host `bodnuti` user to the
  image's `bodnuti` user (uid 1000), so files in `~/cache` stay owned by the
  host user.
- `--read-only` works because bodnuti writes only to `/tmp` (worktrees,
  documentation builds and, through `XDG_CACHE_HOME`, b4's cache) and to the
  mounted repository's `.git`. Images built before commit c5910de need
  `-e XDG_CACHE_HOME=/tmp/.cache` added to `$B`.
- `--memory 2g` leaves room for documentation builds, which hung at 512 MB.
- For a meta-openembedded or yocto-docs target, mount its clone as `/repo`,
  and also mount `~/cache/openembedded-core:/oecore:ro` with
  `-e BODNUTI_OECORE=/oecore`, because patchtest comes from OE-Core.
- Debian uses AppArmor, not SELinux, so the mounts need no `:Z` labels.

## 7. Keeping the clones fresh

As `bodnuti`, create `~/.config/systemd/user/bodnuti-fetch.service`:

```
[Unit]
Description=Update bodnuti layer clones

[Service]
Type=oneshot
ExecStart=/bin/sh -c 'for r in %h/cache/*; do git -C "$r" fetch --quiet --prune origin; done'
```

and `~/.config/systemd/user/bodnuti-fetch.timer`:

```
[Unit]
Description=Update bodnuti layer clones hourly

[Timer]
OnCalendar=hourly
Persistent=true

[Install]
WantedBy=timers.target
```

Then:

```
systemctl --user daemon-reload
systemctl --user enable --now bodnuti-fetch.timer
```

## 8. Publishing reports

Reports go to the bodnuti.dev host with rsync over SSH. On that host, give
the worker's deploy key a forced command limited to one directory, so a
compromised worker cannot touch anything else, for example in
`~/.ssh/authorized_keys`:

```
command="rrsync /var/www/bodnuti.dev/reports",restrict ssh-ed25519 AAAA... bodnuti-worker
```

Watching the lists and generating reports is phase 2 and is not built yet;
this section describes where its output goes.

## Not yet verified

- The image on x86. On arm64 the two benchmark checks above pass on a
  Hetzner worker in rootless Podman, read-only and with `--network none`.
