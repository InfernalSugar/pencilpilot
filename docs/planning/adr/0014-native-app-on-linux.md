# ADR-0014: The native app on Linux: our own Wayland client, Vulkan, and presenting through dmabuf

- **Status:** accepted (2026-11-27), Phase 1 of the native rebuild.
- **Holds it to:** the budgets in [tools/baseline/BUDGETS.md](../../../tools/baseline/BUDGETS.md).

## What the owner asked for

- **A native Rust app:** no webview anywhere, written from scratch, including the windowing on
  each OS, the GPU backends, and fonts.
- **Order:** Linux first (Hyprland, with a Huion pen), then Windows, then macOS.
- **Refresh rate:** the app draws at the monitor's own rate and follows the window between
  monitors. The owner has 165, 100, 75 and 60 Hz screens.

## Decisions

1. **Wayland, spoken directly** (`crates/wl`). The wire protocol runs over the compositor's socket,
   with file descriptors passed by `sendmsg`/`recvmsg`. The typed requests and events are generated
   at build time from the protocol XML in `crates/wl/protocols/`, by our own reader. There is no
   libwayland. X11 is not a goal: XWayland covers X11-only desktops until a native X11 backend is
   worth writing.

2. **Vulkan through the system loader** (`libvulkan.so.1`, opened at run time). The bindings for
   the calls the app uses are our own. D3D12 and Metal follow, for Windows and macOS.

3. **We present frames ourselves.**
   - **Why not Vulkan's Wayland path:** it needs a libwayland `wl_display*`, which (1) rules out.
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

`crates/wl` against mutter (headless, 2560×1440 at 165 Hz):
- it listed 43 globals, including `zwp_tablet_manager_v2`, `wp_presentation`, `zwp_linux_dmabuf_v1`
  and `wp_linux_drm_syncobj_manager_v1`;
- it opened a window and showed a frame from shared memory, its descriptor passed over the socket;
- the compositor reported the frame presented with a refresh of 6,060,606 ns, which is 165.0 Hz.

Hyprland, the owner's desktop, supports the same protocols.

## Phase 1 steps

Each step is its own PR, with tests that need no window on the screen.

1. `crates/wl`: the Wayland client (this ADR's first PR).
2. `crates/vk`: the Vulkan bindings, a device on the GPU, images exported as dmabufs, explicit
   sync.
3. The window: xdg-shell, scale (fractional too), resizing, frame timing and presentation
   feedback, following the refresh rate.
4. Input: pointer, keyboard (a keymap reader of our own), and the pen (tablet v2).
5. The first canvas: the brush engine (`crates/brush`) paints tiles, and the GPU composites and
   presents them.
   - **Measured in the baseline:** pen to screen, frame rate at 60/75/100/165 Hz, memory and power.
   - **Phase 1's gate:** pen to screen within budget on the owner's Huion.
