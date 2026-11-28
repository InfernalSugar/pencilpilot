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

## Phase 4: animation (2026-11-28)

### GPU layers

- **What changed:** the layers are composited on the GPU (`crates/gpu` `Compositor`,
  `shaders/layers.wgsl`; fed by `engine/frames/gpu.rs`). The painter (the brush engine) still
  paints and keeps the drawings; the GPU keeps each drawing's tiles and makes the picture.
  - **Tiles kept on the GPU:** a drawing goes up once (64 × 64 tiles, premultiplied linear
    fix15, as the painter keeps them); after that only tiles the painter says changed, each
    checked against a hash. Going to another frame only points the shader at other tiles.
    The compositor takes at most 920 MiB of GPU memory, everything included (or a quarter of
    the GPU's memory); past that, the least recently shown drawings make room. Drawings not showing stay compressed on the CPU as before.
  - **Going up ahead:** the drawings of the frames around the current one (then the whole
    cut, while there's room) go up between frames and while nothing else happens, decoded on
    another thread; nothing goes up while input comes in.
  - **The bottom layers kept composited:** layers unchanged for 4 frames, from the bottom (a
    background, still layers under the animation), are kept composited (each pixel's colour
    and clipping alpha, exact), so a frame change composites only the layers above them.
  - **The painter catches up later:** while scrubbing or playing, the painter doesn't swap in
    the frame's drawings (or decode them); it does once the frame rests (80 ms) or before
    anything is done on the frame (a stroke, a tool, an edit). Its own picture (for the
    eyedropper, a fill of what's seen, the file's thumbnail) catches up a little at a time
    once the frame rests.
  - **The same picture as the CPU's:** the shader does the brush engine's arithmetic step by
    step (its colour table, every blend mode, opacity, hidden layers and groups, clipping,
    masks, the onion skin's integer multiply). A test renders both and compares them across
    every blend mode, opacities, clipping (a hidden base too), masks, groups, the view's
    hidden layers, the onion skin, frame changes, painting mid-stroke, undo and new drawings,
    the kept bottom layers: at most 1 level a channel (most cases 0).
  - **Still on the CPU:** adjustment layers, layer styles, painting a mask, more than 32
    layers (the picture goes back to the CPU path as it was), exports and PNGs.
- **Playback timing:** frames are made ahead (as long as the slowest recent one took, and half
  again) and handed over when due: as long before their time as frames take to reach the
  screen, measured (on headless mutter about 0.3 ms: it shows a commit at once, so the old
  lead of a refresh and a half showed frames up to 1.4 refreshes early). The frame advanced
  by the clock is now shown at its time whatever it took to make.
- **Measured** (release, headless mutter 1440 × 900, private D-Bus; `tools/native-qa/gpu_layers.py`,
  3 runs each, the range of the runs; before = `dev` at #364; the machine shared with other
  builds, load average 3–10):

  | What | Budget | Before | After |
  |---|---|---|---|
  | Scrubbing 4K / 20 layers, 165 Hz: a frame change's work, p50 / p95 / p99 | the display's rate | 49–55 / 95–98 / 95–98 ms | 1.9 / 4.1–4.6 / 5.7–6.4 ms |
  | — key to the frame shown, p50 / p95 / p99 | | 83–94 / 580–692 / 727–841 ms (backlog) | 2.5–2.6 / 4.1–4.5 / 4.6–4.9 ms |
  | — steps never shown; frame changes over 1.5 refreshes apart | 0 | 32–34 of 46; 7–8 | 0; 0 (one change every 6.1 ms) |
  | Scrubbing 4K / 20 layers, 60 Hz: key to shown, p50 / p99; never shown | the display's rate | 63–72 / 854–1274 ms; 21–25 | 2.8–3.2 / 5.5–6.7 ms; 0 |
  | Scrubbing 500 drawings (1080p), 165 Hz: work p50 / p95 / p99 | the display's rate | 11.7–12.3 / 13.7–27.4 / 14.6–30.3 ms | 1.3–1.4 / 4.0–4.4 / 4.5–4.8 ms |
  | — key to shown p99; never shown; late changes | | 1731–1808 ms; 54–73; 87–108 | 4.7–4.9 ms; 0; 0 |
  | Scrubbing 500 drawings, 60 Hz: key to shown p99; never shown | | 13–21 ms; 0 | 4.2–4.5 ms; 0 |
  | Scrubbing 0.8 s after launch (drawings still going up), 4K 165 Hz: key to shown p99; never shown | | — | 4.4–5.2 ms; 0 (500 drawings: 4.6–4.7 ms; 0) |
  | Playback 4K / 20 layers, 24 fps: skipped frames | 0 | 0 | 0 (165 and 60 Hz) |
  | — each frame against its time (fit), worst | half a refresh: 3.03 / 8.33 ms | 31–36 / 25–36 ms | 1.5–2.0 / 2.5 ms |
  | — against the cut's own clock, mean (worst) | | — | +0.03–0.05 (+2.0) / −0.11–−0.14 (−2.5) ms |
  | Playback 500 drawings: skipped; worst against time | 0; as above | 0; 2.3–3.1 / 1.9–4.3 ms | 0; 1.0–1.1 / 0.3–0.4 ms |
  | Wait before the first frame (Return to the first new frame shown) | 0.5 s | 52–64 ms | 50–53 ms |
  | CPU while playing (user + system, of one core) | 25 % | 4K 324–338 %, 500 drawings 95 % | 4K 4 %, 500 drawings 3 % |
  | Memory (PSS) while playing, peak | 4K 600 MB (peak 1 GB); 500 drawings 700 MB | 391–431 / 172–178 MiB | 314–331 / 150–161 MiB |
  | GPU memory: the compositor's own (machine's VRAM in use, rise) | — | — | 4K 685 MiB (728–817); 500 drawings 1076 MiB (1096–1186) |
  | Pen to screen p99, one continuous stroke, 165 / 60 Hz (2 runs, before / after alternating) | 10.06 / 20.67 ms | 8.29–8.34 / 18.83–18.89 ms | 8.24–8.29 / 18.88–18.93 ms |
  | Composite, every tile of 4K × 20 layers (GPU, `layers_speed` test) | — | — | ~2 ms; 10 tiles of a stroke 0.23 ms |

  Notes: "work" is the app's own `workNs` (from when the frame was due to start; at 60 Hz it
  counts the wait for drawing late, so its p95 reads 11–14 ms while the key reaches the screen
  in under 7). The GPU memory is mostly the tiles (4K: 14 854 tiles, 464 MiB; 500 drawings:
  26 938, the 1 GiB cap reached) and the kept bottom layers (16 bytes a pixel, 127 MiB at 4K).
  The fit's "worst" before at 4K is frames shown a refresh or more late (each took ~50 ms).
- **Budgets:** scrubbing at the display's rate, playback (0 skipped, on the nearest display
  frame, first frame under 0.5 s), CPU while playing, and memory pass for both test projects at
  165 and 60 Hz. Pen to screen is unchanged.
