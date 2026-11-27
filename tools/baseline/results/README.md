# Baseline results

## 2026-11-27 · Linux · today's app (Tauri + WebKitGTK)

**Machine and build**
- **Machine:** Ryzen 7 5700X (16 threads), 31 GB, Radeon RX 6800 XT, CachyOS (kernel 7.3), headless
  mutter at 3840×2160, 60 Hz.
- **Build:** `dev` at 28bb303, plus #350 (big projects keep their drawings when opened). The binary
  is 21.7 MB, plus WebKitGTK from the system.

**The reference run:** [`2026-11-27-linux-quiet.json`](2026-11-27-linux-quiet.json).
- Every other program was closed (1-minute load average under 1).
- With the app closed, the machine drew 28.1 W on the CPU package and 35.5 W on the GPU before
  the run, and 27.9 W and 35.0 W after it. Power above that is the app's.
- The GPU's 35 W is this card's own floor with the monitors attached, not the app.

| What | Result |
|---|---|
| **Startup** | 872 ms cold; 338 ms warm (median of 5) |
| **Memory** | 308 MB at the home screen; 419 MB with an empty 1920×1080 canvas |
| | 20 layers at 4K: 1.06 GB open, 1.75 GB at its peak while playing |
| | 500 drawings at 1080p: 1.23 GB, 1.34 GB at its peak while playing |
| **Idle** (30 s) | 0 frames asked for; 0.07% of a core; under 1 W above the machine without the app |
| **Pen to screen** (240 Hz strokes with pressure) | Empty canvas: 28 ms median, 40 ms p99 |
| | 20 layers at 4K: 29 ms median, 40 ms p99 |
| **Frames while drawing** | 16 ms median, 1 missed in about 800 |
| **Drawing, CPU and power** | Empty canvas: 29% of a core, +6.3 W CPU, +2.0 W GPU |
| | 20 layers at 4K: 33% of a core, +7.8 W CPU, +2.1 W GPU |
| **Open** | 20 layers at 4K: 0.88 s; 500 drawings: 0.95 s |
| **Playback, 20 layers at 4K** | 6.8 s before the first frame; 25 of 361 frames skipped |
| | 63% of a core, +10.6 W CPU, +4.1 W GPU |
| **Playback, 500 drawings** | 9.6 s before the first frame; none skipped |
| | 78% of a core, +13.1 W CPU, +3.7 W GPU |

**Other runs the same day** (on a machine in use)
- [`2026-11-27-linux.json`](2026-11-27-linux.json) and two runs not kept.
- **Stable within about 15%:** memory, idle, warm startup, open times, and pen latency.
- **Swings with machine load:** 4K drawing jank (0 to 108 missed frames), 4K playback skips (13 to
  74), and cold startup (0.7 to 1.8 s).
- **Power can't be read** on a machine in use: with the app closed it moved by ±10 W.

Power numbers need a run like the reference one: every other program closed.
