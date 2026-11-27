#!/usr/bin/env python3
"""Phase 4 (GPU layers, sound) in a real window on a headless mutter: playback, scrubbing,
memory and CPU while playing, measured the same way before and after the GPU compositor.

    python3 -I phase4.py fixtures <out-dir>
    python3 -I phase4.py run <pencilpilot binary> <out-dir> [scenario ...]

`fixtures` writes the test projects (fixed seeds, `tools/baseline/fixtures.py` and a few of
its own) into <out-dir>. `run` needs `PP_QA_FIXTURES=<that dir>`; scenarios (default: all):

- `play`: 20 layers at 4K opened as the first argument, Play (Return), 10 s, at 165 Hz and
  60 Hz, `PP_QA_RUNS` times each (default 3): the wait from the key to the first frame
  change, skipped or repeated timeline frames, each frame's time on screen against the
  display frame nearest its time, the process's CPU while playing (utime + stime) and its
  PSS (sampled every 50 ms; VRAM from the DRM fdinfo, when the driver gives it).
- `drift`: the same for 62 s at 165 Hz (60 Hz with `PP_QA_DRIFT_HZ`): the error's slope
  over the minute and the first against the last 5 s.
- `play500`: the same as `play` with 500 drawings.
- `scrub`: Right taps one display frame apart through the opened 20 layers at 4K and 500
  drawings, then Left back: key to shown, the work of each frame that changed the timeline
  frame (`workNs`), steps never shown.
- `switch`: Play, then Right / Left taps while playing (scrubbing while playing), then
  Return (stop) and Return (play) again; then a cut switch through the timeline while
  playing (two-cut project): the app keeps drawing, no stall over 100 ms, and what the
  last frame shows.
- `latency`: pen to screen on the 4K 20-layer project (strokes on its top layer) at 165 and
  60 Hz, as `canvas_window.py latency` measures it on the demo.

Every run is in its own D-Bus session with its own mutter (no window on any real screen),
`PENCILPILOT_NO_SYSTEM_WINDOWS=1`, and no sound device: PulseAudio / PipeWire point at
nothing and ALSA's default is its `null` device (plus `PP_QA_APP_ENV`, JSON, for the app's
own switches). Results go to <out-dir>/<scenario>.json and <out-dir>/phase4.json.
"""

import json
import os
import statistics
import struct
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.realpath(__file__))
sys.dont_write_bytecode = True  # (the folder's __pycache__ is in git)
sys.path.insert(0, HERE)
import canvas_window as cw  # noqa: E402  (the window driver: mutter, the app, input)

FIX = os.environ.get("PP_QA_FIXTURES", "")
RUNS = int(os.environ.get("PP_QA_RUNS", "3"))
CLK = os.sysconf("SC_CLK_TCK")


# ------------------------------------------------------------------------------------------
# the fixtures

def fixtures(out):
    """The baseline's three projects and Phase 4's own: two cuts of 20 layers at 4K (switching
    cuts while playing), a minute of 20 layers at 4K with clicks, and the sound projects."""
    os.makedirs(out, exist_ok=True)
    root = os.path.dirname(os.path.dirname(HERE))
    subprocess.run([sys.executable, "-I", os.path.join(root, "tools/baseline/fixtures.py"), out],
                   check=True, cwd=root)
    pics = os.path.join(out, "pics")
    spec = json.load(open(os.path.join(pics, "layers20-4k.json")))
    cut = spec["scenes"][0]["cuts"][0]
    # the second cut: the same layers, the animation reversed, 12 frames
    second = {"name": "Cut 2", "tracks": {}}
    for name, cels in cut["tracks"].items():
        if name.startswith("Anim"):
            second["tracks"][name] = list(reversed(cels))[:6]
        else:
            second["tracks"][name] = [{**cels[0], "length": 12}]
    spec["scenes"][0]["cuts"] = [cut, second]
    build_spec(root, spec, os.path.join(pics, "layers20-4k-2cuts.json"), os.path.join(out, "layers20-4k-2cuts.ppstudio"))
    # 20 layers at 4K for a minute (the 24 frames 60 times), clicks every half second
    sounds(pics)
    long = json.load(open(os.path.join(pics, "layers20-4k.json")))
    c = long["scenes"][0]["cuts"][0]
    c["tracks"] = {n: cels * 60 for n, cels in c["tracks"].items()}
    long["scenes"][0]["audio"] = [{"file": "clicks.wav", "start": 0}]
    build_spec(root, long, os.path.join(pics, "layers20-4k-60s-sound.json"),
               os.path.join(out, "layers20-4k-60s-sound.ppstudio"))
    # the sound projects: 640 x 360, a frame number drawn on every frame
    for name, s in sound_projects(pics).items():
        build_spec(root, s, os.path.join(pics, f"{name}.json"), os.path.join(out, f"{name}.ppstudio"))


