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
| 2222 | TCP | the bodnuti.dev host only, ideally its private-network IP | rsync of reports (§8) |

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

Reports go to the bodnuti.dev host with rsync over SSH, as a dedicated
account whose key can only write one directory. The commands below assume
both hosts are on one Hetzner private network (the worker `10.1.0.2`, the web
host `10.1.0.3`) and that the web host's sshd listens on 2222, as in step 1.

### On the web host

1. Create the deploy account and the reports directory Caddy serves:

   ```sh
   sudo apt install rsync          # provides /usr/bin/rrsync
   sudo useradd --system --create-home --shell /bin/sh bodnuti-deploy
   sudo install -d -o bodnuti-deploy -g bodnuti-deploy -m 755 /var/www/bodnuti.dev/reports
   sudo install -d -o bodnuti-deploy -g bodnuti-deploy -m 700 /home/bodnuti-deploy/.ssh
   ```

   `.ssh` must be owned by `bodnuti-deploy`: sshd reads `authorized_keys` as
   that user, and a root-owned `700` directory makes every login fail with
   only `Connection closed by authenticating user bodnuti-deploy [preauth]`
   in the log.

2. If `sshd_config` has an `AllowUsers` line, add `bodnuti-deploy` and run
   `sudo systemctl reload ssh`.

3. Note the host key fingerprint, to check on the worker:

   ```sh
   ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub
   ```

4. Report pages carry subjects and messages from mail. They are escaped and
   have no scripts; a Content-Security-Policy in the `bodnuti.dev` block of
   the Caddyfile makes sure nothing slipped through can run:

   ```
   header /reports/* Content-Security-Policy "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'"
   ```

   then `sudo systemctl reload caddy`.

### On the worker, as `bodnuti`

5. Create the key, without a passphrase since a timer uses it:

   ```sh
   ssh-keygen -t ed25519 -N "" -C bodnuti-worker -f ~/.ssh/bodnuti-deploy
   ```

6. Name the web host in `~/.ssh/config` (mode `600`):

   ```
   Host bodnuti-web
       HostName 10.1.0.3
       Port 2222
       User bodnuti-deploy
       IdentityFile ~/.ssh/bodnuti-deploy
       IdentitiesOnly yes
   ```

7. Record the host key, and compare the fingerprint with step 3 before
   appending it:

   ```sh
   ssh-keyscan -p 2222 -t ed25519 10.1.0.3 > /tmp/k && ssh-keygen -lf /tmp/k
   cat /tmp/k >> ~/.ssh/known_hosts && rm /tmp/k
   ```

8. Print the `authorized_keys` line, built from the key file so a terminal
   paste cannot break it:

   ```sh
   printf 'command="/usr/bin/rrsync -wo /var/www/bodnuti.dev/reports",restrict %s\n' \
     "$(cat ~/.ssh/bodnuti-deploy.pub)"
   ```

### Back on the web host

9. Install that line as the account's only key:

   ```sh
   echo '<the line from step 8>' | sudo tee /home/bodnuti-deploy/.ssh/authorized_keys
   sudo chown bodnuti-deploy: /home/bodnuti-deploy/.ssh/authorized_keys
   sudo chmod 600 /home/bodnuti-deploy/.ssh/authorized_keys
   ```

   `rrsync` confines the key to the reports directory, and `-wo` makes it
   write-only: a compromised worker can replace the reports but not read
   anything back or run a shell.

10. Allow TCP 2222 from the worker in the web host's firewall, and the
    outbound rule in the worker's (table at the top). Keep the web host's
    2222 closed to everything else; scanners find it within minutes.

### Test from the worker

```sh
mkdir -p /tmp/t && echo ok > /tmp/t/test.html
rsync -a --delete /tmp/t/ bodnuti-web:./
curl -s https://bodnuti.dev/reports/test.html   # ok
ssh bodnuti-web ls                              # refused by rrsync
rm /tmp/t/test.html && rsync -a --delete /tmp/t/ bodnuti-web:./
```

### Configure bodnuti

Set the destination in `~/.config/bodnuti/watch.toml`. `rrsync` roots every
path at the directory in its forced command, so `./` is the reports
directory itself:

```toml
publish_to = "bodnuti-web:./"
retain_months = 6
```

`bodnuti publish` renders the reports into `~/state/site/` and copies them
with `rsync -a --delete`. Render once with `bodnuti publish --local` and look
at `~/state/site/index.html` before the first copy.

## 9. The watcher

`bodnuti poll` finds new series on lore, fetches and checks each one in the
image, and records the results under `~/state`; `bodnuti publish` renders and
copies them (§8). Both run on the host as `bodnuti`, which needs `git` (for
patch-ids) and `rsync`:

```
sudo apt install git rsync
```

The image must be current: the watcher runs `bodnuti check --json` inside it.
After loading a new image (§4), copy its binary to the host, as `bodnuti`:

```
mkdir -p ~/.local/bin
podman create --name bx localhost/bodnuti
podman cp bx:/usr/local/bin/bodnuti ~/.local/bin/bodnuti
podman rm bx
```

The image and the VM are both Debian trixie, so the binary runs on the host.

`~/.config/bodnuti/watch.toml` (designs/PHASE2.md §2). Start with one list
and add others once its reports look right:

```toml
image = "localhost/bodnuti"
state = "~/state"
cache = "~/cache"
settle_minutes = 15
initial_hours = 24
retain_months = 6
publish_to = "bodnuti-web:./"

[[list]]
name = "openembedded-core"
repo = "openembedded-core"

[repos]
openembedded-core = "https://git.openembedded.org/openembedded-core"
meta-openembedded = "https://git.openembedded.org/meta-openembedded"
bitbake = "https://git.openembedded.org/bitbake"
yocto-docs = "https://git.yoctoproject.org/yocto-docs"
```

Run it once by hand:

```
~/.local/bin/bodnuti poll --dry-run openembedded-core   # lore is reachable
~/.local/bin/bodnuti poll
~/.local/bin/bodnuti publish --local                    # look at ~/state/site/
~/.local/bin/bodnuti publish
```

Then run both from a timer. `~/.config/systemd/user/bodnuti-poll.service`:

```
[Unit]
Description=Poll lore, check new series, publish reports

[Service]
Type=oneshot
# "-": a failed list must not stop publishing the others.
ExecStart=-%h/.local/bin/bodnuti poll
ExecStart=%h/.local/bin/bodnuti publish
```

`~/.config/systemd/user/bodnuti-poll.timer`:

```
[Unit]
Description=Poll lore every 5 minutes after the previous run ends

[Timer]
OnBootSec=2min
OnUnitInactiveSec=5min

[Install]
WantedBy=timers.target
```

```
systemctl --user daemon-reload
systemctl --user enable --now bodnuti-poll.timer
journalctl --user -u bodnuti-poll -f
```

`OnUnitInactiveSec` counts from the end of the previous run, so a long run
of checks never overlaps the next. Keep `bodnuti-fetch.timer` (§7) enabled:
matching applied patches reads `origin/<branch>` in the clones.

## Not yet verified

- The image on x86. On arm64 the two benchmark checks above pass on a
  Hetzner worker in rootless Podman, read-only and with `--network none`.
