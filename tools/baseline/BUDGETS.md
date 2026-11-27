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
| **Moving a selection of 2,000 strokes** | Freezes the app | **At the display's rate while dragging; done within 100 ms of letting go** |
| **Clicks, menus, tool switches** | (not measured yet) | **The result shows on the next display frame** |
| **Scrubbing the timeline** | (not measured yet) | **At the display's rate** |
| **Panning and zooming a 4K canvas** | (not measured yet) | **At the display's rate** |

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
| **A long session** (not measured yet) | | **No growth over 30 minutes of drawing and playing** |

## Heat

Power is measured above the machine without the app.

| What | Today | Budget |
|---|---|---|
| **Idle** | 0 frames drawn, 0.07% of a core, under 1 W | **The same** |
| **Drawing**, 20 layers at 4K | 33% of a core; about 10 W | **Under 15% of a core; under 5 W** |
| **Playing** | 63–78% of a core; 15–17 W | **Under 25% of a core; under 8 W** |
| **Heavy brushes** (the airbrush or wet paint at the pen's full rate) | (not measured yet) | **Within the drawing budgets above** |

## Start, open, export, size

| What | Today | Budget |
|---|---|---|
| **Startup** | 338 ms warm; 872 ms cold | **Under 200 ms warm; under 500 ms cold** |
| **Open** (either test project) | 0.88–0.95 s | **Under 400 ms** |
| **Export** (not measured yet) | | **Set from today's numbers once the tool measures it** |
| **App size** | 21.7 MB, plus WebKitGTK | **Under 40 MB, everything included** |

## Still to add to the tool

These need measuring in today's app, with their numbers set from what that shows:
- export;
- heavy brushes;
- pan and zoom;
- scrubbing;
- moving a selection of many strokes;
- click to result;
- a 30-minute session.
