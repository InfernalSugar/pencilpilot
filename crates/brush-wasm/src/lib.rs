//! JavaScript bindings (wasm-bindgen).

use hokusai::{Brush, BrushSetting};
use wasm_bindgen::prelude::*;

use brush::{Blend, LayerProps, Painter, Symmetry, SymmetryMode, set_brush_rgb};

/// A MyPaint brush loaded from a `.myb` file.
#[wasm_bindgen]
pub struct PaintBrush {
    inner: Brush,
    /// The brush's own radius (log2 px) so size changes are relative to it.
    natural_radius: f32,
}

#[wasm_bindgen]
impl PaintBrush {
    #[wasm_bindgen(constructor)]
    pub fn new(myb_json: &str) -> Result<PaintBrush, JsError> {
        let inner = hokusai::myb::from_str(myb_json).map_err(|e| JsError::new(&e.to_string()))?;
        let natural_radius = inner.get(BrushSetting::Radius).base_value;
        Ok(Self {
            inner,
            natural_radius,
        })
    }

    /// Colour in sRGB 0–255.
    #[wasm_bindgen(js_name = setColor)]
    pub fn set_color(&mut self, r: u8, g: u8, b: u8) {
        set_brush_rgb(&mut self.inner, r, g, b);
    }

    /// Size multiplier relative to the brush's designed size (1 = as designed).
    #[wasm_bindgen(js_name = setSizeScale)]
    pub fn set_size_scale(&mut self, scale: f32) {
        self.inner.get_mut(BrushSetting::Radius).base_value =
            self.natural_radius + scale.max(0.05).log2();
    }

    /// Opacity multiplier 0–1.
    #[wasm_bindgen(js_name = setOpacity)]
    pub fn set_opacity(&mut self, opacity: f32) {
        self.inner.get_mut(BrushSetting::OpaqueMultiply).base_value = opacity.clamp(0.0, 1.0);
    }

    /// The brush's base radius in canvas pixels at its current size (for the
    /// brush outline cursor). Pressure and other inputs can still vary a dab.
    pub fn radius(&self) -> f32 {
        self.inner
            .get(BrushSetting::Radius)
            .base_value
            .exp()
            .clamp(0.2, 1000.0)
    }

    /// Turn this brush into an eraser (keeps its shape and texture).
    #[wasm_bindgen(js_name = setEraser)]
    pub fn set_eraser(&mut self, on: bool) {
        self.inner.get_mut(BrushSetting::Eraser).base_value = if on { 1.0 } else { 0.0 };
    }
}

/// The drawing surface.
#[wasm_bindgen]
pub struct PaintCanvas {
    inner: Painter,
}

#[wasm_bindgen]
impl PaintCanvas {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32) -> PaintCanvas {
        Self {
            inner: Painter::new(width, height),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.inner.width()
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.inner.height()
    }

    /// Start a stroke on the active layer; false if it is locked or hidden.
    #[wasm_bindgen(js_name = beginStroke)]
    pub fn begin_stroke(&mut self) -> bool {
        self.inner.begin_stroke()
    }

    /// Start a clone stroke (the Clone tool): it copies from (dx, dy) away,
    /// from this layer or what you see; `heal` blends it into where it lands.
    #[wasm_bindgen(js_name = beginCloneStroke)]
    pub fn begin_clone_stroke(&mut self, dx: f32, dy: f32, all_layers: bool, heal: bool) -> bool {
        self.inner.begin_clone_stroke(dx, dy, all_layers, heal)
    }

    #[wasm_bindgen(js_name = strokeTo)]
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_to(
        &mut self,
        brush: &PaintBrush,
        x: f32,
        y: f32,
        pressure: f32,
        xtilt: f32,
        ytilt: f32,
        dtime: f64,
    ) {
        self.inner
            .stroke_to(&brush.inner, x, y, pressure, xtilt, ytilt, dtime);
    }

    /// Abandon the stroke in progress without a history step.
    #[wasm_bindgen(js_name = cancelStroke)]
    pub fn cancel_stroke(&mut self) {
        self.inner.cancel_stroke();
    }

    #[wasm_bindgen(js_name = endStroke)]
    pub fn end_stroke(&mut self, brush: &PaintBrush) {
        self.inner.end_stroke(&brush.inner);
    }

    /// Erase the active layer. False if there was nothing to erase (no history step).
    pub fn clear(&mut self) -> bool {
        self.inner.clear()
    }

    // ---- layers ----

    /// `[{id, name, visible, opacity, blend, locked, alphaLock, empty}]`, bottom → top.
    #[wasm_bindgen(js_name = layersJson)]
    pub fn layers_json(&self) -> String {
        self.inner.layers_json()
    }

    #[wasm_bindgen(js_name = activeLayer, getter)]
    pub fn active_layer(&self) -> u32 {
        self.inner.active_layer()
    }

    #[wasm_bindgen(js_name = setActiveLayer)]
    pub fn set_active_layer(&mut self, id: u32) -> bool {
        self.inner.set_active_layer(id)
    }

    /// New empty layer above the active one; returns its id (0 = limit reached).
    #[wasm_bindgen(js_name = addLayer)]
    pub fn add_layer(&mut self, name: &str) -> u32 {
        self.inner.add_layer(name)
    }

    #[wasm_bindgen(js_name = duplicateLayer)]
    pub fn duplicate_layer(&mut self, id: u32) -> u32 {
        self.inner.duplicate_layer(id)
    }

    #[wasm_bindgen(js_name = deleteLayer)]
    pub fn delete_layer(&mut self, id: u32) -> bool {
        self.inner.delete_layer(id)
    }

    /// Move to position `to` (0 = bottom).
    #[wasm_bindgen(js_name = moveLayer)]
    pub fn move_layer(&mut self, id: u32, to: usize) -> bool {
        self.inner.move_layer(id, to)
    }

    #[wasm_bindgen(js_name = mergeDown)]
    pub fn merge_down(&mut self, id: u32) -> bool {
        self.inner.merge_down(id)
    }

