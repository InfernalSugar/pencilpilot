//! JavaScript bindings (wasm-bindgen).

use hokusai::{Brush, BrushSetting};
use wasm_bindgen::prelude::*;

use crate::{Blend, LayerProps, Painter, set_brush_rgb};

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
        record: bool,
    ) -> bool {
        let props = layer_props(name, visible, opacity, blend, locked, alpha_lock);
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
    ) -> bool {
        let before = layer_props(name, visible, opacity, blend, locked, alpha_lock);
        self.inner.commit_layer_props(id, before)
    }

    /// Straight-alpha RGBA8 preview of one layer, `w`×`h`.
    #[wasm_bindgen(js_name = layerThumbnail)]
    pub fn layer_thumbnail(&self, id: u32, w: u32, h: u32) -> Vec<u8> {
        self.inner.layer_thumbnail(id, w, h)
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
) -> LayerProps {
    LayerProps {
        name: name.to_string(),
        visible,
        opacity,
        blend: Blend::parse(blend).unwrap_or_default(),
        locked,
        alpha_lock,
    }
}