- **Follow-up after QA's gate on the combined branch** (QA's `tools/native-qa/phase4.py`, 3
  runs each, release, headless mutter):
  - **Pen to screen at 4K × 20 layers had regressed** (strokes on the top layer: 165 Hz p99 8.5
    → 13.0 ms with 6–8 misses; 60 Hz 25.6 → 37.3 ms). Cause: when a stroke ended, its drawing
    was marked changed (for its thumbnail) and the GPU's copy of it thrown away, so the whole
    drawing went up again and was composited again in the next frame, every stroke; the late
    frames widened the pacer's margin. A drawing a layer shows is kept up to date tile by tile,
    so now only one changed out of sight is sent up again. Thumbnails and the painter's own
    picture also wait until the pen has rested 0.4 s (each takes milliseconds of the window's
    thread at 4K; they had landed between strokes). Now: **165 Hz p99 8.35–8.45 ms, 0–1 missed;
    60 Hz 19.1–19.8 ms, 0 missed** (before the GPU layers: 8.5 / 19.3–26.3 ms).
  - **4K scrubbing's slowest frame change** (6.5 ms): a pause made the layers that change every
    other frame count as still, so the kept bottom layers were rebuilt on the next two frame
    changes. Layers' stillness is now counted in frames where something changed. The atlas
    grows between frames (ahead of what goes up), not in a frame. Now 4K 165 Hz work p99
    4.5–6.5 ms (median 4.8), key to shown p99 7.3–12.1 ms, never shown 0; 500 drawings work
    p99 4.6–4.7 ms.
  - **GPU memory:** the cap now covers all the compositor takes (tiles, picture, kept bottom
    layers, staging), and the atlas grows a quarter at a time (doubling left up to half
    unused). The whole app, from amdgpu's fdinfo (resident VRAM + GTT; QA's sampler adds both
    `drm-memory-*` and `drm-resident-*` lines, which double counts: its 1584 / 2273 MiB are
    792 / 1137 MiB): **4K × 20 layers 768 MiB (VRAM 702 + GTT 66), up to ~890 MiB while playing
    (the kept bottom layers, 127 MiB, made at the first frame change); 500 drawings 948 MiB
    (906 + 42)**. At 4K the compositor is 662 MiB: tiles 486 (14 854 held), kept bottom layers
    127, picture 32, staging 16; the rest of the app ~100 MiB (the canvas image 32 and its
    staging 32, the window's buffers, the interface's atlas and its staging).
  - Playback and memory unchanged: 0 skipped, all frames on time, 3–4 % of a core, PSS 333 /
    164 MiB.