    /// Change a layer's settings. `record` = false previews (no undo step);
    /// then call `commitLayer` with the settings from before the preview. `style`:
    /// its layer style as numbers (see `brush::style::LayerStyle::from_numbers`;
    /// empty = none).
    #[wasm_bindgen(js_name = setLayer)]
    #[allow(clippy::too_many_arguments)]
    pub fn set_layer(
        &mut self,
        id: u32,
        name: &str,
        visible: bool,
        opacity: f32,
        blend: &str,
        locked: bool,
        alpha_lock: bool,
        clip: bool,
        record: bool,
        style: &[f32],
    ) -> bool {
        let props = layer_props(
            name, visible, opacity, blend, locked, alpha_lock, clip, style,
        );
        self.inner.set_layer_props(id, props, record)
    }

    /// Record previewed changes as one undo step; pass the settings from before the preview.
    #[wasm_bindgen(js_name = commitLayer)]
    #[allow(clippy::too_many_arguments)]
    pub fn commit_layer(
        &mut self,
        id: u32,
        name: &str,
        visible: bool,
        opacity: f32,
        blend: &str,
        locked: bool,
        alpha_lock: bool,
        clip: bool,
        style: &[f32],
    ) -> bool {
        let before = layer_props(
            name, visible, opacity, blend, locked, alpha_lock, clip, style,
        );
        self.inner.commit_layer_props(id, before)
    }

    /// Straight-alpha RGBA8 preview of one layer, `w`×`h`.
    #[wasm_bindgen(js_name = layerThumbnail)]
    pub fn layer_thumbnail(&self, id: u32, w: u32, h: u32) -> Vec<u8> {
        self.inner.layer_thumbnail(id, w, h)
    }

    /// Add a layer holding a picture (straight-alpha sRGB RGBA8, canvas size). Returns its id (0 = couldn't).
    #[wasm_bindgen(js_name = addImageLayer)]
    pub fn add_image_layer(&mut self, name: &str, rgba: &[u8]) -> u32 {
        self.inner.add_image_layer(name, rgba)
    }

    /// Bring back a saved layer (drawing history); `imported` keeps that mark.
    #[wasm_bindgen(js_name = restoreLayer)]
    pub fn restore_layer(&mut self, name: &str, rgba: &[u8], imported: bool) -> u32 {
        self.inner.restore_layer(name, rgba, imported)
    }

    /// Give a layer a saved mask (one byte per canvas pixel, 255 = shown).
    #[wasm_bindgen(js_name = restoreMask)]
    pub fn restore_mask(&mut self, id: u32, gray: &[u8]) -> bool {
        self.inner.restore_mask(id, gray)
    }

    // ---- adjustments and gradients ----

    /// Start adjusting the active layer (or a gradient on it); false if it can't be.
    #[wasm_bindgen(js_name = adjustBegin)]
    pub fn adjust_begin(&mut self) -> bool {
        self.inner.adjust_begin()
    }

    #[wasm_bindgen(js_name = isAdjusting)]
    pub fn is_adjusting(&self) -> bool {
        self.inner.is_adjusting()
    }

    /// Preview colour adjustments: hue (degrees), saturation / brightness /
    /// contrast (−1…1), blur radius (px), an optional recolour (sRGB 0–1), curves
    /// / levels (`tone`: 768 bytes, a 256-entry table for R, G then B) and a
    /// gradient map (`map`: 768 bytes, 256 RGB colours from dark to light); an
    /// empty `tone` / `map` = none.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = adjustPreview)]
    pub fn adjust_preview(
        &mut self,
        hue: f32,
        saturation: f32,
        brightness: f32,
        contrast: f32,
        blur: f32,
        recolour: bool,
        r: f32,
        g: f32,
        b: f32,
        tone: &[u8],
        map: &[u8],
    ) {
        self.inner.adjust_preview(brush::adjust::ColourAdjust {
            blur,
            recolour: recolour.then_some([r, g, b]),
            ..colour_adjust(hue, saturation, brightness, contrast, tone, map)
        });
    }

    /// Preview a filter: `kind` is sharpen (a amount, b radius), motion (a angle,
    /// b distance), noise (a amount, `mono`, `seed`), halftone (a size, `mono`),
    /// chromatic (a amount), bloom (a threshold, b size, c strength) or glitch (a
    /// amount, b block size, `seed`). False for an unknown kind.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = filterPreview)]
    pub fn filter_preview(
        &mut self,
        kind: &str,
        a: f32,
        b: f32,
        c: f32,
        mono: bool,
        seed: u32,
    ) -> bool {
        let Some(f) = brush::filters::Filter::from_parts(kind, a, b, c, mono, seed) else {
            return false;
        };
        self.inner.filter_preview(f);
        true
    }

