#!/usr/bin/env python3
"""The sound in the windowed app on a headless mutter (no window on any real screen, no
sound out of any speaker: the app plays to its silent output, which records each buffer it
played and when it would be heard).

    python3 -I sound_window.py <pencilpilot binary> <fixture.ppstudio> <out-dir> [scenario ...]

The fixture: `PP_SOUND_FIXTURE=<path> cargo test --release -p pencilpilot-native -- --ignored
sound_fixture` (a 1500-frame cut at 24 fps under 65 s of music, 1920 x 1080).

Scenarios (default: all):
- sync-165, sync-60: Play (Return) for 61 s at 165 / 60 Hz; for every frame shown, the
  sample heard at that moment against the frame's own first sample (offset, ms), its drift
  over the minute (the offsets' slope), and how long after Play the sound was first heard.
- idle: the project open, nothing happening for 4 s: frames drawn (0), and the sound's
  threads asleep (CPU time of the process over those seconds).

It runs in its own D-Bus session with its own mutter (canvas_window.py's), which it kills.
"""

import importlib.util
import json
import os
import statistics
import sys
import time

HERE = os.path.dirname(os.path.realpath(__file__))
_spec = importlib.util.spec_from_file_location("canvas_window", os.path.join(HERE, "canvas_window.py"))
cw = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cw)


def heard(bufs, t):
    """The scene sample (at 48 kHz) heard at CLOCK_MONOTONIC `t`, from the output's buffers
    ([heardNs, pos|None, peak, frames]); None in silence or outside them."""
    lo, hi = 0, len(bufs) - 1
    if not bufs or t < bufs[0][0]:
        return None
    while lo < hi:
        mid = (lo + hi + 1) // 2
        if bufs[mid][0] <= t:
            lo = mid
        else:
            hi = mid - 1
    b = bufs[lo]
    dt = (t - b[0]) / 1e9
    if b[1] is None or dt * 48000 >= b[3]:
        return None
    return b[1] + dt * 48000


def offsets(st, t_from, t_to):
    """For every frame first shown between two times: (seconds since the first, offset ms)."""
    snd = st.get("sound") or {}
    bufs = snd.get("buffers") or []
    fps, cut0 = snd.get("fps", 24), snd.get("cutStart", 0)
    fr = sorted((f for f in st["frames"] if f["shownNs"] and t_from <= f["shownNs"] <= t_to),
                key=lambda f: f["shownNs"])
    changes, last = [], None
    for f in fr:
        if f["sheetFrame"] != last:
            changes.append((f["shownNs"], f["sheetFrame"]))
            last = f["sheetFrame"]
    # (the first: the frame showing when Play was pressed)
    changes = changes[1:]
    # (playback loops over the cut: positions are within it, so a buffer running over its
    # end is measured modulo the cut's length)
    span = (max(f["sheetFrame"] for f in st["frames"]) + 1) * 48000 / fps
    offs = []
    for t, f in changes:
        pos = heard(bufs, t)
        if pos is not None:
            want = (cut0 + f) * 48000 / fps
            d = (pos - want + span / 2) % span - span / 2
            offs.append(((t - changes[0][0]) / 1e9, d / 48.0))
    return changes, offs


