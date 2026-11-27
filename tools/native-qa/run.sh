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
#   SKIP_FILES=1 skip files and export (step 5)
#   PP_QA_FIXTURES  the test projects (tools/baseline/fixtures.py); without it they're built
#                   into <out-dir>/fixtures, with their reference renders in <out-dir>/refs
set -uo pipefail
# Everything runs on a private D-Bus session with no display: the system's own windows (the
# file dialog through the desktop portal, the file manager) can never reach a real screen.
if [ -z "${PP_QA_PRIVATE_BUS:-}" ]; then
  exec env -u DISPLAY -u WAYLAND_DISPLAY PP_QA_PRIVATE_BUS=1 PENCILPILOT_NO_SYSTEM_WINDOWS=1 \
    PENCILPILOT_DIALOG_ANSWER= dbus-run-session -- "$0" "$@"
fi
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
    cargo test --release --no-fail-fast -p pencilpilot-native --test canvas_qa --test canvas_tools_qa --test canvas_frames_qa -- --include-ignored --test-threads 1 \
    >"$out/canvas-tests.txt" 2>&1
  grep -E "^test |test result" "$out/canvas-tests.txt"
  if [ "${SKIP_HEADLESS:-0}" != 1 ]; then
    echo "== the canvas in a window on a headless mutter"
    python3 -I "$here/canvas_window.py" "$bin" "$out/canvas-window" >"$out/canvas-window.log" 2>&1
    echo "results: $out/canvas-window/canvas-window.json"
    pgrep -f "mutter --headless --no-x11 --wayland-display pp-qa-canvas" >/dev/null && echo "WARNING: a QA mutter is still running"
  fi
fi
if [ "${SKIP_FILES:-0}" != 1 ]; then
  echo "== files and export (step 5)"
  fx="${PP_QA_FIXTURES:-$out/fixtures}"
  refs="$out/refs"
  [ -f "$fx/drawings500.ppstudio" ] || python3 tools/baseline/fixtures.py "$fx"
  cli="${CARGO_TARGET_DIR:-$root/target}/release/ppstudio"
  [ -x "$cli" ] || cargo build -q --release -p pencilpilot-ppstudio-cli
  for n in empty layers20-4k drawings500; do
    [ -f "$refs/$n/0001.png" ] || "$cli" render "$fx/$n.ppstudio" "$refs/$n" --every >/dev/null
  done
  PP_QA_FIXTURES="$fx" PP_QA_REFS="$refs" PP_QA_OUT="$out/files" \
    PP_QA_DROP_CACHES="${DROP_CACHES:-}" \
    cargo test --release --no-fail-fast -p pencilpilot-native --test files_qa --test export_qa -- --include-ignored --test-threads 1 \
    >"$out/files-tests.txt" 2>&1
  grep -E "^test |test result" "$out/files-tests.txt"
  # one project per process for memory
  for n in empty layers20-4k drawings500; do
    PP_QA_PSS=$n PP_QA_FIXTURES="$fx" PP_QA_OUT="$out/files" \
      cargo test -q --release -p pencilpilot-native --test files_qa pss_after_open -- --include-ignored >/dev/null 2>&1
  done
  # no FFmpeg: hidden from the test in a mount namespace
  if command -v bwrap >/dev/null; then
    t=$(grep -o "/[^ ]*export_qa-[0-9a-f]*" "$out/files-tests.txt" | head -1)
    hide=()
    for f in /usr/bin/ffmpeg /usr/local/bin/ffmpeg /usr/bin/ffprobe; do [ -e "$f" ] && hide+=(--bind /dev/null "$f"); done
    PP_QA_FIXTURES="$fx" PP_QA_OUT="$out/files" bwrap --dev-bind / / "${hide[@]}" "$t" --include-ignored --test-threads 1 ffmpeg_missing 2>&1 | grep -E "^test "
  fi
  if [ "${SKIP_HEADLESS:-0}" != 1 ]; then
    echo "== files and export in a window on a headless mutter"
    btn=$(python3 -c "import json;d=json.load(open('$out/files/export-button.json'));print(json.dumps(d['export']), json.dumps(d['webm']))" 2>/dev/null)
    PP_QA_FIXTURES="$fx" DROP_CACHES="${DROP_CACHES:-}" \
      PP_QA_EXPORT_BUTTON="${btn%% *}" PP_QA_WEBM_CHOICE="${btn##* }" \
      python3 -I "$here/canvas_window.py" "$bin" "$out/files-window" open-cli open-menu quit scrub export-memory export-window >"$out/files-window.log" 2>&1
    echo "results: $out/files-window/canvas-window.json"
    pgrep -f "mutter --headless --no-x11 --wayland-display pp-qa-canvas" >/dev/null && echo "WARNING: a QA mutter is still running"
  fi
  echo "export checks against the file's frames: tools/native-qa/export_check.py (in $out/files/export-*.json)"
fi
echo "== done: $out"