    /// Add an adjustment layer above the active one: hue (degrees), saturation /
    /// brightness / contrast (−1…1), curves (`tone`) and gradient map (`map`) as
    /// for `adjustPreview`. `spec` is kept as given (`layersJson` returns it).
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = addAdjustmentLayer)]
    pub fn add_adjustment_layer(
        &mut self,
        name: &str,
        spec: &str,
        hue: f32,
        saturation: f32,
        brightness: f32,
        contrast: f32,
        tone: &[u8],
        map: &[u8],
    ) -> u32 {
        let a = colour_adjust(hue, saturation, brightness, contrast, tone, map);
        self.inner.add_adjustment_layer(name, a, spec)
    }

    /// Change an adjustment layer live; `adjustmentCommit` makes it one undo step.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = adjustmentPreview)]
    pub fn adjustment_preview(
        &mut self,
        id: u32,
        spec: &str,
        hue: f32,
        saturation: f32,
        brightness: f32,
        contrast: f32,
        tone: &[u8],
        map: &[u8],
    ) -> bool {
        let a = colour_adjust(hue, saturation, brightness, contrast, tone, map);
        self.inner.adjustment_preview(id, a, spec)
    }

    /// Add an adjustment layer on top without an undo step (a shared one); its id.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = addAdjustmentLayerSilent)]
    pub fn add_adjustment_layer_silent(
        &mut self,
        name: &str,
        spec: &str,
        hue: f32,
        saturation: f32,
        brightness: f32,
        contrast: f32,
        tone: &[u8],
        map: &[u8],
    ) -> u32 {
        let a = colour_adjust(hue, saturation, brightness, contrast, tone, map);
        self.inner.add_adjustment_layer_silent(name, a, spec)
    }

    /// Change an adjustment layer without an undo step (changed elsewhere).
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = setAdjustmentSilent)]
    pub fn set_adjustment_silent(
        &mut self,
        id: u32,
        spec: &str,
        hue: f32,
        saturation: f32,
        brightness: f32,
        contrast: f32,
        tone: &[u8],
        map: &[u8],
    ) -> bool {
        let a = colour_adjust(hue, saturation, brightness, contrast, tone, map);
        self.inner.set_adjustment_silent(id, a, spec)
    }

    /// A layer's mask as packed tiles; empty when it has none.
    #[wasm_bindgen(js_name = maskTilesPacked)]
    pub fn mask_tiles_packed(&self, id: u32) -> Vec<u8> {
        self.inner.mask_tiles_packed(id)
    }

    /// Put a layer's mask from packed tiles, no undo step; empty bytes remove it.
    #[wasm_bindgen(js_name = putMaskTiles)]
    pub fn put_mask_tiles(&mut self, id: u32, packed: &[u8]) -> bool {
        self.inner.put_mask_tiles(id, packed)
    }

    #[wasm_bindgen(js_name = adjustmentCommit)]
    pub fn adjustment_commit(&mut self) -> bool {
        self.inner.adjustment_commit()
    }

    /// Store a grey picture (`w`×`h` bytes, 255 = paint) as brush texture `id`.
    #[wasm_bindgen(js_name = setTexture)]
    pub fn set_texture(&mut self, id: u32, w: u32, h: u32, px: &[u8]) -> bool {
        self.inner.set_texture(id, w, h, px.to_vec())
    }

    /// Texture the next strokes (texture ids from `setTexture`; 0 = none): grain
    /// (canvas px per texel, strength 0–1), a tip picture, and a dual brush
    /// (its size and scatter as fractions of the brush radius, stamps per dab).
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = setDabStyle)]
    pub fn set_dab_style(
        &mut self,
        grain: u32,
        grain_scale: f32,
        grain_strength: f32,
        tip: u32,
        dual: u32,
        dual_size: f32,
        dual_scatter: f32,
        dual_count: u32,
    ) {
        self.inner.set_dab_style(
            grain,
            grain_scale,
            grain_strength,
            tip,
            dual,
            dual_size,
            dual_scatter,
            dual_count,
        );
    }

    /// Preview a gradient from (x0, y0) to (x1, y1), colours sRGB 0–1 with alpha.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = gradientPreview)]
    pub fn gradient_preview(
        &mut self,
        radial: bool,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        r0: f32,
        g0: f32,
        b0: f32,
        a0: f32,
        r1: f32,
        g1: f32,
        b1: f32,
        a1: f32,
    ) {
        use brush::adjust::{Gradient, GradientKind};
        self.inner.gradient_preview(Gradient {
            kind: if radial {
                GradientKind::Radial
            } else {
                GradientKind::Linear
            },
            from: (x0, y0),
            to: (x1, y1),
            c0: [r0, g0, b0],
            a0,
            c1: [r1, g1, b1],
            a1,
        });
    }

    /// Keep the preview (one undo step); false if nothing changed.
    #[wasm_bindgen(js_name = adjustCommit)]
    pub fn adjust_commit(&mut self) -> bool {
        self.inner.adjust_commit()
    }

    #[wasm_bindgen(js_name = adjustCancel)]
    pub fn adjust_cancel(&mut self) {
        self.inner.adjust_cancel()
    }

    // ---- layer groups ----

    #[wasm_bindgen(js_name = groupsJson)]
    pub fn groups_json(&self) -> String {
        self.inner.groups_json()
    }

    /// Put a layer in a new group; returns the group's id (0 = couldn't).
    #[wasm_bindgen(js_name = addGroup)]
    pub fn add_group(&mut self, layer: u32) -> u32 {
        self.inner.add_group(layer)
    }

    #[wasm_bindgen(js_name = setGroup)]
    pub fn set_group(
        &mut self,
        id: u32,
        name: &str,
        visible: bool,
        opacity: f32,
        record: bool,
    ) -> bool {
        self.inner.set_group(id, name, visible, opacity, record)
    }

    /// One undo step after previewing: the values from before.
    #[wasm_bindgen(js_name = commitGroup)]
    pub fn commit_group(&mut self, id: u32, name: &str, visible: bool, opacity: f32) -> bool {
        self.inner.commit_group(id, name, visible, opacity)
    }

    pub fn ungroup(&mut self, id: u32) -> bool {
        self.inner.ungroup(id)
    }

    #[wasm_bindgen(js_name = deleteGroup)]
    pub fn delete_group(&mut self, id: u32) -> bool {
        self.inner.delete_group(id)
    }

    #[wasm_bindgen(js_name = mergeGroup)]
    pub fn merge_group(&mut self, id: u32) -> u32 {
        self.inner.merge_group(id)
    }

    #[wasm_bindgen(js_name = moveGroup)]
    pub fn move_group(&mut self, id: u32, up: bool) -> bool {
        self.inner.move_group(id, up)
    }

    /// Move a layer to `to` (0 = bottom) and into `group` (0 = none), one undo step.
    #[wasm_bindgen(js_name = moveLayerTo)]
    pub fn move_layer_to(&mut self, id: u32, to: usize, group: u32) -> bool {
        self.inner.move_layer_to(id, to, group)
    }

    /// A layer's own pixels, full size (straight-alpha sRGB RGBA8, mask not applied).
    #[wasm_bindgen(js_name = layerRgba)]
    pub fn layer_rgba(&self, id: u32) -> Vec<u8> {
        self.inner.layer_rgba(id)
    }

    /// A layer as it shows, full size (straight-alpha sRGB RGBA8): its mask and style
    /// baked in. With `drawing` (not 0), that drawing on the layer instead.
    #[wasm_bindgen(js_name = layerStyledRgba)]
    pub fn layer_styled_rgba(&self, id: u32, drawing: u32) -> Vec<u8> {
        self.inner.layer_styled_rgba(id, drawing)
    }

    /// Greyscale RGBA8 preview of a layer's mask (empty if it has none).
    #[wasm_bindgen(js_name = maskThumbnail)]
    pub fn mask_thumbnail(&self, id: u32, w: u32, h: u32) -> Vec<u8> {
        self.inner.mask_thumbnail(id, w, h)
    }

    #[wasm_bindgen(js_name = addMask)]
    pub fn add_mask(&mut self, id: u32) -> bool {
        self.inner.add_mask(id)
    }

    #[wasm_bindgen(js_name = deleteMask)]
    pub fn delete_mask(&mut self, id: u32) -> bool {
        self.inner.delete_mask(id)
    }

    #[wasm_bindgen(js_name = applyMask)]
    pub fn apply_mask(&mut self, id: u32) -> bool {
        self.inner.apply_mask(id)
    }

    /// Paint on a layer's mask (true) or its colour; makes it the active layer.
    #[wasm_bindgen(js_name = setMaskEditing)]
    pub fn set_mask_editing(&mut self, id: u32, on: bool) -> bool {
        self.inner.set_mask_editing(id, on)
    }

    #[wasm_bindgen(js_name = isEditingMask)]
    pub fn is_editing_mask(&self) -> bool {
        self.inner.is_editing_mask()
    }

    pub fn undo(&mut self) -> bool {
        self.inner.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.inner.redo()
    }

    /// Flood-fill the active layer from (x, y); see `Painter::fill`. One undo step.
    #[allow(clippy::too_many_arguments)]
    pub fn fill(
        &mut self,
        x: u32,
        y: u32,
        r: u8,
        g: u8,
        b: u8,
        opacity: f32,
        tolerance: f32,
        all_layers: bool,
        grow: u32,
        gap: u32,
        reference_layer: u32,
    ) -> bool {
        // 0 = no reference layer (layer ids start at 1)
        let reference = (reference_layer != 0).then_some(reference_layer);
        self.inner.fill_from(
            x,
            y,
            [r, g, b],
            opacity,
            tolerance,
            all_layers,
            grow,
            gap,
            reference,
        )
    }

    /// Paint a coverage mask (one byte per canvas pixel) in this colour: the lasso fill.
    /// A step of the app's own in the history (the timeline changed); `id` > 0.
    #[wasm_bindgen(js_name = pushNote)]
    pub fn push_note(&mut self, id: u32) -> bool {
        self.inner.push_note(id)
    }

    /// The app's step the next undo goes back over (0 = none).
    #[wasm_bindgen(js_name = undoNote)]
    pub fn undo_note(&self) -> u32 {
        self.inner.undo_note()
    }

    /// The app's step the next redo goes forward over (0 = none).
    #[wasm_bindgen(js_name = redoNote)]
    pub fn redo_note(&self) -> u32 {
        self.inner.redo_note()
    }

    /// Join the last two undo steps into one.
    #[wasm_bindgen(js_name = joinLastEdits)]
    pub fn join_last_edits(&mut self) -> bool {
        self.inner.join_last_edits()
    }

    /// Fill the selection with a colour (one undo step).
    #[wasm_bindgen(js_name = fillSelection)]
    pub fn fill_selection(&mut self, r: u8, g: u8, b: u8, opacity: f32) -> bool {
        self.inner.fill_selection([r, g, b], opacity)
    }

    /// The last selection fill is still the latest edit.
    #[wasm_bindgen(js_name = selectionFillLive)]
    pub fn selection_fill_live(&self) -> bool {
        self.inner.selection_fill_live()
    }

    /// Fill again in place of the last selection fill (new colour or selection).
    #[wasm_bindgen(js_name = refillSelection)]
    pub fn refill_selection(&mut self, r: u8, g: u8, b: u8, opacity: f32) -> bool {
        self.inner.refill_selection([r, g, b], opacity)
    }

    #[wasm_bindgen(js_name = fillMask)]
    pub fn fill_mask(&mut self, coverage: &[u8], r: u8, g: u8, b: u8, opacity: f32) -> bool {
        self.inner.fill_mask(coverage, [r, g, b], opacity)
    }

    // ---- symmetry ----

    /// `mode`: 0 off, 1 vertical, 2 horizontal, 3 quadrant, 4 radial (`n` copies); centre (cx, cy).
    #[wasm_bindgen(js_name = setSymmetry)]
    pub fn set_symmetry(&mut self, mode: u8, n: u32, cx: f32, cy: f32) {
        let mode = match mode {
            1 => Some(SymmetryMode::Vertical),
            2 => Some(SymmetryMode::Horizontal),
            3 => Some(SymmetryMode::Quadrant),
            4 => Some(SymmetryMode::Radial(n)),
            _ => None,
        };
        self.inner
            .set_symmetry(mode.map(|mode| Symmetry { mode, cx, cy }));
    }

    // ---- liquify ----

    #[wasm_bindgen(js_name = liquifyBegin)]
    pub fn liquify_begin(&mut self) -> bool {
        self.inner.liquify_begin()
    }

    /// Push pixels around (x, y) by (dx, dy) within `radius`.
    #[wasm_bindgen(js_name = liquifyPush)]
    pub fn liquify_push(&mut self, x: f32, y: f32, dx: f32, dy: f32, radius: f32, strength: f32) {
        self.inner.liquify_push(x, y, dx, dy, radius, strength);
    }

    /// One undo step; false if nothing changed.
    #[wasm_bindgen(js_name = liquifyEnd)]
    pub fn liquify_end(&mut self) -> bool {
        self.inner.liquify_end()
    }

    // ---- transform ----

    /// Lift the selection (or the whole layer); returns its box [x0, y0, x1, y1] or [].
    #[wasm_bindgen(js_name = transformBegin)]
    pub fn transform_begin(&mut self) -> Vec<f32> {
        self.inner.transform_begin()
    }

    /// Preview with matrix dest = (a x + c y + e, b x + d y + f).
    #[wasm_bindgen(js_name = transformPreview)]
    #[allow(clippy::too_many_arguments)]
    pub fn transform_preview(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> bool {
        self.inner.transform_preview([a, b, c, d, e, f])
    }

    /// Preview with a projective matrix (row-major 3×3): distort and perspective.
    #[wasm_bindgen(js_name = transformPreviewH)]
    pub fn transform_preview_h(&mut self, h: &[f32]) -> bool {
        let Ok(h) = <[f32; 9]>::try_from(h) else {
            return false;
        };
        self.inner.transform_preview_h(h)
    }

    /// Warp: `nodes` are (x, y) pairs for a (cols+1)×(rows+1) grid over the lifted box.
    #[wasm_bindgen(js_name = transformPreviewMesh)]
    pub fn transform_preview_mesh(&mut self, nodes: &[f32], cols: u32, rows: u32) -> bool {
        self.inner
            .transform_preview_mesh(nodes, cols as usize, rows as usize)
    }

    #[wasm_bindgen(js_name = transformCommit)]
    pub fn transform_commit(&mut self) -> bool {
        self.inner.transform_commit()
    }

    #[wasm_bindgen(js_name = transformCancel)]
    pub fn transform_cancel(&mut self) {
        self.inner.transform_cancel();
    }

    #[wasm_bindgen(js_name = isTransforming)]
    pub fn is_transforming(&self) -> bool {
        self.inner.transforming()
    }

    // ---- selection (not part of the undo history) ----

    /// Set the selection from a canvas-sized mask (byte > 127 = selected);
    /// `op`: 0 replace, 1 add, 2 subtract, 3 intersect.
    /// Soften the selection's edge over about `radius` px.
    #[wasm_bindgen(js_name = featherSelection)]
    pub fn feather_selection(&mut self, radius: f32) -> bool {
        self.inner.feather_selection(radius)
    }

    #[wasm_bindgen(js_name = setSelection)]
    pub fn set_selection(&mut self, mask: &[u8], op: u8) -> bool {
        self.inner.set_selection(mask, op)
    }

    /// Magic wand at (x, y).
    #[wasm_bindgen(js_name = selectWand)]
    pub fn select_wand(
        &mut self,
        x: u32,
        y: u32,
        tolerance: f32,
        all_layers: bool,
        op: u8,
    ) -> bool {
        self.inner.select_wand(x, y, tolerance, all_layers, op)
    }

    /// Select every pixel of a similar colour (joined up or not); see `selectWand`.
    #[wasm_bindgen(js_name = selectColourRange)]
    pub fn select_colour_range(
        &mut self,
        x: u32,
        y: u32,
        tolerance: f32,
        all_layers: bool,
        op: u8,
    ) -> bool {
        self.inner
            .select_colour_range(x, y, tolerance, all_layers, op)
    }

    /// Select a colour (sRGB) wherever it is, on this layer or in what you see.
    #[wasm_bindgen(js_name = selectColour)]
    #[allow(clippy::too_many_arguments)]
    pub fn select_colour(
        &mut self,
        r: u8,
        g: u8,
        b: u8,
        tolerance: f32,
        all_layers: bool,
        op: u8,
    ) -> bool {
        self.inner
            .select_colour([r, g, b], tolerance, all_layers, op)
    }

    /// Select what's on a layer (soft where it's see-through).
    #[wasm_bindgen(js_name = selectLayer)]
    pub fn select_layer(&mut self, id: u32, op: u8) -> bool {
        self.inner.select_layer(id, op)
    }

    /// Grow the selection by `px` (shrink it with a negative `px`).
    #[wasm_bindgen(js_name = growSelection)]
    pub fn grow_selection(&mut self, px: i32) -> bool {
        self.inner.grow_selection(px)
    }

    /// New canvas size (one undo step): moved by (dx, dy), or with `scale` resized to fit.
    /// Every drawing (an animation's other frames too) and mask changes with it; text
    /// layers `text_ids` get the descriptions in `texts` (joined by U+0001).
    #[wasm_bindgen(js_name = resizeCanvas)]
    #[allow(clippy::too_many_arguments)]
    pub fn resize_canvas(
        &mut self,
        width: u32,
        height: u32,
        dx: i32,
        dy: i32,
        scale: bool,
        text_ids: &[u32],
        texts: &str,
    ) -> bool {
        let specs: Vec<(u32, String)> = if text_ids.is_empty() {
            Vec::new()
        } else {
            text_ids
                .iter()
                .copied()
                .zip(texts.split('\u{1}').map(str::to_string))
                .collect()
        };
        self.inner
            .resize_canvas(width, height, dx, dy, scale, &specs)
    }

    /// The drawings the next undo (`redo` false) or redo of a canvas size changes (none:
    /// the next step isn't one). They must all be here first.
    #[wasm_bindgen(js_name = canvasStepDrawings)]
    pub fn canvas_step_drawings(&self, redo: bool) -> Vec<u32> {
        self.inner.canvas_step_drawings(redo)
    }

    /// Tiles of paint held here (layers, masks and kept drawings).
    #[wasm_bindgen(js_name = tileCount)]
    pub fn tile_count(&self) -> u32 {
        self.inner.tile_count() as u32
    }

    #[wasm_bindgen(js_name = selectAll)]
    pub fn select_all(&mut self) {
        self.inner.select_all();
    }

    pub fn deselect(&mut self) {
        self.inner.deselect();
    }

    #[wasm_bindgen(js_name = invertSelection)]
    pub fn invert_selection(&mut self) {
        self.inner.invert_selection();
    }

    #[wasm_bindgen(js_name = hasSelection)]
    pub fn has_selection(&self) -> bool {
        self.inner.has_selection()
    }

    /// The selection mask (empty when there is none).
    #[wasm_bindgen(js_name = selectionMask)]
    pub fn selection_mask(&self) -> Vec<u8> {
        self.inner.selection_mask()
    }

    /// A layer without an undo step (for another person on a shared canvas).
    #[wasm_bindgen(js_name = addLayerSilent)]
    pub fn add_layer_silent(&mut self, name: &str) -> u32 {
        self.inner.add_layer_silent(name)
    }

    /// Erase a layer without an undo step.
    #[wasm_bindgen(js_name = clearLayerSilent)]
    pub fn clear_layer_silent(&mut self, id: u32) {
        self.inner.clear_layer_silent(id);
    }

    /// Start someone else's stroke `key` on `layer` (alongside the local stroke).
    #[wasm_bindgen(js_name = remoteBegin)]
    pub fn remote_begin(&mut self, key: u32, layer: u32) -> bool {
        self.inner.remote_begin(key, layer)
    }

    #[wasm_bindgen(js_name = remoteTo)]
    pub fn remote_to(
        &mut self,
        key: u32,
        brush: &PaintBrush,
        x: f32,
        y: f32,
        pressure: f32,
        dtime: f64,
    ) {
        self.inner
            .remote_to(key, &brush.inner, x, y, pressure, dtime);
    }

    #[wasm_bindgen(js_name = remoteEnd)]
    pub fn remote_end(&mut self, key: u32, brush: &PaintBrush) {
        self.inner.remote_end(key, &brush.inner);
    }

    // ---- drawings (animation cels) ----

    /// The drawing a layer shows (0 = none yet).
    #[wasm_bindgen(js_name = layerDrawing)]
    pub fn layer_drawing(&self, id: u32) -> u32 {
        self.inner.layer_drawing(id)
    }

    /// Give layers with paint but no drawing one of their own.
    #[wasm_bindgen(js_name = ensureDrawings)]
    pub fn ensure_drawings(&mut self) {
        self.inner.ensure_drawings();
    }

    /// Show other drawings: (layer id, drawing id) pairs, flattened; 0 = an empty cel.
    #[wasm_bindgen(js_name = showDrawings)]
    pub fn show_drawings(&mut self, pairs: &[u32]) -> bool {
        self.inner.show_drawings(pairs)
    }

    #[wasm_bindgen(js_name = newDrawing)]
    pub fn new_drawing(&mut self) -> u32 {
        self.inner.new_drawing()
    }

    /// A boiled copy of a drawing (lines nudged up to `amount` px); its id, 0 if none.
    #[wasm_bindgen(js_name = boilDrawing)]
    pub fn boil_drawing(&mut self, drawing: u32, amount: f32, seed: u32) -> u32 {
        self.inner.boil_drawing(drawing, amount, seed)
    }

    #[wasm_bindgen(js_name = copyDrawing)]
    pub fn copy_drawing(&mut self, drawing: u32) -> u32 {
        self.inner.copy_drawing(drawing)
    }

    /// A drawing's pixels (straight-alpha RGBA8, canvas size).
    #[wasm_bindgen(js_name = drawingRgba)]
    pub fn drawing_rgba(&self, drawing: u32) -> Vec<u8> {
        self.inner.drawing_rgba(drawing)
    }

    #[wasm_bindgen(js_name = drawingEmpty)]
    pub fn drawing_empty(&self, drawing: u32) -> bool {
        self.inner.drawing_empty(drawing)
    }

    /// Load a drawing's pixels (opening a project). Not an undo step.
    #[wasm_bindgen(js_name = putDrawing)]
    pub fn put_drawing(&mut self, drawing: u32, rgba: &[u8]) -> bool {
        self.inner.put_drawing(drawing, rgba)
    }

    /// Forget kept drawings not in `keep` (and not needed by undo).
    #[wasm_bindgen(js_name = dropDrawingsExcept)]
    pub fn drop_drawings_except(&mut self, keep: &[u32]) {
        self.inner.drop_drawings_except(keep);
    }

    /// A drawing as packed file tiles (u32 count; per tile i32 x, i32 y, 64×64 RGBA8).
    #[wasm_bindgen(js_name = drawingTilesPacked)]
    pub fn drawing_tiles_packed(&self, drawing: u32) -> Vec<u8> {
        self.inner.drawing_tiles_packed(drawing)
    }

    /// Load a drawing from packed file tiles. Not an undo step.
    #[wasm_bindgen(js_name = putDrawingTiles)]
    pub fn put_drawing_tiles(&mut self, drawing: u32, packed: &[u8]) -> bool {
        self.inner.put_drawing_tiles(drawing, packed)
    }

    #[wasm_bindgen(js_name = hasDrawing)]
    pub fn has_drawing(&self, drawing: u32) -> bool {
        self.inner.has_drawing(drawing)
    }

    // ---- text layers ----

    /// Start typing on text layer `id`, or (0) on a new layer above the active one.
    /// Returns the layer's id (0 = can't: busy, locked, hidden or no room).
    #[wasm_bindgen(js_name = textBegin)]
    pub fn text_begin(&mut self, id: u32, name: &str) -> u32 {
        self.inner.text_begin(id, name)
    }

    /// The text layer being typed on (0 = none).
    #[wasm_bindgen(js_name = textLayer)]
    pub fn text_layer(&self) -> u32 {
        self.inner.text_layer()
    }

    /// Show the text: the layer becomes `rgba` (straight-alpha, `w`×`h`) at (`x`, `y`).
    /// `spec` is the app's description of it. Not an undo step yet.
    #[wasm_bindgen(js_name = textPreview)]
    #[allow(clippy::too_many_arguments)]
    pub fn text_preview(
        &mut self,
        spec: &str,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        rgba: &[u8],
    ) -> bool {
        self.inner.text_preview(spec, x, y, w, h, rgba)
    }

    /// Keep the text typed (one undo step).
    #[wasm_bindgen(js_name = textCommit)]
    pub fn text_commit(&mut self) -> bool {
        self.inner.text_commit()
    }

    #[wasm_bindgen(js_name = textCancel)]
    pub fn text_cancel(&mut self) {
        self.inner.text_cancel();
    }

    /// Turn a text layer into a plain one (one undo step).
    #[wasm_bindgen(js_name = rasterizeText)]
    pub fn rasterize_text(&mut self, id: u32) -> bool {
        self.inner.rasterize_text(id)
    }

    /// Set a layer's text description, not an undo step (opening a project).
    #[wasm_bindgen(js_name = setTextSilent)]
    pub fn set_text_silent(&mut self, id: u32, spec: &str) -> bool {
        self.inner.set_text_silent(id, spec)
    }

    // ---- line (vector) layers ----

    /// Add an empty line layer above the active one (one undo step). 0 = no room.
    #[wasm_bindgen(js_name = addLineLayer)]
    pub fn add_line_layer(&mut self, name: &str) -> u32 {
        self.inner.add_line_layer(name)
    }

    /// Make a layer a line layer (or not), not an undo step (opening a project,
    /// drawing together).
    #[wasm_bindgen(js_name = setLinesSilent)]
    pub fn set_lines_silent(&mut self, id: u32, on: bool) -> bool {
        self.inner.set_lines_silent(id, on)
    }

    /// "Convert to pixels": a line layer becomes a plain one (one undo step).
    #[wasm_bindgen(js_name = rasterizeLines)]
    pub fn rasterize_lines(&mut self, id: u32) -> bool {
        self.inner.rasterize_lines(id)
    }

    /// Start a line stroke on the active layer, seeded so `linesStroke` can paint it
    /// again (symmetry copy k: `seed + k + 1`). Then `strokeTo`… `endStroke`.
    #[wasm_bindgen(js_name = beginLineStroke)]
    pub fn begin_line_stroke(&mut self, seed: u32) -> bool {
        self.inner.begin_line_stroke(seed)
    }

    /// Start redrawing tiles of a layer: `tiles` = tx, ty, tx, ty… (64 px tiles).
    #[wasm_bindgen(js_name = linesBegin)]
    pub fn lines_begin(&mut self, layer: u32, tiles: &[i32]) -> bool {
        self.inner.lines_begin(layer, tiles)
    }

    /// Replay one whole stroke into the redraw: `pts` = x, y, pressure, xtilt, ytilt,
    /// dtime (seconds) per point.
    #[wasm_bindgen(js_name = linesStroke)]
    pub fn lines_stroke(&mut self, brush: &PaintBrush, seed: u32, pts: &[f32]) {
        self.inner.lines_stroke(&brush.inner, seed, pts);
    }

    /// Put the redrawn tiles on the layer (not an undo step until `linesPush`).
    #[wasm_bindgen(js_name = linesCommit)]
    pub fn lines_commit(&mut self) -> bool {
        self.inner.lines_commit()
    }

    /// The line edits since the last push become one undo step.
    #[wasm_bindgen(js_name = linesPush)]
    pub fn lines_push(&mut self) -> bool {
        self.inner.lines_push()
    }

    /// Keep the line edits since the last push without an undo step.
    #[wasm_bindgen(js_name = linesForget)]
    pub fn lines_forget(&mut self) {
        self.inner.lines_forget();
    }

    /// Undo the line edits since the last push (a cancelled drag).
    #[wasm_bindgen(js_name = linesCancel)]
    pub fn lines_cancel(&mut self) {
        self.inner.lines_cancel();
    }

    #[wasm_bindgen(js_name = linesPending)]
    pub fn lines_pending(&self) -> bool {
        self.inner.lines_pending()
    }

    // ---- editable strokes on plain layers: the pixels under them (bases) ----

    /// Start replaying strokes into a drawing's base (strokes becoming plain pixels);
    /// `linesStroke`… then `linesCommit` (joins the last undo step).
    #[wasm_bindgen(js_name = baseBegin)]
    pub fn base_begin(&mut self, drawing: u32, layer: u32, tiles: &[i32]) -> bool {
        self.inner.base_begin(drawing, layer, tiles)
    }

    /// Forget base tiles no editable stroke needs (joins the last undo step).
    #[wasm_bindgen(js_name = baseDrop)]
    pub fn base_drop(&mut self, drawing: u32, tiles: &[i32]) -> bool {
        self.inner.base_drop(drawing, tiles)
    }

    /// Base tiles other paint changed: `drawing, count, x, y, …` runs.
    #[wasm_bindgen(js_name = takeTainted)]
    pub fn take_tainted(&mut self) -> Vec<i32> {
        self.inner.take_tainted()
    }

    /// Bytes the bases take (drawing 0: all of them).
    #[wasm_bindgen(js_name = baseBytes)]
    pub fn base_bytes(&self, drawing: u32) -> usize {
        self.inner.base_bytes(drawing)
    }

    #[wasm_bindgen(js_name = baseTiles)]
    pub fn base_tiles(&self, drawing: u32) -> usize {
        self.inner.base_tiles(drawing)
    }

    /// Forget the bases of drawings not in `keep`.
    #[wasm_bindgen(js_name = keepBases)]
    pub fn keep_bases(&mut self, keep: &[u32]) {
        self.inner.keep_bases(keep);
    }

    /// A drawing's base, packed (saving).
    #[wasm_bindgen(js_name = basePacked)]
    pub fn base_packed(&self, drawing: u32) -> Vec<u8> {
        self.inner.base_packed(drawing)
    }

    /// Load a drawing's base (opening a project); no undo step.
    #[wasm_bindgen(js_name = putBasePacked)]
    pub fn put_base_packed(&mut self, drawing: u32, packed: &[u8]) -> bool {
        self.inner.put_base_packed(drawing, packed)
    }

    /// A drawing's base changes since the last call (drawing together).
    #[wasm_bindgen(js_name = baseChangesPacked)]
    pub fn base_changes_packed(&mut self, drawing: u32) -> Vec<u8> {
        self.inner.base_changes_packed(drawing)
    }

    /// Someone else's base changes; no undo step.
    #[wasm_bindgen(js_name = putBaseChanges)]
    pub fn put_base_changes(&mut self, drawing: u32, packed: &[u8]) -> bool {
        self.inner.put_base_changes(drawing, packed)
    }

    /// Drawings kept here that aren't showing.
    #[wasm_bindgen(js_name = keptDrawings)]
    pub fn kept_drawings(&self) -> Vec<u32> {
        self.inner.kept_drawings()
    }

    /// Drawings the undo history needs.
    #[wasm_bindgen(js_name = historyDrawings)]
    pub fn history_drawings(&self) -> Vec<u32> {
        self.inner.history_drawings()
    }

    /// Let go of a kept drawing (the frame store has it).
    #[wasm_bindgen(js_name = forgetDrawing)]
    pub fn forget_drawing(&mut self, drawing: u32) -> bool {
        self.inner.forget_drawing(drawing)
    }

    /// Onion skin: (drawing, 0xRRGGBB, opacity 0–255) triples, farthest first, as tinted
    /// lines on transparency, made `shrink` times smaller. Straight-alpha RGBA8.
    #[wasm_bindgen(js_name = onionRgba)]
    pub fn onion_rgba(&self, entries: &[u32], shrink: u32) -> Vec<u8> {
        self.inner.onion_rgba(entries, shrink)
    }

    /// An animation frame as it would look ((layer, drawing) pairs flattened), made
    /// `shrink` times smaller; RGBA8. Changes nothing.
    /// A frame on transparency (straight-alpha RGBA8), for exports with alpha.
    #[wasm_bindgen(js_name = renderFrameAlpha)]
    pub fn render_frame_alpha(&self, pairs: &[u32], shrink: u32) -> Vec<u8> {
        self.inner.render_frame_alpha(pairs, shrink)
    }

    #[wasm_bindgen(js_name = renderFrame)]
    pub fn render_frame(&self, pairs: &[u32], shrink: u32) -> Vec<u8> {
        self.inner.render_frame(pairs, shrink)
    }

    /// The drawing the next undo changes (0 = none).
    #[wasm_bindgen(js_name = undoDrawing)]
    pub fn undo_drawing(&self) -> u32 {
        self.inner.undo_drawing()
    }

    /// The drawing the next redo changes (0 = none).
    #[wasm_bindgen(js_name = redoDrawing)]
    pub fn redo_drawing(&self) -> u32 {
        self.inner.redo_drawing()
    }

    /// Forget the undo/redo history.
    /// Move a layer without an undo step (a shared layer someone else moved); then clear
    /// the history.
    #[wasm_bindgen(js_name = moveLayerSilent)]
    pub fn move_layer_silent(&mut self, id: u32, to: usize) -> bool {
        self.inner.move_layer_silent(id, to)
    }

    /// Remove a layer without an undo step (a shared layer someone else deleted); then
    /// clear the history.
    #[wasm_bindgen(js_name = removeLayerSilent)]
    pub fn remove_layer_silent(&mut self, id: u32) -> bool {
        self.inner.remove_layer_silent(id)
    }

    /// How many undo steps are kept (10–500); fewer drops the oldest now.
    #[wasm_bindgen(js_name = setUndoLimit)]
    pub fn set_undo_limit(&mut self, n: u32) {
        self.inner.set_undo_limit(n as usize);
    }

    #[wasm_bindgen(js_name = clearHistory)]
    pub fn clear_history(&mut self) {
        self.inner.clear_history();
    }

    #[wasm_bindgen(js_name = canUndo)]
    pub fn can_undo(&self) -> bool {
        self.inner.can_undo()
    }

    #[wasm_bindgen(js_name = canRedo)]
    pub fn can_redo(&self) -> bool {
        self.inner.can_redo()
    }

    #[wasm_bindgen(js_name = isBlank)]
    pub fn is_blank(&self) -> bool {
        self.inner.is_blank()
    }

    /// Update the RGBA buffer. Returns `[x, y, w, h]` of the changed area, or
    /// an empty array if nothing changed.
    pub fn flush(&mut self) -> Vec<u32> {
        match self.inner.flush() {
            Some(r) => vec![r.x, r.y, r.w, r.h],
            None => Vec::new(),
        }
    }

    /// Pointer to the RGBA8 buffer in wasm memory (`width * height * 4` bytes).
    #[wasm_bindgen(js_name = rgbaPtr)]
    pub fn rgba_ptr(&self) -> *const u8 {
        self.inner.rgba().as_ptr()
    }
}

