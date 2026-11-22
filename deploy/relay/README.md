# PencilPilot's relay

Drawing together connects the members' apps directly, peer to peer ([ADR-0013](../../docs/planning/adr/0013-solo-and-shared-projects.md)).
A relay introduces them and carries the traffic of the few that can't connect directly
(strict office or school networks). Everything it carries is encrypted end to end; it
sees only which apps talk, when and how much. It stores nothing.

Until this is running, the app uses iroh's free public relays (no uptime promise, run by
another company). Running our own keeps users' connection details with us and the uptime
in our hands.

## What it needs

- A small VPS: Hostinger **KVM 1** is plenty (KVM 2 if the website runs there too).
  Ubuntu 24.04 or Debian 12.
- A domain name for it, e.g. `relay.yourdomain.com`, with an **A record** (and AAAA for
  IPv6) pointing at the VPS. Set this first: the certificate needs it.
- Open ports: **80/tcp** (certificate check), **443/tcp** (the relay), **7842/udp**
  (helps apps find their public address). The script opens them in the VPS's firewall;
  if Hostinger's panel has its own firewall, open them there too.

## Set it up

On the VPS, as a user with sudo:

```sh
git clone https://github.com/InfernalSugar/pencilpilot.git
cd pencilpilot/deploy/relay
sudo ./install.sh relay.yourdomain.com you@yourdomain.com
```

It builds `iroh-relay` 1.5.0 from source (the version the app uses), installs it as the
`iroh-relay` service under its own user, gets a Let's Encrypt certificate, opens the
firewall and starts it.

Check it: `curl -I https://relay.yourdomain.com` answers, and
`systemctl status iroh-relay` says active. Logs: `journalctl -u iroh-relay -f`.

## Point the app at it

In the app: **Settings → Drawing together → Relay**, `https://relay.yourdomain.com`.
Everyone in a session should use the same relay (the invite code carries the host's
relay, so joiners reach it either way; set it on every app for the best results).
For testing, `PENCILPILOT_RELAY=https://relay.yourdomain.com` does the same.

## Files

| File | What |
| --- | --- |
| `relay.toml` | the relay's settings: Let's Encrypt, QUIC address discovery, rate limits (4 MB/s per app, bursts of 16 MB), metrics on localhost only |
| `iroh-relay.service` | the systemd service: own user, ports 80/443 without root, sandboxed, restarts on failure, 1 GB memory cap |
| `install.sh` | builds, installs and starts it (above) |

## Updating

When the app moves to a newer iroh, change `VERSION` in `install.sh` and run it again;
it rebuilds and restarts the service. The relay and apps should be on the same major
version (1.x).

## Tested

`crates/share`'s `own_relay` test runs this same relay server in-process (with a test
certificate) and checks that members who know each other only by id and relay join,
draw and hand the session over through it.
