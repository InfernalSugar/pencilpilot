# ADR-0012 — Where the app paints: in the window, with the native engine for whole canvases

**Status:** Accepted (2026-11-21). It replaces S1-T05's original wording ("the native engine
behind PaintCanvas, same API as WASM").

## Question
Should the app's live canvas run the paint engine natively (in the Rust process, over
Tauri's IPC) instead of as WebAssembly in the window, like the website?

## Measurements
These were taken on the owner's machine: Linux, 16 threads, WebKitGTK window. The same
workload ran in every case:
- **Painting:** pencil, airbrush at 3× and liner × 60 strokes × 120 samples at 1920×1080,
  flushed every 4 samples.
- **Recompositing:** 10 painted layers, whole canvas.

The tools are `cargo run --release -p pencilpilot-brush --example bench` (native) and the
app's `/dev/bench` page (WebAssembly). `PP_BENCH=1 pencilpilot-studio` runs the page in a
real window and prints the results.

| | Pencil / sample | Airbrush 3× / sample | All / sample | Recomposite 10 layers |
|---|---|---|---|---|
| WebAssembly in the app (WebKitGTK) | 0.10 ms | 4.0 ms | 1.40 ms | 186 ms |
| WebAssembly in Chromium | 0.09 ms | 4.0 ms | 1.41 ms | 196 ms |
| Native, one core | 0.073 ms | 2.95 ms | 1.03 ms | 149 ms |
| **Native, compositing on every core** | 0.060 ms | 1.18 ms | 0.43 ms | **23 ms** |

IPC round trip in the app: under 1 ms, both empty and carrying a 256×256 RGBA rect. The
window's timer has 1 ms resolution, so these are upper bounds.

## Decision
1. **Live painting stays in the window (WebAssembly).** Pencils, inks and markers take
   ~0.1 ms a sample, or ~0.4 ms a frame with a 240 Hz pen. Moving them across IPC would
   mean making the 3 400-line canvas asynchronous, for no visible gain.
2. **The native engine composites on every core** (rayon; the browser build stays
   single-threaded). That makes it 8× faster than the window at flattening whole canvases.
   Animation needs it: playback caches (every frame flattened), export, thumbnails and
   onion skins all run natively (S2, S3).
3. **Animation frames live natively** (S2's frame store). The window holds only the frame
   being drawn, so WebAssembly's 4 GB ceiling doesn't limit long cuts.
4. **Revisit live painting** if the pen test on a tablet shows lag with heavy brushes. The
   airbrush case takes ~16 ms a frame at 240 Hz in the window and ~5 ms natively. The
   options then are WebAssembly threads (Tauri can send the COOP / COEP headers they
   need) or a native canvas behind an asynchronous PaintCanvas.

## Consequences
- The engine's shared state is `Arc`, not `Rc`, so layers can be read from many threads.
- The app and the website keep one canvas implementation, so tools land in both at once.
