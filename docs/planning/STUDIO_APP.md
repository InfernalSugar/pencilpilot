# The PencilPilot app: plan and milestones

A drawing and frame-by-frame animation app with the website's look and feel, plus AI that
helps you make frames in **your own** style. Decisions: [ADR-0010](adr/0010-studio-app.md)
(app, platform, file format) and [ADR-0011](adr/0011-ai-assist.md) (rules for the AI).

Each `##` heading below is a GitHub milestone (`S0`–`S11`). Each bullet is an issue titled
`Sx-Tyy — …`. Labels: `app` (shell, files, platform), `anim` (animation core), `ai`,
`brush` (engine), `design`, `infra`, `legal`.

---

## The idea in one page

**What you do in it:** draw (everything the website's studio does, natively and faster), and
animate: scenes, cuts, a timeline / exposure sheet, onion skin, light table, playback with
sound, and export to MP4, GIF or a PNG sequence.

**What the AI does for you.** None of it is required; the app works fully without AI.

| You ask… | It… | You then… |
|---|---|---|
| "Next key pose: she turns and points" (a scribble or a few words) | makes a **keyframe guide** from your previous frames and your model sheet, in your style | trace it, or let **Coach mode** walk you through it stroke by stroke |
| "Help me fill this frame slowly" | turns the guide into **ordered strokes**: gesture → big shapes → contours → details → clean lines | draw each stroke over a ghost; it checks each one, and **"ink this stroke"** draws any stroke with *your* brush when you'd rather it did |
| "Adjust this frame" | lets you move, warp, **re-pose** (skeleton handles) or **re-generate a region** of the guide | trace the changed guide |
| "In-betweens for 12 → 18, ease out" | makes **in-between guides** at the spacing you chart | trace or clean each one |
| "Colour this shot" | fills flats from your character's palette across every frame, plus optional shading guides | fix and add shadows |
| "Make me references" | **turnarounds, expression sheets, pose and background references** in your style | draw from them, or put them in your style pack |
| "Is this working?" | the **Director** (a vision LLM) reviews arcs, spacing, silhouettes and volume, and proposes keys and timing from a script beat | decide |

**Why your style stays exact:**
1. Guides are conditioned on your own frames and model sheet. Optionally a small **style
   LoRA** is trained on your work.
2. The final lines are **your strokes with your brush**. Even "ink this stroke" replays a
   vector path through your brush engine, so line weight, texture and pressure behaviour are
   literally yours.
3. A **style check** compares each finished frame with your style pack (line weight
   distribution, palette, proportions against the model sheet, using the website's grader
   measurements) and flags drift.

---

## Architecture

```
apps/studio            Tauri 2 shell + SvelteKit (static SPA): the app
apps/web               the website (unchanged in purpose)
packages/ui            design tokens + Fun components (moved from apps/web)
packages/paint         PaintCanvas, panels, tool UIs (moved from apps/web)
crates/brush           the engine (pure Rust, native); crates/brush-wasm = its browser bindings
crates/anim            frame store, timeline, onion skin, playback cache, export
crates/studio-ai       provider interfaces, job queue, cost meter, local ONNX models
crates/grader          reused: stroke checks in Coach mode, style / on-model checks
```

- **Frame store:** sparse 64×64 tiles per drawing. Identical tiles are shared through content
  hashes, so held frames and copies cost almost nothing. Frames near the playhead stay in
  memory; the rest are compressed to disk. Line art touches about 5–20% of tiles, so a 1080p
  drawing is ≈0.4–1.6 MB instead of 8 MB.
- **Playback:** flattened frames are cached as GPU textures at view size. The rate is fixed
  (24 fps, timed on 1s / 2s / 3s) and sound is synced.
- **Export:** native ffmpeg in the app (MP4/H.264, ProRes 4444 with alpha, WebM), plus GIF,
  APNG and PNG sequences. On the web build: WebCodecs.
- **AI jobs** run in Rust with the provider's key from the OS keychain. Each job is
  (capability, inputs, provider, estimated cost); results become Guide layers with
  provenance.

### AI capabilities and candidate back ends (checked November 2026; prices move, so re-check)

| Capability | First choice | Alternatives | Notes |
|---|---|---|---|
| Keyframe guide from references | **Seedream** (ByteDance, BytePlus), the owner's choice; multi-reference, ≈$0.045/image | Gemini 3 Pro Image (14 refs, ≈$0.13), Gemini 3.1 Flash Image, gpt-image-2, FLUX.2 (8 refs) | Strong character identity. Line weight is not pixel-exact, which is why we trace or auto-ink |
| Style LoRA on your frames | FLUX.2 klein 4B (Apache-2.0) via fal/Replicate trainers, a few $ per run | Qwen-Image-Edit LoRA; SDXL anime + LoRA locally | The best route to your exact line feel. Licence per model (ADR-0011 §7) |
| Region edit / re-pose | the same edit models with a mask; pose from a DWPose-style 2D skeleton | ControlNet openpose (local) | |
| Line extraction | AniLines / MangaLineExtraction (MIT), ONNX, local | Anime2Sketch | Runs on CPU; no provider needed |
| Strokes and their order | centreline vectorising (skeleton → Bézier) + an ordering heuristic | "Draw This First" (NeurIPS 2026) once code is out | No hosted API exists yet; ours is local |
| In-betweens | **Seedance 2.5** first/last-frame video (ByteDance, BytePlus), the owner's choice, sampled at your spacing; line-art in-betweening (thin-plate-spline, local) for simple moves | Wan 2.x (≈$0.10/s), Kling O1, Veo 3.1 Lite; ToonComposer (needs a 57 GB GPU) | Video models blur lines: sample → extract lines → guide |
| Colour | flat fill from the palette (our fill engine), tracking regions across frames | MangaNinja / AniDoc / ColorFlow (self-hosted), image-edit models as a guide | Flats are deterministic and on-model |
| Upscale line art | APISR (ONNX, local) | — | |
| Lip-sync | Rhubarb Lip Sync (MIT, WASM / native) → your mouth chart | — | Local |
| Director / critique / storyboard / tool orchestration | Claude Opus 5.5 (`claude-opus-5-5`, vision, 1M context, $4 / $20 per M tokens) | Claude Sonnet 5.5 for quick passes; Gemini / GPT as fallbacks | Contact sheets with frame numbers; style pack and project bible as a cached prefix |

**Rough costs** (bring your own key): a 4-second cut animated on 2s is 48 drawings. With 6
AI keyframe guides (≈$0.05–0.13 each), one first/last-frame in-between pass (≈$0.40–1.20)
and two director reviews (≈$0.05–0.20), that's **≈$1–3 per cut**. Line extraction, strokes,
colour flats and lip-sync are free (local).

---

## S0 — Foundations (≈2 weeks)
- **S0-T01** ADR-0010 / ADR-0011 accepted; the name, platforms order and BYOK-vs-credits decided `infra` `legal`
  ✅ Both accepted: the name is PencilPilot, desktop first, ByteDance (Seedance 2.5 + Seedream) as the first AI provider. Bring-your-own-key vs credits stays open until S11-T03.
- **S0-T02** Split the engine: `crates/brush` (pure Rust) + `crates/brush-wasm` (bindings); native build and tests `brush` `infra`
  ✅ `crates/brush` (`pencilpilot-brush`, lib `brush`) holds the engine and its tests; `crates/brush-wasm` is only the wasm-bindgen bindings. The web package is unchanged.
- **S0-T03** Move the design system and studio components into `packages/ui` and `packages/paint`; the website keeps passing every test `design` `infra`
  ✅ `@pencilpilot/ui` (theme, Button, Logo, Tile) and `@pencilpilot/paint` (the whole studio, plus the tracing and grayscale helpers it used from the site). The site imports them; root lint / check / test cover both packages.
- **S0-T04** Tauri 2 spike: the studio running natively on Windows, macOS and Linux, plus an iPad and Android build `app` `infra`
  🟡 `apps/studio`: Tauri 2 + SvelteKit (single-page) with the shared studio; runs and packages on Linux (`.deb`, 4.2 MB). Windows / macOS builds come with CI (S0-T06); iPad / Android builds need Xcode and the Android SDK.
- **S0-T05** Native pen input spike: pressure, tilt and barrel button, latency budget (<16 ms), palm rejection on tablets `app`
  🟡 The app's **Pen test** page measures pressure levels, tilt, twist, buttons, sample rate (with coalesced events) and event delay; results per system to be recorded here.
- **S0-T06** CI for the app: build matrix, Rust + UI tests, nightly artifacts `infra`
  ✅ `.github/workflows/app.yml`: the shell's fmt / clippy / tests, then installers on Linux (deb, rpm, AppImage), Windows (msi, exe) and macOS (dmg) for every app change, nightly and on demand; the app's UI is linted and type-checked by the main CI.

## S1 — The app shell, same UI (≈3 weeks)
- **S1-T01** Studio layout in the app: the same top bar, tool rail, options bar, dock and themes as the website `app` `design`
  ✅ The app's studio is the website's: every tool's options (fill, gradient, transform / distort / warp, liquify, selections, smudge, pen path, measure, lasso fill), Guides (grid, perspective, ruler, symmetry), QuickShape, Adjust (colour, curves, levels, gradient map, adjustment layers), the quick menu, paste / import, Timelapse. The tool options and Adjust live once in `@pencilpilot/paint` (`ToolOptionsBar`, `tool-options.svelte.ts`) for both. Tooltips show the keys chosen in Settings; more actions can have keys (gradient, adjust, copy / cut, selection, view). One theme, as on the website.
- **S1-T02** Home: a project browser with thumbnails, recent files, templates (canvas sizes, frame rates) `app` `design`
  ✅ Home is the first screen: New (HD, 4K, square, vertical, storyboard, A4 illustration, custom size and fps), Recent (thumbnails, size, layers, when; missing files marked, remove from list), Open…, and unsaved work to recover. Saves and opens add to Recent. The studio opens from here and its back button asks before losing changes.
- **S1-T03** `.ppstudio` format v1 (zip: manifest, tiles, strokes, references, provenance) + a format spec doc `app` `anim`
  ✅ `crates/ppstudio` reads and writes it (spec: `crates/ppstudio/FORMAT.md`): 64×64 tiles, empty ones skipped, repeats stored once (a 1080p line drawing is under 64 KB); timeline-shaped for S2. The app saves (Ctrl+S / Ctrl+Shift+S) and opens (Ctrl+O) with native dialogs. References and provenance join with S4/S6.
- **S1-T04** Autosave, crash recovery, version history (snapshots), native open / save / export dialogs `app`
  ✅ Unsaved work is copied to the app's data folder every 20 s and offered back at the next start (Recover / Discard / Decide later); each save keeps the replaced file as a version (last 20, Versions menu); closing or opening over unsaved work asks first; Export → PNG or layered PSD.
- **S1-T05** The native engine behind PaintCanvas (same API as WASM), with multi-threaded compositing `brush` `app`
  ✅ Re-scoped by measurement (ADR-0012): live painting stays in the window (0.1 ms a sample for pencils and inks); the native engine composites on every core (whole-canvas recomposite 149 → 23 ms, 8× the window) for playback, export and thumbnails in S2/S3. Benchmarks: `cargo run --release -p pencilpilot-brush --example bench`, the app's `/dev/bench`, `PP_BENCH=1`.
- **S1-T06** Settings: keyboard shortcuts editor, pen curve, tablet gestures, storage location `app`
  ✅ A Settings screen (from home and the studio): every shortcut can be changed (a taken key swaps), pen pressure on / off and the pressure curve, what one finger does and the tap gestures, the projects folder (dialogs start there), how often unsaved work is copied, and how many versions are kept. Kept in the app's data folder.

## S2 — Animation core (≈4–5 weeks)
- **S2-T01** `crates/anim` frame store: sparse tiles, hash dedup, memory / disk tiers, tests at 1080p and 4K `anim`
  ✅ `crates/anim`: every drawing as 64×64 tiles, each kept once (held frames and copies cost nothing), in memory up to a budget (1 GB in the app) and on disk past it, oldest first; tested on 1080p and 4K cuts (40 4K frames under a 4 MB budget come back exactly). The app keeps it in its Rust side: the window's engine holds the drawings showing plus a few recent ones and fetches the rest when their frames show; undo brings back the drawing it changes first. Saving makes the file from the store (tiles go in by hash, no pixels cross from the window); opening fills it.
- **S2-T02** Timeline: scenes → cuts → layers × frames; exposures (1s / 2s / 3s), hold, insert, delete, drag `anim` `app`
  ✅ A timeline under the canvas: scenes and cuts, a row per layer with its exposures. New drawing, hold on 1s / 2s / 3s or ±1, a whole layer on ones / twos / threes, insert / delete frames, remove a drawing; drag a drawing to move it or its edge to hold it longer; `,` / `.` step frames, `N` new drawing. Drawing on an empty cel starts a drawing there. The engine keeps every drawing and swaps them when the frame changes (nothing copied); undo steps remember their drawing, and undoing one from another frame goes to that frame first. Saved in `.ppstudio` (`timeline.scenes`, FORMAT.md). (Resizing the canvas of an animation is still to come.)
- **S2-T03** Exposure-sheet view (X-sheet) alongside the timeline, with dialogue and action columns `anim` `design`
  ✅ An "X‑sheet" dock tab beside the timeline: the cut read down the page, a row per frame (a heavy line every second, a lighter one every half), with Action and Dialogue columns and a column per layer (top layer first). Drawing numbers are written where a drawing starts, a line runs down while it's held, × is an empty cel, S a static layer. You write on it like a paper sheet: pick a cell, type a drawing number (Enter / ↓ goes down; the next number makes a new drawing), X or Delete for an empty cel, arrow keys to move; type straight into Action and Dialogue. Notes move with inserted and deleted frames and are kept in the file (a cut's `action` / `dialogue`). Fixed on the way: the canvas wrote an old drawing back onto the timeline if a layer was picked before a frame change had shown.
