# PencilPilot's cloud keeper

Drawing together connects the members' apps directly ([ADR-0013](../../docs/planning/adr/0013-solo-and-shared-projects.md)).
The keeper is the cloud ([ADR-0014](../../docs/planning/adr/0014-cloud-keeper.md)): an
always-on member of every shared project it's asked to keep. It receives every edit and
keeps the latest snapshot plus the edits since, so members can catch up when no one else
is online, open their shared projects on any computer (home's Cloud tab), and meet in one
session instead of each hosting their own copy. It never hosts or draws: live drawing stays
between the apps. Solo projects never reach it.

**What it can see:** what it keeps is stored as received (no end-to-end encryption yet),
so whoever runs this server can read the shared projects kept on it. Only a project's
members can get it from the keeper.

## What it needs

- The relay's VPS ([deploy/relay](../relay/README.md)) is fine: Ubuntu 24.04 or Debian 12.
  The keeper is light (one process, memory capped at 1 GB); disk grows with the projects
  (1 GB per account at most by default).
- Open port: **7843/udp** (the script opens it; open it in Hostinger's panel too if it has
  its own firewall).
- The website's pass key pair (`node apps/web/scripts/pass-key.mjs`): the public half for
  the keeper, the private half (`PASS_SIGNING_KEY`) to sign the keeper's pass once.

## Set it up

On the VPS, as a user with sudo, in a checkout of this repository:

```sh
cd pencilpilot/deploy/keeper
sudo ./install.sh <PENCILPILOT_PASS_KEY> https://relay.yourdomain.com
```

It builds `pencilpilot-keeper` from the checkout, installs it as the `pencilpilot-keeper`
service under its own user (config in `/etc/pencilpilot-keeper`, data in
`/var/lib/pencilpilot-keeper`), opens UDP 7843 and prints **the keeper's id**.

Then its pass: wherever the site's `PASS_SIGNING_KEY` is (keep it off the VPS if the site
runs elsewhere),

```sh
node apps/web/scripts/keeper-pass.mjs <PASS_SIGNING_KEY> <the keeper's id> 365 > pass.json
```

Copy `pass.json` to `/etc/pencilpilot-keeper/pass.json` on the VPS, then
`sudo chgrp pencilpilot-keeper /etc/pencilpilot-keeper/pass.json`,
`sudo chmod 640 /etc/pencilpilot-keeper/pass.json` and
`sudo systemctl restart pencilpilot-keeper`. The pass lasts a year; the keeper's log warns
a month before it runs out.

Check it: `systemctl status pencilpilot-keeper` says active and its log
(`journalctl -u pencilpilot-keeper -f`) says `keeper <id> listening`.

## Point the apps at it

The address is `<id>@<the VPS's name>:7843` (the script prints it).

- Release builds: set the repository variable `PENCILPILOT_CLOUD_ADDR` to it (the App
  workflow builds it in, like `PENCILPILOT_SITE_URL`).
- One app: **Settings → Drawing together → Cloud**, or `PENCILPILOT_CLOUD=…` when running
  it. `off` turns the cloud off.

## Settings (`keeper.toml`)

| Setting | What |
| --- | --- |
| `data`, `key`, `pass` | where projects are kept, its computer key (made on the first start), its pass |
| `site_key` | the website's public key for passes (the apps' `PENCILPILOT_PASS_KEY`) |
| `port` | the UDP port (7843) |
| `relays` | our own relay, for apps that can't reach it directly (`[]`: iroh's public relays) |
| `limits.account_mb` | what one account's projects may take (1024) |
| `limits.projects_per_account` | projects one account may have kept (200) |
| `limits.compact_after_mb`, `limits.compact_every_minutes` | when to ask for a new snapshot (32 MB of edits, or every 30 minutes while people draw) |

## Updating

`git pull`, then run `install.sh` again: it rebuilds, keeps the config and data, and
restarts the service. The keeper and apps must speak the same protocol versions
(`pencilpilot/share/2`, `pencilpilot/cloud/1`).

## Backups

Everything is in `/var/lib/pencilpilot-keeper`: the key (keep it: the id is the address
apps use) and `projects/<id>/` per project. Copying the folder while it runs is safe enough
(files are replaced by renaming); stop the service for an exact copy.

## Try it on one computer

```sh
cargo build -p pencilpilot-keeper
mkdir -p /tmp/keeper && cat > /tmp/keeper/keeper.toml <<'EOF'
data = "/tmp/keeper/data"
key = "/tmp/keeper/key"
pass = "/tmp/keeper/pass.json"
# the website's development key (debug apps and `pnpm --filter @pencilpilot/web dev` use it)
site_key = "3b43523f310add603b4687893fafc8f094b5772e6a73d614a49b1a845c0045d0"
port = 7843
relays = []
EOF
ID=$(target/debug/pencilpilot-keeper --id /tmp/keeper/keeper.toml)
# the development signing key (public knowledge: apps/web/src/lib/server/pass.ts)
node apps/web/scripts/keeper-pass.mjs 62e1111add10abf5114b66e2caee869c95d97cf13b95d154524efcfee558cbf6 "$ID" 30 > /tmp/keeper/pass.json
target/debug/pencilpilot-keeper /tmp/keeper/keeper.toml
```

Then run the website (`pnpm --filter @pencilpilot/web dev`) and two apps
(`PENCILPILOT_CLOUD=$ID@127.0.0.1:7843 PENCILPILOT_SHARE_PROFILE=b pnpm app` for the second),
sign both in, host in one and join from the other, leave both, and Rejoin from either:
it catches up from the keeper.

## Tested

`crates/keeper`'s tests run the keeper in-process with apps on the same computer: joining,
keeping every edit, a member catching up after everyone left, two members coming back at
once meeting in one session, compaction, the space limit, non-members refused, and the
keeper following a host hand-over.
