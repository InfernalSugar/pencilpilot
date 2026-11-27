#!/usr/bin/env python3
"""The canvas in a real window on a headless mutter (no window on any real screen): strokes
through a zoomed, turned and flipped view, the view not painting, pen to screen, the frame rate
while drawing, zooming and panning, idle, memory over hundreds of strokes, playback.

    python3 -I canvas_window.py <pencilpilot binary> <out-dir> [scenario ...]

Scenarios (default: all): view, latency, viewfps, idle, memory, playback, transform.
It runs itself in its own D-Bus session (`dbus-run-session`) with its own mutter, which it
kills; each scenario's results go to <out-dir>/<scenario>.json (and the app's stats and
saved documents next to them); <out-dir>/canvas-window.json has them all.

Input goes through mutter's RemoteDesktop API (relative pointer motion from the screen's
top-left corner, buttons, keysyms). The app's default 1280 x 800 window opens centred on the
1440 x 900 virtual monitor, at (80, 50): window = screen - (80, 50). The `view` scenario checks
that offset from where the stroke lands.
"""

import json
import math
import os
import signal
import subprocess
import sys
import time

MON_W, MON_H = 1440, 900
WIN_OFF = (80, 50)
BTN_LEFT, BTN_MIDDLE = 0x110, 0x112
KEYSYM = {
    "0": 0x30, "1": 0x31, "plus": 0x2B, "minus": 0x2D, "equal": 0x3D, "h": 0x68,
    "space": 0x20, "Shift_L": 0xFFE1, "Control_L": 0xFFE3, "Escape": 0xFF1B, "z": 0x7A,
    "Return": 0xFF0D, "m": 0x6D, "v": 0x76, "b": 0x62,
}


# ------------------------------------------------------------------------------------------
# mutter and the app

class Mutter:
    def __init__(self, out, hz):
        self.name = f"pp-qa-canvas-{os.getpid()}"
        self.log = open(os.path.join(out, "mutter.log"), "a")
        self.p = subprocess.Popen(
            ["mutter", "--headless", "--no-x11", "--wayland-display", self.name,
             "--virtual-monitor", f"{MON_W}x{MON_H}@{hz}"],
            stdout=self.log, stderr=self.log)
        sock = os.path.join(os.environ["XDG_RUNTIME_DIR"], self.name)
        for _ in range(200):
            if os.path.exists(sock):
                break
            time.sleep(0.05)
        time.sleep(0.5)

    def stop(self):
        if self.p.poll() is None:
            self.p.terminate()
            try:
                self.p.wait(5)
            except subprocess.TimeoutExpired:
                self.p.kill()
                self.p.wait()


class App:
    def __init__(self, binary, mutter, out, tag, secs, workspace="animate"):
        self.stats = os.path.join(out, f"{tag}-stats.json")
        self.doc = os.path.join(out, f"{tag}-doc.png")
        env = dict(os.environ, WAYLAND_DISPLAY=mutter.name)
        self.p = subprocess.Popen(
            [binary, "--quit-after", str(secs), "--stats", self.stats, "--save-doc", self.doc,
             "--workspace", workspace], env=env)
        self.t0 = time.monotonic()

    def pss_kib(self):
        try:
            with open(f"/proc/{self.p.pid}/smaps_rollup") as f:
                for line in f:
                    if line.startswith("Pss:"):
                        return int(line.split()[1])
        except OSError:
            return None

    def wait(self):
        self.p.wait()
        with open(self.stats) as f:
            return json.load(f)


# ------------------------------------------------------------------------------------------
# input