- **S2-T04** Onion skin (red before, green after, how many, opacity, tint) and a light table to pin any frames `anim`
  ✅ Onion skin (O): the drawings before in red and after in green, counted in drawings (a held drawing is one step), 0–5 each way, fading with distance; opacity, both colours, and this layer or all layers. Light table: pin any frames (Shift+O, "Pin frame") to show them in blue while drawing others. The engine tints and blends them into one picture over the canvas (multiplied, so it never hides your lines); hidden while playing.
- **S2-T05** Playback: fixed fps, loop a range, ping-pong, scrub, frame-step keys, playback cache `anim`
  ✅ Play / stop (Shift+Space, Esc stops), 6–60 fps, once / loop / ping-pong, a loop range (In / Out, Alt+I / Alt+O), scrubbing along the frame numbers, `,` / `.` frame steps. Frames are rendered once each, small (at most 960 px wide), by the engine without touching the canvas, and kept by what's on them, so playing again is instant and only changed frames render again (24 frames at 1080p get ready in about 0.4 s in the window).
- **S2-T06** Cel layers (one drawing per exposure) vs static layers (backgrounds, overlays), and per-cut camera frame (safe areas) `anim`
  ✅ Each timeline row has a C / S switch: a static layer (a background, an overlay) shows its first drawing in the cut on every frame, draws as one bar across the cut, doesn't make the cut longer, and is left alone by insert / delete frame, timing and the onion skin. Its other drawings are kept, so making it a cel layer again animates it as before. Each cut has a camera (whole canvas, 16:9, 4:3, 1:1, 2.39:1, 9:16, exact numbers, zoom in / out; a new cut starts with the last one's): outside it is dimmed on the canvas, safe areas (action 90%, title 80%) can be shown, and playback shows only what's inside. Both are kept in the file (`staticLayers`, a cut's `camera`; FORMAT.md).