- **Open:** VRAM has no budget yet (the compositor takes up to 920 MiB; a smaller GPU gets a
  quarter of its memory and evicts); a project whose one frame needs more tiles than that goes back to the
  CPU path for 3 s at a time; layer styles and adjustment layers are still composited on the CPU;
  on a real display (Hyprland, a vblank grid) playback's lead is measured the same way but not
  checked yet.

### Sound and PSD

- **Sound** (`crates/sound`, `apps/native/.../engine/sound`):
  - **Decoding:** Symphonia (pure Rust) reads WAV, AIFF, MP3, FLAC, Ogg Vorbis, AAC-LC (ADTS,
    MP4/M4A) and ALAC, once, off the window's thread; everything is kept at 48 kHz (other rates
    resampled once, a Kaiser-windowed sinc). What Symphonia can't read — Opus (`.opus`, Ogg or
    WebM) and HE-AAC — FFmpeg decodes when it's installed (it already is for video export).
  - **Output:** cpal (ALSA, which reaches PipeWire and PulseAudio through their ALSA plugins). The
    mixer runs in cpal's callback and never allocates, locks or frees there (a test counts it):
    commands come through a lock-free ring (rtrb) and go back the same way to be dropped. The
    output is opened (not playing) when a project with sound opens, plays from Play or a scrub,
    and pauses 1.5 s after the sound stops, so idle stays at 0 frames and 0 % CPU. The device's
    own buffer size is used (a fixed 480 underran every cycle under PipeWire's 1024 quantum).
  - **In step with the picture:** the mixer knows when each buffer will be heard (cpal's
    stream timestamps, mapped to the app's clock by their smallest offset seen: cpal's
    callback time is when the device last reported, up to a quantum ago) and plays the scene
    sample playback's clock says is due then, warping its time by at most 0.2 % to follow the
    clock (no drift, no pitch change); a jump over 3 ms (an underrun) is taken at once.
  - **Starting:** a play's clock starts when its first sound can be heard (the mixer says
    when; the frame showing holds that long, the output's latency), so a sound on the first
    frame is heard from its start, as in today's app. Frames reach the screen a
    little away from the clock's time (on mutter, 0.6–0.8 of a refresh early: playback's lead
    overestimates the show delay); the sound follows the frames as presented (the median of the
    play's latest 48 frames' presentation times, from its third frame; kept for the next play).
  - **Scrubbing** plays the frame's snippet (1/12 s or two frames, faded); the timeline shows a
    row per clip with its waveform (made once, off-thread), mutes, moves (dragged by frames) and
    removes clips; saving writes them back.
  - **Export:** a video gets the cut's (or loop range's) mix: clips by scene frame, cuts one
    after another, gain, mute, negative starts, sample-exact (tests), written to a WAV a second
    at a time on the export's thread; MP4 gets AAC, WebM Opus.
  - **No sound in tests and checks:** the system's output opens only for a normal start;
    `PENCILPILOT_NO_SOUND`, `PENCILPILOT_NO_SYSTEM_WINDOWS`, `--stats` and `--quit-after` play to
    a silent output (no device) that records what it played. Only ALSA's `null` device was
    opened to prove the device path.
