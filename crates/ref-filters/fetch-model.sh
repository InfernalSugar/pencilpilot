#!/usr/bin/env bash
# Download the NSFW safety-guard model (Yahoo open_nsfw as ONNX, BSD-2-Clause)
# into crates/ref-filters/models/ and verify its SHA-256. The file is
# git-ignored: never commit it. See README.md for provenance.
set -euo pipefail

URL="https://huggingface.co/bluefoxcreation/open-nsfw/resolve/0fd53306e9c78e89b216aea7d30f65842a66d591/open-nsfw.onnx"
SHA256="864bb37bf8863564b87eb330ab8c785a79a773f4e7c43cb96db52ed8611305fa"

dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/models"
out="$dir/open_nsfw.onnx"
mkdir -p "$dir"

check() { echo "$SHA256  $1" | sha256sum --check --status; }

if [[ -f "$out" ]] && check "$out"; then
    echo "already present and verified: $out"
    exit 0
fi

tmp="$(mktemp "$dir/.open_nsfw.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
echo "downloading $URL"
curl -fL --retry 3 --progress-bar -o "$tmp" "$URL"
if ! check "$tmp"; then
    echo "SHA-256 mismatch: got $(sha256sum "$tmp" | cut -d' ' -f1), want $SHA256" >&2
    exit 1
fi
chmod 644 "$tmp"
mv "$tmp" "$out"
trap - EXIT
echo "ok: $out ($(du -h "$out" | cut -f1))"
