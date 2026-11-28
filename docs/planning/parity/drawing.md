# Parity inventory: drawing

Phase 7 ("full parity, then switch"). Every user-facing drawing capability the current app (Svelte/TS + Tauri)
has, and where the native app (`apps/native`, branch `native/phase7`) stands on it.

- **Current app** paths are under `packages/paint/src/` unless they start with `apps/studio/`. `PaintCanvas` is
  `PaintCanvas.svelte`; `ToolOptionsBar` is `ToolOptionsBar.svelte`; "the page" is
  `apps/studio/src/routes/studio/+page.svelte`.
- **Native** paths are under `apps/native/src/shell/` unless they start with `crates/`. "Engine" means
  `crates/brush` (`Painter`), which both apps share: where it already does the pixel work, the native gap is
  interface and wiring, and the size reflects that.
- **Parity means the capability exists**, not the same UI: the new design (FullApp artboards) declutters, and
  puts rarely used actions in right-click menus.
- **Statuses were checked in code.** Several native menu items exist but do nothing: they push
  `Action::Other(..)`, and no handler takes them (`state.rs:110`: "for menu items not wired yet"). Those are
  **Missing**, with the menu item noted as evidence.
- **Performance budgets** (`tools/baseline/BUDGETS.md`) are cited where a feature risks them: pen to screen
  within 1 frame + 4 ms; clicks shown on the next frame; heavy brushes within the drawing budgets; moving
  2,000 strokes at the display's rate.
- **Out of scope here** (sibling files): timeline, exposure sheet, onion skin, playback, export formats,
  drawing together, project files. They appear only where a drawing feature depends on them.

## 1. Tools, shortcuts and menus

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Tool column per workspace; hide and show tools | `ToolRail.svelte` `ALL_TOOLS`; the page's "Customise workspaces" (+page.svelte:4207) | Done | `tools.rs:57` `rail_tools`, `tools.rs:300-331` (Hide from the rail / Show every tool) | – | Native workspaces are fixed sets. Choosing tools per workspace is a workspace-layout question, not a drawing one. |
| Single-key tool shortcuts | `apps/studio/src/lib/shortcuts.ts` `ACTIONS` (B E I G F U S J P V K M L W Shift+G T Shift+V Ctrl+U) | Partial: B E G F M V I T U L work. S J P K W Shift+G Shift+V Ctrl+U are missing because their tools are missing. | `tools.rs:41` `tool_info`, `tools.rs:186-191`; `engine/tools/overlay.rs:413-422` | S | Key clashes: U is **Shape** in native but **Measure** in the current app. F and L both give the native lasso. |
| Changing shortcuts (rebinding, clash warnings) | `shortcuts.ts` `comboOf`, `clashes`, saved keys | Missing ("Set shortcut…" does nothing) | `tools.rs:313,328` → `Action::Other`, no handler | M | Needs a settings surface. |
| Tool menu: Tool settings…, Reset this tool | (tool options bar) | Partial: Tool settings… opens the settings pop-up; Reset this tool does nothing | `engine/tools/overlay.rs:435`; `tools.rs:329` | S | |
| Quick menu on right-click / long-press (Undo, Redo, Eyedropper, Brush↔Eraser, Mirror view, Deselect) | the page `quickActions()` (+page.svelte:1989), `QuickMenu.svelte` | Partial: right-click opens a canvas menu (Paste, Select all, Guides, Symmetry, Snap, Rotate/Flip/Reset view, Timelapse). Undo/Redo are in the header, Mirror is H, Deselect is Ctrl+D. There is no Brush↔Eraser swap or Eyedropper in the menu. | `canvas.rs:200-223` | S | Design question 4. |

