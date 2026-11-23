#!/usr/bin/env bash
# Set up PencilPilot's cloud keeper on the relay's VPS (deploy/relay), or any Ubuntu/Debian.
#
#   sudo ./install.sh <site public key> <relay URL>
#
# <site public key>: the website's pass key, the apps' PENCILPILOT_PASS_KEY (64 hex).
# <relay URL>: our own relay, e.g. https://relay.yourdomain.com (or "iroh" for iroh's).
#
# Builds the keeper from this checkout, installs it as a service with its own user, opens
# UDP 7843 and prints its id. It starts once its pass is in place (see README.md); run the
# script again after `git pull` to update it.
set -euo pipefail

SITE_KEY=${1:?usage: install.sh <site public key> <relay URL>}
RELAY=${2:?usage: install.sh <site public key> <relay URL>}
PORT=7843
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)

[ "$(id -u)" = 0 ] || { echo "run with sudo"; exit 1; }
[[ "$SITE_KEY" =~ ^[0-9a-fA-F]{64}$ ]] || { echo "the site key is 64 hex characters"; exit 1; }

echo "== building pencilpilot-keeper (a few minutes on 2 cores)"
apt-get update -qq
apt-get install -y -qq build-essential pkg-config curl ufw >/dev/null
if ! command -v cargo >/dev/null; then
	curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"
(cd "$ROOT" && cargo build --release --locked -p pencilpilot-keeper)
install -m 755 "$ROOT/target/release/pencilpilot-keeper" /usr/local/bin/pencilpilot-keeper

echo "== user, config, service"
id pencilpilot-keeper >/dev/null 2>&1 ||
	useradd --system --home /var/lib/pencilpilot-keeper --shell /usr/sbin/nologin pencilpilot-keeper
install -d -m 700 -o pencilpilot-keeper -g pencilpilot-keeper /var/lib/pencilpilot-keeper
install -d -m 750 -g pencilpilot-keeper /etc/pencilpilot-keeper
if [ ! -f /etc/pencilpilot-keeper/keeper.toml ]; then
	if [ "$RELAY" = iroh ]; then RELAYS='[]'; else RELAYS="[\"$RELAY\"]"; fi
	sed -e "s/^site_key = .*/site_key = \"$SITE_KEY\"/" \
		-e "s|^relays = .*|relays = $RELAYS|" \
		-e "s/^port = .*/port = $PORT/" \
		"$HERE/keeper.toml" >/etc/pencilpilot-keeper/keeper.toml
	chgrp pencilpilot-keeper /etc/pencilpilot-keeper/keeper.toml
	chmod 640 /etc/pencilpilot-keeper/keeper.toml
fi
install -m 644 "$HERE/pencilpilot-keeper.service" /etc/systemd/system/pencilpilot-keeper.service

echo "== firewall"
ufw allow 22/tcp >/dev/null
ufw allow "$PORT/udp" >/dev/null
ufw --force enable >/dev/null

# its id (the key is made now, owned by the service's user)
ID=$(sudo -u pencilpilot-keeper /usr/local/bin/pencilpilot-keeper --id /etc/pencilpilot-keeper/keeper.toml)
systemctl daemon-reload
systemctl enable pencilpilot-keeper >/dev/null
HOST=$(hostname -f)
echo
echo "The keeper's id: $ID"
if [ -f /etc/pencilpilot-keeper/pass.json ]; then
	systemctl restart pencilpilot-keeper
	sleep 2
	systemctl --no-pager --lines=5 status pencilpilot-keeper || true
else
	echo
	echo "Next: its pass. Where the site's PASS_SIGNING_KEY is, run"
	echo "  node apps/web/scripts/keeper-pass.mjs <PASS_SIGNING_KEY> $ID > pass.json"
	echo "copy pass.json to /etc/pencilpilot-keeper/pass.json here, then:"
	echo "  sudo chgrp pencilpilot-keeper /etc/pencilpilot-keeper/pass.json"
	echo "  sudo chmod 640 /etc/pencilpilot-keeper/pass.json"
	echo "  sudo systemctl restart pencilpilot-keeper"
fi
echo
echo "In the app: Settings → Drawing together → Cloud: $ID@$HOST:$PORT"
echo "(or build the app with PENCILPILOT_CLOUD_ADDR=$ID@$HOST:$PORT)"