- **PSD export** (`crates/export` `psd`): the export dialog's PSD writes the frame showing with
  its layers (names, opacity, blend, visibility, clipping, masks, groups, styles drawn in,
  adjustments kept hidden by name) and the flattened picture: the current app's writer, ported;
  for the same document its bytes are the same (a fixture written by `psd.ts`), and it reads back
  (a reader after `psd-read.ts`). Layers are asked for one at a time, on a thread of its own.
- **Proposed budgets** (new with this step): sound against the frame shown within ±1 display
  frame; no drift over 60 s; sound heard within 50 ms of Play.
- **Measured** (release, headless mutter, private D-Bus, the silent output at 20 ms latency;
  a 1500-frame cut at 24 fps under 65 s of music, `tools/native-qa/sound_window.py`):

  | What | Budget | Measured |
  |---|---|---|
  | Sound vs the frame shown, 60 s, 165 Hz | ±6.06 ms | p50 0.0, p95 0.17, worst 3.6 ms (1463 frames) |
  | Sound vs the frame shown, 60 s, 60 Hz | ±16.67 ms | p50 0.13, p95 0.40, worst 2.9 ms |
  | Drift over the minute | none | 0.02 / 0.27 ms a minute (165 / 60 Hz) |
  | The session's first play, its first two frames | as above | −8.2, −4.4 ms at 165 Hz (−11.2, −8.9 at 60); the rest as above |
  | Play to sound heard | 50 ms | 31.5–31.8 ms (20 ms of it the output's latency) |
  | Simulated sound card ±200 ppm, 1 ms callback jitter, 60 s (unit test) | — | under 1 ms throughout, under 0.5 ms at the end (the test's bounds) |
  | Idle, project with sound open | 0 frames, ~0 CPU | 0 frames, 0.0 % of a core |
  | Export with sound, 1080p (62.5 s): MP4 / WebM peak | 1 GB | 384 / 460 MiB; streams 62.5 s, AAC / Opus |
  | Export with sound, 4K MP4 peak | 1 GB | 652 MiB (WebM 1.19 GB: libvpx, as without sound) |
  | Exported sound vs the mix | — | lag 0 ms; correlation 1.000 (AAC), 0.984 (Opus) |

- **Measured through PipeWire** (QA's harness, `tools/native-qa/phase4.py sync` and
  `sound-start` on `native/p4-qa-work`: the app's ALSA `null` device is a private, device-less
  PipeWire null sink recorded on its own sample clock; clicks every 12 frames for 10 s, 3 runs
  each at 165 and 60 Hz, two projects; offsets less what the recorder adds):

  | What | Budget | Quantum 1024 (the owner's) | Quantum 256 |
  |---|---|---|---|
  | Click vs its frame shown, p50 | ±1 display frame | −0.48 to −0.03 ms | −0.44 to −0.05 ms |
  | Click vs its frame shown, worst after the first | ±6.06 / 16.67 ms | −0.93 to +0.01 ms | within ±4.7 ms (all clicks) |
  | The first click (on the frame showing at Play) | — | +11 to +20 ms after that frame first showed: it holds until the sound can be heard | +0 to +4.7 ms |
  | Play to sound heard | 50 ms | 36–48 ms | 7–19 ms |
  | A tone from frame 0: its start not heard | 0 | 0–1 ms (5 runs), once 21 ms heard early | 0–5 ms |

  Before the fix (QA's gate): clicks −0.1 to +19.4 ms late at quantum 1024 (a fixed 480-sample
  buffer underran every cycle or two, and the mixer took 20 ms jumps as drift to warp
  through), and the first 19–26 ms of a play weren't heard. One of the twelve 1024 runs at
  165 Hz isn't matched by the harness: its picture skipped frames 11–12 under load (load
  average 5), which leaves no anchor frame.

- **Fixed on the way:** a video scaled while written lost its sound (the sound's `-i` came after
  `-vf`, which FFmpeg refuses); `-shortest` held up to 10 s of frames back (1 GB at 1080p): the
  mix is made as long as the frames instead.
- **Open:** the first play of a session at 165 Hz on the silent output (its first frame 2 ms past
  the budget, until the frames' timing is known); the real device's latency on the owner's PipeWire (only ALSA
  `null` was opened here); adding sounds and lip-sync (the current app's) aren't ported yet;
  undo for clip moves.

## Phase 6: drawing together (2026-11-28)

The native app takes part in the current app's live sessions (ADR-0013): it hosts, joins with
a `pp1-` code, and draws with others, the current app included, in the same session. **The rule
for a mixed session:** the native app never destroys or alters what it doesn't support.

- **The session's state is `crates/together`'s `Together`** (the current app's `together.ts`,
  ported, edit for edit): layers, groups, who may draw on each, asks, the whole timeline (every
  scene and cut, notes, markers, sound clips, mouth charts, stages, palettes), masks, line
  strokes, sound files, the joiner's snapshot, coming back after a break. It runs on the
  session's own thread (`engine/together/net.rs`, a small tokio runtime around
  `crates/share`'s `Session`), never on the window's.
- **The canvas is asked, not shared** (`engine/together/port.rs`, `serve.rs`): `Together`'s
  `CanvasPort` calls go to the window's thread, which answers them between frames (3 ms a
  tick at most) and never waits on the session's. The heavy work stays on the session's
  thread: a drawing's tiles encoded from the painter's copy, tiles put in decoded and
  compared there (a hash each: only those that changed reach the painter). A pipe the window's
  wait watches wakes it (`crates/window`'s `Waker`): no polling.
- **What the native app doesn't show stays as it came:**
  - the timeline is the session's, whole; only the tracks of the cut the native app shows (the
    sheet) are read back from it, and a static layer's length stays the session's;
  - a layer's look comes back as the session set it, but for what changes here (eye,
    opacity, blend where the native app has it): a blend mode or style it hasn't isn't
    rewritten;
  - line and adjustment layers made in the current app show their pixels (line layers) and stay
    locked here; masks show; groups are kept (the painter's groups; shown in the layers panel,
    changed in the current app);
  - others' tiles are kept exactly as they came (straight RGBA, compressed) and used for saving
    and passing on; a drawing not in the painter is kept compressed as it came.
- **Saving a shared project:** the session's whole timeline and every drawing in it (other cuts'
  too), its groups and the sound files it sent; tiles past the canvas are left out and tiles at
  its edge are written as they are, as the current app's writer does; live-preview layers
  aren't saved.
