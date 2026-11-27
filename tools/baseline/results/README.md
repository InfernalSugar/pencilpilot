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

## 2026-11-27 · Refresh rate

`run.mjs --hz <rate> --refresh-only` measures the app on a hidden display at each of the owner's
monitor rates.
- **Idle:** the app's animation frames, asked for back to back for 3 s on an empty canvas.
- **Drawing:** 3 pen strokes at 240 Hz.

| Display | The app draws at | Frames while drawing (median / p99) | Pen to screen (median / p99) |
|---|---|---|---|
| 165 Hz | 62.5 fps | 16 / 19 ms | 29.5 / 39.5 ms |
| 100 Hz | 62.5 fps | 16 / 18 ms | 27.5 / 39.2 ms |
| 75 Hz | 62.5 fps | 16 / 18 ms | 28.7 / 39.3 ms |
| 60 Hz | 62.5 fps | 16 / 19 ms | 28.8 / 38.8 ms |

**Control:** a GTK 4 window on the same hidden 165 Hz display runs at 165.0 fps. The display isn't
the limit: WebKitGTK caps the app at about 60 fps on every display.

## 2026-11-27 · The rest of the budgets

Raw reports: [`2026-11-27-linux-more.json`](2026-11-27-linux-more.json) and
[`2026-11-27-linux-session.json`](2026-11-27-linux-session.json).
- **Machine:** quiet, load average under 1.4. With the app closed: 29.9–30.5 W CPU, 36.7 W GPU.
- **Build:** `dev` at 9ec79d6.
- **Export:** goes to the baseline's own folder through the app's `bench_export_dir`, so no save
  dialog opens.

| What | Result |
|---|---|
| **Airbrush (Revoy), 220 px** | Pen to screen 28 / 39 ms (median / p99), 0 of 499 frames missed; 43% of a core, +8 W |
| **Oil brush, 11 px** | **Pen to screen 410 ms / 1.2 s, 42 of 273 frames missed**; 84% of a core, +11.5 W |
| **Blend+paint, 27 px** | 28 / 38 ms, 0 missed; 31% of a core |
| **Water diffuser, 24 px** | 28 / 39 ms, 0 missed; 52% of a core, +11 W |
| **Pan and zoom**, 20 layers at 4K | 60 fps with no frame missed; 35% of a core, +8 W |
| **Scrubbing**, 500 drawings | 83 of 85 frames shown, at about 14 fps (69 ms a frame); 1.4 cores |
| **Scrubbing**, 20 layers at 4K | 21 of 22 frames shown, at about 10 fps, with 0.4 s stalls |
| **Moving 500 strokes** | Lifted in 18 ms; dragged at about 24 fps; 45 ms to finish |
| **Moving 2,000 strokes** | Lifted in 51 ms; dragged at about 23 fps; 156 ms to finish |
| **Click to result** (tools, panels, menus, frame steps, onion skin) | 24 ms median, at most 46 ms |
| **Export MP4**, 20 layers at 4K, 24 frames | 10.1 s (421 ms a frame); 3.2 GB peak; 1.7 cores, +16 W |
| **Export MP4**, 500 drawings at 1080p, 100 frames | 12.5 s (125 ms a frame); 2.7 GB peak; 2.1 cores, +22 W |
| **30-minute session**, 20 layers at 4K (112 rounds of 2 strokes, 4 s playing, 2 s scrubbing) | 1.06 GB → 1.41 GB, 2.0 GB peak |

**How the scenarios run**
- **Brushes:** 6 strokes of 1 s at 240 Hz on an empty 1920×1080 canvas, each brush picked by
  searching the Brushes panel. The screenshots show every stroke landed.
- **Moving strokes:** short strokes on a new line layer, selected with Ctrl+A, moved with Transform
  (V) for 2 s, then finished by picking the brush. The owner's case, long strokes over a shirt,
  froze the app outright; these short ones don't reach that.
- **Click to result:** two animation frames after the click, so about one frame after the result is
  drawn.
- **Export:** the MP4 files were checked: the right size, every frame, every layer.
