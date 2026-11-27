# Baseline

Measures the current app (Tauri + WebKitGTK) on one machine, so the native rebuild is judged
against real numbers. Every number in a later performance PR is compared with a report from
this tool, taken on the same machine.

Nothing opens on the screen. The app runs on a headless Wayland display (`mutter --headless`,
rendering on the real GPU), is driven over WebDriver, and keeps its data, settings and caches
in a folder of its own, so your own recent projects and settings are never touched.

## Needs

Linux with `mutter`, `WebKitWebDriver` (part of webkit2gtk-4.1), `tauri-driver`
(`cargo install tauri-driver`), Node 22+, and Python 3 with Pillow.

## Run

```sh
# the app, as released
pnpm wasm:brush && (cd apps/studio && pnpm tauri build --no-bundle)
# the test projects (the same every time)
python3 tools/baseline/fixtures.py target/baseline/projects
# the measurements (about 6 minutes; --quick for about 2)
node tools/baseline/run.mjs --app apps/studio/src-tauri/target/release/pencilpilot-studio \
  --projects target/baseline/projects --out target/baseline/report.json [--cold]
```

`--hz <rate>` runs the hidden display at that refresh rate (60 by default). With
`--refresh-only`, the tool only measures the app's frame rate and drawing at that rate.

`--cold` adds a cold start: the system's file cache is dropped first with `sudo`, which reads
its password from this program's input when it isn't cached.

## What it measures

| Scenario | Numbers |
|---|---|
| Machine without the app | CPU package and GPU power, before and after, the reference for "above idle" |
| Startup | Start to home screen, warm (5 runs) and cold; memory at the home screen |
| Empty canvas (1920×1080) | Memory; 30 s idle: CPU, frames the app asks for, GPU, power; drawing |
| 20 layers at 4K | Open time, memory, idle, drawing, playback |
| 500 drawings (1920×1080) | Open time, peak and settled memory, playback |

- **Drawing:** 10 pen strokes of 1 s at 240 Hz, with pressure, sent to the canvas as pen
  pointer events.
  - *Pen to drawn* is the time from when each sample was due to the end of the app's redraw
    that includes it.
  - *Pen to screen* is the start of the next frame, when the drawn pixels are on their way to
    the display.
  - Both are measured inside the page. A pen's own trip through the OS (a few ms) is not
    included, and neither is the display's scan-out.
- **Frame time** is the gap between animation frames while drawing or playing (16.7 ms at
  60 Hz). A gap over 1.5 frames is a missed frame.
- **Playback** loops the cut and records when the shown frame changes. *Start* is the wait
  between Play and the first frame. *Skipped* counts frames that were never shown.
- **Memory** is PSS: the app's process and its WebKit processes, with shared pages counted
  once.
- **CPU** is a share of one core: 100 means one core fully busy.
- **Power** is the CPU package (RAPL) and the GPU's own sensor, minus the machine without the
  app. It is the whole machine, so other programs add noise: compare runs taken with the
  desktop left alone.

The budgets the native app is held to are in [BUDGETS.md](BUDGETS.md), and the results in [results/](results/README.md).
