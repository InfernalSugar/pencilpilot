#!/usr/bin/env python3
"""Scrubbing and playback with the test projects, in a window on a headless mutter (no window
on any real screen), for the GPU layers (Phase 4): what a frame change costs, and how
playback keeps time.

    PP_QA_FIXTURES=<dir with layers20-4k / drawings500 .ppstudio> \
        python3 -I gpu_layers.py <pencilpilot binary> <out-dir> [scrub|play ...] [--runs N]

It runs itself in its own D-Bus session with its own mutter (canvas_window.py's), at 165 Hz
and 60 Hz. Results: <out-dir>/gpu-layers.json.

- scrub: the project opened from the command line, then Right taps one display frame apart
  through the cut and Left back. A frame change's cost is the app's own `workNs` for the
  frames that show another sheet frame (the key, the interface's pass, showing the frame's
  drawings, compositing, the GPU); also each key to the frame shown with it, and steps never
  shown.
- play: Return, then 6 s of playback: timeline frames skipped or repeated, each frame's
  shown time against its place on the 24 fps schedule (the budget: the display frame
  nearest, so within half a refresh), the wait from Return to the first new frame shown,
  CPU (user + system) of the app while playing as a share of one core, PSS (peak), and the
  GPU layers' memory the app reports.
"""

import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import canvas_window as cw  # noqa: E402

FIXTURES = os.environ.get("PP_QA_FIXTURES", "")
# how long after the launch scrubbing starts (the project opened and shown by then; its
# drawings going up to the GPU meanwhile): PP_QA_SCRUB_WAIT seconds
SCRUB_WAIT = float(os.environ.get("PP_QA_SCRUB_WAIT", "2.5"))
VRAM = next((p for p in ("/sys/class/drm/card0/device/mem_info_vram_used",
                         "/sys/class/drm/card1/device/mem_info_vram_used") if os.path.exists(p)), None)


def vram_mib():
    """The GPU's memory in use, the whole machine's (MiB), where the driver says (amdgpu)."""
    if not VRAM:
        return None
    with open(VRAM) as f:
        return int(f.read()) / 2**20
# each project: Right taps through it (its cut, less one), and its cut
PROJECTS = {"drawings500": 99, "layers20-4k": 23}
CUT = {"drawings500": 100, "layers20-4k": 24}


def q(v):
    if not v:
        return None
    return {"n": len(v), "p50": round(cw.pct(v, 0.5), 2), "p95": round(cw.pct(v, 0.95), 2),
            "p99": round(cw.pct(v, 0.99), 2), "max": round(max(v), 2)}


def cpu_ticks(pid):
    with open(f"/proc/{pid}/stat") as f:
        parts = f.read().rsplit(")", 1)[1].split()
    return int(parts[11]) + int(parts[12])


def scrub(binary, out, name, hz, run):
    n = PROJECTS[name]
    m = cw.Mutter(out, hz)
    try:
        path = os.path.join(FIXTURES, f"{name}.ppstudio")
        app = cw.App(binary, m, out, f"gl-scrub-{name}-{hz}-{run}", 12, "animate", args=[path])
        time.sleep(SCRUB_WAIT)
        inp = cw.Input()
        inp.to(560, 330)
        time.sleep(0.3)
        period = 1 / hz
        sent = []
        for key in ("Right", "Left"):
            for _ in range(n):
                sent.append((cw.mono_ns(), key))
                inp.key(key)
                time.sleep(period)
            time.sleep(0.5)
        inp.stop()
        st = app.wait()
    finally:
        m.stop()
    fr = [f for f in st["frames"] if f["drawnNs"] >= sent[0][0]]
    # the frames that show another sheet frame than the one drawn before them
    before = [f for f in st["frames"] if f["drawnNs"] < sent[0][0]]
    last = before[-1]["sheetFrame"] if before else 0
    change_work, change_gpu = [], []
    for f in fr:
        if f["sheetFrame"] != last:
            change_work.append(f["workNs"] / 1e6)
            change_gpu.append(f["gpuNs"] / 1e6)
        last = f["sheetFrame"]
    shown = sorted((f for f in fr if f["shownNs"]), key=lambda f: f["shownNs"])
    lat, missing = [], 0
    for i, (t, _) in enumerate(sent):
        want = i + 1 if i < n else n - (i - n + 1)
        hit = next((f for f in shown if f["shownNs"] >= t and f["sheetFrame"] == want), None)
        if hit is None:
            missing += 1
        else:
            lat.append((hit["shownNs"] - t) / 1e6)
    changes = [b for a, b in zip(shown, shown[1:]) if b["sheetFrame"] != a["sheetFrame"]]
    gaps = [(b["shownNs"] - a["shownNs"]) / 1e6 for a, b in zip(changes, changes[1:])
            if b["shownNs"] - a["shownNs"] < 0.4e9]
    period_ms = 1000 / hz
    return {
        "frameChangeWorkMs": q(change_work), "frameChangeGpuMs": q(change_gpu),
        "keyToShownMs": q(lat), "frameChangeIntervalMs": q(gaps),
        "changesLate": sum(1 for g in gaps if g > 1.5 * period_ms),
        "steps": len(sent), "neverShown": missing, "lastSheetFrame": st["state"]["frame"],
        "displayFrameMs": round(period_ms, 2), "gpuLayers": st.get("gpuLayers"),
    }