- **Hosting a project from its file** shares its whole timeline: drawings of cuts the native
  app doesn't show go as the file has them (no decoding).
- **Joining** puts the project open aside (hidden, locked), as the current app does; a native
  host's snapshot adds the project's size and frame (fields the current app ignores), and a
  joiner whose project is another size opens one of that size.
- **Live strokes:** this app's stroke goes as it's drawn, a frame's worth of points (24 a
  datagram) at most every 16 ms; its start is said again soon after (40, 100, 200 ms) and then
  every 0.5 s, so a start lost on a quiet link still shows. Others' are drawn with their brush on
  a layer of their own until their tiles come (or 2 s after the stroke's end, a lost end).
- **Losing touch:** a session lost (closed under it) or split (this app took over alone while
  others were there) is told, and the native app goes back once to whoever of it is online, or
  hosts it again. Leaving keeps the session to rejoin within the app's run.
- **Identity:** the current app's key file (`$XDG_DATA_HOME/app.pencilpilot.studio/share/key`).
  **Checks never use a relay**; `--host` / `--join <code>` / `--name` start a session without the
  interface, for checks only; `PENCILPILOT_WIRE_LOG` writes what this app sends (no pixels).
- **Measured** (release, native ↔ native on loopback, each on a headless mutter and D-Bus of
  its own; 3 runs, load average 2.4–3.6 except the 8-member runs, whose 16 processes load the
  machine themselves to 4–11):

  | What | Budget | 165 Hz | 60 Hz |
  |---|---|---|---|
  | Pen to the other's screen (live), p99 | 50 ms | 6.2–6.6 ms | 13.3–13.5 ms (one run 29.7) |
  | Stroke's end to its tiles shown there, worst | 100 ms | 15–21 ms | 30 ms |
  | Pen to screen while the other draws, p99 | 10.06 / 20.67 ms | 8.3–9.5 ms | 18.5–18.6 ms |
  | Missed frames while both draw | 0 | 0–3 | 0–1 |
  | Joining 4K × 20 layers: connected → drawable | 3 s | 1.03–1.11 s (a current-app host: 0.67 s) | |
  | — memory, peak PSS (joiner / host) | 600 MiB | 439–467 / 447–456 MiB (joining a current-app host: 383–536) | |
  | Another member's 4K picture coming in while drawing: PSS; pen p99 | 600 MiB | 588–619 MiB; 8.1–8.4 ms (4 missed) | 33.5 ms (2 missed) |
  | 8 members drawing at once | none lost | every edit; all 8 pictures the same | |
  | — pen to screen meanwhile, p99 | 10.06 ms | 8.5–12.9 ms (4–7 missed); the same app with no one drawing 8.4–12.8 | |

  With the current app (QA's harness, run here): first strokes, timeline merges, the whole
  timeline both ways (but the current app's copy of a file a native host shares, below),
  offline 5 s and 40 s, big erases pass.
- **Since:** the crate's cache is compressed and tiles come in as they came over the wire
  (`put_deflated`: kept compressed, shared with the crate's copy, decoded on the session's own
  few threads, at a lower priority than the window's); the window answers the session's
  questions back to back while it joins (waiting up to a tick's room for the next, not a loop
  of the window each); a stroke's start goes out at pen-down straight from the window's thread
  (not behind the session thread's work); the window no longer spins when work is due within
  the frames' lead while not playing (it was waking at once again and again, idle);
  taking over as host in a hand-over waits 5 s for the others to come over before calling the
  session lost.
- **Open:** pen to screen at 60 Hz misses a refresh at some stroke starts (the frame is drawn
  for the refresh after next: the frame callback came late; no work on the window's thread
  over 0.5 ms around them), which is what puts the 60 Hz import's p99 at 33 ms. The current app joining a native host that opened a file with several
  scenes saves no pixels for that file's layers, though they're on the wire (native joiners
  get them). The pen while 8 draw at 165 Hz.
- **Phase 7 parity (not yet):** rejoining a session from the project file (the current app keeps
  it in `extra.share`); drawing on line layers and editing groups natively; layer styles drawn;
  adjustment layers shown; the invite as a QR code.
