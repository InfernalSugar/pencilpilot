#!/usr/bin/env bash
# Idle, startup, memory and frame rate of the windowed app, on a headless mutter only (no
# window on any real screen).
#
#   tools/native-qa/headless.sh <pencilpilot binary> <out-dir>
#
# DROP_CACHES: a command that drops the page cache (it needs root, e.g.
# `sudo sh -c "echo 3 > /proc/sys/vm/drop_caches"`); when set, a cold start is measured too.
# Writes <out-dir>/headless.json and prints it.
set -euo pipefail

if [ "${1:-}" != "--inner" ]; then
  bin=$(realpath "$1"); out=$(realpath -m "$2"); mkdir -p "$out"
  exec env -u DISPLAY -u WAYLAND_DISPLAY dbus-run-session -- bash "$(realpath "$0")" --inner "$bin" "$out"
fi
bin=$2; out=$3
here=$(dirname "$(realpath "$0")")
name="pp-qa-$$"
mpid=""
stop_mutter() {
  if [ -n "$mpid" ]; then kill "$mpid" 2>/dev/null || true; wait "$mpid" 2>/dev/null || true; mpid=""; fi
}
trap stop_mutter EXIT
start_mutter() { # <WxH@Hz>
  stop_mutter
  mutter --headless --no-x11 --wayland-display "$name" --virtual-monitor "$1" >>"$out/mutter.log" 2>&1 &
  mpid=$!
  for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/$name" ] && break; sleep 0.05; done
  sleep 0.5
}
app() { WAYLAND_DISPLAY="$name" "$bin" "$@"; }
# the app in the background; its pid in $apid (not a subshell's)
app_bg() { WAYLAND_DISPLAY="$name" "$bin" "$@" & apid=$!; }
field() { python3 -I -c "import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])" "$1" "$2"; }
pss() { awk '/^Pss:/ {print $2}' "/proc/$1/smaps_rollup" 2>/dev/null || echo null; }
rss() { awk '/^Rss:/ {print $2}' "/proc/$1/smaps_rollup" 2>/dev/null || echo null; }

# 1. idle at 165 Hz, 1440x900: frames after startup, and memory while it runs
start_mutter 1440x900@165
app_bg --quit-after 6 --stats "$out/idle-165.json"
sleep 2; pss_2=$(pss $apid); rss_2=$(rss $apid)
sleep 3; pss_5=$(pss $apid); rss_5=$(rss $apid)
wait $apid

# 2. warm startup: five runs
warm=()
for i in 1 2 3 4 5; do
  app --quit-after 0.6 --stats "$out/warm-$i.json"
  warm+=("$(field "$out/warm-$i.json" startupToFirstFrameMs)")
done

# 3. cold startup (page cache dropped)
cold=null
if [ -n "${DROP_CACHES:-}" ]; then
  sync; bash -c "$DROP_CACHES" >/dev/null 2>&1
  app --quit-after 0.8 --stats "$out/cold.json"
  cold=$(field "$out/cold.json" startupToFirstFrameMs)
fi

# 4. frame rate while something changes every frame: the pointer moves between two tool
#    buttons 500 times a second. The window (1280x800, the app's default) opens centred on
#    the 1440x900 monitor, at (80, 50). (On a monitor its own size, mutter reports no frame
#    presented at all, so that can't be used.)
fps() { # <Hz> (sets $rate; no subshell, so mutter's pid stays known)
  start_mutter "1440x900@$1"
  app_bg --quit-after 5 --stats "$out/fps-$1.json"
  sleep 1.2
  python3 -I "$here/inject.py" hover 3 128 160 128 210 2 2>>"$out/inject.log"
  wait $apid
  rate=$(python3 -I -c "
import json,sys
s=json.load(open(sys.argv[1]))
print(round((s['framesPresented']-2)/3.0,1))" "$out/fps-$1.json")
}
fps 165; fps165=$rate
fps 60; fps60=$rate
stop_mutter

python3 -I - "$out" "$pss_2" "$pss_5" "$rss_2" "$rss_5" "$cold" "$fps165" "$fps60" "${warm[@]}" <<'PY'
import json, statistics, sys
out, pss2, pss5, rss2, rss5, cold, f165, f60, *warm = sys.argv[1:]
idle = json.load(open(f"{out}/idle-165.json"))
num = lambda v: None if v in ("null", "") else float(v)
warm = [float(w) for w in warm if w != "None"]
res = {
    "idle165": idle,
    "pssKiB": {"at2s": num(pss2), "at5s": num(pss5)},
    "rssKiB": {"at2s": num(rss2), "at5s": num(rss5)},
    "startupWarmMs": {"runs": warm, "median": statistics.median(warm) if warm else None},
    "startupColdMs": num(cold),
    "fpsWhileHovering": {"165": num(f165), "60": num(f60)},
    "budgets": {
        "idleFramesAfterStartup": "0 (at most 2 counted: the first, and one when system fonts join)",
        "startupWarmMs": 200, "startupColdMs": 500, "memoryMiB": 150, "fps": "the display's rate",
    },
}
json.dump(res, open(f"{out}/headless.json", "w"), indent=2)
print(json.dumps(res, indent=2))
PY
