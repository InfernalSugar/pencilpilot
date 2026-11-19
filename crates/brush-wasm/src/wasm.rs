//! JavaScript bindings (wasm-bindgen).

use hokusai::{Brush, BrushSetting};
use wasm_bindgen::prelude::*;

use crate::{Blend, LayerProps, Painter, Symmetry, SymmetryMode, set_brush_rgb};

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
    /// then call `commitLayer` with the settings from before the preview.
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
    ) -> bool {
        let props = layer_props(name, visible, opacity, blend, locked, alpha_lock, clip);
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
    ) -> bool {
        let before = layer_props(name, visible, opacity, blend, locked, alpha_lock, clip);
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
    /// contrast (−1…1), blur radius (px), and an optional recolour (sRGB 0–1).
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
    ) {
        self.inner.adjust_preview(crate::adjust::ColourAdjust {
            hue,
            saturation,
            brightness,
            contrast,
            blur,
            recolour: recolour.then_some([r, g, b]),
        });
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
        use crate::adjust::{Gradient, GradientKind};
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
    ) -> bool {
        self.inner
            .fill(x, y, [r, g, b], opacity, tolerance, all_layers, grow, gap)
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

    /// Forget the undo/redo history.
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

fn layer_props(
    name: &str,
    visible: bool,
    opacity: f32,
    blend: &str,
    locked: bool,
    alpha_lock: bool,
    clip: bool,
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
    }
}