## 2. Brushes

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Built-in brush library: 312 brushes in 11 categories (Animation, Pencils, Inks & pens, Markers, Charcoal & pastel, Airbrush & soft, Paint, Watercolour, Blend & smudge, Texture & FX, Erasers) | `brushes.ts` `loadBrush`, `apps/web/static/brushes/manifest.json` | Partial: 5 brushes compiled in (2B pencil, Rough pencil, Liner, Marker, Airbrush) | `engine/mod.rs:92-114` `BRUSHES` | M | Every brush must stay within the heavy-brush budget. Today the oil brush is 410 ms pen to screen, so benchmark the whole library. Load `.myb` files lazily (parsing is cached: `engine/mod.rs:46`). |
| Brush picker: category tabs, search | `BrushPicker.svelte` (categories, "Search N brushes…") | Missing (one flat list) | `panel.rs:616` `brushes` | S | Comes with the library. |
| Favourite brushes | `BrushPicker.svelte` `toggleFav`, `pp.favouriteBrushes` | Missing ("Add to favourites" does nothing) | `panel.rs:1088`, `panel.rs:1204` → `Action::Other` | S | |
| Real brush previews (a thumbnail per brush, a live preview stroke) | `PackBrushThumb.svelte`, `BrushPreview.svelte`, `/brushes/<id>.png` | Partial: a made-up wave whose width is guessed from the brush's name | `panel.rs:560` `brush_look`, `panel.rs:592` | S | Render through the engine off-screen, once, then cache. |
| Brush size range (up to 6400 %, with a max-size setting) | `view-prefs.ts` `maxBrushSize`; brush size in the page | Partial: the slider runs from 1 to 60 px. Sizes up to 2000 load from a file. | `panel.rs:633-647`, `engine/tools/overlay.rs:475`; `engine/files/load.rs:346` | S | Big dabs cost per sample: check pen to screen at the top size. |
| Size keys `[` `]`; drag to resize (Ctrl+Alt+drag, Alt+right-drag) with a live outline | `shortcuts.ts` smaller/bigger; `PaintCanvas` `sizeDrag` (PaintCanvas:4009-4034) | Missing | none | S | |
| Size per brush remembered | page state | Done | `panel.rs:620-655` `panel.sizes` | – | |
| Brush opacity | page / `ToolOptionsBar` | Done | `engine/tools/mod.rs:594` `tune_brush` (opacity on `opaque_multiply`) | – | |
| New brush / edit brush. Shape: size, size wobble, edge, tip, tip angle, tip follows the stroke. Stroke: spacing, smoothing, scatter, shaky line. Pressure and speed: thicker, darker, faster, tilt wider. Colour: opacity, blend with the paint underneath, carry paint, colour and light/dark wobble. Test pad. | `BrushCreator.svelte`, `custom-brushes.ts` `applyTweaks`, `BrushTestPad.svelte` | Missing ("Edit brush…" does nothing) | `panel.rs:1083` | L | Needs a store for custom brushes (the current app keeps them in browser storage). |
| My brushes: duplicate, rename, delete | `custom-brushes.ts` `saveCustomBrush`, `deleteCustomBrush` | Missing (menu items do nothing) | `panel.rs:1084-1090` | S | Depends on the brush editor. |
| Import brushes: `.myb`, `.abr` (sampled tips), `.brush` / `.brushset` (shape + grain pictures), PNG/JPEG/WebP as a tip | `brush-import.ts` `importBrushFile`, `my-brush-import.ts` `importBrushFiles` | Missing | none | M | Port the parsers to Rust. `.brushset` is a zip. |
| Brush textures: paper grain (scale, strength), picture tip, dual brush (size, scatter, count) | `brush-textures.ts`, `BrushTexturePanel.svelte`, `TexturePicker.svelte` | Missing | engine has it: `crates/brush/src/lib.rs:839` `set_texture`, `:856` `set_dab_style`, `texture.rs` | M | Built-in grains are drawn in TS today and need porting. |
| Strand (bristle) brushes: strands, spread, wobble, taper | `BrushCreator.svelte` "Strands" | Missing | engine: `lib.rs:890` `set_strands`, `strands.rs` | S | Depends on the brush editor. Heavier per dab: check the budget. |
| Pressure changes size / opacity, per tool | the brush's own dynamics | Done | `tools.rs:310-326`, `engine/tools/mod.rs:594-606` | – | The native app adds per-tool toggles. |

## 3. Pen feel, smoothing and drawing assists

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Pen feel: a personal pressure curve (Light touch / Normal / Heavy hand, plus a firmness slider) | `pressure.ts`, `PenFeel.svelte` | Missing | none | S | Apply it to the pressure before `stroke_to`. No latency cost. |
| Pressure and tilt off (every stroke at a steady pressure) | `PaintCanvas` `penPressure` (PaintCanvas:393) | Partial: size and opacity toggles per tool; tilt always reaches the brush | `tools.rs:310`; `engine/mod.rs:394` | S | |
| Steady lines (stabiliser) | `stabilizer.ts` (a "pulled string" that catches up at pen-up), one amount per tool: brush, eraser, smudge | Partial: one slider for every tool, mapped to the brush's slow-tracking (it trails rather than holds back) | `engine/tools/mod.rs:596-599`; `panel.rs:670` | S | Pen-to-screen budget figures were taken without smoothing. Check that the native trailing doesn't read as lag. |
| Snap shapes: hold at a stroke's end to snap it to a line, polyline, triangle, rectangle, circle or ellipse; then drag its points | `quickshape.ts`, `shape-edit.ts`, `PaintCanvas` `setShapeForm` / `setShapeCurve`; toggle at +page.svelte:4590 | Missing | none | M | The native app has a separate drag-out **Shape** tool instead (`engine/tools/mod.rs:1172`). Design question 3. |
| Close and fill: a stroke ending where it began fills inside | `tool-options.svelte.ts` `closeFill`; +page.svelte:4602 | Missing | none | S | |
| Snap to ink: stroke ends land on nearby lines | `ink-snap.ts` | Missing | none | M | Needs stroke data (see section 11). |
| Ruler guide: strokes started next to it run straight along it | `guides.ts` `ruler`; `ToolOptionsBar` "Ruler" | Missing | none | S | |
| Guide snap per layer: strokes follow grid, isometric or perspective directions | `guides.ts` assist; `PaintCanvas` `setLayerAssist`; Layers "Guide snap" | Missing | none | M | Native snapping only moves Select and Shape points onto the 64 px grid (`engine/tools/mod.rs:306`). |

