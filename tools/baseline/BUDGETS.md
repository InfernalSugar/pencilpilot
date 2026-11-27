# Performance budgets for the native app

Approved by the owner on 2026-11-27. They can change later if real use turns up performance problems
that need them.

- **Starting point:** today's app, measured in [`results/`](results/README.md).
- **Where they're measured:** with `tools/baseline` on the reference machine (Ryzen 7 5700X, RX 6800
  XT, Linux), with every other program closed when power is measured.
- **What's checked:** every performance PR shows its numbers against these, and a miss blocks the
  merge.
- **Before the switch to the native app:** the same budgets are also checked on a modest Windows
  laptop.

## Display rate

The app draws at the refresh rate of the monitor its window is on: 60, 75, 100, 144, 165 Hz or
more. It switches when the window moves to a monitor with another rate.
- **Sources:** Wayland frame callbacks and presentation feedback on Linux, DXGI on Windows, and the
  display link on macOS.
- **Rule:** nothing assumes 60 Hz. Budgets are written in display frames wherever a frame is
  involved.

Today's app is capped at 60 fps by WebKitGTK on every display. A GTK window on the same 165 Hz
display runs at 165 fps.

| What | Today | Budget |
|---|---|---|
| **Drawing frame rate** | 62.5 fps at 60, 75, 100 and 165 Hz | **The display's rate** (165 fps at 165 Hz) |
| **Pen to screen**, p99 | 40 ms at any rate | **Within 1 display frame + 4 ms** (10 ms at 165 Hz, 21 ms at 60 Hz) |
| **Missed frames while drawing**, 20 layers at 4K | 0–17%, depending on machine load | **0**, with the machine busy too |
| **Moving a selection of 2,000 strokes** | About 23 fps while dragging; 156 ms to finish (the owner's real case froze the app) | **At the display's rate while dragging; done within 100 ms of letting go** |
| **Clicks, menus, tool switches** | 24 ms to the frame that shows it (about 1.5 frames) | **The result shows on the next display frame** |
| **Scrubbing the timeline** | 500 drawings: about 14 fps; 20 layers at 4K: about 10 fps, with 0.4 s stalls | **At the display's rate** |
| **Panning and zooming a 4K canvas** | 60 fps (the cap); 35% of a core, +8 W | **At the display's rate** |

## Playback

| What | Today | Budget |
|---|---|---|
| **Wait before the first frame** | 6.8 s (20 layers at 4K); 9.6 s (500 drawings) | **Under 0.5 s** |
| **Skipped frames** | 25 of 361 (20 layers at 4K) | **0** |
| **Timing** | | Each frame shows on the display frame nearest its time, so 24 fps on 165 Hz doesn't drift; with variable refresh on, exactly on time |

## Memory

| What | Today | Budget |
|---|---|---|
| **Empty canvas** | 419 MB | **Under 150 MB** |
| **20 layers at 4K** | 1.06 GB; 1.75 GB at its peak while playing | **Under 600 MB; peak under 1 GB** |
| **500 drawings** | 1.23 GB | **Under 700 MB** |
| **A long session** (30 min of drawing, playing and scrubbing at 4K) | 1.06 → 1.41 GB, 2.0 GB at its peak (some of it the undo history of 224 strokes) | **No growth beyond what the undo history holds** |

## Heat

Power is measured above the machine without the app.

| What | Today | Budget |
|---|---|---|
| **Idle** | 0 frames drawn, 0.07% of a core, under 1 W | **The same** |
| **Drawing**, 20 layers at 4K | 33% of a core; about 10 W | **Under 15% of a core; under 5 W** |
| **Playing** | 63–78% of a core; 15–17 W | **Under 25% of a core; under 8 W** |
| **Heavy brushes** (the airbrush or wet paint at the pen's full rate) | Airbrush, Blend+paint, Water diffuser: like the pencil. **Oil brush: 410 ms pen to screen (1.2 s p99), 15% of frames missed** | **Within the drawing budgets above, every brush** |

## Start, open, export, size

| What | Today | Budget |
|---|---|---|
| **Startup** | 338 ms warm; 872 ms cold | **Under 200 ms warm; under 500 ms cold** |
| **Open** (either test project) | 0.88–0.95 s | **Under 400 ms** |
| **Export to MP4** | 20 layers at 4K: 421 ms a frame, 3.2 GB peak; 500 drawings at 1080p: 125 ms a frame, 2.7 GB peak | **Under 100 ms a 4K frame, 30 ms a 1080p frame; peak under 1 GB** |
| **App size** | 21.7 MB, plus WebKitGTK | **Under 40 MB, everything included** |