- **S2-T07** Frame tools: copy / paste / duplicate / reverse frames, cycles, "boil" (redraw-looking jitter on held frames) `anim`
  ✅ "Frames ▾" in the timeline, on the frames chosen with Shift+click (or the drawing on this frame), for this layer or all layers: copy / paste (Alt+C / Alt+V: the same drawings, as on an exposure sheet), duplicate (Shift+D: new drawings to change), reverse, cycle ×N (a loop or back and forth, holds kept) and boil (2–4 drawings, on ones / twos / threes, subtle to strong). A boil is the engine redrawing a copy of the drawing through a smooth ~1 px wobble (scaled to the canvas), so a held drawing breathes without moving; the defaults (3 drawings on twos, light) follow the house style of 12 changes a second and restrained motion. Ranges can start or end mid-hold. A drawing exposed again keeps its number in the timeline.

## S3 — Sound and export (≈2–3 weeks)
- **S3-T01** Audio track: import, waveform, scrub audio, sync markers `anim`
  ✅ "+ Sound" in the timeline adds sound files (WAV, MP3, OGG, M4A, FLAC: what the webview decodes) starting on the current frame. Clips belong to the scene (its cuts play one after another, so music runs under several cuts): each shows as a row with its waveform for the frames of this cut, slides along by dragging, mutes, and comes out. Playback plays them in step (a ping-pong's way back is silent); stepping or scrubbing plays a short snippet at the frame, to find a word or a beat by ear. Markers (Shift+M, or the Marker button; tapping Shift+M while playing adds one on the beat) show on the frame numbers and in a list with labels. Video exports carry the mixed sound of the same frames (AAC in MP4, PCM in ProRes, Opus in WebM). Sound files are kept in the project as imported (`audio/<hash>.<ext>`), with the clips and markers in each scene (FORMAT.md).
- **S3-T02** Lip-sync: Rhubarb → mouth shapes on the X-sheet, linked to your drawn mouth chart `anim`
  ✅ A mouth button on each sound row opens Lip-sync: pick the mouth layer and say which of its drawings is each mouth shape (A–F, and G, H, X if drawn; "Fill in order" matches drawings A, B, C… as drawn, with thumbnails of each). Rhubarb Lip Sync (MIT) listens to that clip's frames in the cut — English (it hears words, helped by the line typed in, which starts from the X-sheet's dialogue) or any other language (it hears sounds) — and the mouths go onto the layer, on twos by default (ones and threes too, and 0–2 frames of lead), never losing a closed mouth (M B P) to the twos; a shape with no drawing gets the nearest one. The cut can grow to hold the whole line. The chart is kept in the file (`mouthCharts`), with drawings no frame shows. **Rhubarb is bundled** like ffmpeg: `scripts/fetch-rhubarb.mjs` fetches 1.14.0 per platform (SHA-256 pinned) into the app's resources with its speech models (~90 MB); CI runs it on a made-up recording. Its macOS build is Intel-only, so Apple silicon Macs need Rosetta (the app says so).
