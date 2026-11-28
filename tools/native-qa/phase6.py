#!/usr/bin/env python3
"""Phase 6 (draw together) QA: the current app and the native app in one live session.

    python3 -I tools/native-qa/phase6.py <out-dir> [scenario ...]

Environment: PP_STUDIO (the current app, `pencilpilot-studio`, built from dev), PP_NATIVE
(the native app), PP_PEER (`qa_peer`: `cargo build --release -p pencilpilot-share --example
qa_peer`), PP_PROJECTS (the baseline's test projects: tools/baseline/fixtures.py).

Nothing shows on a real screen and nothing reaches a speaker or the internet:
- it restarts itself in its own D-Bus session (`dbus-run-session`) without DISPLAY or
  WAYLAND_DISPLAY;
- each app gets a headless mutter of its own (the current app's in a D-Bus session of its
  own too, so only the native app's mutter answers RemoteDesktop);
- both apps run with PENCILPILOT_NO_SYSTEM_WINDOWS=1, PENCILPILOT_NO_SOUND=1, sound
  servers pointed nowhere, and their data, settings and caches in the run's folder;
- sessions run without relays (`PENCILPILOT_RELAY=off`, the current app's own switch;
  `Relays::Off` in the native app and in qa_peer): members reach each other on this machine.

The current app is the real one (Tauri + WebKitGTK, `together.ts` in its window, the session
in its Rust side), driven over WebDriver (tauri-driver) like tools/baseline/run.mjs does: pen
strokes go to its canvas as pen pointer events. qa_peer (crates/share/examples/qa_peer.rs) is
a scripted member: it logs everything on the wire and sends what a test needs (unknown kinds,
AI-made content, many members).
"""

import base64
import http.client
import json
import math
import os
import shutil
import signal
import socket
import statistics
import subprocess
import sys
import time
import urllib.request
import zipfile
import zlib

HERE = os.path.dirname(os.path.realpath(__file__))
ROOT = os.path.realpath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)

NOWHERE = {
    "PENCILPILOT_NO_SYSTEM_WINDOWS": "1",
    "PENCILPILOT_NO_SOUND": "1",
    "PENCILPILOT_RELAY": "off",
    # no sound server: nothing can play
    "PULSE_SERVER": "unix:/nonexistent/pulse",
    "PIPEWIRE_REMOTE": "/nonexistent/pipewire",
    "JACK_NO_START_SERVER": "1",
    "ALSA_CONFIG_PATH": "/nonexistent/alsa.conf",
    "GTK_USE_PORTAL": "0",
    "GDK_DEBUG": "no-portals",
    "NO_AT_BRIDGE": "1",
}


# a D-Bus session that starts no services (no portal, no notifications, no file manager)
BUS_CONF = """<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><keep_umask/><listen>unix:tmpdir=/tmp</listen>
<servicedir>{empty}</servicedir><policy context="default"><allow send_destination="*" eavesdrop="true"/>
<allow eavesdrop="true"/><allow own="*"/></policy></busconfig>
"""


def bus_conf(out):
    empty = os.path.join(out, "no-services")
    os.makedirs(empty, exist_ok=True)
    path = os.path.join(out, "bus.conf")
    open(path, "w").write(BUS_CONF.format(empty=empty))
    return path


def mono_ns():
    return time.clock_gettime_ns(time.CLOCK_MONOTONIC)


def real_to_mono_ms():
    """CLOCK_REALTIME - CLOCK_MONOTONIC, in ms (the webview's clock is realtime)."""
    best = None
    for _ in range(20):
        a = time.clock_gettime_ns(time.CLOCK_MONOTONIC)
        r = time.clock_gettime_ns(time.CLOCK_REALTIME)
        b = time.clock_gettime_ns(time.CLOCK_MONOTONIC)
        if best is None or b - a < best[0]:
            best = (b - a, r - (a + b) / 2)
    return best[1] / 1e6


def pct(v, p):
    v = sorted(v)
    return v[min(len(v) - 1, int(round((len(v) - 1) * p)))] if v else None


def dist(v):
    v = [x for x in v if x is not None]
    if not v:
        return None
    return {"n": len(v), "median": round(statistics.median(v), 2), "p95": round(pct(v, 0.95), 2),
            "p99": round(pct(v, 0.99), 2), "min": round(min(v), 2), "max": round(max(v), 2)}


def load():
    return open("/proc/loadavg").read().split()[:3]


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def wait_sock(name, secs=10):
    sock = os.path.join(os.environ["XDG_RUNTIME_DIR"], name)
    t = time.monotonic()
    while time.monotonic() - t < secs:
        if os.path.exists(sock):
            return True
        time.sleep(0.05)
    return False


def kill_tree(pid):
    """SIGTERM then SIGKILL a process and its children, by pid (never by pattern)."""
    def children(p):
        out = []
        try:
            for t in os.listdir(f"/proc/{p}/task"):
                with open(f"/proc/{p}/task/{t}/children") as f:
                    out += [int(c) for c in f.read().split()]
        except OSError:
            pass
        return out
    todo, seen = [pid], []
    while todo:
        p = todo.pop()
        seen.append(p)
        todo += children(p)
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for p in seen:
            try:
                os.kill(p, sig)
            except OSError:
                pass
        time.sleep(0.4)


def sockets_of(pid):
    """The UDP and TCP sockets a process tree holds, from /proc (inode → local/remote)."""
    inodes = set()
    def walk(p):
        try:
            for fd in os.listdir(f"/proc/{p}/fd"):
                try:
                    l = os.readlink(f"/proc/{p}/fd/{fd}")
                except OSError:
                    continue
                if l.startswith("socket:["):
                    inodes.add(l[8:-1])
            for t in os.listdir(f"/proc/{p}/task"):
                with open(f"/proc/{p}/task/{t}/children") as f:
                    for c in f.read().split():
                        walk(int(c))
        except OSError:
            pass
    walk(pid)
    found = []
    for proto in ("tcp", "tcp6", "udp", "udp6"):
        try:
            rows = open(f"/proc/net/{proto}").read().splitlines()[1:]
        except OSError:
            continue
        for r in rows:
            c = r.split()
            if c[9] in inodes:
                found.append({"proto": proto, "local": c[1], "remote": c[2], "state": c[3]})
    return found


# ------------------------------------------------------------------------------------------
# a headless mutter

class Mutter:
    """A headless mutter on a D-Bus bus of its own (which starts no services): its
    RemoteDesktop is this one's only. `bus` is that bus's address."""

    def __init__(self, out, tag, size, hz, own_bus=True):
        self.name = f"q6-{tag}-{os.getpid()}"
        self.log = open(os.path.join(out, f"mutter-{tag}.log"), "a")
        env = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
        self.daemon = None
        self.bus = os.environ.get("DBUS_SESSION_BUS_ADDRESS")
        if own_bus:
            self.daemon = subprocess.Popen(
                ["dbus-daemon", "--config-file", bus_conf(out), "--nofork", "--print-address=1", "--print-pid=2"],
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env=env)
            self.bus = self.daemon.stdout.readline().strip()
            env["DBUS_SESSION_BUS_ADDRESS"] = self.bus
        self.p = subprocess.Popen(
            ["mutter", "--headless", "--no-x11", "--wayland-display", self.name,
             "--virtual-monitor", f"{size}@{hz}"], stdout=self.log, stderr=self.log, env=env)
        if not wait_sock(self.name):
            self.stop()
            raise RuntimeError(f"mutter {tag} did not start")
        time.sleep(0.5)

    def stop(self):
        kill_tree(self.p.pid)
        try:
            self.p.wait(5)
        except subprocess.TimeoutExpired:
            pass
        if self.daemon:
            kill_tree(self.daemon.pid)


MON_W, MON_H = 1440, 900
# the native app's default 1280 x 800 window, centred on the 1440 x 900 monitor
WIN_OFF = (80, 50)
BTN_LEFT = 0x110
KEYSYM = {"Shift_L": 0xFFE1, "Control_L": 0xFFE3, "s": 0x73, "z": 0x7A, "Escape": 0xFF1B}