def build_spec(root, spec, spec_path, target):
    json.dump(spec, open(spec_path, "w"))
    subprocess.run(["cargo", "run", "-q", "--release", "-p", "pencilpilot-ppstudio-cli", "--", "build",
                    spec_path, target], check=True, cwd=root, stdout=subprocess.DEVNULL)
    print(target, f"{os.path.getsize(target) / 1e6:.1f} MB")


RATE = 48000


def write_wav(path, x, rate=RATE):
    """Float samples (n,) or (n, channels), -1 to 1, as 16-bit PCM WAV."""
    import wave
    import numpy as np
    if x.ndim == 1:
        x = x[:, None]
    pcm = (np.clip(x, -1, 1) * 32767).round().astype("<i2")
    with wave.open(path, "wb") as w:
        w.setnchannels(pcm.shape[1])
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm.tobytes())


def sounds(pics):
    """The sound files (fixed seeds): clicks.wav (48 kHz stereo, 62 s, a 4 ms 2 kHz burst every
    0.5 s, louder on whole seconds); tone.wav (mono, 44.1 kHz, 3 s of 440 Hz at 0.4);
    noise.ogg (Vorbis, stereo, 6 s, seeded noise, the channels different); chirp.mp3 (4 s,
    200 to 2000 Hz); broken.mp3 (not sound at all)."""
    import numpy as np
    p = lambda n: os.path.join(pics, n)
    if not os.path.exists(p("clicks.wav")):
        x = np.zeros(62 * RATE)
        burst = np.sin(2 * np.pi * 2000 * np.arange(int(0.004 * RATE)) / RATE)
        for k in range(124):
            i = k * RATE // 2
            x[i:i + len(burst)] = burst * (0.8 if k % 2 == 0 else 0.4)
        write_wav(p("clicks.wav"), np.stack([x, x], 1))
    if not os.path.exists(p("tone.wav")):
        t = np.arange(3 * 44100) / 44100
        write_wav(p("tone.wav"), 0.4 * np.sin(2 * np.pi * 440 * t), 44100)
    if not os.path.exists(p("noise.ogg")):
        rng = np.random.default_rng(7)
        write_wav(p("noise-src.wav"), rng.uniform(-0.3, 0.3, (6 * RATE, 2)))
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", p("noise-src.wav"), "-c:a", "libvorbis",
                        "-q:a", "6", p("noise.ogg")], check=True)
    if not os.path.exists(p("chirp.mp3")):
        t = np.arange(4 * RATE) / RATE
        f = 200 + (2000 - 200) * t / 4
        write_wav(p("chirp-src.wav"), 0.5 * np.sin(2 * np.pi * np.cumsum(f) / RATE))
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", p("chirp-src.wav"), "-c:a", "libmp3lame",
                        "-b:a", "192k", p("chirp.mp3")], check=True)
    if not os.path.exists(p("broken.mp3")):
        import random
        r = random.Random(11)
        open(p("broken.mp3"), "wb").write(bytes(r.randrange(256) for _ in range(20000)))


def numbered(pics, k, w=640, h=360):
    """A drawing showing frame number k (a bar whose place is k, and the digits)."""
    from PIL import Image, ImageDraw
    path = os.path.join(pics, f"num-{k}.png")
    if not os.path.exists(path):
        img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        g = ImageDraw.Draw(img)
        g.rectangle([10 + (k % 60) * 10, 300, 18 + (k % 60) * 10, 350], fill=(20, 20, 30, 255))
        g.text((20, 20), str(k), fill=(20, 20, 30, 255))
        img.save(path)
    return f"num-{k}.png"


def sound_projects(pics):
    """sound: two scenes; scene 1 has cuts A (48 frames) and B (72) with clicks (from 0), the
    tone (from frame -12, gain 0.5), noise (from 30, gain 0.8), chirp (from 60, muted) and the
    broken file; scene 2's cut C (48) has the chirp from frame 10. sound-none: the same
    frames, no sounds. sound-60s: one 1440-frame cut, clicks from 0."""
    def cut(name, frames, first):
        return {"name": name, "tracks": {"Lines": [{"image": numbered(pics, first + i), "length": 1}
                                                   for i in range(frames)]}}
    base = {"width": 640, "height": 360, "fps": 24, "layers": [{"name": "Lines"}]}
    s1 = {"name": "Scene 1", "cuts": [cut("A", 48, 0), cut("B", 72, 48)],
          "audio": [{"file": "clicks.wav", "start": 0},
                    {"file": "tone.wav", "start": -12, "gain": 0.5},
                    {"file": "noise.ogg", "start": 30, "gain": 0.8},
                    {"file": "chirp.mp3", "start": 60, "muted": True},
                    {"file": "broken.mp3", "start": 0}]}
    s2 = {"name": "Scene 2", "cuts": [cut("C", 48, 120)], "audio": [{"file": "chirp.mp3", "start": 10}]}
    none = [{**s1, "audio": []}, {**s2, "audio": []}]
    long = {"name": "Scene 1", "cuts": [{"name": "Long", "tracks": {"Lines": [
        {"image": numbered(pics, i % 240), "length": 1} for i in range(1440)]}}],
            "audio": [{"file": "clicks.wav", "start": 0}]}
    return {"sound": {**base, "scenes": [s1, s2]}, "sound-none": {**base, "scenes": none},
            "sound-60s": {**base, "scenes": [long]}}


