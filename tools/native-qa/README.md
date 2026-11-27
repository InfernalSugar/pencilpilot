# Native shell QA

Checks the native app's shell (Phase 2 step 3) against the design and the budgets in
[`tools/baseline/BUDGETS.md`](../baseline/BUDGETS.md). Nothing here opens a window on a real
screen: the app renders off-screen (`--render`), runs on a headless mutter, and the design
renders in headless Chromium.

```sh
DESIGN_DIR=/path/to/design/project tools/native-qa/run.sh            # everything
SKIP_HEADLESS=1 tools/native-qa/run.sh                               # no windowed runs
```

Results go to `target/native-qa/` (or the folder given): `native/`, `design/`, `compare/`
(side-by-side and difference images, `compare.md`), `frames/`, `tests.txt`, `sweeps.log`,
`headless/headless.json`. The combined shell's run is kept in [`evidence/`](evidence/).

## What's in it

| File | What it does |
|---|---|
| `run.sh` | The whole pass, in order: build, render both, compare, tests, headless. |
| `render-design.mjs` | Serves the design's Design Component pages (with `support.js` mapped to the design tool's runtime) and screenshots them at 1440 × 900 in headless Chromium. It also saves each page's laid-out regions, tabs and buttons as JSON. |
| `compare.py` | Design vs native, per workspace. It writes side-by-side and difference images. Each region's box, colour and corner radius is measured the same way on both pictures (a flood fill from inside the region's padding), and checked against the design's DOM boxes. It also lists the key colours the design uses that the native region lacks. |
| `../../apps/native/tests/shell_qa.rs` | Interaction tests through the library (`shell::build` with `Ui` and simulated input). |
| `headless.sh`, `inject.py` | The windowed app on a headless mutter (its own D-Bus session): idle frames, startup (warm, and cold with `DROP_CACHES`), PSS memory from `/proc/<pid>/smaps_rollup`, and the frame rate at 165 Hz and 60 Hz while the pointer is moved through mutter's RemoteDesktop API. |

## The interaction tests

They are `#[ignore]`d, so a plain `cargo test` (and CI, which has no GPU) skips them. Run them
with:

```sh
cargo test --release -p pencilpilot-native --test shell_qa -- --include-ignored --test-threads 1
```

They don't hard-code where each piece puts its widgets. Each test clicks across a region (a
sweep) from a clean state. It records what each click changed in the `State`, which `Action`s
it asked for, and whether a menu or pop-up opened. Then it puts everything back. What the
design shows is the expectation:
- the workspace tabs in the design's order and place;
- the tools per workspace, in the design's order;
- the colour swatch opens colours;
- three panel tabs (none in Review);
- File and View menus;
- Undo, Redo, Draw together and Export;
- each strip's main controls;
- right-click menus on the strips and the canvas.

`PP_QA_LOG=<file>` writes every click that did something; `PP_SAVE=<folder>` saves the frames
around a workspace click.

Two budget checks run through the library:
- **Idle:** with nothing happening, a pass draws nothing in every workspace, hovering
  included, and nothing is due.
- **A click shows on the next frame:** the pass a click lands in must be complete. The pass
  after it, with no input, must have nothing left to draw. Otherwise the window keeps the
  half-updated frame until the next input.

## Measuring notes

- **Region boxes** are measured from pixels on both pictures. The detector reproduces the
  design's own DOM boxes to the pixel, which checks it.
- **Tolerances:** ±2 px for positions and sizes, ±6 a channel for colours, ±4 px for corner
  radii (measured from the diagonal, so 24 reads as about 24 and 22 as about 20.5).
- **The frame rate** is the frames presented during 3 s of the pointer alternating between two
  tool buttons every 2 ms, divided by 3. The app's default 1280 × 800 window opens centred on
  the 1440 × 900 virtual monitor, at (80, 50). On a monitor of the window's own size, mutter
  reports no frame presented at all, so that setup can't be used.
- **Memory:** PSS covers the process's CPU-side memory. GPU allocations in VRAM don't show in
  it.

## The canvas (Phase 2 step 4)

| File | What it does |
|---|---|
| `../../apps/native/tests/canvas_qa.rs` | Through the library: the view's maths (round trips at zoom 2 %–6400 %, turned, flipped, panned; zoom and turn keep the point under the pointer), a stroke through a 300 %, 30°, flipped view landing under the pointer (the engine's pixels), the view's keys and that they don't fire while typing, and moving a 1200 × 800 painted selection (time a step, lift, commit, undo). `PP_QA_OUT=<folder>` keeps the JSON and PNGs. |
| `canvas_window.py` | The app in a window on its own headless mutter, driven through RemoteDesktop. It reads the app's `--stats` (each frame's oldest input, when it was handed over, when it was shown, the view, the state) and `--save-doc`. Scenarios: `view` (a real stroke at about 305 %, turned 30°, flipped, lands under the pointer; panning, zooming and turning paint nothing), `latency` (pen to screen and the rate while drawing at 165 and 60 Hz), `viewfps` (the rate while panning and zooming), `idle`, `memory` (PSS over 600 strokes, then undo), `playback`. |

```sh
cargo test --release -p pencilpilot-native --test canvas_qa -- --include-ignored --test-threads 1
python3 -I tools/native-qa/canvas_window.py target/release/pencilpilot target/native-qa/canvas-window [scenario …]
```

Measuring notes:
- **Pen to screen** is given three ways: from the compositor's event time (`inputMs`), from when
  the app received the event, and from when the frame was handed to the compositor. The
  shown time is mutter's presentation feedback for its virtual monitor, not a real display's
  scan-out.
- **The rate** counts only the intervals where input was already waiting when the earlier
  frame was shown, so pauses between strokes aren't counted as misses. A miss is an interval
  over 1.5 display frames. A stall is input that waited over 2 frames to be drawn.
- **RemoteDesktop** drops the input sent in about the first 0.2 s after a session starts,
  the first key included. The driver waits and sends a Shift tap first. Relative motion
  isn't accelerated: a 200 px middle-drag pans exactly 200 px.