- **S3-T03** Export: MP4, ProRes 4444 (alpha), WebM, GIF, transparent GIF, APNG, PNG sequence; range, scale and fps options `anim` `app`
  ✅ Export → Animation…: MP4 (H.264), ProRes 4444, WebM (VP9), GIF, animated PNG and a PNG sequence, for this cut, the loop range, the scene or everything, at 100 / 75 / 50 / 25% of the cut's camera and the project's or another frame rate (frames follow time), with or without the paper (transparency for everything but MP4), looping or once. The window renders each distinct frame once (a drawing on twos crosses once, held for 2) and the app's Rust side writes the file (`src-tauri/src/export.rs`): PNG / GIF / APNG itself, video by piping frames into ffmpeg. Transparent frames come from the engine compositing over white and over black. Cuts framed at another shape are fitted with bars. **ffmpeg is bundled** in the installers (a GPL v3 build, so MP4 has H.264): `scripts/fetch-ffmpeg.mjs` fetches a pinned build per platform (BtbN for Linux and Windows, Martin Riedl's for macOS) and checks its SHA-256; it ships as `pencilpilot-ffmpeg` next to the app with `ffmpeg/NOTICE.md` and the GPL text. An installed ffmpeg (or `PENCILPILOT_FFMPEG`) is used when running from source.
