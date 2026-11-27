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
5. The first canvas: the brush engine (`crates/brush`) paints tiles, and the GPU composites and
   presents them.
   - **Measured in the baseline:** pen to screen, frame rate at 60/75/100/165 Hz, memory and power.
   - **Phase 1's gate:** pen to screen within budget on the owner's Huion.
