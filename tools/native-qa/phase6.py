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
    def __init__(self, out, tag, size, hz, own_bus=False):
        self.name = f"q6-{tag}-{os.getpid()}"
        self.log = open(os.path.join(out, f"mutter-{tag}.log"), "a")
        cmd = ["mutter", "--headless", "--no-x11", "--wayland-display", self.name,
               "--virtual-monitor", f"{size}@{hz}"]
        if own_bus:
            # its RemoteDesktop and other names on a bus of its own
            cmd = ["dbus-run-session", "--config-file", bus_conf(out), "--"] + cmd
        env = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
        self.p = subprocess.Popen(cmd, stdout=self.log, stderr=self.log, env=env)
        if not wait_sock(self.name):
            raise RuntimeError(f"mutter {tag} did not start")
        time.sleep(0.5)

    def stop(self):
        kill_tree(self.p.pid)
        try:
            self.p.wait(5)
        except subprocess.TimeoutExpired:
            pass


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

    def __init__(self, binary, out, tag, hz=60, project=None, name="Studio", size="1920x1080"):
        self.out, self.tag = out, tag
        self.mutter = Mutter(out, f"studio-{tag}", size, hz, own_bus=True)
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
        self.wd("POST", f"/session/{self.sid}/timeouts", {"script": 600_000, "pageLoad": 120_000})
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
    """Two {(x, y): bytes} maps: equal, and where not (tiles only in one, differing bytes)."""
    a, b = nonempty(a), nonempty(b)
    only_a = sorted(set(a) - set(b))
    only_b = sorted(set(b) - set(a))
    diff = []
    maxd = 0
    for k in set(a) & set(b):
        if a[k] != b[k]:
            d = max(abs(x - y) for x, y in zip(a[k], b[k]))
            maxd = max(maxd, d)
            diff.append(k)
    return {"equal": not (only_a or only_b or diff), "tiles": [len(a), len(b)],
            "onlyFirst": only_a[:8], "onlySecond": only_b[:8], "differ": len(diff),
            "maxChannelDiff": maxd}


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


SCENARIOS = {
    "studio-wire": scenario_studio_wire,
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