class Input:
    def __init__(self):
        import dbus  # (the system's python-dbus)
        bus = dbus.SessionBus()
        name = "org.gnome.Mutter.RemoteDesktop"
        rd = bus.get_object(name, "/org/gnome/Mutter/RemoteDesktop")
        path = dbus.Interface(rd, name).CreateSession()
        self.s = dbus.Interface(bus.get_object(name, path), name + ".Session")
        self.s.Start()
        self.dbus = dbus
        self.at = (0.0, 0.0)
        # a new session's first input is dropped for about the first 0.2 s (measured: a move
        # and a key sent 0.1 s after Start never reached the app)
        time.sleep(0.4)
        self.home()
        # (and the first key: a Shift tap goes first)
        self.key("Shift_L")
        time.sleep(0.05)

    def home(self):
        self.s.NotifyPointerMotionRelative(self.dbus.Double(-100000), self.dbus.Double(-100000))
        self.at = (0.0, 0.0)

    def to(self, x, y):
        """The pointer to window point (x, y) (logical)."""
        sx, sy = x + WIN_OFF[0], y + WIN_OFF[1]
        dx, dy = sx - self.at[0], sy - self.at[1]
        if dx or dy:
            self.s.NotifyPointerMotionRelative(self.dbus.Double(dx), self.dbus.Double(dy))
        self.at = (sx, sy)

    def button(self, b, down):
        self.s.NotifyPointerButton(self.dbus.Int32(b), self.dbus.Boolean(down))

    def key(self, name, down=None):
        k = self.dbus.UInt32(KEYSYM[name])
        if down is None:
            self.s.NotifyKeyboardKeysym(k, True)
            self.s.NotifyKeyboardKeysym(k, False)
        else:
            self.s.NotifyKeyboardKeysym(k, self.dbus.Boolean(down))

    def scroll(self, steps):
        # axis 0: vertical
        self.s.NotifyPointerAxisDiscrete(self.dbus.UInt32(0), self.dbus.Int32(steps))

    def drag(self, pts, interval=0.004, button=BTN_LEFT):
        self.to(*pts[0])
        time.sleep(0.02)
        self.button(button, True)
        for p in pts[1:]:
            time.sleep(interval)
            self.to(*p)
        time.sleep(interval)
        self.button(button, False)

    def stop(self):
        self.s.Stop()


def line(a, b, n):
    return [(a[0] + (b[0] - a[0]) * i / n, a[1] + (b[1] - a[1]) * i / n) for i in range(n + 1)]


def arc(c, r, a0, a1, n):
    return [(c[0] + r * math.cos(a0 + (a1 - a0) * i / n), c[1] + r * math.sin(a0 + (a1 - a0) * i / n))
            for i in range(n + 1)]


# ------------------------------------------------------------------------------------------
# what came out

def view_to_screen(st, d):
    """Document point -> window point (logical), the app's View::to_screen."""
    v, (cx, cy, cw, ch) = st["view"], st["canvasRect"]
    dw, dh = v["docSize"]
    vx = (d[0] - dw / 2) * v["zoom"]
    vy = (d[1] - dh / 2) * v["zoom"]
    if v["flip"]:
        vx = -vx
    c, s = math.cos(v["angle"]), math.sin(v["angle"])
    return (cx + cw / 2 + v["pan"][0] + c * vx - s * vy, cy + ch / 2 + v["pan"][1] + s * vx + c * vy)


def ink(png, base=None):
    """The document's pixels with paint on them; with `base` (the document the app starts
    with, the demo's drawings), the pixels that differ from it."""
    import numpy as np
    from PIL import Image
    a = np.asarray(Image.open(png).convert("RGBA")).astype(int)
    if base:
        b = np.asarray(Image.open(base).convert("RGBA")).astype(int)
        ys, xs = np.nonzero(np.abs(a - b).max(axis=2) > 3)
        return xs + 0.5, ys + 0.5
    # (a mouse stroke with the 2B pencil at pressure 0.5 is light and grainy)
    ys, xs = np.nonzero((a[:, :, 0] < 250) & (a[:, :, 3] > 0))
    return xs + 0.5, ys + 0.5


BASE = {}