# ------------------------------------------------------------------------------------------
# no sound: the app finds no sound server and ALSA's default device is the null one

def quiet_env(out):
    asound = os.path.join(out, "asound-null.conf")
    with open(asound, "w") as f:
        f.write("pcm.!default { type null }\nctl.!default { type hw card 0 }\n")
    env = {
        "PULSE_SERVER": "unix:/nonexistent/pp-qa-no-pulse",
        "PIPEWIRE_REMOTE": "/nonexistent/pp-qa-no-pipewire",
        "ALSA_CONFIG_PATH": asound,
        "PENCILPILOT_NO_SOUND": "1",
    }
    env.update(json.loads(os.environ.get("PP_QA_APP_ENV", "{}")))
    return env


# ------------------------------------------------------------------------------------------
# a private PipeWire with one null sink and no devices: the app's sound goes there, and
# sinkclock records the sink's monitor with the graph's clock (no sound reaches a speaker:
# no ALSA, udev, Bluetooth or JACK module is loaded, and the daemon is reachable only through
# its own runtime folder)

PW_CONF = """
context.properties = {
    core.daemon = true
    core.name = pipewire-0
    default.clock.rate = 48000
    default.clock.quantum = 256
    default.clock.min-quantum = 64
    default.clock.max-quantum = 1024
    link.max-buffers = 16
    mem.allow-mlock = false
    log.level = 2
}
context.spa-libs = {
    audio.convert.* = audioconvert/libspa-audioconvert
    audio.adapt     = audioconvert/libspa-audioconvert
    support.*       = support/libspa-support
}
context.modules = [
    { name = libpipewire-module-protocol-native }
    { name = libpipewire-module-metadata }
    { name = libpipewire-module-spa-node-factory }
    { name = libpipewire-module-client-node }
    { name = libpipewire-module-access args = { } }
    { name = libpipewire-module-adapter }
    { name = libpipewire-module-link-factory }
]
stream.properties = { adapter.auto-port-config = { mode = dsp } }
context.objects = [
    { factory = metadata args = { metadata.name = default } }
    { factory = adapter
        args = {
            factory.name = support.null-audio-sink
            node.name = qa-sink
            media.class = Audio/Sink
            audio.position = [ FL FR ]
            audio.rate = 48000
            object.linger = true
            priority.driver = 30000
            node.always-process = true
            monitor.channel-volumes = true
            adapter.auto-port-config = { mode = dsp monitor = true position = preserve }
        }
    }
]
"""