class Input:
    """mutter's RemoteDesktop on a given bus (as canvas_window.py's, for several mutters)."""

    def __init__(self, bus_address):
        import dbus
        self.dbus = dbus
        bus = dbus.bus.BusConnection(bus_address)
        self.bus = bus
        name = "org.gnome.Mutter.RemoteDesktop"
        rd = bus.get_object(name, "/org/gnome/Mutter/RemoteDesktop")
        path = dbus.Interface(rd, name).CreateSession()
        self.s = dbus.Interface(bus.get_object(name, path), name + ".Session")
        self.s.Start()
        self.at = (0.0, 0.0)
        time.sleep(0.4)
        self.s.NotifyPointerMotionRelative(dbus.Double(-100000), dbus.Double(-100000))
        self.key("Shift_L")
        time.sleep(0.05)

    def to(self, x, y):
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

    def combo(self, mod, key):
        self.key(mod, True)
        time.sleep(0.03)
        self.key(key)
        time.sleep(0.03)
        self.key(mod, False)

    def stroke(self, pts, interval=1 / 240):
        """A drag through window points; when each point was sent (CLOCK_MONOTONIC ns)."""
        times = []
        self.to(*pts[0])
        time.sleep(0.02)
        t0 = time.monotonic()
        self.button(BTN_LEFT, True)
        times.append(mono_ns())
        for i, p in enumerate(pts[1:], 1):
            wait = t0 + i * interval - time.monotonic()
            if wait > 0:
                time.sleep(wait)
            self.to(*p)
            times.append(mono_ns())
        time.sleep(interval)
        self.button(BTN_LEFT, False)
        times.append(mono_ns())
        return times

    def stop(self):
        try:
            self.s.Stop()
        except Exception:
            pass


class Native:
    """The native app on a headless mutter of its own (its own bus too), hosting or joining
    through its check hook (`--host` / `--join <invite>`, `--name`), its `--stats` read when
    it quits."""

    def __init__(self, binary, out, tag, secs, hz=None, project=None, host=False, join=None,
                 name=None, args=(), input=True, env=None):
        hz = hz or int(os.environ.get("PP_HZ", "60"))
        self.out, self.tag = out, tag
        self.mutter = Mutter(out, f"native-{tag}", f"{MON_W}x{MON_H}", hz)
        self.home = os.path.join(out, f"native-{tag}")
        shutil.rmtree(self.home, ignore_errors=True)
        os.makedirs(self.home)
        self.stats = os.path.join(self.home, "stats.json")
        self.doc = os.path.join(self.home, "doc.png")
        self.project = None
        if project:
            self.project = os.path.join(self.home, os.path.basename(project))
            shutil.copy(project, self.project)
        e = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
        e.update(NOWHERE)
        e.update({"WAYLAND_DISPLAY": self.mutter.name, "DBUS_SESSION_BUS_ADDRESS": self.mutter.bus,
                  "PENCILPILOT_DIALOG_ANSWER": "",
                  "XDG_CONFIG_HOME": os.path.join(self.home, "config"),
                  "XDG_STATE_HOME": os.path.join(self.home, "state"),
                  "XDG_DATA_HOME": os.path.join(self.home, "data"),
                  "XDG_CACHE_HOME": os.path.join(self.home, "cache")})
        e.update(env or {})
        cmd = [binary, "--quit-after", str(secs), "--stats", self.stats, "--save-doc", self.doc]
        if host:
            cmd.append("--host")
        if join:
            cmd += ["--join", join]
        if name:
            cmd += ["--name", name]
        cmd += list(args)
        if self.project:
            cmd.append(self.project)
        self.launched = mono_ns()
        self.errlog = open(os.path.join(self.home, "stderr.log"), "w")
        self.p = subprocess.Popen(cmd, env=e, stdout=subprocess.PIPE, stderr=self.errlog, text=True)
        self.pid = self.p.pid
        self.invite = None
        self.lines = []
        if host:
            t = time.monotonic()
            while time.monotonic() - t < 20:
                line = self.p.stdout.readline()
                if not line:
                    break
                self.lines.append(line)
                m = line.strip()
                i = m.find("pp1-")
                if i >= 0:
                    self.invite = m[i:].split()[0]
                    break
            if not self.invite:
                raise RuntimeError(f"native {tag}: no invite on stdout ({self.lines[-3:]})")
        self.inp = None
        if input:
            time.sleep(1.0)
            self.inp = Input(self.mutter.bus)

    def pss_kib(self):
        try:
            for line in open(f"/proc/{self.pid}/smaps_rollup"):
                if line.startswith("Pss:"):
                    return int(line.split()[1])
        except OSError:
            return None

    def wait(self, extra=30):
        try:
            self.p.wait(extra + 60)
        except subprocess.TimeoutExpired:
            kill_tree(self.pid)
        if self.inp:
            self.inp.stop()
        self.mutter.stop()
        try:
            return json.load(open(self.stats))
        except (OSError, ValueError):
            return None

    def kill(self):
        kill_tree(self.pid)
        self.mutter.stop()


# ------------------------------------------------------------------------------------------
# the current app, over WebDriver

# installed in the page once the studio is up: what goes to and comes from the session, on
# the page's clock (performance.timeOrigin + now: realtime ms)
HOOKS = r"""
if (window.__q6) return true;
const I = window.__TAURI_INTERNALS__;
const now = () => performance.timeOrigin + performance.now();
const q = (window.__q6 = { out: [], ev: [], frames: [], keep: arguments[0] || false });
const raf = window.requestAnimationFrame.bind(window);
q.raf = raf;
const orig = I.invoke.bind(I);
I.invoke = (cmd, args, opts) => {
  if (typeof cmd === 'string' && cmd.startsWith('share_')) {
    const b = args && args.body;
    const live = b && b.live;
    q.out.push({ t: now(), cmd, k: b && b.k, live: live ? live.type : undefined,
                 n: live && live.pts ? live.pts.length / 4 : undefined,
                 to: args && args.to, body: q.keep ? args : undefined });
  }
  return orig(cmd, args, opts);
};
const cb = I.transformCallback((e) => {
  const p = e.payload || {};
  const b = p.body || {};
  const r = { t: now(), kind: p.kind, seq: p.seq, from: p.from, k: b.k,
              live: b.live ? b.live.type : undefined,
              n: b.live && b.live.pts ? b.live.pts.length / 4 : undefined,
              body: q.keep || p.kind === 'direct' && b.k === 'chat' ? b : undefined,
              reason: p.reason };
  q.ev.push(r);
  // drawn on the next frame, on screen at the one after (as the baseline counts it)
  raf(() => { r.drawn = now(); raf(() => { r.shown = now(); }); });
});
orig('plugin:event|listen', { event: 'share-event', target: { kind: 'Any' }, handler: cb });
return true;
"""

STUDIO_READY = r"""
const f = document.querySelector('[data-testid="frame"]');
if (!f || !document.querySelector('.viewport canvas')) return false;
return ![...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Opening…');
"""

# pen strokes on the canvas, as pointer events (tools/baseline/run.mjs's way); each stroke a
# list of [x, y, pressure] in the viewport's fractions; returns when each point was sent
PEN = r"""
const [strokes, hz, gapMs] = arguments;
const done = arguments[arguments.length - 1];
const vp = document.querySelector('.viewport');
const r = vp.getBoundingClientRect();
const now = () => performance.timeOrigin + performance.now();
const ev = (type, x, y, p) => vp.dispatchEvent(new PointerEvent(type, {
  pointerId: 7, pointerType: 'pen', isPrimary: true, bubbles: true, cancelable: true,
  composed: true, clientX: r.left + r.width * x, clientY: r.top + r.height * y, pressure: p,
  width: 1, height: 1, button: type === 'pointermove' ? -1 : 0, buttons: type === 'pointerup' ? 0 : 1 }));
const sent = [];
(async () => {
  for (const s of strokes) {
    const st = performance.now();
    const dt = 1000 / hz;
    const times = [];
    for (let i = 0; i < s.length; i++) {
      const wait = st + i * dt - performance.now();
      if (wait > 0) await new Promise((res) => setTimeout(res, wait));
      ev(i === 0 ? 'pointerdown' : 'pointermove', s[i][0], s[i][1], s[i][2]);
      times.push(now());
    }
    const l = s[s.length - 1];
    ev('pointerup', l[0], l[1], 0);
    times.push(now());
    sent.push(times);
    await new Promise((res) => setTimeout(res, gapMs));
  }
  done(sent);
})().catch((e) => done({ err: String(e) }));
"""