## 4. Eraser

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Eraser tool | brush tool with `eraser` on | Done | `engine/mod.rs:116,361-385` | – | |
| Erase with any brush (the brush tool's eraser mode; the Erasers category) | the page `eraser` flag; manifest "Erasers" | Partial: one fixed hard eraser | `engine/mod.rs:116` `ERASER` | S | Design question 15. |

## 5. Fill and lasso fill

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Flood fill that goes behind the lines | `PaintCanvas` `fillAtCanvas` → `paint.fill` | Done | `engine/tools/mod.rs:626` `fill_at` | – | |
| Fill: gap closing | `fillAtCanvas` (always 3 px) | Done (a 4 px toggle) | `engine/tools/mod.rs:633`; `engine/tools/overlay.rs:491`; `panel.rs:497` | – | |
| Fill: tucks under soft line edges | `fillAtCanvas` (grows 2 px, always) | Done ("Fill under the lines" toggle) | `engine/tools/mod.rs:634` | – | |
| Fill: tolerance | `ToolOptionsBar` Tolerance; `fillTolerance` | Missing (fixed at 0.12) | `engine/tools/mod.rs:637` | S | |
| Fill: what you see (all layers) vs this layer only | `fillAllLayers` | Missing (always all layers) | `engine/tools/mod.rs:638` | S | |
| Fill: lines from a reference layer (colour on one layer, lines on another) | `fillRefLayer`; `ToolOptionsBar` "Lines from" | Missing | engine: `lib.rs:2303` `fill_from` | S | |
| Fill on a line layer goes onto the layer under it (a new "Colour" layer if needed) | `PaintCanvas` `fillUnderLines` | Missing | none | S | Depends on line layers (section 11). |
| Set up colour layers (Colour under the lines, Shadow and Highlight clipped, Tone lines above) | `tool-options.svelte.ts` `setupColour`; `ToolOptionsBar` | Missing | none | S | Needs a clipping control (section 10). |
| Colour drop: drag a swatch onto the drawing to fill; slide to widen; keep tapping to fill more | `Swatches.svelte`; `PaintCanvas` `dropAt`, `refillDrop`, `dropContinue` | Missing | none | M | Refilling on each slider move is undo + fill: keep it to one frame at 4K. |
| Fill carries on through the following frames | `apps/studio/src/lib/colour.ts`; `fillAcross` | Missing | none | M | An animation colouring feature. Run it off the window thread. |
| Lasso fill (freehand) | `PaintCanvas` `commitLassoFill`; tool `lasso-fill` (F) | Partial: fills only in the Colour workspace; in Draw the lasso selects | `engine/tools/mod.rs:695-703` | S | Design question 2. |
| Lasso fill: straight edges between clicked corners, Enter to finish | `lasso-path.ts` | Missing | none | S | |
| Lasso fill: Erase mode (or Alt) cuts the shape out | `lassoMode` | Missing | none | S | |
| Lasso fill: smoothing, hard edges | `lassoSmoothing`, `lassoHard` | Missing | none | S | |
| Fill the selection with the colour | `PaintCanvas` `fillSelection` | Done | `engine/tools/overlay.rs:330`; `engine/tools/mod.rs:495` | – | |
| Colour-fill selections: each selection fills as it's made, and refills on a colour change until deselected | `selectFill` | Missing | engine: `lib.rs:2630` `refill_selection` | S | |

## 6. Selection

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Rectangle select | `select-rect` (M) | Done | `engine/tools/mod.rs:684` | – | |
| Ellipse select | `selectShape: 'ellipse'` | Missing | none | S | |
| Lasso select | `select-lasso` (L) | Done (in Draw) | `engine/tools/mod.rs:695` | – | |
| Add / take away (Shift / Alt) | `selectMode` | Done | `engine/tools/mod.rs:280` `op` | – | |
| Intersect | `selectMode: 'intersect'` | Missing | none | S | |
| Magic wand: tolerance, joined-up or every similar colour, what you see or this layer | `select-wand` (W); `wandTolerance`, `wandContiguous`, `wandAllLayers` | Missing | engine: `lib.rs:2714` `select_wand`, `:2738` `select_colour_range` | S | |
| Select the current colour wherever it is | `PaintCanvas` `selectColour` | Missing ("Select every area with it" in the palette menu does nothing) | `panel.rs:1102`; engine `select.rs:77` | S | |
| Select a layer's contents (Ctrl+click its picture) | `selectLayerContents` | Missing | engine `select.rs:101` `select_layer` | S | |
| Select all, Deselect | Ctrl+A, Ctrl+D | Done | `engine/tools/overlay.rs:394-395` | – | |
| Invert selection (Ctrl+Shift+I) | `invertSelection` | Partial: the action is handled, but no key or menu item sends it | `engine/tools/mod.rs:472`; not in `overlay.rs:319-331` | S | One line. |
| Feather | `featherSelection` | Missing | engine `lib.rs:3021` | S | |
| Grow / shrink | `growSelection` | Missing | engine `select.rs:39` | S | |
| Clear what's selected (Delete) | `clear-selected` | Done | `engine/tools/overlay.rs:402` | – | |

## 7. Transform, liquify and the clipboard

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Move, scale, turn with handles; Shift keeps proportions, turns in 15° steps, moves along one axis | `PaintCanvas` transform | Done | `engine/tools/mod.rs:946-1110`; ADR 0014 Phase 2 step 4 (moving on the GPU, within budget) | – | |
| Flip horizontally / vertically | `transformFlip` | Done | `engine/tools/mod.rs:1113`; `overlay.rs:327-328` | – | |
| Turn 90° button | `transformRotate(90)` | Missing (the handle with Shift gets there) | none | S | |
| Reset the transform | `transformReset` | Missing | none | S | |
| Distort / perspective (corners on their own; Ctrl+drag a corner) | `distort`; `homography.ts` | Missing | engine `lib.rs:3264` `transform_preview_h` | M | The GPU float is affine only (`engine/mod.rs:39`). A projective preview needs shader work to keep the moving budget. |
| Warp (a 4×4 grid) | `warp.ts`; `warp` | Missing | engine `lib.rs:3305` `transform_preview_mesh` | M | Same: a GPU mesh preview, or a CPU preview risks the frame rate at 4K. |
| Liquify (size, strength) | tool `liquify` (K) | Missing | engine `lib.rs:3633-3751` | M | CPU per move. Measure at 4K against the drawing budget. |
| Copy / Cut | `copySelection` | Done (inside the app, active layer) | `engine/tools/mod.rs:716` | – | |
| Paste | `pasteCopied` → a **new layer**, then Transform | Partial: floats over the **active layer**, kept on commit | `engine/tools/mod.rs:823` | S | Design question 1. The native app adds Paste in place (Ctrl+Shift+V). |
| Paste a picture from the system clipboard; drop an image file on the canvas | the page `onpaste` (+page.svelte:2254); `PaintCanvas` `drop` | Missing | none | M | Wayland clipboard and drag-and-drop in `crates/window`. |
| Copy to a new layer (Ctrl+J) | `copy-to-layer` | Missing | none | S | |

## 8. Other painting tools

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Smudge: any brush pushes paint, with a strength | `smudge.ts`; tool `smudge` (S) | Missing | none (the engine smudges through brush settings) | S | The heavy-brush budget applies. Smudge brushes were the slow ones. |
| Clone / heal: Alt+click or "Set source", Aligned, all layers, Heal | `clone.ts`; tool `clone` (J) | Missing | engine `clone.rs:196` `begin_clone_stroke` | M | |
| Gradient: linear or radial, to transparent or to a colour | tool `gradient` (Shift+G); `gradientKind`, `gradientEnd` | Missing | engine `lib.rs:4056` `gradient_preview`, `adjust.rs` `Gradient` | S | |
| Pen path: corners and smooth handles, close; stroke with the brush (taper), fill, take back a point | `pen-path.ts`; `PaintCanvas` `penStroke`, `penFill`, `penBack` | Missing | none | M | |
| Measure (pixels, or units marked on a reference) | tool `measure`; `ToolOptionsBar` | Missing | none | S | Units depend on references (section 14). |
| Text: type on the canvas | `text-tool.svelte.ts`, `TextOptions.svelte` | Partial: typed into the layer's pixels in Inter; size = brush size × 8; Shift+Enter for new lines | `engine/tools/mod.rs:1240-1310`, `engine/tools/text.rs` | – | Sized by the rows below. |
| Text layers stay editable (click to change; "Text layer" badge; rasterise) | `PaintCanvas` `textBegin` / `textCommit`, `rasterizeLayer` | Missing | engine `text.rs:212` `text_redraw`, `:241` `set_text_silent`, `:187` `rasterize_text` | L | Text layers in opened projects show as pixels only. |
| Text: font choice, plus font files imported and kept in the project | `TextOptions.svelte` Font, font file | Missing (Inter only) | `engine/tools/text.rs:8` | M | `cosmic-text` is already in use. |
| Text: size, colour, bold, italic, alignment, line spacing, letter spacing, wrap width, turn | `TextOptions.svelte` | Missing | none | M | |

## 9. Guides, perspective, symmetry, composition

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Square grid | `guides.ts` `grid` | Partial: on/off only, fixed at 64 px | `engine/tools/mod.rs:24` `GRID`; `overlay.rs:168` | S | |
| Grid size; isometric grid | `guides.ts` `grid.size`, `kind: 'isometric'` | Missing | none | S | |
| Perspective guides: 1, 2 or 3 point, vanishing points dragged on the canvas | `guides.ts` `withPerspective`; `ToolOptionsBar` | Missing | none | M | The canvas menu hint already says "Grid · Perspective · Rulers ›" (`canvas.rs:214`). |
| Symmetry: vertical, horizontal, both | `guides.ts` `symmetry` | Done (about the canvas centre) | `engine/tools/mod.rs:1133` | – | |
| Radial symmetry with N copies | `symmetry.mode: 'radial'`, `n` | Missing | engine `lib.rs:594` `SymmetryMode::Radial` | S | |
| Move the symmetry centre / vanishing points; put guides back in the middle | `centreGuides`; `ToolOptionsBar` "Drag vanishing points…" | Missing | none | S | |
| Composition guides (thirds, golden ratio, spiral, triangles, diagonals, cross, armature, safe areas): flip, move, size, reset | `guides.ts` `COMPOSITIONS`, `composition-elements.ts`; `ToolOptionsBar` | Missing | none | M | |
| Composition creator; save and import `.ppcomp.json` | `CompositionCreator.svelte`, `CompositionShapes.svelte`, `custom-compositions.ts` | Missing | none | M | |
| Rulers along the canvas, with units (px, in, cm, ppi) | `Rulers.svelte`; `view-prefs.ts` `rulers`, `units`, `ppi` | Missing | none | S | |

## 10. Adjust and filters

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Adjust the layer: hue, saturation, brightness, contrast, blur, live preview, Apply / Cancel (Ctrl+U) | `tool-options.svelte.ts` `openAdjust` / `previewAdjust`; `PaintCanvas` `adjustBegin` / `adjustPreview` | Missing | engine `lib.rs:3893,4046,4061`; `adjust.rs` | M | The preview runs on the CPU for every slider move. At 4K, run it off the window thread so clicks still show on the next frame. |
| Recolour the whole layer, keeping its shading | `recolour` | Missing | engine `adjust.rs` `ColourAdjust::recolours` | S | |
| Curves (RGB and per channel) | `ToneEditor.svelte`, `tone.ts` `curveTable` | Missing | engine `adjust.rs` | M | Needs a curve-editor widget in `crates/ui`. |
| Levels | `tone.ts` `levelsTable` | Missing | engine `adjust.rs` | S | |
| Gradient map, with presets | `tone.ts` `MAP_PRESETS`, `mapTable` | Missing | engine `adjust.rs` | M | |
| Filters: sharpen, motion blur, noise, halftone, chromatic aberration, bloom, glitch | `filters.ts`, `FilterEditor.svelte` | Missing | engine `filters.rs:18` `Filter`, `lib.rs:4051` `filter_preview` | M | Same off-thread need as Adjust. |

## 11. Layers

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| New, duplicate, delete, move up / down | `LayersPanel.svelte` toolbar | Done | `panel.rs:772-790`; `engine/frames/mod.rs:757,929-961` | – | |
| Show / hide, lock | `LayersPanel.svelte` | Done (the engine refuses paint on a locked layer) | `panel.rs:868-920`; `crates/brush/src/lib.rs:1966` | – | |
| Layer opacity | `LayersPanel.svelte` Opacity | Done (one undo step per drag) | `panel.rs:794-812`; `engine/frames/mod.rs:67,223` | – | |
| Rename (double-click, F2) | `LayersPanel.svelte` `renaming` | Missing | none | S | |
| Layer thumbnails | `LayersPanel.svelte` `use:thumb` | Missing (rows show a C or S badge) | `panel.rs:836-850` | S | Engine `lib.rs:5622` `layer_thumbnail`. Refresh off the pen's path. |
| Blend modes (Normal, Multiply, Screen, Overlay, Darken, Lighten, Add) | `LayersPanel.svelte` Blend | Partial: read from the file and composited on the GPU; no control to change them | `engine/files/load.rs:361`; `engine/frames/gpu.rs:426` | S | |
| Clip to the layer below | `LayersPanel.svelte` "Clip to below" | Partial: read from the file and composited; no control | `load.rs:363`; `gpu.rs:431` | S | |
| Alpha lock | `LayersPanel.svelte` "Alpha lock" | Partial: read from the file and honoured by the engine; no control | `load.rs:362`; `crates/brush/src/lib.rs:1976` | S | |
| Merge down | `mergeDown` | Missing | engine `lib.rs:1863` | S | |
| Layer masks: add, paint on (black hides), apply, delete, thumbnail | `LayersPanel.svelte` mask buttons; `addMask` / `editMask` / `applyMask` / `deleteMask` | Missing. Masks in an opened project **aren't loaded**, so the layer shows unmasked. A save keeps them. | `engine/files/save.rs:434` ("the native app doesn't edit them: kept"); no mask code in `load.rs` | M | Engine `lib.rs:1454-1524`; the GPU path already composites masks (`gpu.rs:300,347`). This affects whether projects look the same after the switch. |
| Groups: new, leave, opacity, show/hide, collapse, move, ungroup, merge, delete | `LayersPanel.svelte` group header; `addGroup` … `mergeGroup` | Missing. Groups in an opened project aren't read (save writes the engine's groups: `save.rs:183`). | none in `load.rs` | M | Engine `lib.rs:1134-1413`. Does the GPU compositor handle group opacity? |
| Adjustment layers (curves, levels, colour, gradient map over what's below; click to change) | `LayersPanel.svelte` "New adjustment layer"; `tool-options.svelte.ts` `setKeepEditable` / `adjustmentLayer` | Missing; not loaded from files | engine `lib.rs:3914-4021` | M | Composited on the CPU (`gpu.rs:13`): check scrubbing and drawing at 4K with 20 layers. |
| Layer styles: stroke (colour, position, size, opacity), drop shadow (colour, opacity, angle, distance, blur), outer glow (colour, size, opacity) | `LayersPanel.svelte` "Layer style"; `layer-style.ts` | Missing; not drawn (the file keeps them: `save.rs:3-4`) | engine `style.rs` `render`, `lib.rs:5375` | M | Also CPU-composited: a performance risk. |
| Import a picture as a new layer | `LayersPanel.svelte` import; `PaintCanvas` `importImage` | Missing (File › Import… does nothing) | `header.rs:30` | S | Engine `lib.rs:3849` `add_image_layer`. Limits: 30 MB, 32 layers. |
| Production stage per layer; sort into stages; new layer from this one in a later stage | `LayersPanel.svelte` Stage, "Sort layers into stages", "New layer from this" | Missing | none | M | Probably covered in the stages / workspaces parity file. Listed so it isn't lost. |
| Convert to pixels / rasterise (line and text layers) | `LayersPanel.svelte` "Convert to pixels", "Rasterize" | Missing | engine `lines.rs:487`, `text.rs:187` | S | Depends on line and text layers. |

## 12. Line (vector) layers and editable strokes

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Line layers: every stroke stays editable, drawn by the same brush engine | `line-layers.svelte.ts`, `lines.ts`; `LayersPanel.svelte` "New line layer" | Missing. Line data in opened projects is kept on save but not used: lines show as the layer's pixels. | `save.rs:3`; engine `lines.rs:459` `add_line_layer`, `:949` `lines_stroke` | L | Drawing on such a layer in native, then reopening it in the current app, could put strokes and pixels out of step. Check before the switch. |
| Editable strokes on plain layers (recent strokes stay pickable until other paint lands on them) | `line-layers.svelte.ts` (`bakePlan`, bases) | Missing | engine `lines.rs:593` `base_begin` … | L | Shares the work with the row above. |
| Select line tool: tap or Shift+tap lines, drag a box | tool `lines` (Shift+V); `ToolOptionsBar` | Missing | none | M | |
| Picked lines: move, scale, turn (Shift constraints), drag points to reshape, arrow keys to nudge, Delete | `lines.ts` `dragHandle`, `handles`; `PaintCanvas` `transformLinesStep` | Missing | none | M | Budget: moving 2,000 strokes at the display's rate, done within 100 ms of letting go. This is where the current app froze. |
| Picked lines take a new colour, size, opacity or brush | `ToolOptionsBar` (lines picked) | Missing | none | S | |
| Line eraser: Normal (cuts lines into pieces) or Whole lines | `lineEraser`; `lines.ts` `eraseCut`, `eraseWhole` | Missing | none | M | |
| Copy and paste strokes as strokes | `lines.ts` `pasteCopies` | Missing | none | S | |

## 13. Colour

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Saturation/value square, hue, hex | `ColorPanel.svelte` | Done | `tools.rs:358` `popup`; `panel.rs:339` `colour_field` | – | |
| Recent colours | `ColorPanel.svelte` Recent | Done (8) | `tools.rs:287` `remember` | – | |
| Project palette (pick from it; it saves with the project) | `Swatches.svelte`, `ColorPanel.svelte` "My palette" | Done | `panel.rs:468` `named_palette`; `engine/files/load.rs:331` | – | |
| Add a colour to the palette; remove colours | `ColorPanel.svelte` `addMine`, `removeMine` | Missing ("Edit colour…", "Rename…", "Delete" do nothing) | `panel.rs:1095-1106` | S | |
| Colour harmony suggestions | `harmony.ts` `HARMONIES`; `ColorPanel.svelte` | Missing | none | S | |
| Eyedropper tool (what you see) | tool `picker` (I) | Done | `engine/tools/mod.rs:646` `pick` | – | |
| Alt+click eyedropper while painting | `PaintCanvas` (PaintCanvas:4061) | Missing (Alt takes away from a selection) | none | S | |

## 14. History

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Undo / redo (Ctrl+Z, Ctrl+Shift+Z, header buttons) | the page `undo` / `redo` | Done | `header.rs` (undo/redo buttons); `mod.rs:633`; `engine/mod.rs:288` | – | |
| History panel: named steps, click to go back or forward several | `HistoryPanel.svelte`; the page `historyGo` | Missing | none | S | Native steps have no names yet. The engine has undo notes (`lib.rs:2593`). |

## 15. Canvas and view

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Zoom at the pointer, + / −, fit (0), 100 % (1) | `PaintCanvas` `zoomIn` / `zoomOut` / `resetView` | Done | `view.rs:121`; `canvas.rs:229-245` | – | |
| Pan: Space, middle button, Hand | `PaintCanvas` `startPan` | Done | `mod.rs:433` | – | |
| Pan with Ctrl+drag | `PaintCanvas` (PaintCanvas:4023) | Missing | none | S | |
| Turn the view: drag, 15° steps | `PaintCanvas` `rotateBy` | Done | `mod.rs:475-504`; `canvas.rs:262` | – | |
| Turn and straighten keys (4, 6, 5) | `shortcuts.ts` | Missing | none | S | The native View menu lists R for Rotate view (`header.rs:45`). The current app uses R for flipping frames. |
| Mirror the view (H) | `toggleMirror` | Done | `view.rs:143`; `canvas.rs:235` | – | |
| Wheel options: scroll or zoom, zoom speed, smooth zoom, zoom to the pointer | `view-prefs.ts` | Partial: the wheel always zooms over the drawing | `mod.rs:542` | S | Design question 6. Probably a settings row too. |
| Brush outline cursor (crosshair when small; shown while painting) | `view-prefs.ts` `cursor`, `crosshairWhenSmall`, `cursorWhilePainting` | Missing | none | S | Drawn on the overlay each move: it must not cost the pen's frame. |
| Canvas size: presets, custom size, anchor (crop or more room) or scale to fit. Applies to every drawing, mask, camera, text box and guide, in one undo step. | `CanvasSizePicker.svelte`, `canvas-size.ts`; `apps/studio/src/lib/canvas-resize.ts`; `PaintCanvas` `resizeCanvas` | Missing | engine `lib.rs:2771` `resize_canvas` | M | Cameras are timeline data (sibling file). |
| Crop to selection | the page `cropToSelection` (+page.svelte:2241) | Missing | none | S | After canvas size. |
| Touch: pinch to zoom and turn, finger pan, palm rejection after a pen, two-finger tap undo, three-finger tap redo, long-press menu | `PaintCanvas` (PaintCanvas:457-461, 1492-1522, 3717) | Missing (`crates/window` reads pen, pointer and scroll, but not touch) | `crates/window/src/input.rs` | M | Matters for Windows tablets and 2-in-1s. |

## 16. References, tracing, imports and timelapse

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Reference pictures: import, kept in the project (up to 200), remove | `apps/studio/src/lib/references.svelte.ts`, `ReferencePanel.svelte` | Missing (save keeps them) | `save.rs:3` | M | |
| Reference viewer: in a corner, beside the canvas (split) or in its own window; resize; show/hide (Alt+R); previous/next; zoom; flip | `RefViewer.svelte`, `apps/studio/src/lib/RefSplit.svelte` | Missing | none | M | A window of its own needs the multi-window support in `crates/window`. Never on the user's display in tests. |
| Reference tools: values (grey) view, blur, canvas grid on the picture, pick a colour from it | `RefViewer.svelte` | Missing | none | S | |
| Reference drawing aids: lay the drawing over the picture; draw on the picture and the line lands on the canvas; mark a unit; find an angle | `RefViewer.svelte` | Missing | none | M | Measure units depend on this. |
| Open a layered `.psd` / `.psb` (layers, groups, masks) | `psd-read.ts`; the page's File menu ("Open a … file", +page.svelte:4245) | Missing | none | M | Native exports PSD (`engine/export/psd.rs`). Reading it needs masks and groups (section 11) to mean anything. |
| Image sequence / video frames in as drawings on a new layer, to trace (rotoscoping) | `apps/studio/src/lib/rotoscope.ts`; Import menu | Missing | none | M | Shared with animation import (sibling file). FFmpeg is there for video. |
| Timelapse: record, replay, film of the drawing (timelapse or one frame drawn live) | `timelapse.ts`, `TimelapsePlayer.svelte`; header button (+page.svelte:4614) | Partial: "Record timelapse" toggles a flag; nothing records | `engine/tools/mod.rs:165` ("only remembered for now"), `:510` | M | Recording must stay off the pen's path. The film export goes with the export file. |

## Design questions for the owner

1. **Paste:** the current app pastes as a **new layer**, then moves it. The native app floats the paste over the
   **active layer** (and adds Paste in place). Which one does the new design want?
2. **Lasso:** the native app has one lasso. It selects in Draw and fills only in the Colour workspace. The current
   app has Lasso fill (F, with an Erase mode) and Lasso select (L) everywhere. Is lasso fill in Draw (and
   erasing with it) dropped?
3. **Shapes:** the native app has a drag-out Shape tool (line, rectangle, ellipse on U). The current app has
   hold-to-snap shapes with editable points, and uses U for Measure. Keep both? Which key?
4. **Quick menu:** right-click on the canvas used to give Undo, Redo, Eyedropper, Brush↔Eraser, Mirror and
   Deselect. In the new design it's a canvas menu (guides, symmetry, view, timelapse). Is the quick action set
   gone for good, including on touch (long-press)?
5. **Second-tier tools:** the Draw rail shows 8 tools. Smudge, Clone/heal, Gradient, Pen path, Liquify,
   Measure, Magic wand, Select line and Adjust have no place in the artboards. Where do they live: a rail
   overflow, the tool's right-click menu, a "more tools" pop-up?
6. **The wheel:** native zooms with the plain wheel over the drawing. The current app scrolls by default
   (zooms with Ctrl) and has a setting. Keep the setting?
7. **Steady lines:** one shared value (native) or one per tool: brush, eraser, smudge (current)?
8. **Brush size:** the native slider stops at 60 px. The current app goes to 6400 %. Is a bigger range wanted
   in the pop-up, or only through keys and drag-resize?
9. **Brush and palette menu items that are new:** "Export brush…", "Swap it everywhere…", "Lock this colour" and
   the layer menu's "Show as outlines" are in the native menus, but the current app has no such features (it
   has "select the current colour", not swap or lock). Are they Phase 7 scope, or should they be hidden until
   built?
