# ADR-0010 — The PencilPilot app: a desktop and tablet app for drawing and frame-by-frame animation

**Status:** Accepted (2026-11-21). The app is called **PencilPilot**, like the website; the docs say "the app" where they need to tell them apart. Desktop first; tablets once the pen test (S0-T05) shows pressure and latency are good enough.

## Context
The website's studio (M10, M11) now has a full raster painting toolset: the hokusai /
libmypaint brush engine, layers, masks, adjustment layers, selections, transform / warp /
liquify, a pen tool, brush textures, timelapse. The owner wants their own application
with the same look and feel, for drawing and for making frame-by-frame anime, with AI
help that the website deliberately doesn't have (ADR-0002, ADR-0009).

## Decision
1. **A separate app, "PencilPilot"**, in this monorepo as `apps/studio` (package `@pencilpilot/studio`).
   The website stays as it is: practice and grading, no generative AI.
2. **Tauri 2** shell (Windows, macOS, Linux, then iPadOS and Android tablets).
   - The paint engine is Rust, so it runs **natively** in the app (multi-threaded, no WASM
     memory ceiling, which matters for hundreds of frames) and stays WASM on the web.
   - The UI stays **Svelte 5**. The design system and the studio components move into shared
     packages (`packages/ui`, `packages/paint`) used by both apps, so the app has the *same* UI.
   - Rejected: Electron (bundles a browser, and the engine would stay WASM). Native Swift/Kotlin
     UIs (two rewrites of the UI). Web-only PWA (no real file access, weak pen APIs, the 4 GB
     WASM limit).
3. **Animation core in Rust** (`crates/anim`): a frame store that keeps only the tiles each
   drawing actually touches, shares identical tiles between frames, and spills cold frames
   to disk compressed. Plus timeline / exposure-sheet data, onion-skin compositing and
   export.
4. **One project file**, `.ppstudio`: a zip holding a JSON manifest, deduplicated tile
   blobs, stroke logs, references, style packs and AI provenance. It is versioned and
   documented, so it can be opened without the app.
5. **The website and the app share the engine and UI code**, but not data or accounts in v1.

## Consequences
- The first milestone is extraction work (shared packages, a native engine build) before any
  new features.
- Tablet support depends on Tauri 2 mobile and native pen input. Pressure, tilt and palm
  rejection get a spike in A0.
- Release work: code signing, installers and auto-update (A11).