class PrivatePW:
    """The daemon in <out>/pw-<pid>; `app_env()` points an app's ALSA and PipeWire at it."""

    def __init__(self, out):
        self.dir = os.path.join(os.path.realpath(out), f"pw-{os.getpid()}-{int(time.monotonic() * 1000)}")
        os.makedirs(self.dir, mode=0o700, exist_ok=True)
        conf = os.path.join(self.dir, "qa-null.conf")
        open(conf, "w").write(PW_CONF)
        self.alsa = os.path.join(self.dir, "asound.conf")
        open(self.alsa, "w").write(
            'pcm_type.pipewire { lib "/usr/lib/alsa-lib/libasound_module_pcm_pipewire.so" }\n'
            'ctl_type.pipewire { lib "/usr/lib/alsa-lib/libasound_module_ctl_pipewire.so" }\n'
            'pcm.!default { type pipewire playback_node "qa-sink" }\n'
            'ctl.!default { type pipewire }\n'
            'pcm.pipewire { type pipewire playback_node "qa-sink" }\n')
        # the clients' config: the system's client.conf, their streams' ports made without a
        # session manager (the user's own config isn't read)
        self.client = os.path.join(self.dir, "client")
        for d in ("client.conf.d", "client-rt.conf.d"):
            os.makedirs(os.path.join(self.client, d), exist_ok=True)
            open(os.path.join(self.client, d, "qa.conf"), "w").write(
                "stream.properties = { adapter.auto-port-config = { mode = dsp monitor = false position = preserve } }\n")
        for f in ("client.conf", "client-rt.conf"):
            if os.path.exists(f"/usr/share/pipewire/{f}"):
                open(os.path.join(self.client, f), "w").write(open(f"/usr/share/pipewire/{f}").read())
        # (the clients: pw-link, sinkclock)
        self.env = {**os.environ, "PIPEWIRE_RUNTIME_DIR": self.dir, "XDG_CONFIG_HOME": os.path.join(self.dir, "cfg"),
                    "PIPEWIRE_REMOTE": "pipewire-0"}
        self.log = open(os.path.join(self.dir, "pipewire.log"), "a")
        # (the daemon: only this folder's config, no drop-ins from anywhere else)
        self.p = subprocess.Popen(["pipewire", "-c", conf], env={**self.env, "PIPEWIRE_CONFIG_DIR": self.dir},
                                  stdout=self.log, stderr=self.log)
        for _ in range(100):
            if os.path.exists(os.path.join(self.dir, "pipewire-0")):
                break
            time.sleep(0.05)
        time.sleep(0.3)

    def app_env(self):
        return {"PIPEWIRE_RUNTIME_DIR": self.dir, "PIPEWIRE_REMOTE": "pipewire-0",
                "PIPEWIRE_CONFIG_DIR": self.client, "ALSA_CONFIG_PATH": self.alsa, "PULSE_SERVER": "unix:/nonexistent/pp-qa-no-pulse",
                "PENCILPILOT_NO_SOUND": ""}

    def run(self, *cmd, timeout=10):
        return subprocess.run(list(cmd), env=self.env, capture_output=True, text=True, timeout=timeout)

    def ports(self, direction):
        """Port names (node:port), `-o` outputs or `-i` inputs."""
        return [ln.strip() for ln in self.run("pw-link", direction).stdout.splitlines() if ln.strip()]

    def link_into(self, node_prefix, dst_node, wait=8.0):
        """Link the output ports of the nodes whose name starts with `node_prefix` (None: any
        node but the sink and the recorder) to `dst_node`'s inputs, FL to FL and FR to FR (a
        mono port to both); returns the links made."""
        end = time.monotonic() + wait
        while time.monotonic() < end:
            outs = [p for p in self.ports("-o")
                    if (p.startswith(node_prefix) if node_prefix else not p.startswith(("qa-sink:", "qa-capture:")))]
            ins = [p for p in self.ports("-i") if p.startswith(dst_node + ":")]
            if outs and len(ins) >= 2:
                made = []
                for o in sorted(outs):
                    sides = ["FL"] if o.endswith("FL") else ["FR"] if o.endswith("FR") else ["FL", "FR"]
                    for side in sides:
                        tgt = next((p for p in ins if p.endswith(side)), None)
                        if tgt and self.run("pw-link", o, tgt).returncode == 0:
                            made.append((o, tgt))
                return made
            time.sleep(0.1)
        return []

    def stop(self):
        if self.p.poll() is None:
            self.p.terminate()
            try:
                self.p.wait(5)
            except subprocess.TimeoutExpired:
                self.p.kill()
                self.p.wait()


def sinkclock_bin(out):
    src = os.path.join(HERE, "sinkclock.c")
    exe = os.path.join(out, "sinkclock")
    if not os.path.exists(exe) or os.path.getmtime(exe) < os.path.getmtime(src):
        flags = subprocess.run(["pkg-config", "--cflags", "--libs", "libpipewire-0.3"], capture_output=True,
                               text=True, check=True).stdout.split()
        subprocess.run(["cc", "-O2", "-o", exe, src, *flags, "-lm"], check=True)
    return exe


def read_sinkclock(path):
    """(times_ns per sample (int64), samples (n, 2) float32) from a sinkclock recording."""
    import numpy as np
    raw = open(path, "rb").read()
    ts, xs, at = [], [], 0
    while at + 24 <= len(raw):
        now, ticks, rate, n = struct.unpack_from("<qQII", raw, at)
        at += 24
        x = np.frombuffer(raw, "<f4", n * 2, at).reshape(n, 2)
        at += n * 8
        ts.append(now + (np.arange(n) * 1e9 / rate).astype(np.int64))
        xs.append(x)
    if not ts:
        return np.zeros(0, np.int64), np.zeros((0, 2), np.float32)
    return np.concatenate(ts), np.concatenate(xs)


def onsets(t, x, thresh=0.05, quiet_s=0.1, rate=48000):
    """When each click starts: the first sample over `thresh` after `quiet_s` under it.
    Returns [(time_ns, peak)]."""
    import numpy as np
    a = np.abs(x).max(axis=1)
    loud = np.nonzero(a > thresh)[0]
    out, last = [], -10**12
    q = int(quiet_s * rate)
    for i in loud:
        if i - last > q:
            out.append((int(t[i]), float(a[i:i + 200].max())))
        last = i
    return out


# ------------------------------------------------------------------------------------------
# sampling a running app

def proc_cpu_s(pid):
    with open(f"/proc/{pid}/stat") as f:
        v = f.read().rsplit(")", 1)[1].split()
    return (int(v[11]) + int(v[12])) / CLK