def sync(binary, fixture, out, hz, secs=61.0):
    """A first play of 2 s (the sound learns how the frames reach the screen), then the
    measured one."""
    m = cw.Mutter(out, hz)
    try:
        app = cw.App(binary, m, out, f"sound-sync-{hz}", secs + 9, "animate", args=[fixture])
        time.sleep(3.0)
        inp = cw.Input()
        inp.to(560, 330)
        time.sleep(0.05)
        inp.key("Return")
        t_first = cw.mono_ns()
        time.sleep(2.0)
        inp.key("Return")
        t_first_end = cw.mono_ns()
        time.sleep(0.6)
        inp.key("Return")
        t_key = cw.mono_ns()
        time.sleep(secs)
        # (stopped before the app closes: what it played is in its stats)
        inp.key("Return")
        time.sleep(0.3)
        inp.stop()
        st = app.wait()
    finally:
        m.stop()
    snd = st.get("sound") or {}
    bufs = snd.get("buffers") or []
    _, first = offsets(st, t_first, t_first_end)
    changes, offs = offsets(st, t_key, t_key + int(secs * 1e9))
    if len(offs) < 100:
        return {"played": False, "frames": len(changes), "measured": len(offs), "output": snd.get("output")}
    ms = [o[1] for o in offs]
    # drift: the offsets' least-squares slope, ms a minute
    tx = [o[0] for o in offs]
    mt, mo = statistics.fmean(tx), statistics.fmean(ms)
    slope = sum((a - mt) * (b - mo) for a, b in zip(tx, ms)) / max(1e-9, sum((a - mt) ** 2 for a in tx))
    first5 = [b for a, b in offs if a < 5]
    last5 = [b for a, b in offs if a > offs[-1][0] - 5]
    plays = snd.get("playsNs") or []
    play = plays[-1] if plays else t_key
    first_heard = next((b[0] for b in bufs if b[0] >= play and b[2] > 0), None)
    refresh = 1000.0 / hz
    absms = [abs(v) for v in ms]
    return {
        "output": snd.get("output"),
        "outputLatencyMs": round(snd.get("latencyNs", 0) / 1e6, 2),
        "framesShown": len(changes),
        "framesMeasured": len(offs),
        "seconds": round(offs[-1][0] - offs[0][0], 1),
        "shiftAtPlaysMs": snd.get("playShiftsMs"), "shiftMs": snd.get("shiftMs"),
        "firstPlayFirstFramesOffsetMs": [round(o[1], 2) for o in first[:8]],
        "firstPlayMaxAbsAfter3FramesMs": round(max(abs(o[1]) for o in first[3:]), 2) if len(first) > 3 else None,
        "firstFramesOffsetMs": [round(o[1], 2) for o in offs[:8]],
        "offsetMs": {"p50": round(pct(ms, .5), 2), "p95abs": round(pct(absms, .95), 2),
                     "maxAbs": round(max(absms), 2), "min": round(min(ms), 2), "max": round(max(ms), 2)},
        "driftMsPerMinute": round(slope * 60, 3),
        "meanFirst5sMs": round(statistics.fmean(first5), 2), "meanLast5sMs": round(statistics.fmean(last5), 2),
        "playToFirstSoundMs": round((first_heard - play) / 1e6, 1) if first_heard else None,
        "refreshMs": round(refresh, 2),
        "pass": max(absms) <= refresh and (first_heard is not None and (first_heard - play) / 1e6 <= 50)
                and abs(slope * 60) < 1.0,
    }


def pct(v, p):
    return cw.pct(v, p)


def proc_cpu_s(pid):
    try:
        with open(f"/proc/{pid}/stat") as f:
            parts = f.read().rsplit(")", 1)[1].split()
        return (int(parts[11]) + int(parts[12])) / os.sysconf("SC_CLK_TCK")
    except OSError:
        return None


def idle(binary, fixture, out):
    m = cw.Mutter(out, 165)
    try:
        app = cw.App(binary, m, out, "sound-idle", 9, "animate", args=[fixture])
        # (opened, decoded, and a frame clicked: a snippet, then the output pauses)
        time.sleep(3.0)
        inp = cw.Input()
        inp.key("Right")
        time.sleep(2.5)
        inp.stop()
        t0 = cw.mono_ns()
        c0 = proc_cpu_s(app.p.pid)
        time.sleep(3.0)
        c1 = proc_cpu_s(app.p.pid)
        t1 = cw.mono_ns()
        st = app.wait()
    finally:
        m.stop()
    snd = st.get("sound") or {}
    n = len(cw.frame_numbers(st, t0, t1))
    snippet = [b for b in snd.get("buffers") or [] if b[2] > 0]
    cpu = None if c0 is None or c1 is None else round((c1 - c0) / 3.0 * 100, 3)
    return {"framesDrawnWhileIdle3s": n, "cpuPercentOfACoreIdle": cpu,
            "snippetBuffersHeard": len(snippet),
            "pass": n == 0 and cpu is not None and cpu < 1.0}


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        sys.exit(2)
    if sys.argv[1] != "--inner":
        binary, fixture, out = (os.path.realpath(a) for a in sys.argv[1:4])
        os.makedirs(out, exist_ok=True)
        env = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
        env["PENCILPILOT_NO_SYSTEM_WINDOWS"] = "1"
        env["PENCILPILOT_NO_SOUND"] = "1"
        os.execvpe("dbus-run-session", ["dbus-run-session", "--", sys.executable, "-I",
                                        os.path.realpath(__file__), "--inner", binary, fixture, out]
                   + sys.argv[4:], env)
    binary, fixture, out = sys.argv[2:5]
    names = sys.argv[5:] or ["sync-165", "sync-60", "idle"]
    res = {}
    for n in names:
        t = time.monotonic()
        try:
            if n.startswith("sync-"):
                r = sync(binary, fixture, out, int(n.split("-")[1]))
            elif n == "idle":
                r = idle(binary, fixture, out)
            else:
                r = {"error": f"no scenario {n}"}
        except Exception as e:  # (a scenario's failure is a result too)
            r = {"error": repr(e)}
        r["seconds"] = round(time.monotonic() - t, 1)
        res[n] = r
        print(n, json.dumps(r, indent=1), flush=True)
    path = os.path.join(out, "sound-window.json")
    old = json.load(open(path)) if os.path.exists(path) else {}
    old.update(res)
    json.dump(old, open(path, "w"), indent=2)


if __name__ == "__main__":
    main()
