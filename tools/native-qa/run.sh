#!/usr/bin/env bash
# The native shell's QA pass: design vs native pictures and measurements, the interaction
# tests, and idle / startup / memory / frame rate on a headless mutter. No window opens on
# any real screen.
#
#   tools/native-qa/run.sh [out-dir]            (default: target/native-qa)
#
# Environment:
#   DESIGN_DIR      the design's project folder (FullApp*.dc.html…); without it, the design
#                   renders already in <out-dir>/design are used, if any
#   DESIGN_RUNTIME  the design tool's runtime, served as support.js
#                   (default: $DESIGN_DIR/../artifact-type/dc-runtime.js)
#   PLAYWRIGHT_DIR  a node_modules folder with @playwright/test (see render-design.mjs)
#   DROP_CACHES     a command that drops the page cache, for a cold start (needs root)
#   SKIP_HEADLESS=1 skip the windowed runs; SKIP_TESTS=1 skip the interaction tests;
#   SKIP_CANVAS=1 skip the canvas's (step 4) tests and windowed runs
set -uo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
here="$root/tools/native-qa"
out=$(realpath -m "${1:-$root/target/native-qa}")
mkdir -p "$out/native" "$out/design" "$out/compare" "$out/frames"
cd "$root"

echo "== build"
cargo build --release -p pencilpilot-native 2>&1 | tail -1
bin="${CARGO_TARGET_DIR:-$root/target}/release/pencilpilot"

echo "== native renders"
for ws in animate draw colour review; do
  env -u DISPLAY -u WAYLAND_DISPLAY "$bin" --render "$ws" --out "$out/native/native-$ws.png" || echo "render $ws failed"
done

if [ -n "${DESIGN_DIR:-}" ]; then
  echo "== design renders (headless Chromium)"
  node "$here/render-design.mjs" "$DESIGN_DIR" "${DESIGN_RUNTIME:-$DESIGN_DIR/../artifact-type/dc-runtime.js}" "$out/design"
fi

echo "== design vs native"
python3 -I "$here/compare.py" "$out/design" "$out/native" "$out/compare" >"$out/compare/stdout.txt" 2>&1
echo "$(tail -1 "$out/compare/stdout.txt") (details: $out/compare/compare.md)"

if [ "${SKIP_TESTS:-0}" != 1 ]; then
  echo "== interaction tests"
  rm -f "$out/sweeps.log"
  PP_QA_LOG="$out/sweeps.log" PP_SAVE="$out/frames" \
    cargo test --release -p pencilpilot-native --test shell_qa -- --include-ignored --test-threads 1 \
    >"$out/tests.txt" 2>&1
  grep -E "^test |test result" "$out/tests.txt"
fi

if [ "${SKIP_HEADLESS:-0}" != 1 ]; then
  echo "== headless mutter: idle, startup, memory, frame rate"
  "$here/headless.sh" "$bin" "$out/headless" 2>>"$out/headless.err" | tail -40
  pgrep -f "mutter --headless --no-x11 --wayland-display pp-qa-" >/dev/null && echo "WARNING: a QA mutter is still running"
fi
if [ "${SKIP_CANVAS:-0}" != 1 ]; then
  echo "== the canvas (step 4): library tests"
  PP_QA_OUT="$out/canvas" \
    cargo test --release -p pencilpilot-native --test canvas_qa --test canvas_tools_qa --test canvas_frames_qa -- --include-ignored --test-threads 1 \
    >"$out/canvas-tests.txt" 2>&1
  grep -E "^test |test result" "$out/canvas-tests.txt"
  if [ "${SKIP_HEADLESS:-0}" != 1 ]; then
    echo "== the canvas in a window on a headless mutter"
    python3 -I "$here/canvas_window.py" "$bin" "$out/canvas-window" >"$out/canvas-window.log" 2>&1
    echo "results: $out/canvas-window/canvas-window.json"
    pgrep -f "mutter --headless --no-x11 --wayland-display pp-qa-canvas" >/dev/null && echo "WARNING: a QA mutter is still running"
  fi
fi
echo "== done: $out"