def vram_kib(pid):
    """The DRM clients' VRAM and GTT of a process (amdgpu fdinfo), in KiB, or None."""
    vram = gtt = 0
    seen = set()
    found = False
    try:
        fds = os.listdir(f"/proc/{pid}/fdinfo")
    except OSError:
        return None
    for fd in fds:
        try:
            with open(f"/proc/{pid}/fdinfo/{fd}") as f:
                txt = f.read()
        except OSError:
            continue
        if "drm-client-id" not in txt:
            continue
        cid = next((ln.split()[1] for ln in txt.splitlines() if ln.startswith("drm-client-id")), fd)
        if cid in seen:
            continue
        seen.add(cid)
        for ln in txt.splitlines():
            k, _, rest = ln.partition(":")
            parts = rest.split()
            if len(parts) < 1:
                continue
            mult = {"KiB": 1, "MiB": 1024, "GiB": 1024 * 1024}.get(parts[1] if len(parts) > 1 else "KiB", 1)
            if k in ("drm-memory-vram", "drm-resident-vram"):
                vram = max(vram, int(parts[0]) * mult) if k == "drm-resident-vram" else vram + int(parts[0]) * mult
                found = True
            elif k in ("drm-memory-gtt", "drm-resident-gtt"):
                gtt += int(parts[0]) * mult
    return {"vramKiB": vram, "gttKiB": gtt} if found else None


class Sampler:
    """PSS (and VRAM) every 50 ms between start() and stop(), and CPU seconds over it."""

    def __init__(self, pid):
        self.pid = pid
        self.pss = []
        self.vram = []
        self.cpu0 = self.t0 = None

    def start(self):
        self.cpu0, self.t0 = proc_cpu_s(self.pid), time.monotonic()

    def run_for(self, secs, every=0.05):
        end = time.monotonic() + secs
        while time.monotonic() < end:
            p = cw_pss(self.pid)
            if p:
                self.pss.append(p)
            v = vram_kib(self.pid)
            if v:
                self.vram.append(v["vramKiB"] + v["gttKiB"])
            time.sleep(every)

    def stop(self):
        cpu1, t1 = proc_cpu_s(self.pid), time.monotonic()
        return {
            "cpuPctOfCore": round(100 * (cpu1 - self.cpu0) / (t1 - self.t0), 1),
            "pssPeakMiB": self.pss and round(max(self.pss) / 1024, 1),
            "pssMedianMiB": self.pss and round(statistics.median(self.pss) / 1024, 1),
            "gpuMemPeakMiB": self.vram and round(max(self.vram) / 1024, 1),
        }


def cw_pss(pid):
    try:
        with open(f"/proc/{pid}/smaps_rollup") as f:
            for line in f:
                if line.startswith("Pss:"):
                    return int(line.split()[1])
    except OSError:
        return None


# ------------------------------------------------------------------------------------------
# what playback showed

def timeline_changes(st, t0, t1):
    """(shownNs, sheetFrame, workNs) of each shown frame where the timeline frame changed,
    between t0 and t1 (by when drawn); and the frame shown just before t0."""
    fr = sorted((f for f in st["frames"] if f["shownNs"]), key=lambda f: f["shownNs"])
    before = [f for f in fr if f["drawnNs"] < t0]
    last = before[-1]["sheetFrame"] if before else None
    out = []
    for f in fr:
        if not (t0 <= f["drawnNs"] <= t1):
            continue
        if f["sheetFrame"] != last:
            out.append((f["shownNs"], f["sheetFrame"], f["workNs"]))
            last = f["sheetFrame"]
    return out, (before[-1]["sheetFrame"] if before else None), fr