def baseline(binary, m, out, ws):
    """The document the app starts with in workspace `ws`, untouched (saved once)."""
    if ws not in BASE:
        a = App(binary, m, out, f"baseline-{ws}", 2.5, ws)
        a.wait()
        BASE[ws] = a.doc
    return BASE[ws]


def seg_dist(p, a, b):
    dx, dy = b[0] - a[0], b[1] - a[1]
    l2 = dx * dx + dy * dy
    t = 0 if l2 == 0 else max(0, min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2))
    return math.hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy)


def pct(v, p):
    v = sorted(v)
    return v[min(len(v) - 1, int(round((len(v) - 1) * p)))] if v else None


def frame_numbers(st, t_from_ns=None, t_to_ns=None):
    """The drawn frames between two CLOCK_MONOTONIC times (by when they were drawn)."""
    fr = st["frames"]
    if t_from_ns is not None:
        fr = [f for f in fr if t_from_ns <= f["drawnNs"] <= t_to_ns]
    return fr


def rate(fr, hz):
    """The display rate while something is happening: the intervals between shown frames
    where the later frame's input had already arrived when the earlier one was shown (so
    the gaps between strokes, with nothing to draw, aren't counted). A miss: such an interval
    longer than 1.5 display frames. Stalls: frames whose input waited over 2 display frames to
    be drawn."""
    fr = sorted((f for f in fr if f["shownNs"]), key=lambda f: f["shownNs"])
    if len(fr) < 3:
        return {"framesShown": len(fr)}
    busy = [(b["shownNs"] - a["shownNs"]) / 1e6 for a, b in zip(fr, fr[1:])
            if b["events"] and b["receivedNs"] and b["receivedNs"] <= a["shownNs"]]
    period = 1000 / hz
    stalls = [round((f["drawnNs"] - f["receivedNs"]) / 1e6, 1) for f in fr
              if f["receivedNs"] and f["drawnNs"] - f["receivedNs"] > 2e6 * period]
    if not busy:
        return {"framesShown": len(fr), "stalls": stalls}
    return {
        "framesShown": len(fr),
        "busyIntervals": len(busy),
        "fps": round(1000 / (sum(busy) / len(busy)), 1),
        "intervalMs": {"p50": round(pct(busy, 0.5), 2), "p99": round(pct(busy, 0.99), 2),
                       "max": round(max(busy), 2)},
        "missed": sum(1 for v in busy if v > 1.5 * period),
        "stallsMs": stalls,
    }


