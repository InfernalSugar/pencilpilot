#!/usr/bin/env bash
# Download the two models ref-service needs into crates/ref-filters/models/
# and verify their SHA-256s. The files are git-ignored: never commit them.
# See README.md for provenance.
#
#   * NSFW safety guard: Yahoo open_nsfw as ONNX (BSD-2-Clause), 22.5 MiB
#   * Picture sorter: OpenAI CLIP ViT-B/32 as ONNX (MIT), ~580 MiB
#     (ADR-0009: sorts reference pictures into category/topic, nothing else)
#
# Usage: fetch-model.sh [nsfw|clip|all]   (default: all)
set -euo pipefail

models="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/models"
what="${1:-all}"

# fetch <url> <sha256> <destination>
fetch() {
    local url="$1" sha="$2" out="$3"
    local dir
    dir="$(dirname "$out")"
    mkdir -p "$dir"
    if [[ -f "$out" ]] && echo "$sha  $out" | sha256sum --check --status; then
        echo "already present and verified: $out"
        return
    fi
    local tmp
    tmp="$(mktemp "$dir/.download.XXXXXX")"
    echo "downloading $url"
    if ! curl -fL --retry 3 --progress-bar -o "$tmp" "$url"; then
        rm -f "$tmp"
        exit 1
    fi
    if ! echo "$sha  $tmp" | sha256sum --check --status; then
        echo "SHA-256 mismatch for $out: got $(sha256sum "$tmp" | cut -d' ' -f1), want $sha" >&2
        rm -f "$tmp"
        exit 1
    fi
    chmod 644 "$tmp"
    mv "$tmp" "$out"
    echo "ok: $out ($(du -h "$out" | cut -f1))"
}

if [[ "$what" == nsfw || "$what" == all ]]; then
    fetch "https://huggingface.co/bluefoxcreation/open-nsfw/resolve/0fd53306e9c78e89b216aea7d30f65842a66d591/open-nsfw.onnx" \
        864bb37bf8863564b87eb330ab8c785a79a773f4e7c43cb96db52ed8611305fa \
        "$models/open_nsfw.onnx"
fi

if [[ "$what" == clip || "$what" == all ]]; then
    # ONNX export of openai/clip-vit-base-patch32 by Xenova (transformers.js),
    # pinned to one revision.
    clip="https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/6b7c523703ae26e39be4b03774966f8571c7f523"
    fetch "$clip/onnx/vision_model.onnx" \
        fd6e1402a588279d1723c7534d4bcba5bc0b14b47dfab0e46f8c47b8270d7d40 \
        "$models/clip-vit-b32/vision_model.onnx"
    fetch "$clip/onnx/text_model.onnx" \
        3f6571f5bad13a97c469c1622e1cfc4d9aef78b79fdbfcff804ca357bfada8cc \
        "$models/clip-vit-b32/text_model.onnx"
    fetch "$clip/vocab.json" \
        5047b556ce86ccaf6aa22b3ffccfc52d391ea4accdab9c2f2407da5b742d4363 \
        "$models/clip-vit-b32/vocab.json"
    fetch "$clip/merges.txt" \
        9fd691f7c8039210e0fced15865466c65820d09b63988b0174bfe25de299051a \
        "$models/clip-vit-b32/merges.txt"
fi