def analyse_play(changes, n_frames, fps, hz, t_key):
    """Skipped / repeated timeline frames, timing against the display frame nearest each
    frame's time (the schedule's start fitted as the median offset), drift."""
    if len(changes) < 3:
        return {"played": False, "changes": len(changes)}
    k, ks, skips, steps = 0, [0], 0, {}
    for a, b in zip(changes, changes[1:]):
        step = (b[1] - a[1]) % n_frames
        steps[step] = steps.get(step, 0) + 1
        if step != 1:
            skips += step - 1 if step > 1 else 1
        k += step
        ks.append(k)
    per = 1e9 / fps
    offs = sorted(c[0] - kk * per for c, kk in zip(changes, ks))
    t_start = offs[len(offs) // 2]
    err = [(c[0] - (t_start + kk * per)) / 1e6 for c, kk in zip(changes, ks)]
    ts = [(c[0] - changes[0][0]) / 1e9 for c in changes]
    # drift: least squares slope of the error (ms per minute)
    mt, me = statistics.fmean(ts), statistics.fmean(err)
    den = sum((t - mt) ** 2 for t in ts)
    slope = sum((t - mt) * (e - me) for t, e in zip(ts, err)) / den * 60 if den else 0.0
    first = [e for t, e in zip(ts, err) if t < 5]
    last = [e for t, e in zip(ts, err) if t > ts[-1] - 5]
    span = (changes[-1][0] - changes[0][0]) / 1e9
    expected = round(span * fps) + 1
    iv = [(b[0] - a[0]) / 1e6 for a, b in zip(changes, changes[1:])]
    half = 500 / hz
    return {
        "played": True,
        "firstFrameAfterKeyMs": round((changes[0][0] - t_key) / 1e6, 1),
        # (the first change is the next timeline frame, due 1 / fps after Play)
        "firstFrameLateMs": round((changes[0][0] - t_key - per) / 1e6, 1),
        "timelineFramesShown": len(changes),
        "expectedFrames": expected,
        "skipped": skips,
        "stepCounts": {str(s): n for s, n in sorted(steps.items())},
        "fpsShown": round((len(changes) - 1) / span, 3) if span else None,
        "onScreenMs": {"p50": round(cw.pct(iv, .5), 2), "min": round(min(iv), 2), "max": round(max(iv), 2)},
        "offScheduleMs": {"p50": round(cw.pct([abs(e) for e in err], .5), 2),
                          "p99": round(cw.pct([abs(e) for e in err], .99), 2),
                          "max": round(max(abs(e) for e in err), 2)},
        "halfDisplayFrameMs": round(half, 2),
        "driftMsPerMin": round(slope, 3),
        "first5sVsLast5sMs": round(statistics.median(last) - statistics.median(first), 2) if first and last else None,
        "workMsOfChanges": {"p50": round(cw.pct([c[2] / 1e6 for c in changes], .5), 2),
                            "p99": round(cw.pct([c[2] / 1e6 for c in changes], .99), 2),
                            "max": round(max(c[2] for c in changes) / 1e6, 2)},
        "onTime": skips == 0 and max(abs(e) for e in err) <= half + 0.5,
    }


def launch(binary, m, out, tag, secs, project, env=None):
    e = quiet_env(out)
    e.update(env or {})
    return cw.App(binary, m, out, tag, secs, "animate", args=[project], env=e)


def wait_opened(app, timeout=8.0):
    """Wait until the opened project is on screen and the app is quiet (no frame drawn for
    0.3 s, judged by the CPU it uses)."""
    t_end = time.monotonic() + timeout
    time.sleep(1.5)
    last = proc_cpu_s(app.p.pid)
    while time.monotonic() < t_end:
        time.sleep(0.3)
        now = proc_cpu_s(app.p.pid)
        if now - last < 0.02:
            return True
        last = now
    return False


def play_once(binary, out, tag, hz, project, secs, n_frames, fps=24):
    m = cw.Mutter(out, hz)
    try:
        app = launch(binary, m, out, tag, secs + 14, project)
        quiet = wait_opened(app)
        pss_open = cw_pss(app.p.pid)
        vram_open = vram_kib(app.p.pid)
        inp = cw.Input()
        inp.to(560, 330)
        time.sleep(0.2)
        smp = Sampler(app.p.pid)
        load = os.getloadavg()[0]
        t_key = cw.mono_ns()
        smp.start()
        inp.key("Return")
        smp.run_for(secs)
        t1 = cw.mono_ns()
        res_s = smp.stop()
        inp.key("Return")  # stop
        inp.stop()
        st = app.wait()
        changes, _, _ = timeline_changes(st, t_key, t1)
        r = analyse_play(changes, n_frames, fps, hz, t_key)
        r.update(res_s)
        r.update({"pssAfterOpenMiB": pss_open and round(pss_open / 1024, 1),
                  "gpuMemAfterOpenMiB": vram_open and round((vram_open["vramKiB"] + vram_open["gttKiB"]) / 1024, 1),
                  "settledBeforePlay": quiet, "load1": round(load, 2), "exit": app.p.returncode,
                  "layers": len(st["state"]["layers"])})
        return r
    finally:
        m.stop()


def summarise(runs, keys):
    """Median and spread (min–max) of each key over the runs."""
    out = {}
    for k in keys:
        v = [r.get(k) for r in runs if isinstance(r.get(k), (int, float))]
        if v:
            out[k] = {"median": round(statistics.median(v), 3), "min": min(v), "max": max(v), "n": len(v)}
    return out


PLAY_KEYS = ["firstFrameAfterKeyMs", "firstFrameLateMs", "skipped", "fpsShown", "cpuPctOfCore", "pssPeakMiB",
             "pssAfterOpenMiB", "gpuMemPeakMiB", "gpuMemAfterOpenMiB", "driftMsPerMin", "load1"]


def nested(runs, path):
    v = []
    for r in runs:
        x = r
        for p in path:
            x = x.get(p) if isinstance(x, dict) else None
        if isinstance(x, (int, float)):
            v.append(x)
    return v and {"median": round(statistics.median(v), 3), "min": min(v), "max": max(v)}


def scenario_play(binary, out, name="layers20-4k", n_frames=24, secs=10.0):
    res = {}
    for hz in (165, 60):
        runs = [play_once(binary, out, f"p4play-{name}-{hz}-{i}", hz, os.path.join(FIX, f"{name}.ppstudio"),
                          secs, n_frames) for i in range(RUNS)]
        s = summarise(runs, PLAY_KEYS)
        s["offScheduleMaxMs"] = nested(runs, ["offScheduleMs", "max"])
        s["workMsOfChangesP99"] = nested(runs, ["workMsOfChanges", "p99"])
        s["onScreenMaxMs"] = nested(runs, ["onScreenMs", "max"])
        s["allOnTime"] = all(r.get("onTime") for r in runs)
        res[str(hz)] = {"summary": s, "runs": runs}
    return res


def scenario_play500(binary, out):
    return scenario_play(binary, out, "drawings500", 100, 10.0)


def scenario_drift(binary, out):
    hz = int(os.environ.get("PP_QA_DRIFT_HZ", "165"))
    r = play_once(binary, out, f"p4drift-{hz}", hz, os.path.join(FIX, "layers20-4k.ppstudio"), 62.0, 24)
    return {str(hz): r}


def scenario_scrub(binary, out):
    res = {}
    for name, n in (("layers20-4k", 23), ("drawings500", 99)):
        for hz in (165, 60):
            runs = []
            for i in range(RUNS):
                m = cw.Mutter(out, hz)
                try:
                    app = launch(binary, m, out, f"p4scrub-{name}-{hz}-{i}", 12 + n * 2 / hz,
                                 os.path.join(FIX, f"{name}.ppstudio"))
                    wait_opened(app)
                    inp = cw.Input()
                    inp.to(560, 330)
                    time.sleep(0.2)
                    period = 1 / hz
                    sent = []
                    for key in ("Right", "Left"):
                        for _ in range(n):
                            sent.append((cw.mono_ns(), key))
                            inp.key(key)
                            time.sleep(period)
                        time.sleep(0.5)
                    t_end = cw.mono_ns()
                    inp.stop()
                    st = app.wait()
                    fr = sorted((f for f in st["frames"] if f["shownNs"] and sent[0][0] <= f["drawnNs"] <= t_end),
                                key=lambda f: f["shownNs"])
                    lat, missing = [], 0
                    for j, (t, key) in enumerate(sent):
                        want = j + 1 if j < n else n - (j - n + 1)
                        hit = next((f for f in fr if f["shownNs"] >= t and f["sheetFrame"] == want), None)
                        if hit is None:
                            missing += 1
                        else:
                            lat.append((hit["shownNs"] - t) / 1e6)
                    changes = [b for a, b in zip(fr, fr[1:]) if b["sheetFrame"] != a["sheetFrame"]]
                    work = [f["workNs"] / 1e6 for f in changes]
                    gaps = [(b["shownNs"] - a["shownNs"]) / 1e6 for a, b in zip(changes, changes[1:])
                            if b["shownNs"] - a["shownNs"] < 0.4e9]
                    q = lambda v: v and {"p50": round(cw.pct(v, .5), 2), "p99": round(cw.pct(v, .99), 2),
                                         "max": round(max(v), 2)}
                    runs.append({"steps": len(sent), "neverShown": missing, "keyToShownMs": q(lat),
                                 "workMsPerChange": q(work), "changeIntervalMs": q(gaps),
                                 "displayFrameMs": round(1000 / hz, 2), "lastSheetFrame": st["state"]["frame"],
                                 "load1": round(os.getloadavg()[0], 2)})
                finally:
                    m.stop()
            s = {k: nested(runs, p) for k, p in (("workP50", ["workMsPerChange", "p50"]),
                                                  ("workP99", ["workMsPerChange", "p99"]),
                                                  ("workMax", ["workMsPerChange", "max"]),
                                                  ("keyToShownP99", ["keyToShownMs", "p99"]),
                                                  ("changeIntervalP50", ["changeIntervalMs", "p50"]),
                                                  ("neverShown", ["neverShown"]))}
            res[f"{name}-{hz}"] = {"summary": s, "runs": runs}
    return res


def stalls(st, t0, t1):
    fr = sorted((f for f in st["frames"] if f["shownNs"] and t0 <= f["drawnNs"] <= t1), key=lambda f: f["shownNs"])
    gaps = [(b["shownNs"] - a["shownNs"]) / 1e6 for a, b in zip(fr, fr[1:])]
    return {"framesShown": len(fr), "longestGapMs": gaps and round(max(gaps), 1),
            "gapsOver100ms": sum(1 for g in gaps if g > 100)}


def scenario_switch(binary, out):
    """Scrubbing while playing, stop / play, and switching cuts while playing."""
    res = {}
    for hz in (165, 60):
        m = cw.Mutter(out, hz)
        try:
            app = launch(binary, m, out, f"p4switch-{hz}", 20, os.path.join(FIX, "layers20-4k-2cuts.ppstudio"))
            wait_opened(app)
            inp = cw.Input()
            inp.to(560, 330)
            time.sleep(0.2)
            t0 = cw.mono_ns()
            inp.key("Return")
            time.sleep(1.5)
            for key in ["Right"] * 10 + ["Left"] * 10:
                inp.key(key)
                time.sleep(0.03)
            time.sleep(1.0)
            inp.key("Return")
            time.sleep(0.5)
            inp.key("Return")
            time.sleep(1.5)
            t1 = cw.mono_ns()
            inp.stop()
            st = app.wait()
            ch, _, _ = timeline_changes(st, t0, t1)
            r = stalls(st, t0, t1)
            r.update({"timelineChanges": len(ch), "exit": app.p.returncode, "playingAtEnd": st["state"]["playing"]})
            res[str(hz)] = r
        finally:
            m.stop()
    return res


def scenario_latency(binary, out):
    """Pen to screen while drawing on the 4K 20-layer project's top layer."""
    res = {}
    for hz in (165, 60):
        runs = []
        for i in range(RUNS):
            m = cw.Mutter(out, hz)
            try:
                e = quiet_env(out)
                app = cw.App(binary, m, out, f"p4lat-{hz}-{i}", 10, "draw",
                             args=[os.path.join(FIX, "layers20-4k.ppstudio")], env=e)
                wait_opened(app)
                inp = cw.Input()
                t0 = cw.mono_ns()
                k = 0
                while (cw.mono_ns() - t0) < 4e9:
                    y = 200 + (k % 30) * 15
                    inp.drag(cw.line((200, y), (900, y + 40), 200), 0.002)
                    k += 1
                t1 = cw.mono_ns()
                inp.stop()
                st = app.wait()
                fr = cw.frame_numbers(st, t0, t1 + 50_000_000)
                inputs = sorted((f for f in fr if f["events"] and f["shownNs"]), key=lambda f: f["shownNs"])
                runs.append({"strokes": k, "rate": cw.rate(fr, hz), "latency": cw.latency(inputs),
                             "budgetMs": round(1000 / hz + 4, 2), "load1": round(os.getloadavg()[0], 2)})
            finally:
                m.stop()
        s = {"penToScreenP99": nested(runs, ["latency", "fromEventTimeMs", "p99"]),
             "penToScreenP50": nested(runs, ["latency", "fromEventTimeMs", "p50"]),
             "fps": nested(runs, ["rate", "fps"]), "missed": nested(runs, ["rate", "missed"])}
        res[str(hz)] = {"summary": s, "runs": runs}
    return res


SCENARIOS = {"play": scenario_play, "drift": scenario_drift, "play500": scenario_play500,
             "scrub": scenario_scrub, "switch": scenario_switch, "latency": scenario_latency}


def main():
    a = sys.argv[1:]
    if len(a) >= 2 and a[0] == "fixtures":
        return fixtures(os.path.realpath(a[1]))
    if len(a) >= 3 and a[0] == "run":
        binary, out = os.path.realpath(a[1]), os.path.realpath(a[2])
        os.makedirs(out, exist_ok=True)
        env = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
        os.execvpe("dbus-run-session", ["dbus-run-session", "--", sys.executable, "-I",
                                        os.path.realpath(__file__), "--inner", binary, out] + a[3:], env)
    if len(a) >= 3 and a[0] == "--inner":
        if not FIX:
            sys.exit("PP_QA_FIXTURES is not set")
        binary, out = a[1], a[2]
        names = a[3:] or list(SCENARIOS)
        path = os.path.join(out, "phase4.json")
        allres = json.load(open(path)) if os.path.exists(path) else {}
        for n in names:
            t = time.monotonic()
            try:
                r = SCENARIOS[n](binary, out)
            except Exception as e:  # one scenario's failure is a result too
                import traceback
                r = {"error": repr(e), "trace": traceback.format_exc()}
            r["seconds"] = round(time.monotonic() - t, 1)
            r["uptime"] = open("/proc/loadavg").read().strip()
            allres[n] = r
            json.dump(r, open(os.path.join(out, f"{n}.json"), "w"), indent=2)
            print(n, json.dumps({k: (v.get("summary", v) if isinstance(v, dict) else v) for k, v in r.items()},
                                indent=1), flush=True)
        json.dump(allres, open(path, "w"), indent=2)
        return
    print(__doc__)
    sys.exit(2)


if __name__ == "__main__":
    main()
