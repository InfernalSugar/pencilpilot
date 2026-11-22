#!/usr/bin/env bash
# Set up PencilPilot's relay on a fresh Ubuntu/Debian VPS (e.g. Hostinger KVM 1 or 2).
#
#   sudo ./install.sh relay.yourdomain.com you@yourdomain.com
#
# Builds iroh-relay from source (the same version the app uses), installs it as a
# service with its own user, opens the firewall (80/tcp, 443/tcp, 7842/udp) and starts it.
# Before running: point the domain's A (and AAAA) record at this server.
set -euo pipefail

DOMAIN=${1:?usage: install.sh <relay domain> <contact email>}
EMAIL=${2:?usage: install.sh <relay domain> <contact email>}
VERSION=1.5.0
HERE=$(cd "$(dirname "$0")" && pwd)

[ "$(id -u)" = 0 ] || { echo "run with sudo"; exit 1; }

echo "== building iroh-relay $VERSION (a few minutes on 2 cores)"
apt-get update -qq
apt-get install -y -qq build-essential pkg-config curl ufw >/dev/null
if ! command -v cargo >/dev/null; then
	curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"
cargo install iroh-relay --version "$VERSION" --features server --locked --root /usr/local

echo "== user, config, service"
id iroh-relay >/dev/null 2>&1 || useradd --system --home /var/lib/iroh-relay --shell /usr/sbin/nologin iroh-relay
install -d -o iroh-relay -g iroh-relay /var/lib/iroh-relay /var/lib/iroh-relay/certs
install -d /etc/iroh-relay
sed -e "s/relay.example.com/$DOMAIN/" -e "s/you@example.com/$EMAIL/" "$HERE/relay.toml" >/etc/iroh-relay/relay.toml
install -m 644 "$HERE/iroh-relay.service" /etc/systemd/system/iroh-relay.service

echo "== firewall"
ufw allow 22/tcp >/dev/null
ufw allow 80/tcp >/dev/null
ufw allow 443/tcp >/dev/null
ufw allow 7842/udp >/dev/null
ufw --force enable >/dev/null

systemctl daemon-reload
systemctl enable --now iroh-relay
sleep 5
systemctl --no-pager --lines=5 status iroh-relay || true
echo
echo "Done. Check it: curl -I https://$DOMAIN  (the first certificate can take a minute)"
echo "In the app: Settings → Drawing together → Relay: https://$DOMAIN"