def latency(fr):
    """Input to the frame being shown: from when the app received the oldest input in the
    frame (receivedNs), and from the compositor's event time (inputMs, ms)."""
    recv = [(f["shownNs"] - f["receivedNs"]) / 1e6 for f in fr if f["shownNs"] and f["receivedNs"]]
    comp = []
    for f in fr:
        if f["shownNs"] and f["inputMs"] is not None:
            shown_ms = (f["shownNs"] // 1_000_000) % (1 << 32)
            comp.append(((shown_ms - f["inputMs"]) % (1 << 32)) + (f["shownNs"] % 1_000_000) / 1e6)
    q = lambda v: {"n": len(v), "p50": round(pct(v, 0.5), 2), "p99": round(pct(v, 0.99), 2),
                   "max": round(max(v), 2)} if v else None
    return {"fromReceivedMs": q(recv), "fromEventTimeMs": q(comp),
            "fromHandedToCompositorMs": q([(f["shownNs"] - f["drawnNs"]) / 1e6 for f in fr if f["shownNs"]])}


def mono_ns():
    return time.clock_gettime_ns(time.CLOCK_MONOTONIC)


# ------------------------------------------------------------------------------------------
# scenarios

def scenario_view(binary, out):
    """Zoom to ~300 % (1, then + ×5), turn 30° (Shift-drag about the card's centre), flip (H),
    pan (Space-drag, middle-drag), scroll-zoom: none of it paints. Then a stroke: it lands under
    the pointer, in document pixels."""
    res = {}
    m = Mutter(out, 165)
    try:
        for tag, stroke in (("view-only", False), ("view-stroke", True)):
            app = App(binary, m, out, tag, 9, "animate")
            time.sleep(1.8)
            inp = Input()
            # the canvas card's middle; the window's canvas rect isn't known until the end, so
            # use the layout's (Animate at 1280 x 800: about x 92..1099, y 84..)
            c = (560.0, 330.0)
            inp.to(*c)
            time.sleep(0.05)
            for k in ["1"] + ["plus"] * 5:
                inp.key(k)
                time.sleep(0.05)
            # turn: Space and Shift held, drag from 0° to 30° around the canvas rect's centre
            # (with the Brush, Shift-drag alone paints; the stats say the angle reached)
            cr = (104 + 823.5 / 2, 96 + 385 / 2)  # Animate's canvas rect in 1280 x 800
            inp.key("space", True)
            inp.key("Shift_L", True)
            time.sleep(0.05)
            inp.drag(arc(cr, 160, 0, math.radians(30), 30), 0.006)
            time.sleep(0.03)
            inp.key("Shift_L", False)
            inp.key("space", False)
            time.sleep(0.05)
            inp.key("h")
            time.sleep(0.05)
            if not stroke:
                # pan with Space held, with the middle button, and zoom with the wheel: none paint
                inp.key("space", True)
                time.sleep(0.03)
                inp.drag(line((500, 300), (560, 340), 20), 0.005)
                inp.key("space", False)
                inp.drag(line((560, 340), (500, 300), 20), 0.005, BTN_MIDDLE)
                inp.to(600, 350)
                inp.scroll(1)
                time.sleep(0.05)
                inp.scroll(-1)
                time.sleep(0.05)
            else:
                a, b = (440.0, 300.0), (680.0, 370.0)
                inp.drag(line(a, b, 80), 0.004)
            inp.stop()
            st = app.wait()
            xs, ys = ink(app.doc, baseline(binary, m, out, "animate"))
            r = {"view": st["view"], "canvasRect": st["canvasRect"], "inkPx": int(len(xs)),
                 "angleDeg": round(math.degrees(st["view"]["angle"]), 3)}
            if stroke and len(xs):
                worst = 0.0
                pts = [view_to_screen(st, (x, y)) for x, y in zip(xs[::7], ys[::7])]
                for p in pts:
                    worst = max(worst, seg_dist(p, a, b))
                # the offset that best fits: the ink's centre vs the line's middle
                mx = sum(p[0] for p in pts) / len(pts)
                my = sum(p[1] for p in pts) / len(pts)
                mid = ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
                brush_reach = (6 / 2 * 1.5 + 2) * st["view"]["zoom"] + 1.5
                r.update({"worstOffLineScreenPx": round(worst, 2), "reachScreenPx": round(brush_reach, 2),
                          "inkCentreMinusLineMiddle": [round(mx - mid[0], 2), round(my - mid[1], 2)],
                          "pass": worst <= brush_reach + 1.0})
            else:
                r["pass"] = len(xs) == 0
            res[tag] = r
    finally:
        m.stop()
    return res


def latency_run(binary, out, tag, mode):
    """Drawing for 4 s, the pointer moving every 2 ms (500 Hz, like a pen), at 165 Hz and
    60 Hz. `mode`: "strokes" (400 ms strokes, the harness's ~20 ms between them), "pauses"
    (the same with 150 ms between them) or "continuous" (one 4 s stroke, zig-zagging).
    Pen to screen for every frame, and apart for the first two frames after a pause (input
    arriving after more than a refresh with none) and the steady rest."""
    res = {}
    for hz in (165, 60):
        m = Mutter(out, hz)
        try:
            app = App(binary, m, out, f"{tag}-{hz}", 8, "draw")
            time.sleep(1.8)
            inp = Input()
            t0 = mono_ns()
            k = 0
            if mode == "continuous":
                pts = []
                for i in range(2000):
                    u = (i % 400) / 400
                    x = 200 + 700 * (u if (i // 400) % 2 == 0 else 1 - u)
                    pts.append((x, 200 + i * 0.2))
                inp.drag(pts, 0.002)
                k = 1
            else:
                while (mono_ns() - t0) < 4e9:
                    y = 200 + (k % 30) * 15
                    inp.drag(line((200, y), (900, y + 40), 200), 0.002)
                    if mode == "pauses":
                        time.sleep(0.15)
                    k += 1
            t1 = mono_ns()
            inp.stop()
            st = app.wait()
            fr = frame_numbers(st, t0, t1 + 50_000_000)
            inputs = sorted((f for f in fr if f["events"] and f["shownNs"]), key=lambda f: f["shownNs"])
            period = 1e9 / hz
            first, steady, since = [], [], 99
            prev_shown = None
            for f in inputs:
                gap = prev_shown is None or f["receivedNs"] - prev_shown > period
                since = 0 if gap else since + 1
                (first if since < 2 else steady).append(f)
                prev_shown = f["shownNs"]
            res[str(hz)] = {"strokes": k, "rate": rate(fr, hz), "latency": latency(inputs),
                            "latencyFirstTwoAfterPause": latency(first),
                            "latencySteady": latency(steady),
                            "budgetMs": round(1000 / hz + 4, 2),
                            "pacer": {x: st.get(x) for x in ("drawingLate", "marginMs", "framesLate", "coolDowns")}}
        finally:
            m.stop()
    return res


def scenario_latency(binary, out):
    return latency_run(binary, out, "draw", "strokes")


def scenario_latency_pauses(binary, out):
    return latency_run(binary, out, "draw-pauses", "pauses")


def scenario_latency_continuous(binary, out):
    return latency_run(binary, out, "draw-continuous", "continuous")


def scenario_viewfps(binary, out):
    """The frame rate while panning (middle-drag) and zooming (the wheel), 3 s each, at 165 Hz
    and 60 Hz."""
    res = {}
    for hz in (165, 60):
        m = Mutter(out, hz)
        try:
            app = App(binary, m, out, f"viewfps-{hz}", 10, "animate")
            time.sleep(1.8)
            inp = Input()
            t0 = mono_ns()
            pts = []
            for i in range(1500):  # 3 s at 2 ms
                ang = i / 1500 * 6 * math.pi
                pts.append((560 + 150 * math.cos(ang), 330 + 100 * math.sin(ang)))
            inp.drag(pts, 0.002, BTN_MIDDLE)
            t1 = mono_ns()
            inp.to(560, 330)
            n = 0
            while mono_ns() - t1 < 3e9:
                inp.scroll(1 if (n // 10) % 2 == 0 else -1)
                n += 1
                time.sleep(0.004)
            t2 = mono_ns()
            inp.stop()
            st = app.wait()
            pan = frame_numbers(st, t0, t1)
            zoom = frame_numbers(st, t1, t2)
            res[str(hz)] = {"pan": {"rate": rate(pan, hz), "latency": latency([f for f in pan if f["events"]])},
                            "zoom": {"rate": rate(zoom, hz), "wheelSteps": n},
                            "docInkAfter": int(len(ink(app.doc, baseline(binary, m, out, "animate"))[0]))}
        finally:
            m.stop()
    return res


def scenario_idle(binary, out):
    """Nothing happening for 6 s (pointer away, then resting on the canvas): frames drawn."""
    m = Mutter(out, 165)
    try:
        app = App(binary, m, out, "idle", 8, "animate")
        time.sleep(1.5)
        t0 = mono_ns()
        time.sleep(3.0)
        inp = Input()
        inp.to(560, 330)
        time.sleep(0.5)
        t1 = mono_ns()
        time.sleep(2.5)
        t2 = mono_ns()
        inp.stop()
        st = app.wait()
        return {"framesDrawnTotal": st["framesDrawn"], "startupMs": st["startupToFirstFrameMs"],
                "framesWhileIdle3s": len(frame_numbers(st, t0, t1 - 500_000_000)),
                "framesResting2.5sOnCanvas": len(frame_numbers(st, t1, t2)),
                "pass": len(frame_numbers(st, t0, t1 - 500_000_000)) == 0 and len(frame_numbers(st, t1, t2)) == 0}
    finally:
        m.stop()


def scenario_memory(binary, out, strokes=600):
    """PSS with the 1920 x 1080 document: at rest, then every 50 strokes over `strokes` strokes,
    then after undoing all it can."""
    m = Mutter(out, 165)
    try:
        app = App(binary, m, out, "memory", 240, "draw")
        time.sleep(2.0)
        samples = [{"strokes": 0, "pssKiB": app.pss_kib()}]
        inp = Input()
        for k in range(1, strokes + 1):
            y = 150 + (k * 7) % 450
            x = 150 + (k * 13) % 300
            inp.drag(line((x, y), (x + 500, y + 60), 40), 0.002)
            time.sleep(0.01)
            if k % 50 == 0:
                time.sleep(0.5)
                samples.append({"strokes": k, "pssKiB": app.pss_kib()})
        # undo everything it keeps
        inp.key("Control_L", True)
        for _ in range(strokes + 10):
            inp.key("z")
            time.sleep(0.004)
        inp.key("Control_L", False)
        time.sleep(1.0)
        samples.append({"afterUndoAll": True, "pssKiB": app.pss_kib()})
        inp.stop()
        app.p.send_signal(signal.SIGTERM)
        app.p.wait()
        mib = [s["pssKiB"] / 1024 for s in samples if s["pssKiB"]]
        # growth once the undo history is full (the brush crate keeps 60 steps): from stroke
        # 150 to the last
        at = {s.get("strokes"): s["pssKiB"] for s in samples if s.get("strokes") is not None}
        late = (at[strokes] - at[150]) / (strokes - 150) if strokes > 150 and at.get(150) else None
        return {"samples": samples, "peakMiB": round(max(mib), 1), "budgetMiB": 150,
                "growthAfterUndoFullKiBPerStroke": late and round(late, 1),
                "pass": max(mib) < 150}
    finally:
        m.stop()


def scenario_playback(binary, out):
    """Play (Return) for 5 s at the cut's 24 fps, at 165 Hz and 60 Hz: timeline frames shown a
    second, skipped or repeated frames, and when each frame shows against its time
    (t0 + n / 24): the budget is the display frame nearest that time, so within half a
    refresh."""
    res = {}
    for hz in (165, 60):
        m = Mutter(out, hz)
        try:
            app = App(binary, m, out, f"playback-{hz}", 9, "animate")
            time.sleep(1.8)
            inp = Input()
            inp.to(560, 330)
            time.sleep(0.05)
            inp.key("Return")
            t0 = mono_ns()
            time.sleep(5.0)
            t1 = mono_ns()
            inp.stop()
            st = app.wait()
            fr = sorted((f for f in frame_numbers(st, t0, t1) if f["shownNs"]), key=lambda f: f["shownNs"])
            changes, last = [], None
            for f in fr:
                if f["sheetFrame"] != last:
                    changes.append((f["shownNs"], f["sheetFrame"]))
                    last = f["sheetFrame"]
            # (the first is the frame shown before Play)
            changes = changes[1:]
            if len(changes) < 10:
                res[str(hz)] = {"played": False, "changes": len(changes), "state": st["state"]}
                continue
            # frames advanced since the first change (wrapping at the loop's end)
            n_frames = max(c[1] for c in changes) + 1
            k, ks, skips = 0, [0], 0
            for a, b in zip(changes, changes[1:]):
                step = (b[1] - a[1]) % n_frames
                if step != 1:
                    skips += 1
                k += step
                ks.append(k)
            # the schedule's start: the best fit of shown = t0 + k / 24 (the median offset)
            offs = sorted(c[0] - kk * 1e9 / 24 for c, kk in zip(changes, ks))
            t_start = offs[len(offs) // 2]
            err = [(c[0] - (t_start + kk * 1e9 / 24)) / 1e6 for c, kk in zip(changes, ks)]
            iv = [(b[0] - a[0]) / 1e6 for a, b in zip(changes, changes[1:])]
            half = 500 / hz
            span = (changes[-1][0] - changes[0][0]) / 1e9
            res[str(hz)] = {
                "played": True, "framesPerSecond": round((len(changes) - 1) / span, 3),
                "skippedOrRepeated": skips,
                "onScreenMs": {"p50": round(pct(iv, .5), 2), "min": round(min(iv), 2), "max": round(max(iv), 2)},
                "offScheduleMs": {"p50": round(pct([abs(e) for e in err], .5), 2), "max": round(max(abs(e) for e in err), 2)},
                "halfRefreshMs": round(half, 2),
                "pass": skips == 0 and max(abs(e) for e in err) <= half + 0.5,
                "displayFramesDrawn": len(fr),
            }
        finally:
            m.stop()
    return res


def scenario_transform(binary, out):
    """The owner's top complaint: moving a large painted area. In the Draw workspace, strokes
    fill about 1200 x 800 document pixels; M (Select) and a marquee around them; V (Transform)
    lifts them (click to result: the key to the frame that shows the box); a 3 s drag inside
    the box (the rate, misses, pen to screen while dragging); Return puts them down (key to
    the frame shown). The document saved after: the pixels moved by the drag."""
    res = {}
    for hz in (165, 60):
        m = Mutter(out, hz)
        try:
            app = App(binary, m, out, f"transform-{hz}", 40, "draw")
            time.sleep(1.8)
            inp = Input()
            # where the document is: Draw's canvas rect at 1280 x 800, fitted (from a dry
            # read of the layout: the stats at the end give the exact view)
            cx, cy = 104 + 980 / 2, 96 + 612 / 2
            z = 0.47  # about the fitted zoom; the strokes only need to cover the area
            x0, x1 = cx - 600 * z, cx + 600 * z
            y = cy - 400 * z
            while y < cy + 400 * z:
                inp.drag(line((x0, y), (x1, y + 6), 60), 0.002)
                y += 5
            time.sleep(0.3)
            inp.key("m")
            time.sleep(0.1)
            inp.drag(line((x0 - 20, cy - 400 * z - 20), (x1 + 20, cy + 400 * z + 20), 20), 0.004)
            time.sleep(0.3)
            t_v = mono_ns()
            inp.key("v")
            time.sleep(0.5)
            t_drag = mono_ns()
            pts = []
            for i in range(1500):
                a = i / 1500 * 4 * math.pi
                pts.append((cx + 60 * math.sin(a) + i * 0.08, cy + 40 * math.sin(2 * a)))
            inp.drag(pts, 0.002)
            t_ret = mono_ns()
            time.sleep(0.2)
            inp.key("Return")
            time.sleep(0.6)
            t_end = mono_ns()
            inp.stop()
            st = app.wait()
            fr = sorted(st["frames"], key=lambda f: f["drawnNs"])
            first_after = lambda t: next((f for f in fr if f["receivedNs"] and f["receivedNs"] >= t and f["shownNs"]), None)
            fv = first_after(t_v)
            fret = first_after(t_ret + 150_000_000)
            drag = [f for f in fr if t_drag <= f["drawnNs"] <= t_ret]
            xs, ys = ink(app.doc, baseline(binary, m, out, "draw"))
            res[str(hz)] = {
                "liftKeyToShownMs": fv and round((fv["shownNs"] - fv["receivedNs"]) / 1e6, 1),
                "commitKeyToShownMs": fret and round((fret["shownNs"] - fret["receivedNs"]) / 1e6, 1),
                "drag": {"rate": rate(drag, hz), "latency": latency([f for f in drag if f["events"]])},
                "netDragScreenPx": [round(pts[-1][0] - pts[0][0], 1), round(pts[-1][1] - pts[0][1], 1)],
                "view": st["view"], "tool": st["state"]["tool"],
                "inkBoxDoc": [int(xs.min()), int(ys.min()), int(xs.max()), int(ys.max())] if len(xs) else None,
                "inkPx": int(len(xs)),
            }
        finally:
            m.stop()
    return res


def scenario_turn(binary, out):
    """Shift-drag turns the view in Review (no drawing tool there); with the Brush in Draw,
    Shift-drag paints (Shift is the tools'), Space+Shift-drag turns."""
    res = {}
    m = Mutter(out, 165)
    try:
        for tag, ws, space in (("review-shift", "review", False), ("draw-shift", "draw", False),
                               ("draw-space-shift", "draw", True)):
            app = App(binary, m, out, f"turn-{tag}", 5, ws)
            time.sleep(1.8)
            inp = Input()
            c = (500.0, 330.0)
            inp.to(*c)
            if space:
                inp.key("space", True)
            inp.key("Shift_L", True)
            time.sleep(0.05)
            inp.drag(arc(c, 150, 0, math.radians(40), 40), 0.005)
            inp.key("Shift_L", False)
            if space:
                inp.key("space", False)
            time.sleep(0.1)
            inp.stop()
            st = app.wait()
            res[tag] = {"angleDeg": round(math.degrees(st["view"]["angle"]), 2),
                        "inkPx": int(len(ink(app.doc, baseline(binary, m, out, ws))[0]))}
    finally:
        m.stop()
    res["pass"] = (res["review-shift"]["angleDeg"] != 0 and res["review-shift"]["inkPx"] == 0
                   and res["draw-shift"]["angleDeg"] == 0 and res["draw-shift"]["inkPx"] > 0
                   and res["draw-space-shift"]["angleDeg"] != 0 and res["draw-space-shift"]["inkPx"] == 0)
    return res


SCENARIOS = {
    "view": scenario_view, "latency": scenario_latency, "viewfps": scenario_viewfps,
    "idle": scenario_idle, "memory": scenario_memory, "playback": scenario_playback,
    "transform": scenario_transform, "turn": scenario_turn,
    "latency-pauses": scenario_latency_pauses, "latency-continuous": scenario_latency_continuous,
}


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(2)
    if sys.argv[1] != "--inner":
        binary, out = os.path.realpath(sys.argv[1]), os.path.realpath(sys.argv[2])
        os.makedirs(out, exist_ok=True)
        env = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
        os.execvpe("dbus-run-session", ["dbus-run-session", "--", sys.executable, "-I",
                                        os.path.realpath(__file__), "--inner", binary, out] + sys.argv[3:], env)
    binary, out = sys.argv[2], sys.argv[3]
    names = sys.argv[4:] or list(SCENARIOS)
    allres = {}
    path = os.path.join(out, "canvas-window.json")
    if os.path.exists(path):
        allres = json.load(open(path))
    for n in names:
        t = time.monotonic()
        try:
            r = SCENARIOS[n](binary, out)
        except Exception as e:  # keep going: one scenario's failure is a result too
            r = {"error": repr(e)}
        r["seconds"] = round(time.monotonic() - t, 1)
        allres[n] = r
        json.dump(r, open(os.path.join(out, f"{n}.json"), "w"), indent=2)
        print(n, json.dumps(r, indent=1), flush=True)
    json.dump(allres, open(path, "w"), indent=2)


if __name__ == "__main__":
    main()