#[allow(clippy::too_many_arguments)]
fn layer_props(
    name: &str,
    visible: bool,
    opacity: f32,
    blend: &str,
    locked: bool,
    alpha_lock: bool,
    clip: bool,
    style: &[f32],
) -> LayerProps {
    LayerProps {
        name: name.to_string(),
        visible,
        opacity,
        blend: Blend::parse(blend).unwrap_or_default(),
        locked,
        alpha_lock,
        clip,
        // kept from the layer by the engine
        imported: false,
        adjust: None,
        adjust_spec: String::new(),
        style: brush::style::LayerStyle::from_numbers(style).unwrap_or_default(),
        text: String::new(),
        lines: false,
    }
}

/// An adjustment from its parts (see `adjustPreview`).
fn colour_adjust(
    hue: f32,
    saturation: f32,
    brightness: f32,
    contrast: f32,
    tone: &[u8],
    map: &[u8],
) -> brush::adjust::ColourAdjust {
    brush::adjust::ColourAdjust {
        hue,
        saturation,
        brightness,
        contrast,
        tone: tables(tone),
        map: colour_table(map),
        ..Default::default()
    }
}

/// 768 bytes → three 256-entry tables (R, G, B); anything else → none.
pub(crate) fn tables(b: &[u8]) -> Option<[[u8; 256]; 3]> {
    (b.len() == 768).then(|| {
        let mut t = [[0u8; 256]; 3];
        for (i, row) in t.iter_mut().enumerate() {
            row.copy_from_slice(&b[i * 256..(i + 1) * 256]);
        }
        t
    })
}

/// 768 bytes → 256 RGB colours; anything else → none.
pub(crate) fn colour_table(b: &[u8]) -> Option<[[u8; 3]; 256]> {
    (b.len() == 768).then(|| {
        let mut t = [[0u8; 3]; 256];
        for (i, c) in t.iter_mut().enumerate() {
            c.copy_from_slice(&b[i * 3..i * 3 + 3]);
        }
        t
    })
}
