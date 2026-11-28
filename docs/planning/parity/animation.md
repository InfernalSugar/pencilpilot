# Parity inventory: animation and production

Phase 7 ("full parity, then switch"). Each row is one capability a user of the current app
(`apps/studio`, Svelte/TS + Tauri) would notice. The status is the native app's (`apps/native`),
checked by reading its code on `native/phase7` (2026-11-28). Parity means the capability
exists, not the same interface: the native app follows the new design (the Option 2 timeline
with icon trays, the FullApp artboards, rare actions in right-click menus).

- **Status:** Done / Partial (what's missing) / Missing. Partial? means unsure, with the reason.
- **Size** (the work left): S under half a day, M 1–2 days, L more. "—" when Done.
- **Paths:** current app files are under `apps/studio/src/` (`lib/…`, `routes/studio/+page.svelte`
  as `page`); native files are under `apps/native/src/` (`strips.rs` is `shell/strips.rs`,
  `frames/` is `shell/engine/frames/`, and so on).
- **Stub:** a native menu item or button that's drawn but not wired. It pushes
  `Action::Other(name)` and nothing handles the name, so `frames::other`
  (`frames/mod.rs:979`) returns `None`.
- **Budgets:** the performance notes refer to `tools/baseline/BUDGETS.md` and the measurements
  in ADR-0014.

The underlying gap: **the native exposure sheet is one cut.** `Sheet` (`frames/sheet.rs:282–296`)
holds that cut's tracks and loop range. Load opens the cut the file had open
(`files/load.rs:44–55, 146–153`). Save writes that cut back and keeps every other scene, cut,
camera, note, marker, mouth chart and stage tag as the file had them (`files/save.rs:1–4,
239–273`). Many rows below depend on a multi-cut model.

---

## 1. Scenes and cuts

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Switch scene and cut (pickers, each cut's length in seconds) | `TimelinePanel.svelte` scene/cut `<select>`s; `timeline.ts` `cutLength` | **Missing**. The Scene and Cut trays list fixed design placeholders ("Scene 1", "Cut 1 · 1.5 s"…). Picking one pushes a stub. | `strips.rs:99–124`, `strips.rs:707–710`; one cut only: `frames/sheet.rs:282–296`, `files/load.rs:146–153` | M | Needs the multi-cut model. Switching cuts uploads another cut's drawings to the GPU: keep the Open budget (under 400 ms) and the ADR's "going up ahead" prefetch. |
| New cut / new scene (a new cut starts with this cut's camera) | `timeline.ts` `addCut`, `addScene`; `TimelinePanel` `newCut`, `newScene` | **Missing**. "New cut" / "New scene" in the Scene, Cut and New trays are stubs. | `strips.rs:110, 122, 133–134` | S | After the row above. |
| Transition into the next cut (hard cut, white flash, dissolve) | `exporting.ts` `TRANSITIONS`; `TimelinePanel` "Into the next cut" select | **Missing**. Kept in the file only. | `files/save.rs:258–264`; `together/serve.rs:286` | S | The picker is small. Playing and exporting with transitions go with "play the whole scene" (section 4) and export. Design question 2. |
| Other scenes and cuts, their cameras, notes and markers survive a save | `project.ts` (the whole `Timeline`) | **Done**. Written back as the file had them; only the open cut's tracks change. | `files/save.rs:239–273`; `crates/ppstudio/src/lib.rs:300–306` | — | Mouth charts and stage tags are kept only for layers that still exist (`save.rs:271–273`). |

## 2. Exposures and timing (the timeline grid)

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| The grid: a row per layer, cels sized by their hold, drawing numbers, the playhead, zoom | `TimelinePanel.svelte` (rows, `numbers`, `CELL`) | **Done** | `strips.rs:754–1036`; zoom `strips.rs:1559–1585`, `1515–1535` | — | |
| Click a cel or the ruler to go to that frame (and pick the layer) | `TimelinePanel` `cellClick`, `pick` | **Done** | `strips.rs:780–787`, `980–983` | — | |
| Scrub by dragging along the frame numbers | `TimelinePanel` `scrubAt` | **Partial**: the ruler takes clicks, not drags | `strips.rs:780–787` | S | Scrubbing at 4K is within budget (ADR Phase 4), so a drag is cheap. |
| New drawing after this one (N) | `TimelinePanel` `addDrawing` | **Done** | `strips.rs:264–266`; `frames/mod.rs:706–709`, `new_hold` `791–801` | — | |
| Hold this drawing on ones, twos or threes | `TimelinePanel` `timing(n)` | **Done** (the Hold tray) | `strips.rs:137–145`; `frames/mod.rs:852–855` | — | |
| Hold one frame longer / shorter | `TimelinePanel` `hold(±1)` | **Done** (the cel's right-click menu) | `strips.rs:1239–1240`; `frames/mod.rs:860–863` | — | |
| Drag a cel's right edge to change its hold; drag a cel to move it | `TimelinePanel` `dragStart`/`dragMove`/`dragEnd`; `timeline.ts` `moveExposure`, `setLength` | **Missing**. The grid's cels take clicks and right-clicks only. | `strips.rs:980–991` | M | Design question 8. |
| Whole layer on ones / twos / threes | `TimelinePanel` `timingAll`; `timeline.ts` `setTiming` | **Done** (the layer's right-click menu) | `strips.rs:513–515`; `frames/mod.rs:923–928`; `sheet.rs` `retime` | — | |
| Insert a frame / delete a frame on every layer | `timeline.ts` `insertFrameAll`, `deleteFrameAll` | **Partial**: only the active layer's exposure grows or shrinks | `frames/mod.rs:860–863` (works on the active track `t`); `sheet.rs:196–216` | S | Design question 6. |
| Take a drawing out of the timeline (the ones after it move back) | `TimelinePanel` `removeHere`; `timeline.ts` `removeExposure` | **Done** | `strips.rs:1238`; `frames/mod.rs:868–872` | — | The ⌫ hint in the menu isn't a bound key (see section 12). |
| Empty cel; painting on an empty frame starts a drawing there | X-sheet `X`/Delete; `PaintCanvas` new drawing on a blank | **Done** ("Blank frame" in the New tray; painting a blank starts a drawing) | `frames/mod.rs:847–851`, `341–362` | — | |
| Static layer: one drawing all cut long (backgrounds, overlays) | `timeline.ts` `setStatic`, `staticDrawing` | **Done** | `strips.rs:510–511`, `522–532`; `sheet.rs:254–270`, `359–365` | — | Picking which drawing a static layer shows is an X-sheet feature (section 3). |
| Mark a drawing as key, breakdown or in-between: shown on the cel, saved | `timeline.ts` `setKind`, `nextKind`; page `markKind` (Shift+K cycles) | **Done** (the Mark frame tray; badges; written to `drawing_kinds`) | `strips.rs:146–156`, `964–969`; `frames/mod.rs:856–859`; `files/save.rs:207–232`; `files/load.rs:243` | — | Shift+K is missing (section 12). |
| Choose frames (Shift+click from here to there); the frame tools work on this layer or all layers | `TimelinePanel` `pick` (`sel`), `scope`; `frametools.ts` `FrameRange` | **Missing**. There's no range selection, and the tools work on the active layer's cel under the playhead. | `frames/mod.rs:822–833` | M | The rows below depend on it. Design question 8. |
| Copy and paste frames (the same drawings; one layer or every layer's) | `frametools.ts` `copyFrames`, `pasteFrames`; `TimelinePanel` `copyRange`, `pasteRange` | **Partial**: one cel of the active layer. Pasting on another layer makes copies, not the same drawings. The Alt+C / Alt+V shown in the menu aren't bound. | `frames/mod.rs:873–893`; `strips.rs:1242–1243` | S | The current app pastes the same drawings ("changing one changes both"). The native app's "a drawing shows on one layer only" differs, but only across layers. |
| Duplicate frames as new drawings (Shift+D) | `frametools.ts` `duplicateFrames`; `TimelinePanel` `duplicateRange` | **Partial**: one drawing ("Duplicate drawing" in the New tray); no range, no key | `frames/mod.rs:834–846` | S | |
| Reverse frames | `frametools.ts` `reverseFrames` (the chosen frames) | **Partial**: reverses every exposure on the layer | `sheet.rs:234–236`; `frames/mod.rs:867` | S | This behaves differently from the current app. Design question 7. |
| Cycle the chosen frames ×n (loop A B C A B C, or back and forth A B C B A) | `frametools.ts` `cycleFrames`; `TimelinePanel` `cycleRange` | **Missing**. "Cycle…" is a stub. | `strips.rs:1244` | S | After range selection. |
| Boil: held drawings swap with wobbled copies (how many copies, every how many frames, strength) | `frametools.ts` `boilFrames`, `BOIL_DEFAULTS`; brush `boilDrawing` | **Partial**: fixed at 2 copies, on twos, strength 2.0, one cel; no settings or range; the strength isn't scaled to the canvas height | `frames/mod.rs:894–922` | S | `Painter::boil_drawing` exists (`crates/brush/src/lib.rs:4470`). |
| Undo / redo of timeline edits (each a named step; a fill and its timing in one step; sound clip moves) | page `recordTimeline`, `putTimeline`, `tlLabel`/`tlJoin` | **Missing**. Undo is the painter's only, so exposure, hold, mark, static, loop and sound edits can't be undone. | `engine/mod.rs:288–289`; ADR-0014:413–415 (clip moves) | L | It touches every edit above and Drawing together's timeline sync. **Highest-risk gap**: an animator can't undo a wrong hold. |
| Undo of a stroke on another frame goes to that frame, so you see it | page `reveal` | **Partial?** The undo applies to that drawing but stays on the current frame (from the engine test; not checked in the window). | `frames/tests.rs:114–124` | S | |

## 3. The exposure sheet (X-sheet)

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| The X-sheet: the cut down the page, a row per frame (with second marks), action and dialogue columns, a column per layer | `XSheet.svelte`; the side panel's X-sheet tab (`workspaces.ts` `PANEL_TABS`; Animate opens on it) | **Missing**. The panel tabs are Colour, Brushes and Layer. | `panel.rs:107–121` | M | Design question 1. |
| Write on it: type a drawing number (Enter or ↓ to the next frame), X or Delete for an empty cel, arrow keys; pick a static layer's drawing | `XSheet.svelte`; `timeline.ts` `setKey`, `setStaticDrawing` | **Missing** | none | M | Needs text input in the panel (the UI crate has text fields). |
| Action and dialogue notes per frame | `timeline.ts` `setNote`; `Cut.action` / `Cut.dialogue` | **Missing**. Kept in the file only. | `crates/ppstudio/src/lib.rs:303–306` | S | The dialogue also fills lip-sync's "what's said" (section 8). |

## 4. Playback

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Play / stop (Return) at the cut's frame rate | page `togglePlay`, `stopPlay` | **Done** | `strips.rs:276–282`, `1456–1466`; `frames/mod.rs:710–723`, `tick` `986+` | — | Meets the budgets: 0 skipped, first frame in 50–53 ms, 3–4 % CPU (ADR Phase 4). |
| Once / loop / ping-pong | `playback.ts` `frameAt`, `PlayMode` | **Done** | `frames/mod.rs:626–640`; `strips.rs:1306–1322` | — | |
| Loop range In / Out, and clearing it | page `setRange`; `playback.ts` `clampRange` | **Done** (the ruler's right-click menu; shown on the ruler) | `strips.rs:1206–1222`, `793+`; `frames/mod.rs:970–978`, `643–649` | — | Alt+I / Alt+O are missing (section 12). |
| Frame rate (6, 8, 12, 15, 24, 25, 30, 48, 50, 60) | `TimelinePanel` fps select | **Partial**: 12, 24, 25, 30, 60 only. A file at 15 fps plays at 15 but the menu shows no tick. | `strips.rs:181`, `1324–1332`; `files/load.rs:316` | S | |
| Play the whole scene: every cut through its camera, with the transitions | page `playScene`; `exporting.ts` `plan`, `outputFrames` | **Missing**. "The whole scene" can be picked, but nothing reads it. | `strips.rs:179`, `1543–1545`; `frames/mod.rs:643–649` (the cut only) | L | Needs multi-cut, the camera and transitions. Perf: each cut's drawings must be on the GPU ahead of time (the 0.5 s first-frame budget; 0 skipped across cut changes); a dissolve composites two frames at once. |
| Step a frame back / on | `TimelinePanel` `step` (`,` and `.`) | **Done** (← / →, the buttons) | `strips.rs:271–276`, `1440–1470`; `frames/mod.rs:724–735` | — | Different keys (section 12). |
| Flip: hold R to flip the drawings around this one; Shift+R through keys and breakdowns | page `startFlip`, `endFlip`; `flip.ts` `rollFrames`, `keyFrames`, `FLIP_FPS` | **Missing**. The Flip button is a stub. | `strips.rs:1433–1435` | M | Cheap on the GPU path (frames are already composited per change). Needs key-up handling. |

## 5. Onion skin and light table

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Onion skin on / off (O) | `onion.ts` `OnionSettings.on` | **Done** (O, the timeline's button, the View menu) | `strips.rs:267–269`, `1380–1400`; `frames/gpu.rs:384` | — | |
| How many drawings before and after (0–5 each) | `onion.ts` `before`, `after`; Settings › Guides | **Partial**: three presets (1·1, 2·1, 2·2) | `strips.rs:173–177`; `frames/mod.rs:657–661` | S | Design question 11. |
| Colours before / after, and the nearest one's opacity | `onion.ts` `beforeColour`, `afterColour`, `opacity` | **Partial**: fixed at the current app's defaults | `frames/mod.rs:63–65` | S | |
| Onion skin of every layer, or just the active one | `onion.ts` `layers: 'active' \| 'all'`; `onionEntries` | **Missing** (active layer only) | `frames/mod.rs:653–656` | M | Perf: the onion picture is made on the CPU, tile by tile (`frames/onion.rs:1–5`). Every layer multiplies the tiles; scrubbing with the onion skin must stay under 6 ms per frame change (ADR Phase 2 table). Measure, or move it into the GPU compositor. |
| Only keys and breakdowns | `onion.ts` `keysOnly` | **Missing** | `frames/mod.rs:653–694` | S | The marks exist, so the filter is small. |
| Shift & trace: move or turn the onion skin's before / after drawings (view only) | `TimelinePanel` `nudge`, shift pad; page `isShifted` | **Missing** | none | M | Needs a transform in the onion composite (GPU). |
| Light table: pin frames to show (blue) while drawing others; unpin all | page `pinFrame`, `pinned`; `onion.ts` `PIN_COLOUR` | **Missing**. "Pin this frame" (the button and the ruler menu) and "Unpin all" are stubs. | `strips.rs:1220–1221`, `1427–1429` | M | The same composite path as the onion skin. |

## 6. Camera

There are no camera moves in either app: the current app's camera is one rectangle per cut
(`timeline.ts` `Camera`).

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| A camera per cut: the whole canvas or a shape (16:9, 4:3, 1:1, 2.39:1, 9:16), X/Y/W/H, zoom in or out about its centre | `timeline.ts` `cameraOf`, `fitCamera`; `TimelinePanel` `setCamera`, `zoomCamera` | **Missing**. The camera button only flips `s.strips.camera`, which nothing reads. | `strips.rs:1416–1424` | M | Kept in the file. Design question 10. |
| The camera frame drawn over the canvas, and its safe areas | page `cameraOverlay`, `cameraView` | **Missing** | none | S | |
| Playback and export through the camera (the export's size is the camera's) | page `playScene`, `exportPass`; `exporting.ts` | **Missing**. Exports are the document's size. | `export_ui.rs:226–253` | M | Cropping on the GPU is cheap; the export path is on the CPU (ADR), with its peak under 1 GB. |

## 7. Markers

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Sync markers on scene frames: Shift+M, or tap while playing (only adds); shown on the ruler with labels; a list to go to, label or remove | `timeline.ts` `toggleMarker`, `Marker`; page `markFrame`; `TimelinePanel` markers pop-up | **Missing**. "Add a marker" and "Markers…" in the ruler menu are stubs. | `strips.rs:1217–1218` | M | Placed by scene frame, so it shares the scene-frame mapping that sound already uses (`sound.cut_start`). |

## 8. Sound and lip-sync

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| The project's sounds play in step with the picture | `audio.ts` `AudioBank.play` | **Done** | `sound/mod.rs:1–18`, `647–790` | — | Measured within ±1 display frame, no drift (ADR Phase 4). |
| A waveform row per clip | `TimelinePanel` `waveform`; `audio.ts` `peaks` | **Done** | `strips.rs:1040–1204`; `sound/mod.rs:547` | — | |
| Mute, remove and drag a clip along the scene | `TimelinePanel` `clipDown`/`clipMove`, `removeClip` | **Done** | `strips.rs:1086–1201` | — | Not undoable (section 2's undo row). |
| A snippet of sound while scrubbing | `audio.ts` `scrub`; page `scrubSound` | **Done** | `sound/mod.rs:323–328`, `756` | — | |
| A setting to turn the scrub sound off | `settings.svelte.ts` `scrubSound` | **Missing** | none | S | |
| Add sound from a file, starting on this frame (WAV, MP3, OGG, M4A, FLAC…) | page `addSounds`; `audio.ts` `add`, `AUDIO_TYPES` | **Missing** | ADR-0014:413–415; `files/save.rs:675` (only the old file's sounds are copied) | M | Decoding exists (Symphonia, FFmpeg fallback). Still to do: a file dialog (only on a normal start: no windows in checks), writing the new sound's bytes on save, and its waveform off-thread. |
| The mix in video exports | `audio.ts` `mixdown`, `wav` | **Done** | `sound/mod.rs:809–823` | — | |
| Lip-sync a clip: the mouth layer, a mouth chart (A–H, X; extended G/H/X; drawings picked with thumbnails, auto-filled), English or phonetic, timing (ones / twos…), lead, what's said (from the dialogue), progress and cancel; the mouths written into the cut | `lipsync.ts` `shapeFrames`, `writeMouths`, `autoChart`; `LipSyncDialog.svelte`; page `openLipSync`, `runLipSync`; `src-tauri/src/lipsync.rs` (Rhubarb) | **Missing**. "Lip sync from a sound…" in the layer menu is a stub. Mouth charts are kept on save. | `strips.rs:518`; `files/save.rs:273`; ADR-0014:413–415 | L | Depends on adding sound (or lip-syncing existing clips), a 16 kHz mix (`crates/sound` has `mix`), and Rhubarb as an external program: check that it's bundled and its licence for the native build. Design question 9. |

## 9. Production stages

The owner's decisions apply: a stage tag per layer, kept in the timeline (saved, undoable,
shared), and viewing a stage is a view filter on ONE canvas.

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| A stage per layer (storyboard, rough, tie-down, clean-up, tone lines, colour, shading, compositing), set in the Layers panel | `timeline.ts` `setStage`, `STAGES`; page `putStage` | **Missing**. Kept in the file only. | `files/save.rs:271`; `together/serve.rs:330–332` | M | |
| Sort layers into stages from their names | `stages.ts` `guessStage`; page `sortStages` | **Missing** | none | S | |
| View by stage: presets and a custom set, on the canvas, in playback and the onion skin; rows dimmed; new layers go in the view's stage; the active layer moves into the view | `stages.ts` `VIEW_PRESETS`, `layersInView`, `singleStage`; page `setView`, `inStage`; `TimelinePanel` Show pop-up | **Missing** | none | M | Perf: it helps, because fewer layers get composited. The kept bottom layers must be rebuilt when the view changes. Design question 3: the native "Show ▾" already means a cel-mark filter. |
| Light table by stage: another stage's drawings faintly under the view, with a tint (non-photo blue) and opacity; never exported | `stages.ts` `lightTableLayers`, `LIGHT_TABLE_OFF`; page `updateUnder` | **Missing** | none | M | An extra composite pass on the GPU, which has to fit the scrubbing budget. |
| New layer from this: a layer in the next stage just above, with empty drawings timed as the source in every cut | `stages.ts` `traceStages`, `traceName`; `timeline.ts` `followTiming`; page `traceLayer` | **Missing** | none | M | "Every cut" needs the multi-cut model. |
| Export by stage: Final (no storyboard, rough or tie-down; tone lines left out), what the view shows, every layer, or each stage as its own file | `exporting.ts` `ExportShow`, `stageFile`; `stages.ts` `exportPasses`, `toneLinesAbove`; `ExportDialog.svelte` | **Missing**. The export covers the cut or the loop range, with every layer. | `export/mod.rs:183–190`; `export_ui.rs:226–240` | M | May overlap the export inventory. |

## 10. Workspaces

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Built-in workspaces: tool and panel layouts over the same canvas | `workspaces.ts` `BUILT_IN` (Everything, Draw, Animate, Ink & colour); page `useWorkspace` | **Done**, as the design's four: Draw, Animate, Colour, Review. There's no "Everything". | `state.rs:10–46`; `layout.rs:14–28`; `panel.rs:107–121` | — | Design question 4. |
| Customise workspaces: new, rename, delete, reset (up to 12); which bars and panels show; the panel's tab; the tools on the strip | `workspaces.ts` `cleanWorkspaces`, `freshId`, `STRIP_TOOLS`; `WorkspaceDialog.svelte` | **Missing** | none | M | Design question 4. |

## 11. Colour flow and character palettes

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Set up colour layers for a line layer: Colour under it, Shadow (clipped, multiply) and Highlight (clipped, screen), Tone lines above; their stages set; the Fill tool picked | page `setupColour` | **Missing** | none | M | The GPU compositor already does clipping and blend modes (ADR Phase 4). |
| Fill using another layer's lines (the line layer as the fill's reference) | page `toolOpts.fillReference`, `carryFill`; `colour.ts` `walls` | **Missing**. The fill sees the picture on screen, with "close gaps" and "fill under the lines". | `tools/mod.rs:626–643` | M | May overlap the drawing inventory. |
| The colour drawing is held as long as its line drawing (made as needed) | `colour.ts` `colourFor` | **Missing** | none | S | |
| Carry a fill through the next frames: the matching shape in each later line drawing, up to the loop range's end or the cut's, all in one undo step | page `carryFill`; `colour.ts` `regions`, `bestMatch`, `seedIn`, `lineStartsAfter` | **Missing** | none | L | Perf: finding the shapes in each frame is CPU work; keep it off the window's thread with progress. Needs timeline undo (section 2). |
| Shadow and highlight fills stop at the tone lines | page `toneLayersFor`; `stages.ts` `toneLinesAbove` | **Missing** | none | S | After stages and the reference fill. |
| Character palettes: several named palettes of named colours; a click paints with one | `palettes.ts`; `PalettePanel.svelte`; `Timeline.palettes` | **Partial**: one palette (the file's first; the others are kept), its colours picked to paint | `panel.rs:461–557`; `files/save.rs:627–646` | M | |
| Edit palettes: add the colour in use, rename, change or remove a colour; new, rename or remove a palette (up to 32 × 64); each an undo step | `palettes.ts` `addColour`, `setColour`, `removeColour`, `addPalette`, `renamePalette`, `removePalette` | **Missing**. The colour menu ("Edit colour…", "Rename…", "Delete"…) is all stubs. | `panel.rs:1094–1107`, `1199–1205` | M | The native menu also offers new items ("Swap it everywhere…", "Lock this colour"). Those are new features, not parity. |
| A palette from the reference picture's flat colours (a model sheet) | `palettes.ts` `flatColours`; page `referenceColours` | **Missing** | none | S | Needs reference pictures (the drawing inventory). |

## 12. Film, rotoscoping, frame memory and keys

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Record a timelapse: a picture after every change, kept in the project | page `currentDoc(timelapse)`; the canvas's snapshots | **Missing**. "Record timelapse" only toggles a flag; nothing is recorded (an old file's timelapse is kept). | `tools/mod.rs:515`; `canvas.rs:223`; `files/save.rs:675` | M | Perf: a snapshot after each change must stay off the pen's path (pen to screen within 1 display frame + 4 ms) and wait for the pen to rest, as thumbnails do. |
| Timelapse film: plays the snapshots over about a chosen length and ends on the finished picture (MP4, WebM, GIF) | `film.ts`; `FilmDialog.svelte`; page `runFilm` | **Missing** | none | M | Reuses the native export's encoders. |
| Drawn-live film: the frame's strokes drawn again at their speed (2×, 4×, 8×; long pauses shortened), then a dissolve into the finished frame; full or half size | `film.ts` `liveSchedule`, `liveSteps`, `strokesOfFrame`, `ease` | **Missing** | `files/save.rs:675–678` (the native app writes no new strokes to the stroke log) | L | Depends on the native app recording the stroke log. |
| Import a picture sequence as frames: number order, fitted inside, onto a new half-transparent layer under the one in use, from this frame on | `rotoscope.ts` `naturalOrder`, `fitInside`, `placeFrames`, `sequenceName`; page `importSequence`, `importFrames` | **Missing**. File › Import… isn't handled. | `header.rs:30`; `files/mod.rs:367+` (no Import case) | M | Memory: put frames in the compressed store as they come (the current app pushes every 12). |
| Import a video's frames at the project's rate (FFmpeg), with progress and stop | page `importVideo`; `src-tauri/src/video_in.rs` | **Missing** | none | M | FFmpeg is already a dependency (export). |
| Frame memory: a limit on drawings kept in memory, the rest moved to disk (`crates/anim` `FrameStore`); Settings › Performance | `frames.ts` `FrameCache`, `memoryPlan`; `settings` `frameMemoryMb`, `framesFolder` | **Partial**: drawings not showing are kept compressed in RAM, and GPU tiles are capped at 920 MiB; there's no disk store or user limit | `frames/store.rs:1–13`; `frames/mod.rs:596–624`; ADR Phase 4 | M | The 500-drawing budget (under 700 MB) is met (150–164 MiB). A feature-length project could still outgrow RAM. Design question 12. `crates/anim` isn't used by the native app. |
| Animation keys: `,` `.` N, Shift+K mark, R / Shift+R flip, Shift+M marker, Alt+C / Alt+V / Shift+D frames, Alt+I / Alt+O loop, pin, O, play | `shortcuts.ts` (Animation group); page `onkey` | **Partial**: N, O, ← / →, Return only | `strips.rs:260–284` | S | Rebinding keys belongs to the general inventory. |

## Native-only features (not parity: for the record)

These exist in the native design but not in the current app, so they aren't counted:

- **Review:** the review player, pins on the canvas, notes with authors, resolve, and jump to
  a frame (`strips.rs:1950–2104`, `canvas.rs:147–185`, `panel.rs:960–1070`; saved in
  `extra.notes`, `files/save.rs:671`). "Ask for changes", "Approve cut", Speed, Loop and
  "Side by side" are stubs.
- **Colour:** the drawings to colour, with a status per drawing (to do, flatted, done), saved
  in `extra.colourStatus` (`strips.rs:1793–1944`, `files/save.rs:659–666`). "Copy its colours to
  the next ones" is a stub.
- **Timeline:** "Show: All / K + B / Keys" filters which cels are drawn (`strips.rs:934–938`);
  "Extend hold" (`sheet.rs:218–232`); previous and next drawing (`frames/mod.rs:736–756`); the
  Draw workspace's frame strip with thumbnails (`strips.rs:1586–1747`); the layer menu's "Show
  as outlines" (a stub).

---

## Design questions for the owner

1. **The X-sheet:** where does it live in the new design? The panel's tabs are Colour, Brushes
   and Layer, and the Animate workspace has the Option 2 timeline. Options: an X-sheet panel
   tab in Animate, or a vertical view of the timeline itself.
2. **The Scene and Cut trays:** they show fixed placeholder options. How do they scale to many
   scenes and cuts (a scrolling tray, a menu)? Where do the transition into the next cut and a
   cut's length live: a right-click on the cut, or the tray?
3. **"Show" means two things:** the native "Show ▾" filters cels by mark (all, keys and
   breakdowns, keys), while the current app's "Show" is the stage view (Final, Rough…). Which
   name goes where, and where does the stage view and its light table sit (the header, or the
   timeline's view bar)?
4. **Workspaces:** the native app has the design's four fixed workspaces (Draw, Animate,
   Colour, Review). The current app has customisable ones (new, rename, delete, reset; bars,
   panel tab, the tools on the strip) and an "Everything" workspace. Keep customising, or drop
   it? Is "Everything" needed?
5. **Review and the colour statuses** are new in the native design. Are they Phase 7 scope to
   finish ("Approve cut", "Ask for changes", Speed, "Side by side" are stubs)? Are notes in
   `extra` the format to keep?
6. **Insert / Remove frame:** the native app changes only the active layer; the current app
   inserts or deletes the frame on every layer. Which one, or both (for example, the Frames
   tray for this layer and the ruler's menu for every layer)?
7. **Reverse frames:** the native app reverses the whole layer; the current app reverses the
   chosen frames. Confirm the current behaviour.
8. **Choosing frames and timing by dragging in the Option 2 grid:** Shift+click, or a drag
   across cels? Is a cel's right edge draggable for its hold, and can a cel be dragged to move
   it? The design shows neither.
9. **Lip-sync:** keep Rhubarb as a bundled external program for the native build (licence and
   packaging), or replace it? Is the mouth chart a dialog, or part of the layer panel?
10. **The camera:** edit it on the canvas (drag the frame) or with numbers as today? Today the
    timeline's camera button only toggles a flag.
11. **The onion skin's full settings** (0–5 before and after, colours, opacity, all layers,
    keys only, shift & trace): in the onion ▾ menu, a pop-over, or Settings?
12. **Frame memory:** is RAM-only compressed storage enough, or does the native app need
    `crates/anim`'s disk store and a user limit for very long projects?
13. **Frame rates:** the native menu offers 5 rates and the current app 10. Restore the 10, or
    allow any number?

## Totals

77 rows (the native-only features aren't counted).

| Status | Rows |
|---|---|
| Done | 22 |
| Partial | 13 (one of them Partial?) |
| Missing | 42 |

| Size of the gaps (Partial + Missing) | Rows |
|---|---|
| S (under ½ day) | 22 |
| M (1–2 days) | 28 |
| L (more) | 5: timeline undo, play the whole scene, lip-sync, carry a fill through frames, drawn-live film |

That comes to roughly 11 + 42 + 15 or more, so about 70 working days for this area at the
sizes above. The multi-cut model and timeline undo come first, because many rows depend on
them.