class WebDriverError(Exception):
    pass


class Studio:
    """The current app on a headless mutter of its own, driven over WebDriver."""

    def __init__(self, binary, out, tag, hz=None, project=None, name="Studio", size="1920x1080"):
        hz = hz or int(os.environ.get("PP_HZ", "60"))
        self.out, self.tag = out, tag
        self.mutter = Mutter(out, f"studio-{tag}", size, hz)
        self.home = os.path.join(out, f"studio-{tag}")
        data = os.path.join(self.home, "data")
        shutil.rmtree(data, ignore_errors=True)
        appdata = os.path.join(data, "app.pencilpilot.studio")
        os.makedirs(appdata, exist_ok=True)
        self.project = None
        recent = []
        if project:
            self.project = os.path.join(self.home, os.path.basename(project))
            shutil.copy(project, self.project)
            recent = [self.project]
        json.dump(recent, open(os.path.join(appdata, "recent.json"), "w"))
        # a name to draw together under; no "save before…?" question
        json.dump({"shareName": name, "askToSave": False},
                  open(os.path.join(appdata, "settings.json"), "w"))
        env = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
        env.update(NOWHERE)
        env.update({"WAYLAND_DISPLAY": self.mutter.name, "GDK_BACKEND": "wayland",
                    "XDG_DATA_HOME": data,
                    "XDG_CONFIG_HOME": os.path.join(self.home, "config"),
                    "XDG_CACHE_HOME": os.path.join(self.home, "cache"),
                    "XDG_STATE_HOME": os.path.join(self.home, "state"),
                    "PP_BENCH_EXPORT_DIR": os.path.join(self.home, "exports")})
        for k in ("XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"):
            os.makedirs(env[k], exist_ok=True)
        self.port = free_port()
        self.drv_log = open(os.path.join(out, f"tauri-driver-{tag}.log"), "a")
        self.driver = subprocess.Popen(
            ["tauri-driver", "--port", str(self.port), "--native-port", str(free_port()),
             "--native-driver", "/usr/bin/WebKitWebDriver"],
            env=env, stdout=self.drv_log, stderr=self.drv_log)
        for _ in range(100):
            try:
                socket.create_connection(("127.0.0.1", self.port), 0.2).close()
                break
            except OSError:
                time.sleep(0.1)
        self.offset = real_to_mono_ms()
        self.launched = mono_ns()
        try:
            s = None
            for attempt in range(20):
                try:
                    s = self.wd("POST", "/session",
                                {"capabilities": {"alwaysMatch": {"tauri:options": {"application": binary}}}})
                    break
                except (OSError, http.client.HTTPException):
                    time.sleep(0.3)
            if not s:
                raise RuntimeError("no WebDriver session")
        except BaseException:
            kill_tree(self.driver.pid)
            self.mutter.stop()
            raise
        self.sid = s["sessionId"]
        for attempt in range(10):
            try:
                self.wd("POST", f"/session/{self.sid}/timeouts", {"script": 600_000, "pageLoad": 120_000})
                break
            except (OSError, http.client.HTTPException):
                time.sleep(0.3)
        self.pid = self.app_pid(binary)

    # -- plumbing
    def wd(self, method, path, body=None, timeout=900):
        req = urllib.request.Request(
            f"http://127.0.0.1:{self.port}{path}", method=method,
            data=None if body is None else json.dumps(body).encode(),
            headers={"content-type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                return json.loads(r.read() or b"{}").get("value")
        except urllib.error.HTTPError as e:
            raise WebDriverError(f"{method} {path}: {e.read()[:400]!r}") from None

    def js(self, script, *args):
        return self.wd("POST", f"/session/{self.sid}/execute/sync", {"script": script, "args": list(args)})

    def js_async(self, script, *args):
        v = self.wd("POST", f"/session/{self.sid}/execute/async", {"script": script, "args": list(args)})
        if isinstance(v, dict) and "err" in v:
            raise WebDriverError(v["err"])
        return v

    def invoke(self, cmd, args=None):
        """A command of the app's Rust side, as the window calls it."""
        return self.js_async(
            "const d = arguments[arguments.length-1];"
            "window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1] || {})"
            ".then(v => d({ok: v === undefined ? null : v}), e => d({err: String(e)}));",
            cmd, args)["ok"]

    def until(self, script, secs=120, *args):
        t = time.monotonic()
        while time.monotonic() - t < secs:
            try:
                if self.js(script, *args):
                    return time.monotonic() - t
            except WebDriverError:
                pass
            time.sleep(0.03)
        raise TimeoutError(script[:120])

    def app_pid(self, binary):
        real = os.path.realpath(binary)
        for _ in range(100):
            todo = [self.driver.pid]
            while todo:
                p = todo.pop()
                try:
                    if os.readlink(f"/proc/{p}/exe") == real:
                        return p
                    for t in os.listdir(f"/proc/{p}/task"):
                        todo += [int(c) for c in open(f"/proc/{p}/task/{t}/children").read().split()]
                except OSError:
                    pass
            time.sleep(0.05)
        raise RuntimeError("the current app did not start")

    def to_mono_ns(self, t_ms):
        """A page time (realtime ms) on CLOCK_MONOTONIC (ns)."""
        return int((t_ms - self.offset) * 1e6)

    # -- the app
    def open(self, secs=180):
        """Home → the project on the Recent list (or a new canvas) → studio ready, hooks in."""
        self.until("return document.readyState === 'complete' && !!document.querySelector('button.primary')", 120)
        if self.project:
            try:
                self.until("const b = [...document.querySelectorAll('button.card')].find((x) => x.title === arguments[0]);"
                           "b?.click(); return !!b;", 20, self.project)
            except TimeoutError:
                cards = self.js("return [...document.querySelectorAll('button.card')].map((x) => x.title);")
                raise RuntimeError(f"{self.project} is not on the Recent list ({cards})") from None
        else:
            self.js("document.querySelector('button.primary').click(); return true;")
        took = self.until(STUDIO_READY, secs)
        self.js(HOOKS, False)
        return took

    def share_dialog(self):
        self.js("const b = [...document.querySelectorAll('button')].find((x) => /Draw together|Drawing together|people/.test(x.textContent));"
                "b?.click(); return !!b;")
        time.sleep(0.2)

    def host(self):
        """Host, through the Draw together dialog; returns the session's info."""
        self.share_dialog()
        ok = self.js("const b = [...document.querySelectorAll('button.go')].find((x) => x.textContent.trim() === 'Host');"
                     "b?.click(); return !!b;")
        if not ok:
            raise RuntimeError("no Host button")
        self.until("return !!document.querySelector('button') && [...document.querySelectorAll('button')].some((x) => x.textContent.trim() === 'Leave');", 30)
        info = self.invoke("share_status")
        self.close_dialog()
        return info

    def join(self, code):
        self.share_dialog()
        ok = self.js(r"""
const input = [...document.querySelectorAll('input, textarea')].find((x) => /code|pp1/i.test(x.placeholder || '') || /code/i.test(x.getAttribute('aria-label') || ''));
if (!input) return false;
const set = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(input), 'value').set;
set.call(input, arguments[0]);
input.dispatchEvent(new Event('input', { bubbles: true }));
return true;""", code)
        if not ok:
            raise RuntimeError("no invite field")
        time.sleep(0.1)
        t0 = mono_ns()
        ok = self.js("const b = [...document.querySelectorAll('button')].find((x) => x.textContent.trim() === 'Join');"
                     "if (!b || b.disabled) return false; b.click(); return true;")
        if not ok:
            raise RuntimeError("Join not pressable")
        self.until("return [...document.querySelectorAll('button')].some((x) => x.textContent.trim() === 'Leave');", 60)
        info = self.invoke("share_status")
        self.close_dialog()
        return info, t0

    def close_dialog(self):
        self.js("const b = [...document.querySelectorAll('button.go')].find((x) => /Back to drawing/.test(x.textContent));"
                "b?.click(); return true;")
        time.sleep(0.1)

    def leave(self):
        self.share_dialog()
        self.js("const b = [...document.querySelectorAll('button')].find((x) => x.textContent.trim() === 'Leave');"
                "b?.click(); return !!b;")
        time.sleep(0.5)
        self.close_dialog()

    def pen(self, strokes, hz=240, gap_ms=250):
        """Strokes (lists of (x, y, pressure), viewport fractions); per stroke, when each
        point was sent (CLOCK_MONOTONIC ns)."""
        sent = self.js_async(PEN, strokes, hz, gap_ms)
        return [[self.to_mono_ns(t) for t in s] for s in sent]

    def take(self):
        """What the page logged since last asked (times on CLOCK_MONOTONIC, ns)."""
        r = self.js("const q = window.__q6; const o = {out: q.out, ev: q.ev}; q.out = []; "
                    "q.ev = q.ev.filter((e) => !e.shown); return o;")
        for k in ("out", "ev"):
            for e in r[k]:
                for f in ("t", "drawn", "shown"):
                    if e.get(f):
                        e[f] = self.to_mono_ns(e[f])
        return r

    def layers(self):
        """The layer panel's names, top to bottom (as shown)."""
        return self.js("return [...document.querySelectorAll('[data-layer-id], .layer-row, .layers li')].map((x) => x.textContent.trim().slice(0, 60));")

    def save(self):
        """Ctrl+S: saved to the file it was opened from (no dialog)."""
        if not self.project:
            raise RuntimeError("save needs a project opened from a file (no dialog)")
        before = os.stat(self.project).st_mtime_ns
        self.js("document.body.dispatchEvent(new KeyboardEvent('keydown', {key: 's', code: 'KeyS', ctrlKey: true, bubbles: true})); return true;")
        t = time.monotonic()
        while time.monotonic() - t < 60:
            if os.stat(self.project).st_mtime_ns != before:
                time.sleep(0.5)
                return self.project
            time.sleep(0.05)
        raise TimeoutError("not saved")

    def pss_kib(self):
        tot = 0
        todo = [self.pid]
        while todo:
            p = todo.pop()
            try:
                for line in open(f"/proc/{p}/smaps_rollup"):
                    if line.startswith("Pss:"):
                        tot += int(line.split()[1])
                for t in os.listdir(f"/proc/{p}/task"):
                    todo += [int(c) for c in open(f"/proc/{p}/task/{t}/children").read().split()]
            except OSError:
                pass
        return tot

    def screenshot(self, path):
        b = self.wd("GET", f"/session/{self.sid}/screenshot")
        open(path, "wb").write(base64.b64decode(b))

    def quit(self):
        try:
            self.wd("DELETE", f"/session/{self.sid}", timeout=5)
        except Exception:
            pass
        kill_tree(self.driver.pid)
        self.mutter.stop()


class NativePeer:
    """The native app's engine as a member, without its window (apps/native/tests/
    phase6_peer_qa.rs, `PP_NATIVE_PEER`: its test binary), driven by appending commands to a
    file; answers read from its log."""

    def __init__(self, binary, out, tag):
        self.home = os.path.join(out, f"npeer-{tag}")
        shutil.rmtree(self.home, ignore_errors=True)
        os.makedirs(self.home)
        self.cmds = os.path.join(self.home, "cmds.jsonl")
        self.log = os.path.join(self.home, "log.jsonl")
        open(self.cmds, "w").close()
        e = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
        e.update(NOWHERE)
        e.update({"PP_QA_PEER_CMDS": self.cmds, "PP_QA_PEER_LOG": self.log,
                  "XDG_CONFIG_HOME": os.path.join(self.home, "config"),
                  "XDG_STATE_HOME": os.path.join(self.home, "state"),
                  "XDG_DATA_HOME": os.path.join(self.home, "data"),
                  "PENCILPILOT_SHARE_DIR": os.path.join(self.home, "share")})
        self.p = subprocess.Popen([binary, "--include-ignored", "--exact", "native_peer", "--nocapture"],
                                  env=e, stdout=open(os.path.join(self.home, "stdout.log"), "w"),
                                  stderr=subprocess.STDOUT)
        self.pid = self.p.pid
        self.n = 0

    def answers(self):
        out = []
        try:
            for line in open(self.log):
                try:
                    out.append(json.loads(line))
                except ValueError:
                    pass
        except OSError:
            pass
        return out

    def do(self, secs=60, **c):
        """A command and its answer."""
        have = len(self.answers())
        with open(self.cmds, "a") as f:
            f.write(json.dumps(c) + "\n")
        t = time.monotonic()
        while time.monotonic() - t < secs:
            a = self.answers()
            if len(a) > have:
                return a[have]
            if self.p.poll() is not None:
                raise RuntimeError(f"native peer gone ({self.p.returncode}): {open(os.path.join(self.home, 'stdout.log')).read()[-800:]}")
            time.sleep(0.01)
        raise TimeoutError(f"native peer: {c}")

    def state(self):
        return self.do(cmd="state")["state"]

    def until(self, pred, secs=30):
        t = time.monotonic()
        while time.monotonic() - t < secs:
            st = self.state()
            if pred(st):
                return st
            time.sleep(0.05)
        return None

    def quit(self):
        try:
            self.do(secs=10, cmd="quit")
            self.p.wait(10)
        except Exception:
            kill_tree(self.pid)


# ------------------------------------------------------------------------------------------
# qa_peer: a scripted member

class Peer:
    def __init__(self, binary, out, tag, invite=None, name=None):
        self.log = os.path.join(out, f"peer-{tag}.jsonl")
        args = ["host", name or tag, self.log] if invite is None else ["join", invite, name or tag, self.log]
        env = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
        self.p = subprocess.Popen([binary, *args], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=open(os.path.join(out, f"peer-{tag}.err"), "a"), text=True, env=env)
        self.ready = json.loads(self.p.stdout.readline() or '{"ready": false}')
        if not self.ready.get("ready"):
            raise RuntimeError(f"qa_peer {tag}: {self.ready}")
        self.id = self.ready["me"]["id"]
        self.invite = self.ready["invite"]
        self.read_at = 0

    def cmd(self, **c):
        self.p.stdin.write(json.dumps(c) + "\n")
        self.p.stdin.flush()

    def edit(self, body):
        self.cmd(cmd="edit", body=body)

    def direct(self, to, body):
        self.cmd(cmd="direct", to=to, body=body)

    def presence(self, body):
        self.cmd(cmd="presence", body=body)

    def members(self):
        self.cmd(cmd="members")
        return json.loads(self.p.stdout.readline())

    def events(self):
        out = []
        try:
            for line in open(self.log):
                try:
                    out.append(json.loads(line))
                except ValueError:
                    pass
        except OSError:
            pass
        return out

    def wait_for(self, pred, secs=30):
        t = time.monotonic()
        while time.monotonic() - t < secs:
            for e in self.events():
                if pred(e):
                    return e
            time.sleep(0.02)
        return None

    def leave(self):
        try:
            self.cmd(cmd="leave")
            self.p.stdin.close()
            self.p.wait(10)
        except Exception:
            kill_tree(self.p.pid)


# ------------------------------------------------------------------------------------------
# tiles: as on the wire ("z:" deflate-raw, "r:" raw, base64) and in .ppstudio files

TILE_BYTES = 64 * 64 * 4


def decode_tile(s):
    if s.startswith("z:"):
        raw = zlib.decompress(base64.b64decode(s[2:]), -15)
    elif s.startswith("r:"):
        raw = base64.b64decode(s[2:])
    else:
        return None
    return raw if len(raw) == TILE_BYTES else None


def encode_tile(px):
    c = zlib.compressobj(6, zlib.DEFLATED, -15)
    return "z:" + base64.b64encode(c.compress(px) + c.flush()).decode()


def stroke_tiles(seed, n=4, opaque=False):
    """A few tiles of a scripted member's "stroke": a soft diagonal band, some of it at low
    alpha (where premultiplied storage rounds), at tiles near (seed, seed)."""
    import numpy as np
    rng = np.random.default_rng(seed)
    out = []
    bx, by = 2 + seed % 20, 2 + (seed // 20) % 12
    for i in range(n):
        yy, xx = np.mgrid[0:64, 0:64]
        d = np.abs(xx - yy - rng.integers(-20, 20))
        a = np.clip(255 - d * rng.integers(6, 30), 0, 255).astype(np.uint8)
        if opaque:
            a = np.where(a > 0, 255, 0).astype(np.uint8)
        rgb = rng.integers(0, 256, 3, dtype=np.uint8)
        px = np.zeros((64, 64, 4), np.uint8)
        px[..., :3] = rgb
        px[..., 3] = a
        px[a == 0] = 0
        out.append([bx + i, by, px.tobytes()])
    return out


def wire_tiles(events):
    """Per (lid, did): {(x, y): bytes}, from a peer's log: the snapshot sent to it (snap.*)
    and every `tiles` edit after, in order."""
    state = {}
    for e in events:
        b = e.get("body") or {}
        if e["ev"] == "direct" and b.get("k") == "snap.tiles":
            m = state.setdefault((b["lid"], b["did"]), {})
            for x, y, s in b.get("set", []):
                m[(x, y)] = decode_tile(s)
        elif e["ev"] == "edit" and b.get("k") == "tiles":
            m = state.setdefault((b["lid"], b["did"]), {})
            for x, y, s in b.get("set", []):
                m[(x, y)] = decode_tile(s)
            for x, y in b.get("clear", []):
                m.pop((x, y), None)
    return state


def wire_frames(events):
    """From a peer's log: shared layer name → [tiles per frame], by the newest timeline (from
    the snapshot or a `timeline` edit) and the tiles sent for each drawing."""
    tiles = wire_tiles(events)
    names, tl = {}, None
    for e in events:
        b = e.get("body") or {}
        if b.get("k") == "snap.begin":
            tl = b.get("tl")
            for l in b.get("layers", []):
                names[l["lid"]] = l["name"]
        elif e["ev"] == "edit" and b.get("k") == "layer":
            names[b["lid"]] = b.get("name")
        elif e["ev"] == "edit" and b.get("k") == "rename":
            names[b["lid"]] = b.get("name")
        elif e["ev"] == "edit" and b.get("k") == "timeline":
            tl = b.get("tl")
    out = {}
    for sc in (tl or {}).get("scenes", []):
        for cut in sc.get("cuts", []):
            for lid, track in cut.get("tracks", {}).items():
                seq = []
                for x in track:
                    seq += [x.get("d") or None] * x.get("n", 1)
                out.setdefault(names.get(lid, lid), []).extend(
                    [nonempty(tiles.get((lid, d), {})) if d else None for d in seq])
    return out


def compare_with_wire(saved, events, names):
    """A saved project's layers against what the wire carried (frame by frame)."""
    pp, _ = project_pixels(saved)
    wf = wire_frames(events)
    res = {}
    for n in names:
        if n not in pp or n not in wf:
            res[n] = {"missing": "saved" if n not in pp else "wire"}
            continue
        fs, fw = pp[n][-1], wf[n]
        diffs = []
        for i in range(min(len(fs), len(fw))):
            c = compare_tiles(fs[i] or {}, fw[i] or {})
            if not c["equal"]:
                diffs.append({"frame": i, **c})
        res[n] = {"equal": not diffs, "samePremul1": all(d["samePremul1"] for d in diffs),
                  "frames": [len(fs), len(fw)], "differ": diffs[:4]}
    return res


def ppstudio_layers(path):
    """A .ppstudio's layers: name → {drawing id → {(x, y): RGBA8 bytes}} (fully transparent
    tiles left out), plus the manifest."""
    z = zipfile.ZipFile(path)
    m = json.loads(z.read("manifest.json"))
    names = {str(l["id"]): l["name"] for l in m["layers"]}
    used = {}
    for sc in m["timeline"].get("scenes", []):
        for cut in sc.get("cuts", []):
            for lid, track in cut.get("tracks", {}).items():
                for e in track:
                    if e.get("drawing"):
                        used.setdefault(lid, []).append(e["drawing"])
    out = {}
    for lid, dids in used.items():
        layer = out.setdefault(names.get(lid, lid), {})
        for did in dict.fromkeys(dids):
            d = m["drawings"].get(did)
            if not d:
                continue
            tiles = {}
            for t in d.get("tiles", []):
                px = z.read(f"tiles/{t['hash']}")
                if any(px):
                    tiles[(t["x"], t["y"])] = px
            layer[did] = tiles
    return out, m


def nonempty(m):
    return {k: v for k, v in m.items() if v and any(v)}


def compare_tiles(a, b):
    """Two {(x, y): bytes} maps: byte-equal, and where not: tiles only in one, tiles that
    differ, the largest difference in a straight channel, in alpha, and premultiplied (the
    current app keeps pixels premultiplied, so a tile put into it comes back with straight
    colour off by a few where alpha is low; premultiplied, by at most 1)."""
    import numpy as np
    a, b = nonempty(a), nonempty(b)
    only_a = sorted(set(a) - set(b))
    only_b = sorted(set(b) - set(a))
    diff, maxd, maxa, maxp = 0, 0, 0, 0
    for k in set(a) & set(b):
        if a[k] != b[k]:
            diff += 1
            x = np.frombuffer(a[k], np.uint8).reshape(-1, 4).astype(np.int32)
            y = np.frombuffer(b[k], np.uint8).reshape(-1, 4).astype(np.int32)
            maxd = max(maxd, int(np.abs(x - y).max()))
            maxa = max(maxa, int(np.abs(x[:, 3] - y[:, 3]).max()))
            px = (x[:, :3] * x[:, 3:4] + 127) // 255
            py = (y[:, :3] * y[:, 3:4] + 127) // 255
            maxp = max(maxp, int(np.abs(px - py).max()))
    # a tile in one only: compare with transparent
    for k in only_a + only_b:
        t = np.frombuffer((a.get(k) or b.get(k)), np.uint8).reshape(-1, 4).astype(np.int32)
        maxa = max(maxa, int(t[:, 3].max()))
        maxp = max(maxp, int(((t[:, :3] * t[:, 3:4] + 127) // 255).max()))
    return {"equal": not (only_a or only_b or diff), "tiles": [len(a), len(b)],
            "onlyFirst": only_a[:8], "onlySecond": only_b[:8], "differ": diff,
            "maxChannelDiff": maxd, "maxAlphaDiff": maxa, "maxPremulDiff": maxp,
            "samePremul1": maxa == 0 and maxp <= 1}


def frames_of(m):
    """A manifest's timeline: layer id → [drawing id per frame] (every scene's cuts, in order)."""
    out = {}
    for sc in m["timeline"].get("scenes", []):
        for cut in sc.get("cuts", []):
            n = max([sum(e.get("length", 1) for e in t) for t in cut.get("tracks", {}).values()] or [0])
            for lid, track in cut.get("tracks", {}).items():
                seq = []
                for e in track:
                    seq += [e.get("drawing") or None] * e.get("length", 1)
                seq += [None] * (n - len(seq))
                out.setdefault(lid, []).extend(seq)
    return out


def project_pixels(path):
    """A .ppstudio: layer name → [[tiles per frame] per layer of that name] ({(x, y): bytes},
    transparent tiles left out; None: no drawing), and the manifest."""
    z = zipfile.ZipFile(path)
    m = json.loads(z.read("manifest.json"))
    cache = {}

    def tiles(did):
        if did not in cache:
            d = m["drawings"].get(did) or {"tiles": []}
            t = {}
            for e in d.get("tiles", []):
                px = z.read(f"tiles/{e['hash']}")
                if any(px):
                    t[(e["x"], e["y"])] = px
            cache[did] = t
        return cache[did]
    fr = frames_of(m)
    out = {}
    for l in m["layers"]:
        seq = fr.get(str(l["id"]), [])
        out.setdefault(l["name"], []).append([tiles(d) if d else None for d in seq])
    return out, m


def compare_projects(a, b, names=None):
    """Two saved projects' layers (by name), frame by frame."""
    pa, ma = project_pixels(a)
    pb, mb = project_pixels(b)
    names = names or sorted(set(pa) & set(pb))
    res = {"layersA": [l["name"] for l in ma["layers"]], "layersB": [l["name"] for l in mb["layers"]],
           "canvas": [ma["canvas"], mb["canvas"]], "layers": {}}
    ok, ok1 = True, True
    for n in names:
        if n not in pa or n not in pb:
            res["layers"][n] = {"missing": "A" if n not in pa else "B"}
            ok = ok1 = False
            continue
        fa, fb = pa[n][-1], pb[n][-1]
        r = {"frames": [len(fa), len(fb)], "differ": [], "sameName": [len(pa[n]), len(pb[n])]}
        same1 = True
        for i in range(min(len(fa), len(fb))):
            c = compare_tiles(fa[i] or {}, fb[i] or {})
            if not c["equal"]:
                r["differ"].append({"frame": i, **c})
                same1 = same1 and c["samePremul1"]
        r["tiles"] = sum(len(t or {}) for t in fa)
        r["equal"] = not r["differ"]
        r["samePremul1"] = same1
        ok, ok1 = ok and r["equal"], ok1 and same1
        r["differ"] = r["differ"][:5]
        res["layers"][n] = r
    res["equal"], res["samePremul1"] = ok, ok1
    return res


# ------------------------------------------------------------------------------------------
# strokes

def wave(i, n=120, y0=0.3):
    """A stroke across the canvas, n points (fractions of the viewport), with pressure."""
    pts = []
    for j in range(n):
        u = j / (n - 1)
        pts.append([0.2 + 0.6 * u, y0 + 0.04 * i + 0.05 * math.sin(u * 9 + i),
                    0.3 + 0.6 * math.sin(u * math.pi) ** 2])
    return pts


# ------------------------------------------------------------------------------------------
# scenarios

def env_bin(name, fallback=None):
    v = os.environ.get(name) or fallback
    if not v or not os.path.exists(v):
        raise RuntimeError(f"{name} not set or missing ({v})")
    return os.path.realpath(v)


def scenario_studio_wire(out):
    """The current app hosts; qa_peer joins, asks for the project and watches the wire while
    the app draws: what the current app sends, how it arrives, the project's tiles."""
    studio = env_bin("PP_STUDIO")
    peer_bin = env_bin("PP_PEER")
    proj = os.path.join(env_bin("PP_PROJECTS"), "empty.ppstudio")
    res = {"load": load()}
    s = Studio(studio, out, "wire", project=proj)
    peers = []
    try:
        res["openS"] = round(s.open(), 2)
        info = s.host()
        res["hostInfo"] = {"members": len(info["members"]), "host": info["host"]}
        p = Peer(peer_bin, out, "wire", invite=info["invite"], name="Probe")
        peers.append(p)
        host_id = info["me"]["id"]
        # ask for the project, as a joining app does
        time.sleep(0.5)
        p.direct(host_id, {"k": "snap.req"})
        end = p.wait_for(lambda e: e["ev"] == "direct" and (e.get("body") or {}).get("k") == "snap.end", 30)
        res["snapshot"] = bool(end)
        sent = s.pen([wave(i) for i in range(3)])
        time.sleep(2.0)
        evs = p.events()
        kinds = {}
        for e in evs:
            k = f'{e["ev"]}:{(e.get("body") or {}).get("k") or ("live:" + str(((e.get("body") or {}).get("live") or {}).get("type")) if e["ev"] == "presence" else "")}'
            kinds[k] = kinds.get(k, 0) + 1
        res["wireKinds"] = kinds
        # live points: from the page sending a point to qa_peer receiving it
        lat = []
        first_pts = [st[0] for st in sent]
        lives = [e for e in evs if e["ev"] == "presence" and (e["body"].get("live") or {}).get("type") == "begin"]
        for t0, e in zip(first_pts, lives):
            lat.append((e["t"] - t0) / 1e6)
        res["liveBeginToWireMs"] = dist(lat)
        # final tiles: the stroke's last point to its tiles arriving
        tiles = [e for e in evs if e["ev"] == "edit" and e["body"].get("k") == "tiles"]
        res["tilesEdits"] = len(tiles)
        if tiles:
            res["lastPointToFirstTilesMs"] = round((tiles[0]["t"] - sent[0][-1]) / 1e6, 1)
        saved = s.save()
        layers, _ = ppstudio_layers(saved)
        wire = wire_tiles(evs)
        res["savedLayers"] = {n: {d: len(t) for d, t in ds.items()} for n, ds in layers.items()}
        res["wireDrawings"] = {f"{l}/{d}": len(nonempty(t)) for (l, d), t in wire.items()}
        # the app's own saved pixels against what it sent (by drawing: the wire's did is the
        # file's drawing id)
        cmp = {}
        for name, ds in layers.items():
            for did, t in ds.items():
                w = [m for (l, d), m in wire.items() if d == did]
                if w:
                    cmp[f"{name}/{did}"] = compare_tiles(t, w[0])
        res["savedVsWire"] = cmp
        with open(os.path.join(out, "studio-wire-trace.jsonl"), "w") as f:
            for e in evs:
                f.write(json.dumps(e) + "\n")
        res["page"] = {k: len(v) for k, v in s.take().items()}
    finally:
        for p in peers:
            p.leave()
        s.quit()
    return res


def live_latency(sent, evs, kinds=("begin", "points", "end")):
    """From the sender's pen (per stroke, each point's time) to the receiver: when its live
    stroke's first points arrived and were shown, and when the stroke's tiles arrived."""
    lives = [e for e in evs if e.get("kind") == "presence" and e.get("live")]
    tiles = [e for e in evs if e.get("kind") == "edit" and e.get("k") == "tiles"]
    first_arrive, first_shown, tiles_after_end = [], [], []
    for st in sent:
        t0, t_end = st[0], st[-1]
        b = next((e for e in lives if e["live"] == "begin" and e["t"] >= t0), None)
        p = next((e for e in lives if e["live"] == "points" and e["t"] >= t0), None)
        if b:
            first_arrive.append((b["t"] - t0) / 1e6)
        if p and p.get("shown"):
            first_shown.append((p["shown"] - t0) / 1e6)
        t = next((e for e in tiles if e["t"] >= t_end), None)
        if t:
            tiles_after_end.append((t["t"] - t_end) / 1e6)
    return {"penToLiveArrivedMs": dist(first_arrive), "penToLiveShownMs": dist(first_shown),
            "strokeEndToTilesArrivedMs": dist(tiles_after_end)}


def scenario_studio_pair(out):
    """Two current apps: A hosts, B joins; A draws; B shows the live stroke and then its
    tiles; both saved projects byte-equal on the shared layer. (The current app against
    itself: the reference for the native app.)"""
    studio = env_bin("PP_STUDIO")
    projects = env_bin("PP_PROJECTS")
    res = {"load": load()}
    a = Studio(studio, out, "pa", project=os.path.join(projects, "empty.ppstudio"), name="Ana")
    b = None
    try:
        b = Studio(studio, out, "pb", project=os.path.join(projects, "tiny.ppstudio"), name="Bo")
        a.open()
        b.open()
        res["soloSocketsA"] = len([x for x in sockets_of(a.pid) if x["proto"].startswith("udp")])
        info = a.host()
        probe = Peer(env_bin("PP_PEER"), out, "pair", invite=info["invite"], name="Probe")
        time.sleep(0.5)
        probe.direct(info["me"]["id"], {"k": "snap.req"})
        jinfo, t_join = b.join(info["invite"])
        res["members"] = len(jinfo["members"])
        time.sleep(1.5)
        b.take()
        sent = a.pen([wave(i) for i in range(5)])
        time.sleep(2.0)
        evs = b.take()["ev"]
        res["aToB"] = live_latency(sent, evs)
        sent = b.pen([wave(i, y0=0.6) for i in range(3)])
        time.sleep(2.0)
        res["bOwnLayerEvents"] = len(a.take()["ev"])
        pa, pb = a.save(), b.save()
        res["compare"] = compare_projects(pa, pb, ["Layer 1"])
        evs = probe.events()
        res["aVsWire"] = compare_with_wire(pa, evs, ["Layer 1"])
        res["bVsWire"] = compare_with_wire(pb, evs, ["Layer 1"])
        probe.leave()
    finally:
        a.quit()
        if b:
            b.quit()
    return res


def peer_layer(p, n=1, name=None):
    """A scripted member's own new layer: its id (the member's id starts it)."""
    lid = f"{p.id[:12]}:{n}"
    p.edit({"k": "layer", "lid": lid, "name": name or f"{p.ready['me']['name']} layer"})
    return lid


def peer_timeline(lid, did):
    return {"k": "timeline", "tl": {"kinds": {}, "mouths": [], "palettes": [], "stages": {}, "statics": [],
            "scenes": [{"name": "Scene 1", "cuts": [{"name": "Cut 1", "tracks": {lid: [{"d": did, "n": 1}]}}]}]}}


def crowd(p, lid, did, strokes, rate, seed0):
    """`strokes` tiles edits at `rate` a second on the member's own layer, cursors between."""
    bodies = []
    for i in range(strokes):
        t = stroke_tiles(seed0 + i)
        bodies.append({"k": "tiles", "lid": lid, "did": did,
                       "set": [[x, y, encode_tile(px)] for x, y, px in t], "clear": []})
    p.cmd(cmd="edits", bodies=bodies, gap_ms=int(1000 / rate))


def scenario_studio_crowd(out, members=8, strokes=30, rate=2.0):
    """The current app hosts 7 scripted members: each makes a layer and draws on it at once
    (tiles edits, `rate` a second, like a person's strokes; cursors too). Every edit must
    reach everyone in the same order, and the app's saved layers must hold every member's
    last tiles."""
    studio = env_bin("PP_STUDIO")
    peer_bin = env_bin("PP_PEER")
    res = {"load": load(), "members": members, "strokesEach": strokes, "rate": rate}
    s = Studio(studio, out, "crowd", project=os.path.join(env_bin("PP_PROJECTS"), "empty.ppstudio"))
    peers = []
    try:
        s.open()
        info = s.host()
        for i in range(members - 1):
            peers.append(Peer(peer_bin, out, f"crowd{i}", invite=info["invite"], name=f"M{i}"))
        time.sleep(1.0)
        res["membersSeen"] = len(s.invoke("share_status")["members"])
        lids = []
        for i, p in enumerate(peers):
            lid = peer_layer(p)
            did = f"{p.id[:12]}:{1000 + i}"
            p.edit(peer_timeline(lid, did))
            lids.append((lid, did))
        time.sleep(1.0)
        t0 = mono_ns()
        for i, (p, (lid, did)) in enumerate(zip(peers, lids)):
            crowd(p, lid, did, strokes, rate, 100 * i)
        # cursors from everyone while they draw
        for k in range(int(strokes / rate * 10)):
            for j, p in enumerate(peers):
                p.presence({"x": 100 + k, "y": 50 * j})
            time.sleep(0.1)
        time.sleep(3.0)
        page = s.take()
        edits = [e for e in page["ev"] if e["kind"] == "edit"]
        seqs = [e["seq"] for e in edits]
        res["appEdits"] = len(edits)
        res["appSeqGaps"] = sum(1 for a, b in zip(seqs, seqs[1:]) if b != a + 1)
        res["appTilesEdits"] = sum(1 for e in edits if e.get("k") == "tiles")
        expected = (members - 1) * strokes
        # every scripted member got every other member's edits
        got = []
        lat = []
        for p in peers:
            ev = [e for e in p.events() if e["ev"] == "edit" and (e.get("body") or {}).get("k") == "tiles"]
            got.append(len(ev))
        sent_t = {}
        for p in peers:
            for e in p.events():
                if e["ev"] == "sent":
                    sent_t[(p.id, e["n"])] = e["t"]
        res["tilesSentTotal"] = expected
        res["tilesAtEachMember"] = got
        res["dropped"] = sum(expected - g for g in got) + (expected - res["appTilesEdits"])
        saved = s.save()
        pp, _ = project_pixels(saved)
        ok = {}
        for i, p in enumerate(peers):
            name = f"M{i} layer"
            want = {}
            for st in range(strokes):
                for x, y, px in stroke_tiles(100 * i + st):
                    want[(x, y)] = px
            have = pp.get(name, [[None]])[-1]
            c = compare_tiles(want, (have[0] if have else None) or {})
            ok[name] = {k: c[k] for k in ("equal", "samePremul1", "tiles", "maxAlphaDiff", "maxPremulDiff")}
        res["savedLayers"] = ok
        res["allLayersSame"] = all(v["samePremul1"] for v in ok.values())
        res["pssMiB"] = round(s.pss_kib() / 1024, 1)
        res["seconds_drawing"] = round((mono_ns() - t0) / 1e9, 1)
    finally:
        for p in peers:
            p.leave()
        s.quit()
    return res


def oracle_corpus(out, n=600, seed=11):
    """Edits from together_oracle.mjs's generator (well made and broken, unknown kinds and
    AI-made content among them)."""
    path = os.path.join(out, f"corpus-{n}-{seed}.jsonl")
    if not os.path.exists(path):
        with open(path, "w") as f:
            subprocess.run(["node", os.path.join(HERE, "together_oracle.mjs"), "gen", str(n), str(seed)],
                           stdout=f, check=True)
    return [json.loads(l) for l in open(path)]


GARBAGE = [
    {"k": "ai", "prompt": "a cat", "image": "z:AAAA"},
    {"k": "generate", "model": "qwen-image"},
    {"k": "ai_image", "set": [], "clear": []},
    {"k": "v2-layer", "name": "from a newer app"},
    {"k": "tiles", "lid": "zz", "did": "zz", "set": [[1, 1, "z:%%%"]], "clear": []},
    {"k": "tiles", "set": "x"},
    {"k": "layer", "lid": "not-an-id", "name": "x"},
    "just text", [1, 2, 3], None, 42, {"k": 5},
]


def garbage_into(app_kind, a, p, host_id, own_lid=None):
    """qa_peer (a member) sends edits no app may take (unknown kinds, AI-made content, broken
    ones, others' layers), then one good one: the app must still be in the session, have
    taken nothing of the first and the good one."""
    bodies = list(GARBAGE)
    # the generator's broken and unknown ones (layer ids of other members: refused by rule)
    bodies += [c["body"] for c in oracle_corpus(a.out)][:400]
    # a member's live stroke with nonsense in it
    for live in ({"type": "begin", "k": 1, "style": {"brushId": "x" * 500}}, {"type": "points", "k": 1, "pts": ["a", None, 1e308]},
                 {"type": "points", "k": 99, "pts": [1, 2, 3]}, {"type": "end", "k": "x"}):
        p.presence({"live": live})
    p.cmd(cmd="edits", bodies=bodies, gap_ms=2)
    time.sleep(3.0)
    # then a good layer with tiles on it
    lid = f"{p.id[:12]}:500"
    p.edit({"k": "layer", "lid": lid, "name": "After garbage"})
    did = f"{p.id[:12]}:501"
    p.edit(peer_timeline(lid, did))
    t = stroke_tiles(5)
    p.edit({"k": "tiles", "lid": lid, "did": did, "set": [[x, y, encode_tile(px)] for x, y, px in t], "clear": []})
    time.sleep(2.0)
    return lid, t


def scenario_studio_garbage(out):
    """The current app (host) gets edits no app may take from a member: it stays in the
    session and takes the good edit after them. (The reference for the native app.)"""
    res = {"load": load()}
    s = Studio(env_bin("PP_STUDIO"), out, "garbage", project=os.path.join(env_bin("PP_PROJECTS"), "empty.ppstudio"))
    p = None
    try:
        s.open()
        info = s.host()
        p = Peer(env_bin("PP_PEER"), out, "garbage", invite=info["invite"], name="Mallory")
        time.sleep(0.8)
        lid, t = garbage_into("studio", s, p, info["me"]["id"])
        st = s.invoke("share_status")
        res["stillInSession"] = bool(st) and len(st["members"]) == 2
        pp, m = project_pixels(s.save())
        res["layers"] = [l["name"] for l in m["layers"]]
        have = (pp.get("After garbage") or [[None]])[-1]
        want = {(x, y): px for x, y, px in t}
        c = compare_tiles(want, (have[0] if have else None) or {})
        res["goodEditTaken"] = c["samePremul1"]
        # (the current app saves others' live-stroke preview layer, "… (drawing now)", into
        # the file: noted, not counted here)
        res["livePreviewSaved"] = [n for n in res["layers"] if n.endswith("(drawing now)")]
        real = [n for n in res["layers"] if not n.endswith("(drawing now)")]
        res["pass"] = res["stillInSession"] and res["goodEditTaken"] and len(real) == 2
    finally:
        if p:
            p.leave()
        s.quit()
    return res


def native_wave(i, n=120, y0=230):
    """A stroke across the native window's canvas (window points)."""
    return [(300 + 500 * j / (n - 1), y0 + 30 * i + 25 * math.sin(j / (n - 1) * 9 + i)) for j in range(n)]


def native_edits(st, kind=None):
    t = (st or {}).get("together") or {}
    return [e for e in t.get("edits", []) if kind is None or e["kind"] == kind]


def native_live_latency(sent, st):
    """Studio pen (per stroke, each point's time) → the native app: the live stroke's first
    frame on screen, and the stroke's tiles on screen after its last point."""
    ev = native_edits(st)
    out = {"penToLiveShownMs": [], "penToLiveReceivedMs": [], "strokeEndToTilesShownMs": [],
           "strokeEndToTilesReceivedMs": []}
    for stroke in sent:
        t0, t_end = stroke[0], stroke[-1]
        live = next((e for e in ev if e["kind"].startswith("live") and e["receivedNs"] >= t0), None)
        if live:
            out["penToLiveReceivedMs"].append((live["receivedNs"] - t0) / 1e6)
            if live["shownNs"]:
                out["penToLiveShownMs"].append((live["shownNs"] - t0) / 1e6)
        tl = next((e for e in ev if e["kind"] == "tiles" and e["receivedNs"] >= t_end), None)
        if tl:
            out["strokeEndToTilesReceivedMs"].append((tl["receivedNs"] - t_end) / 1e6)
            if tl["shownNs"]:
                out["strokeEndToTilesShownMs"].append((tl["shownNs"] - t_end) / 1e6)
    return {k: dist(v) for k, v in out.items()}


def scenario_x_pair(out, native_hosts=True, strokes=5):
    """The native app and the current app in one session. The host draws `strokes` strokes,
    the joiner some on its own layer; the other shows them live, then the tiles; both save,
    and every shared layer must hold the same pixels in both files."""
    native_bin = env_bin("PP_NATIVE")
    studio = env_bin("PP_STUDIO")
    projects = env_bin("PP_PROJECTS")
    res = {"load": load(), "nativeHosts": native_hosts, "hz": int(os.environ.get("PP_HZ", "60"))}
    s = n = probe = None
    try:
        s = Studio(studio, out, "x", project=os.path.join(projects, "empty.ppstudio" if not native_hosts else "tiny.ppstudio"),
                   name="Studio")
        s.open()
        if native_hosts:
            n = Native(native_bin, out, "x", 40, project=os.path.join(projects, "empty.ppstudio"), host=True,
                       name="Nat", args=["--workspace", "animate"])
            invite = n.invite
        else:
            info = s.host()
            invite = info["invite"]
            n = Native(native_bin, out, "x", 40, project=os.path.join(projects, "tiny.ppstudio"), join=invite,
                       name="Nat", args=["--workspace", "animate"])
        probe = Peer(env_bin("PP_PEER"), out, "x", invite=invite, name="Probe")
        if native_hosts:
            jinfo, _ = s.join(invite)
            res["studioMembers"] = len(jinfo["members"])
        time.sleep(2.5)
        s.take()
        # the host draws, then the joiner
        if native_hosts:
            sent_n = [n.inp.stroke(native_wave(i)) for i in range(strokes)]
            time.sleep(2.0)
            res["nativeToStudio"] = live_latency(sent_n, s.take()["ev"])
            sent_s = s.pen([wave(i, y0=0.6) for i in range(3)])
        else:
            sent_s = s.pen([wave(i) for i in range(strokes)])
            time.sleep(2.0)
            sent_n = [n.inp.stroke(native_wave(i, y0=500)) for i in range(3)]
            res["studioToNativeEvents"] = len(s.take()["ev"])
        time.sleep(2.5)
        res["studioSaved"] = s.save()
        n.inp.combo("Control_L", "s")
        time.sleep(3.0)
        st = n.wait()
        n = None
        res["nativeStats"] = {k: v for k, v in ((st or {}).get("together") or {}).items() if k != "edits"}
        res["studioToNative"] = native_live_latency(sent_s, st)
        res["nativeEditKinds"] = {}
        for e in native_edits(st):
            res["nativeEditKinds"][e["kind"]] = res["nativeEditKinds"].get(e["kind"], 0) + 1
        nproj = os.path.join(out, "native-x", "tiny.ppstudio" if not native_hosts else "empty.ppstudio")
        sp, _ = project_pixels(res["studioSaved"])
        np_, _ = project_pixels(nproj)
        shared = sorted(set(sp) & set(np_) - {"Ink"})
        res["sharedLayerNames"] = shared
        res["compare"] = compare_projects(res["studioSaved"], nproj, shared)
        evs = probe.events()
        res["studioVsWire"] = compare_with_wire(res["studioSaved"], evs, shared)
        res["nativeVsWire"] = compare_with_wire(nproj, evs, shared)
    finally:
        if probe:
            probe.leave()
        if n:
            n.kill()
        if s:
            s.quit()
    return res


SCENARIOS = {
    "studio-wire": scenario_studio_wire,
    "studio-pair": scenario_studio_pair,
    "studio-crowd": scenario_studio_crowd,
    "studio-garbage": scenario_studio_garbage,
    "x-pair-native-hosts": lambda out: scenario_x_pair(out, True),
    "x-pair-studio-hosts": lambda out: scenario_x_pair(out, False),
}


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    if sys.argv[1] != "--inner":
        out = os.path.realpath(sys.argv[1])
        os.makedirs(out, exist_ok=True)
        env = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
        env.update(NOWHERE)
        os.execvpe("dbus-run-session", ["dbus-run-session", "--config-file", bus_conf(out), "--", sys.executable, "-I",
                                        os.path.realpath(__file__), "--inner", out] + sys.argv[2:], env)
    out = sys.argv[2]
    names = sys.argv[3:] or list(SCENARIOS)
    path = os.path.join(out, "phase6.json")
    allres = json.load(open(path)) if os.path.exists(path) else {}
    for n in names:
        base, _, rep = n.partition("#")
        t = time.monotonic()
        try:
            r = SCENARIOS[base](out)
        except Exception as e:  # one scenario's failure is a result too
            import traceback
            r = {"error": repr(e), "trace": traceback.format_exc()[-1500:]}
        r["seconds"] = round(time.monotonic() - t, 1)
        allres[n] = r
        json.dump(allres, open(path, "w"), indent=1, default=str)
        print(json.dumps({n: r}, default=str)[:3000], flush=True)


if __name__ == "__main__":
    main()