- **S3-T04** Per-frame PSD and layered PNG export; import of image sequences and video frames for rotoscoping `anim` `app`
  ✅ Export → Animation… has two more formats: **PSD per frame** (a layered Photoshop file for each frame, groups kept, cropped to the cut's camera and scaled like the rest, `name_0001.psd`…) and **PNG per layer** (each layer that shows anything as its own transparent sequence, `name_Layer_0001.png`…, to composite elsewhere). Held frames are written once per output frame, so the numbering follows time. **Import** (top bar) brings frames in to trace over: an **image sequence** (pictures put in number order, `walk2` before `walk10`) or a **video's frames** (the bundled ffmpeg decodes it at the project's frame rate, at most 2000 frames, `src-tauri/src/video_in.rs`). Either way each frame is fitted inside the canvas as a drawing on a new layer named after the files, under the others and half see-through, from the current frame on; the layer being drawn on stays picked. Frames go to the frame store as they come, so long clips don't fill the window; Stop keeps the frames already in.
- **S3-T05** Timelapse of the whole project (from the stroke log) and a "drawn live" export `anim`
  ✅ Export → **Film of the drawing…**. **Timelapse:** the canvas takes a small picture after every change (as on the website); the studio now keeps them in the project (`timelapse/<n>.jpg`, at most 540 px, with when each was taken), so the timelapse covers the whole project across sessions, thinned evenly past 1200 pictures. It exports at 10 / 15 / 30 / 60 seconds through the app's own exporter (MP4, WebM with the bundled ffmpeg, or GIF), ending on the finished picture for 1.5 s. (Recovery copies leave the timelapse out, to stay light.) **Drawn live:** the strokes of the frame showing are drawn again on a canvas out of sight, at the speed they were drawn (pauses between strokes kept to 0.1–0.5 s, a pen held still to 0.25 s; as drawn, 2×, 4× or 8×), cropped to the cut's camera; then, after a beat, the finished frame (fills and everything that isn't a stroke) eases in over 0.6 s and holds — the eased dissolve of the house style. Each stroke now notes the drawing its layer showed, so an animation's frame replays only its own strokes. Replay is from the stroke log, so it is as faithful as the log: fills, transforms and selections arrive with the dissolve.

## S4 — AI platform (≈2–3 weeks)
- **S4-T01** Provider interfaces (generate-from-refs, edit-region, first/last-frame video, colourise, critique) and the first back ends `ai`
- **S4-T02** Keys in the OS keychain (BYOK); cost estimate before each action and a project cost meter `ai` `app`
- **S4-T03** Job queue: background jobs, progress, cancel, retry, falling back to another provider on refusal or error `ai`
- **S4-T04** Guide layers: shown like a light table, never exported, with per-guide opacity, colour and lock `ai` `anim`
- **S4-T05** Provenance per frame (hand / traced / auto-inked / AI pixels), shot "made by hand" share, C2PA/XMP on export `ai` `legal`
- **S4-T06** Local model runtime (ONNX, on GPU when available) for line extraction, upscaling and pose `ai`
- **S4-T07** Content-policy handling: clear messages, the "my own original character" note, safe defaults `ai` `legal`

## S5 — Style packs (≈3 weeks)
- **S5-T01** Style pack: pick your frames, model sheets, palettes and brushes; one per project or shared `ai` `design`
- **S5-T02** Measure your style: line weight distribution, line taper, palette, proportions; match the best brush settings `ai` `brush`
- **S5-T03** Optional style LoRA: train on your pack through a provider (cost shown), version it, use it in every generation `ai`
- **S5-T04** Style check: score a frame against the pack (grader measurements) and flag drift (line weight, off-model proportions, palette) `ai` `brush`
- **S5-T05** Licence and ownership gate: confirm the files are yours; mark any reference images that aren't `legal`

## S6 — AI keyframes and references (≈4 weeks)
- **S6-T01** Keyframe guide: from a scribble and / or text plus the previous keys and the style pack → candidates → pick one → Guide layer `ai`
- **S6-T02** Adjust a guide: move, warp, liquify, **re-pose** with skeleton handles, **re-generate a masked region** `ai` `anim`
- **S6-T03** Turnarounds, expression sheets and pose sheets for a character, made from your model sheet `ai`
- **S6-T04** Background and layout references in your style, at the cut's camera size `ai`
- **S6-T05** Send any generated reference to the website-style reference viewer (grid, values, measuring) `ai` `design`
- **S6-T06** Clean-line mode: a generated colour guide → extracted lines (local) → a clean line guide `ai`

## S7 — Coach mode: guided drawing (≈4 weeks)
- **S7-T01** Vectorise a guide: line extraction → centreline → Bézier strokes with width estimates `ai` `brush`
- **S7-T02** Stroke order: passes (gesture → big shapes → contours → details → clean-up), then a large → small, top → bottom heuristic inside each pass; a model can replace it later `ai`
- **S7-T03** Coach: ghost of the next stroke, you draw it, a check (chamfer distance + direction, from the grader) → accept, retry or skip `ai` `brush`
- **S7-T04** "Ink this stroke" / "ink this pass": the vector stroke replayed through *your* brush (pressure from the width estimate), as real, editable strokes `brush` `ai`
- **S7-T05** Pace and detail controls: how many hints, show the whole guide or one stroke, sketch-first or ink-first `ai` `design`
- **S7-T06** Works on any picture: coach yourself through a reference, a guide or a rough frame of your own (cleanup) `ai`

## S8 — In-betweens and timing (≈4 weeks)
- **S8-T01** Timing / spacing charts on the timeline (ease in / out, thirds, custom) that drive the in-betweens `anim`
- **S8-T02** Line-art in-betweens (thin-plate-spline style, local) between two of your inked keys → guides `ai` `anim`
- **S8-T03** Video route: first/last-frame models → sample at your spacing → extract lines → guides; compare candidates `ai`
- **S8-T04** Smear and motion-blur frames for fast actions (procedural + AI option) `ai` `anim`
- **S8-T05** Review strip: accept, re-time or redo each in-between; cleanup in Coach mode `ai` `design`

## S9 — Colour and finishing (≈3 weeks)
- **S9-T01** Character palettes from the model sheet; region fill that follows regions across frames (fill once, colour the cut) `anim` `brush`
- **S9-T02** Shadow / highlight layers with tone lines (coloured trace lines), clipped to flats `anim` `brush`
- **S9-T03** AI colour guides (reference-based colourisation) as Guide layers `ai`
- **S9-T04** Compositing: background / cel / effect layers per cut, glows, gradients, film grain `anim`
- **S9-T05** Camera: pan, zoom, truck and multiplane parallax over large backgrounds; camera shake `anim`

## S10 — The Director (≈3 weeks)
- **S10-T01** Director chat with the project in context (style pack, bible, shot list cached as a prefix): Claude Opus 5.5 with vision `ai`
- **S10-T02** Critique: send a cut as a contact sheet with frame numbers → notes on arcs, spacing, silhouettes, volume and acting, each pinned to frames `ai`
- **S10-T03** From a script beat → shot list → key poses (as thumbnails) + a timing chart → an animatic `ai` `anim`
- **S10-T04** Tool use: the Director can run app actions (make guides, in-betweens, colour flats) with your approval and the cost shown `ai`
- **S10-T05** Learning loop: Director notes link to the website's lessons and drills (shared content) `ai` `design`

## S11 — Beta and release (≈3 weeks)
- **S11-T01** Installers, code signing, notarisation, auto-update (Windows, macOS, Linux); app store builds for iPad / Android `infra` `app`
- **S11-T02** Crash reporting (opt-in), performance budgets (playback at 1080p / 24 fps with 30+ layers), memory tests on tablets `infra` `anim`
- **S11-T03** Optional hosted AI credits (proxy, rate limits, billing); BYOK stays free `ai` `infra` `legal`
- **S11-T04** Docs: getting started, keyboard map, the file format, "how the AI helps" (ADR-0011 in plain words) `design`
- **S11-T05** Licence review of every model and dependency; privacy policy for the app `legal`
- **S11-T06** Closed beta with a few animators; fix list; v1.0 `app`

## S12 — Solo and shared projects (≈6–8 weeks)
Two kinds of project: **solo** (private, on your computer, as today) and **shared** (several
people drawing live, the work and the credits belong to everyone), peer to peer over iroh. Design:
[ADR-0013](adr/0013-solo-and-shared-projects.md).
- **S12-T00** AI is solo only: hidden and refused while drawing together (`aiAllowed`, `ai_allowed`); a session carries no AI edits; Share a copy leaves AI layers in the private original `ai` `app`
  ✅ The rule, the guard in the shell and the window, and the director plugin's rule
- **S12-T01** Solo stays private, provably: start screen with **My projects** / **Shared projects**; a test that fails if the window's CSP gains a network origin; the shell opens no connection for a solo project; **Share a copy…** leaves the private original untouched `app`
- **S12-T02** The peer-to-peer connection (`crates/share`): a key per computer, invite codes, the session over iroh (the host numbers edits, cursors as datagrams, joins and leaves), tested locally and through iroh's public relays `infra`
  ✅ First version: identity, invites, host ordering, presence, refusals; an app joined through the public relay in 1.4 s
- **S12-T03** The session in the app: Tauri commands and events in the Rust shell (the window stays offline), every member's full copy and change log, joining downloads the project, host hand-over, our own `iroh-relay` on the VPS before release `app` `infra`
  ◐ First part: the session in the Rust shell (`share_*` commands, "share-event"), **Draw together** (host / join with a code, who's here), everyone's live cursors on the canvas; `cargo run -p pencilpilot-share --example demo` to try it alone. Still to do: a VPS for the relay; the timeline in a session (one frame for now, S12-T08); saving a shared project's session to rejoin later
  ✅ Strokes and layers: each member's own layer; layers stack in the order they were made, the same everywhere; changed tiles (64×64, deflated) sent after each change (fills, transforms and imports too); Ask to draw here → Let them draw / Hand it over / No, Take back; others' layers locked; Add layer goes through the session; a closed list of edits (anything else, AI included, dropped)
  ✅ Live strokes: each stroke streams as it's drawn (as datagrams, like cursors) and shows on a preview layer per person until its real pixels arrive
  ✅ Deleting and moving shared layers (their owner): the same everywhere; the engine moves / removes them without undo steps, and the undo history is cleared then (its steps place layers by position)
  ✅ Joining downloads the project (a snapshot from the host: layers, access, tiles), then the edits after it; everyone keeps a full copy (Save works as usual; leaving keeps the layers, unlocked); your work from before joining stays, hidden and unshared
  ✅ Host hand-over: any member lets a joiner in (passing them to the host); edits signed and numbered by their author (no forging, never applied twice); when the host leaves, the longest-connected member takes over and everyone catches up
  ✅ Shorter codes: 140 characters instead of 642; relay only, no home IP addresses; the project id comes from its secret
  ✅ Our own relay, ready to deploy: `deploy/relay` (config, sandboxed systemd service, install script for a KVM VPS), Settings → Drawing together → Relay; tested with the same relay server run in-process (join, draw, hand-over)
- **S12-T04** The engine for shared mode: the tile hashes a change touched; put in other people's tiles outside the local undo; layers you can't draw on are locked; undo kept per layer you can draw on `brush`
- **S12-T05** Live layers on one frame: live strokes as previews, tile changes as the record, late joiners load hashes then tiles near the playhead `app` `brush`
- **S12-T06** Layer access: owner, the people allowed to draw, **Ask to draw here** → let them / hand it over / no, take it back; adding layers as in solo `app` `design`
- **S12-T07** Cursors and presence: named, coloured cursors (~20/s, newest wins), who is on which layer and frame, badges on the timeline, **Follow** `app` `design`
- **S12-T08** The timeline and project shared: layers, groups, masks, scenes, cuts, frames, exposures, marks, transitions, dialogue and sound as small edits in the server's order (later wins; destructive edits to others' work refused) `anim` `infra`
- **S12-T09** Your own undo (never someone else's; refuses to wipe work drawn over since), going offline and back (queued changes; drawing on a layer you lost becomes a new layer of yours) `app` `brush`
- **S12-T10** Credits: who made each change; a credits panel; credits in exports' metadata and an optional end card; any member exports or saves a solo copy with the credits `app` `anim`
- **S12-T11** Limits and safety: 8 people, upload size per project, rate limits, removing a member, a load test (8 people drawing, 1080p), a security review, the joining note's wording (legal) `infra` `legal`

---

## Order and first usable version
- **MVP (≈3 months):** S0 → S1 → S2 → S3, then S7 on your own rough frames (Coach mode
  with no generation needed). A complete hand-drawn animation app with guided cleanup.
- **AI v1 (+≈2 months):** S4 → S5 → S6 (keyframe guides, references) → S7 on guides.
- **Full (+≈3 months):** S8 → S9 → S10 → S11.

## Risks
| Risk | Plan |
|---|---|
| Generated frames don't match the artist's line work | Never ship AI lines as art: trace / auto-ink with your brush, plus an optional style LoRA |
| Content policies block youthful-looking anime characters | Several providers with fallbacks, the "original character" note, clear errors, local back ends |
| Model / API churn and prices | Provider interfaces, prices in config, a cost meter |
| Licences (non-commercial weights, provider terms) | A per-back-end licence flag, commercial-safe defaults (ADR-0011 §7) |
| Memory on tablets with long cuts | Tile dedup, disk tier, view-size playback cache |
| Tauri mobile and pen latency | S0 spikes before committing; fall back to desktop-first if tablets lag |

## Decided (2026-11-21)
- The app is called **PencilPilot**. Desktop first; tablets after the pen test (S0-T05).
- First AI provider: ByteDance via BytePlus: **Seedance 2.5** (in-betweens) and
  **Seedream** (keyframes, references). See ADR-0011.

## Open questions for the owner
1. Bring-your-own-key only, or hosted credits as well (S11-T03)?
2. Free, paid, or free with paid AI credits?
3. Should finished animations be shareable back to the website (a gallery)?
