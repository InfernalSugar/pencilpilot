# Baseline results

## 2026-11-27 · Linux · today's app (Tauri + WebKitGTK)

Raw report: [`2026-11-27-linux.json`](2026-11-27-linux.json).

**Machine and build**
- **Machine:** Ryzen 7 5700X (16 threads), 31 GB, Radeon RX 6800 XT, CachyOS (kernel 7.3), headless
  mutter at 3840×2160, 60 Hz.
- **Build:** `dev` at 28bb303, plus #350 (big projects keep their drawings when opened). The binary
  is 21.7 MB, plus WebKitGTK from the system.
- **Load:** the desktop was in use (1-minute load average about 3), so the power figures are not
  usable (see the end).

| What | Result |
|---|---|
| **Startup** | 722 ms cold; 376 ms warm (median of 5, worst 415 ms) |
| **Memory** | 243 MB at the home screen; 360 MB with an empty 1920×1080 canvas |
| | 20 layers at 4K: 989 MB open, 1.68 GB at its peak while playing |
| | 500 drawings at 1080p: 1.16 GB |
| **Idle** | 0 frames asked for in 30 s, 0.1% of a core (empty canvas and 4K) |
| **Pen to screen** (240 Hz strokes with pressure) | Empty canvas: 28 ms median, 40 ms p99 |
| | 20 layers at 4K: 29 ms median, 40 ms p99 |
| **Pen to drawn** (end of the app's redraw) | 12–14 ms median, 24 ms p99 |
| **Frames while drawing** | 16 ms median, 0–2 missed in about 800 |
| **CPU while drawing** | 33% of a core (empty); 42% (4K, 20 layers) |
| **Open** | 20 layers at 4K: 0.88 s; 500 drawings: 0.99 s |
| **Playback, 20 layers at 4K** | 7.6 s before the first frame; 16 of 361 frames skipped; 70% of a core |
| **Playback, 500 drawings** | 9.7 s before the first frame; none skipped; 88% of a core |

**Between runs** (three runs on 2026-11-27; the other two were on a busier machine)
- **Stable within 10%:** memory, idle, warm startup, open times, and pen latency on the empty canvas.
- **Swings with machine load:** the 4K drawing jank (0 to 108 missed frames), playback skips (13 to
  74), and cold startup (0.7 to 1.8 s).

**Power:** these runs can't separate the app's power from the rest of the desktop. The CPU package
(RAPL) and the GPU sensor read the whole machine, and with the app closed they varied by ±10 W
between readings. Until a run on an otherwise idle machine, use CPU time and GPU busy as the proxies
for heat.