def play(binary, out, name, hz, run, secs=6.0):
    m = cw.Mutter(out, hz)
    try:
        path = os.path.join(FIXTURES, f"{name}.ppstudio")
        vram0 = vram_mib()
        app = cw.App(binary, m, out, f"gl-play-{name}-{hz}-{run}", 12, "animate", args=[path])
        time.sleep(2.5)
        inp = cw.Input()
        inp.to(560, 330)
        time.sleep(0.3)
        pss_before = app.pss_kib()
        c0 = cpu_ticks(app.p.pid)
        inp.key("Return")
        t0 = cw.mono_ns()
        peak, vpeak = 0, 0
        while cw.mono_ns() - t0 < secs * 1e9:
            time.sleep(0.1)
            peak = max(peak, app.pss_kib() or 0)
            vpeak = max(vpeak, vram_mib() or 0)
        t1 = cw.mono_ns()
        c1 = cpu_ticks(app.p.pid)
        inp.key("Return")
        inp.stop()
        st = app.wait()
    finally:
        m.stop()
    hzt = os.sysconf("SC_CLK_TCK")
    cpu = (c1 - c0) / hzt / ((t1 - t0) / 1e9)
    fr = sorted((f for f in st["frames"] if f["shownNs"] and t0 <= f["drawnNs"] <= t1),
                key=lambda f: f["shownNs"])
    changes, last = [], None
    first_frame = None
    for f in fr:
        if last is None:
            last = f["sheetFrame"]
            continue
        if f["sheetFrame"] != last:
            changes.append((f["shownNs"], f["sheetFrame"]))
            first_frame = first_frame or f["shownNs"]
            last = f["sheetFrame"]
    if len(changes) < 10:
        return {"played": False, "changes": len(changes)}
    n_frames = CUT[name]
    k, ks, skips = 0, [0], 0
    for a, b in zip(changes, changes[1:]):
        step = (b[1] - a[1]) % n_frames
        if step != 1:
            skips += 1
        k += step
        ks.append(k)
    offs = sorted(c[0] - kk * 1e9 / 24 for c, kk in zip(changes, ks))
    t_start = offs[len(offs) // 2]
    err = [abs(c[0] - (t_start + kk * 1e9 / 24)) / 1e6 for c, kk in zip(changes, ks)]
    # against the cut's own clock (the app's: when each frame was due): signed, + is late
    clock = [(f["shownNs"] - f["dueNs"]) / 1e6 for f in fr if f.get("dueNs")]
    span = (changes[-1][0] - changes[0][0]) / 1e9
    half = 500 / hz
    return {
        "played": True, "framesPerSecond": round((len(changes) - 1) / span, 3),
        "skippedOrRepeated": skips, "offScheduleMs": q(err), "halfRefreshMs": round(half, 2),
        "onTime": max(err) <= half + 0.5,
        "vsClockMs": clock and {"mean": round(sum(clock) / len(clock), 2), "min": round(min(clock), 2),
                                "max": round(max(clock), 2), "first": [round(c, 2) for c in clock[:3]]},
        "firstFrameWaitMs": round((first_frame - t0) / 1e6, 1),
        "cpuOfOneCore": round(cpu, 3),
        "pssMiB": {"before": pss_before and round(pss_before / 1024, 1), "peak": round(peak / 1024, 1)},
        "gpuLayers": st.get("gpuLayers"),
        "vramMiBMachineDelta": vram0 is not None and round(vpeak - vram0, 1),
    }


def main():
    args = [a for a in sys.argv[1:]]
    if len(args) < 2:
        print(__doc__)
        sys.exit(2)
    if args[0] != "--inner":
        binary, out = os.path.realpath(args[0]), os.path.realpath(args[1])
        os.makedirs(out, exist_ok=True)
        env = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
        env["PENCILPILOT_NO_SYSTEM_WINDOWS"] = "1"
        env["PYTHONDONTWRITEBYTECODE"] = "1"
        os.execvpe("dbus-run-session", ["dbus-run-session", "--", sys.executable, "-I",
                                        os.path.realpath(__file__), "--inner", binary, out] + args[2:], env)
    binary, out = args[1], args[2]
    rest = args[3:]
    runs = 3
    if "--runs" in rest:
        i = rest.index("--runs")
        runs = int(rest[i + 1])
        rest = rest[:i] + rest[i + 2:]
    which = rest or ["scrub", "play"]
    projects = [p for p in PROJECTS if not os.environ.get("PP_QA_ONLY") or p == os.environ["PP_QA_ONLY"]]
    path = os.path.join(out, "gpu-layers.json")
    res = json.load(open(path)) if os.path.exists(path) else {}
    for w in which:
        for name in projects:
            for hz in (165, 60):
                for run in range(runs):
                    key = f"{w}-{name}-{hz}-{run}"
                    try:
                        r = (scrub if w == "scrub" else play)(binary, out, name, hz, run)
                    except Exception as e:  # one run's failure is a result too
                        r = {"error": repr(e)}
                    res[key] = r
                    json.dump(res, open(path, "w"), indent=1)
                    print(key, json.dumps(r), flush=True)


if __name__ == "__main__":
    main()
