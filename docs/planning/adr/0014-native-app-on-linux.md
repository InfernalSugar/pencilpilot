# ADR-0014: The native app on Linux: standard Wayland and Vulkan bindings, presenting through dmabuf

- **Status:** accepted (2026-11-27), Phase 1 of the native rebuild.
- **Holds it to:** the budgets in [tools/baseline/BUDGETS.md](../../../tools/baseline/BUDGETS.md).

## What the owner asked for

- **A native Rust app:** no webview anywhere. The app's own layers (windowing on each OS,
  rendering, input) are written for it, on standard bindings to the OS and the GPU (see 1).
- **Order:** Linux first (Hyprland, with a Huion pen), then Windows, then macOS.
- **Refresh rate:** the app draws at the monitor's own rate and follows the window between
  monitors. The owner has 165, 100, 75 and 60 Hz screens.

## Decisions

1. **Standard bindings, our own everything above them.**
   - **What we use:** `wayland-client` and `wayland-protocols` (pure Rust, no libwayland) to talk to
     the compositor, and `ash` for Vulkan through the system's loader. The owner chose these on
     2026-11-27, over bindings of our own.
   - **Why:** they are thin layers with the same control, the same pen data and the same frame
     timing, maintained and tested by many apps.
   - **What stays ours:** the window code, the renderer, frame pacing and input handling.
   - **History:** our own Wayland client (`crates/wl`, #354) worked and was replaced by these.
   - **Not a goal:** X11. XWayland covers X11-only desktops.

2. **The GPU module** (`crates/gpu`) picks the best GPU (a discrete one first), makes one device
   with a graphics queue, and creates images the compositor can show as they are.

3. **We present frames ourselves.**
   - **Why not Vulkan's own Wayland path (a swapchain):** it decides its own buffering. Presenting
     ourselves keeps every frame's timing in our hands.
   - **How instead:** the app renders into images it exports as dmabufs (`linux-dmabuf-v1`) and
     attaches them to its surface. It syncs with the compositor through explicit sync
     (`wp_linux_drm_syncobj_v1`), falling back to implicit sync where that isn't offered.
   - **What this gives us:** full control over when each frame goes, which is what the
     pen-to-screen budget needs.

4. **Frame timing comes from the compositor.**
   - **Frame callbacks** say when to draw.
   - **`wp_presentation` feedback** says when each frame reached the screen, and the refresh rate of
     the output it reached. The rate follows the window from monitor to monitor.
   - **The `fifo` and `commit-timing` protocols** pace playback where the compositor offers them.
   - **The pen-to-screen measurement:** the pen event's timestamp to the presentation timestamp,
     both on the compositor's clock.

5. **Pen input through `zwp_tablet_manager_v2`.** It carries pressure, tilt, distance, rotation and
   buttons, per tool, straight from the compositor. This is the input path the webview loses
   pressure on today.

## Checked

Against mutter (headless, 2560×1440 at 165 Hz) on the owner's RX 6800 XT (RADV):
- the compositor offers `zwp_tablet_manager_v2`, `wp_presentation`, `zwp_linux_dmabuf_v1` and
  `wp_linux_drm_syncobj_manager_v1`;
- the GPU rendered a frame into a tiled dmabuf (one of the 8 modifiers the compositor takes for
  XRGB8888) and the app handed it over;
- the compositor reported the frame presented with a refresh of 6,060,606 ns, which is 165.0 Hz.

Hyprland, the owner's desktop, supports the same protocols.

## Phase 1 steps

Each step is its own PR, with tests that need no window on the screen.

1. ~~Our own Wayland client~~ (done, then replaced by the standard bindings).
2. `crates/gpu`: a device on the GPU, and images exported as dmabufs (done). Explicit sync comes
   with step 3.
3. `crates/window`: xdg-shell, scale (fractional too), resizing, frame pacing by the
   compositor's frame callbacks, presentation feedback, following the refresh rate (done).
   - **Checked** on headless mutter with two monitors (165 Hz and 60 Hz): 165.0 fps with no frame
     skipped on the 165 Hz one. Moved to the 60 Hz one, the compositor reported 60.0 Hz and the
     window drew at 60.0 fps. Maximising resized the frames.
   - **Frames are handed over** once the GPU has finished them (a fence wait). Explicit sync,
     which lets the compositor wait on the GPU instead, comes with the canvas, where its gain can
     be measured.
4. Input in `crates/window` (done): pointer (each compositor frame one moment), keyboard
   (libxkbcommon, loaded at run time: the person's layouts, modifiers and text), and the pen
   (tablet v2: pressure, tilt, rotation, distance, the eraser end, side buttons, per tool, with
   the compositor's timestamps).
   - **Tests:** recorded sequences cover a whole stroke, the eraser and buttons, leaving while
     touching, a tap within one moment, and pointer moments and scrolls. A US keymap types "a",
     "!" with Shift, and nothing for Ctrl+A.
   - **The real pen:** `cargo run --release -p pencilpilot-window --example pen` opens a window
     that darkens with pressure and prints what the pen reports.
5. The first canvas (`apps/native`, the `pencilpilot` binary; done, waiting on the owner's check):
   - **How it draws:** the brush engine paints from the pen (or mouse). The changed rectangle goes
     up to a canvas image kept on the GPU (`crates/gpu` `Canvas`), and each frame copies the canvas
     into the image the compositor shows.
   - **How it measures pen to screen:** from each reading's time to the compositor's report that
     the frame with it was shown, both on CLOCK_MONOTONIC. `--replay` feeds a recorded 240 Hz
     stroke, for measuring without a pen.
   - **Drawing late:** the frame is drawn just before the compositor's next refresh is due, the
     margin adjusting itself. Where it needs more than three quarters of a frame, drawing late
     doesn't pay and the app draws as soon as it's asked.
   - **Measured on headless mutter at 165 Hz** (2026-11-27), against today's app on the same
     display (28 ms median, 40 ms p99, 60 fps): 165 fps, 0 frames missed. Pen to screen 9.0 ms
     median, 12.0 ms p99. Painting 0.2 ms and the GPU 0.1 ms a frame.
   - **mutter adds a frame of its own:** a commit is shown one refresh after the frame callback,
     whenever it comes, so drawing late doesn't help there (60 Hz: 27 ms median, 31 ms p99).
     Hyprland's own timing can only be measured on the owner's screen: it can't run headless
     without a synthetic timer.
   - **The owner's check** (2026-11-27, Hyprland 0.57, a Huion pen on the 2560×1440 165 Hz
     monitor, 29 s of drawing):
     - 165 fps, steady. The owner: "works smoothly".
     - Pen to screen about 16 ms median, about 19 ms slowest. This counts from the tablet's own
       timestamp, so the trip from the tablet into Hyprland is in it.
     - Drawing late made no difference: Hyprland shows a commit about two refreshes after it
       arrives.
   - **Open:** the 10 ms budget at 165 Hz isn't met yet. To try, with the full canvas
     (Phase 3):
     - presenting without waiting for the refresh (`wp_tearing_control_v1`, which Hyprland honours
       for fullscreen windows);
     - explicit sync.

     The owner chose to go on to Phase 2 meanwhile.
   - **Measured in the baseline:** pen to screen, frame rate at 60/75/100/165 Hz, memory and power.
   - **Phase 1's gate:** pen to screen within budget on the owner's Huion.

## Phase 2: the interface (2026-11-27)

1. **The renderer** (`crates/gpu` `Renderer2d`): one WGSL shader, compiled to SPIR-V by naga at build
   time. It draws rounded rectangles, borders, soft shadows, glyphs, pictures, gradients and
   clipping. The drawing is copied in between the interface's layers.
2. **Text** (`crates/ui` `text`): cosmic-text, with Inter and Bricolage Grotesque built in.
   - The system's fonts (Arabic, emoji and the rest) load in the background.
   - The design's fonts are ready in under 1 ms.
3. **Widgets** (`crates/ui` `ui`): the design's tokens, buttons, toggles, sliders, tabs, icon
   buttons (the design's line icons via resvg), tooltips, text fields, scroll areas, menus and
   pop-ups.
   - **Overlays** take the pointer from what's under them.
   - **Idle:** a pass that would draw what's already on screen draws nothing.
4. **The shell** (`apps/native`): the header, the tool column, the canvas card, the side panel and
   each workspace's bottom strip (the Option 2 timeline with icon trays, the frame strip, the
   drawings to colour, the review player), as the design's FullApp artboards. Each piece has its
   own file and state.
   - The canvas card paints with the brush engine (pen or mouse). Undo and Redo work.
   - **Measured** on headless mutter at 165 Hz:
     - 24 ms from start to the first frame shown (budget under 200 ms);
     - 25.6 MB PSS (budget under 150 MB empty; today's app 419 MB);
     - 3 frames drawn in 6 s with no input, all at startup (the first frame, a settling pass,
       and the system fonts joining), and none after.
   - **Screenshots:** `pencilpilot --render <workspace> --out x.png` draws any workspace off-screen,
     so checks need no window.

## Phase 2 step 4: the canvas and its tools (2026-11-27)

- **The document** (1920×1080 for now) is drawn by the GPU through a view: zoom at the pointer, pan
  (the Hand, Space, the middle button), turn (Space+Shift, or Shift with the Hand), flip (H), fit (0),
  100 % (1). From 200 % its pixels show as squares.
- **Tools:** Fill (closing gaps), Select and Lasso (Shift adds, Alt takes away), Transform (move,
  scale, turn), Eyedropper, Shape, Text; the selection and canvas menus; cut, copy and paste; guides,
  snapping and symmetry; brush settings. A tap paints a dot.
- **Moving pixels** happens on the GPU: the lifted pixels are a second picture, so a drag step
  costs under a microsecond (it was 65–95 ms, the freeze in today's app). Putting them down takes
  55–68 ms (budget 100 ms).
- **Layers and frames:** layers follow the painter; an exposure sheet drives which drawing each
  layer shows; onion skin; playback at the cut's rate (0 skipped frames); thumbnails in the strips.
- **Drawing late:** each frame is drawn just before its refresh, with a margin measured from how
  long frames take to make (the window crate's `Pacer`).
- **Measured** on headless mutter (release; QA's suites and window scenarios):

  | What | Budget | Measured |
  |---|---|---|
  | Pen to screen, 165 Hz, p99 | 10.06 ms | 9.8–9.9 ms for a continuous stroke; up to 10.8 ms between strokes |
  | Pen to screen, 60 Hz, p99 | 20.67 ms | 20.5–20.6 ms continuous; 25–26 ms in runs with one late frame |
  | Frame rate while drawing, panning, moving pixels | the display's | 165 / 60 fps, 0–1 missed |
  | Frame rate while wheel-zooming | the display's | 162–163 fps at 165 Hz (about 1 % late wakes on a busy machine) |
  | Idle | 0 frames | 0 |
  | Scrubbing with onion skin, a frame change | under 6 ms | p95 1.2 ms |
  | Memory, PSS | under 150 MiB | 53 MiB at rest, 121 MiB after 600 strokes |
  | Startup | under 200 ms | 57 ms |

- **Closed after step 4** (headless mutter, release, two to three runs each):
  - **Wheel zoom:** 0 missed frames at 165 and 60 Hz (6–8 and 2 before). Frames whose only input
    moved the view are drawn at once: drawing late is for the pen's newest sample, and for zooming
    it only risked a missed refresh (at once: 0 missed; late: 6–8).
  - **Transform:** V shows the box in about 3 ms (35 ms before). The box comes from a quick scan
    of what would be lifted (`Painter::lift_bounds`, 1 ms, on every core); the pixels are lifted
    at the first press or flip, which the drag doesn't notice (165 / 60 fps, 0 missed). The lift
    itself is 7–8 ms (25 ms before): tile copies and the lifted pixels' conversion on every core,
    and one copy fewer. What the interface's actions change shows in the same frame (a second
    pass after them).
  - **Pen to screen:** within budget in every run (three, release, headless mutter), p99 from
    the compositor's event time:

    | | Strokes with pauses | One continuous stroke | Budget |
    |---|---|---|---|
    | 165 Hz | 8.5–9.3 ms (p50 from receipt 6.5–7.0) | 8.4–9.0 ms | 10.06 ms |
    | 60 Hz | 19.1–20.1 ms | 19.9–20.2 ms | 20.67 ms |

    What it took: a stroke's samples that come while its frame is being made go in that frame
    (input is read again just before drawing; they waited a whole refresh before); after a pause
    of over two refreshes the next frame is drawn at once (the compositor has gone idle, and a
    late frame then missed, which also widened the margin for every frame after); the pacer's
    slack starts at 1 ms and tightens every 50 frames on time.
  - **Tried and left out:** a real-time priority for the drawing thread (directly or through
    rtkit). Under load it made no consistent difference in three runs against three, so it isn't
    worth its risks.
- **Left for the owner's check on a real display:** pen to screen with the Huion on a real
  monitor (headless mutter's input goes through its remote-desktop interface, about 1.5 ms of
  the event-time figures above).

## Phase 2 step 5: files and export (2026-11-27)

- **Projects:** Open, Save, Save as, New, recent files, opening from the command line, an unsaved
  marker, autosave to a recovery copy (offered back at the next start), Quit and the window's close
  asking to save first. `.ppstudio` both ways with the current app; a save re-encodes only the
  tiles that changed and runs off the window thread. Drawings not showing are kept compressed (the
  file's own tile bytes when unchanged).
- **Export:** MP4, WebM (FFmpeg), GIF and PNG sequences from a dialog, rendered on their own
  threads and pool; the window copies at most a few hundred kilobytes a tick.
- **The system's own windows** (the portal's file dialog, the file manager) open only for a normal
  start of the app: never in tests or checks (they had reached the owner's screen once).
- **Measured** (release, headless mutter, private D-Bus):

  | What | Budget | Measured |
  |---|---|---|
  | Open, launch to shown (warm / cold) | under 400 ms | 4K 179 / 311 ms, 500 drawings 91 / 210 ms |
  | Memory with a project open | under 600 / 700 MiB | 360 / 148 MiB |
  | Export MP4, a frame | 30 ms at 1080p, 100 ms at 4K | 15–18 ms, 47–48 ms |
  | Export peak, whole process | under 1 GB | 1080p MP4 ≈ 0.48 GB, 4K MP4 ≈ 0.85 GB |
  | The window while exporting (165 Hz) | the display's rate | 160–163 fps |

- **Open:** scrubbing at 165 Hz (a frame change costs ~12 ms at 1080p, ~49 ms at 4K: layers are
  composited on the CPU — the next step moves that to the GPU); pen to screen while an export runs
  (p99 ~16 ms at 165 Hz); 4K WebM peaks ~1.5 GB (libvpx); 4K MP4 files are ~40 % larger at the same
  quality (the encoder settings that keep the export under 1 GB).
