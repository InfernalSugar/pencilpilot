# PencilPilot Studio: plan and milestones

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
| Keyframe guide from references | Gemini 3 Pro Image ("Nano Banana Pro"), up to 14 reference images, ≈$0.13/image | Gemini 3.1 Flash Image (≈$0.05–0.07), gpt-image-2, FLUX.2 (pro/flex: 8 refs), Seedream 5 | Strong character identity. Line weight is not pixel-exact, which is why we trace or auto-ink |
| Style LoRA on your frames | FLUX.2 klein 4B (Apache-2.0) via fal/Replicate trainers, a few $ per run | Qwen-Image-Edit LoRA; SDXL anime + LoRA locally | The best route to your exact line feel. Licence per model (ADR-0011 §7) |
| Region edit / re-pose | the same edit models with a mask; pose from a DWPose-style 2D skeleton | ControlNet openpose (local) | |
| Line extraction | AniLines / MangaLineExtraction (MIT), ONNX, local | Anime2Sketch | Runs on CPU; no provider needed |
| Strokes and their order | centreline vectorising (skeleton → Bézier) + an ordering heuristic | "Draw This First" (NeurIPS 2026) once code is out | No hosted API exists yet; ours is local |
| In-betweens | line-art in-betweening (thin-plate-spline, local), then cleanup | first/last-frame video: Wan 2.x (≈$0.10/s), Kling O1, Veo 3.1 Lite, Seedance; ToonComposer (needs a 57 GB GPU) | Video models blur lines: sample at your spacing → extract lines → guide |
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
- **S0-T02** Split the engine: `crates/brush` (pure Rust) + `crates/brush-wasm` (bindings); native build and tests `brush` `infra`
  ✅ `crates/brush` (`pencilpilot-brush`, lib `brush`) holds the engine and its tests; `crates/brush-wasm` is only the wasm-bindgen bindings. The web package is unchanged.
- **S0-T03** Move the design system and studio components into `packages/ui` and `packages/paint`; the website keeps passing every test `design` `infra`
  ✅ `@pencilpilot/ui` (theme, Button, Logo, Tile) and `@pencilpilot/paint` (the whole studio, plus the tracing and grayscale helpers it used from the site). The site imports them; root lint / check / test cover both packages.
- **S0-T04** Tauri 2 spike: the studio running natively on Windows, macOS and Linux, plus an iPad and Android build `app` `infra`
  🟡 `apps/studio`: Tauri 2 + SvelteKit (single-page) with the shared studio; runs and packages on Linux (`.deb`, 4.2 MB). Windows / macOS builds come with CI (S0-T06); iPad / Android builds need Xcode and the Android SDK.
- **S0-T05** Native pen input spike: pressure, tilt and barrel button, latency budget (<16 ms), palm rejection on tablets `app`
  🟡 The app's **Pen test** page measures pressure levels, tilt, twist, buttons, sample rate (with coalesced events) and event delay; results per system to be recorded here.
- **S0-T06** CI for the app: build matrix, Rust + UI tests, nightly artifacts `infra`

## S1 — The app shell, same UI (≈3 weeks)
- **S1-T01** Studio layout in the app: the same top bar, tool rail, options bar, dock and themes as the website `app` `design`
- **S1-T02** Home: a project browser with thumbnails, recent files, templates (canvas sizes, frame rates) `app` `design`
- **S1-T03** `.ppstudio` format v1 (zip: manifest, tiles, strokes, references, provenance) + a format spec doc `app` `anim`
- **S1-T04** Autosave, crash recovery, version history (snapshots), native open / save / export dialogs `app`
- **S1-T05** The native engine behind PaintCanvas (same API as WASM), with multi-threaded compositing `brush` `app`
- **S1-T06** Settings: keyboard shortcuts editor, pen curve, tablet gestures, storage location `app`

## S2 — Animation core (≈4–5 weeks)
- **S2-T01** `crates/anim` frame store: sparse tiles, hash dedup, memory / disk tiers, tests at 1080p and 4K `anim`
- **S2-T02** Timeline: scenes → cuts → layers × frames; exposures (1s / 2s / 3s), hold, insert, delete, drag `anim` `app`
- **S2-T03** Exposure-sheet view (X-sheet) alongside the timeline, with dialogue and action columns `anim` `design`
- **S2-T04** Onion skin (red before, green after, how many, opacity, tint) and a light table to pin any frames `anim`
- **S2-T05** Playback: fixed fps, loop a range, ping-pong, scrub, frame-step keys, playback cache `anim`
- **S2-T06** Cel layers (one drawing per exposure) vs static layers (backgrounds, overlays), and per-cut camera frame (safe areas) `anim`
- **S2-T07** Frame tools: copy / paste / duplicate / reverse frames, cycles, "boil" (redraw-looking jitter on held frames) `anim`

## S3 — Sound and export (≈2–3 weeks)
- **S3-T01** Audio track: import, waveform, scrub audio, sync markers `anim`
- **S3-T02** Lip-sync: Rhubarb → mouth shapes on the X-sheet, linked to your drawn mouth chart `anim`
- **S3-T03** Export: MP4, ProRes 4444 (alpha), WebM, GIF, transparent GIF, APNG, PNG sequence; range, scale and fps options `anim` `app`
- **S3-T04** Per-frame PSD and layered PNG export; import of image sequences and video frames for rotoscoping `anim` `app`
- **S3-T05** Timelapse of the whole project (from the stroke log) and a "drawn live" export `anim`

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

## Open questions for the owner
1. Name, and which platforms come first (desktop only, or iPad from the start)?
2. BYOK only, or hosted credits as well (S11-T03)?
3. Free, paid, or free with paid AI credits?
4. Should finished animations be shareable back to the website (a gallery)?