10. **Fill options:** the native fill pop-up has opacity, gap closing and "under the lines" only. Do tolerance,
    "this layer only" and "lines from" another layer come back? Where?
11. **Layer controls in the decluttered panel:** blend mode, clipping, alpha lock, masks, groups, styles,
    adjustment layers, merge and rename have no place in the artboards. Should they go on a layer row's
    right-click menu, or in a layer settings pop-up?
12. **Line layers and editable strokes:** these are the largest single item (L+L). Are they kept as in the current
    app, and on plain layers too?
13. **Text:** native types into pixels. Are editable text layers (fonts, styling, re-editing) required for
    the switch?
14. **Eraser:** one fixed hard eraser, or erasing with any brush, as today?
15. **Grid:** fixed at 64 px in native. Bring back grid size, isometric, perspective and the ruler under the
    canvas menu's "Guides ›" (its hint already promises Perspective and Rulers)?
16. **Composition guides and their creator:** keep them, or drop them as rarely used?

## Totals

One row per capability: 143 rows.

| Status | Rows |
|---|---|
| Done | 30 |
| Partial | 19 |
| Missing | 94 |

| Size of the remaining work (Partial + Missing: 113 rows) | Rows |
|---|---|
| S (under ½ day) | 68 |
| M (1–2 days) | 40 |
| L (more) | 4 |
| No size of its own (text typed into pixels: its rows below carry the work) | 1 |

The 4 L items are the brush editor, editable text layers, line layers, and editable strokes on plain layers.
Done rows carry no size. Rough total: 68 × ½ + 40 × 1½ + 4 × 4 ≈ 110 days of work, before design time.

Many Missing rows are interface and wiring only: the shared engine (`crates/brush`) already does the pixel
work for masks, groups, adjustment layers, styles, the magic wand, feathering, liquify, clone, filters,
curves, canvas size, distort and warp. The main risks against the budgets are:

- the CPU-composited paths (adjustment layers, styles);
- live previews at 4K (Adjust, filters, liquify, warp);
- heavy brushes from the full library;
- moving many line strokes.
