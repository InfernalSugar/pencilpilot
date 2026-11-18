//! JavaScript bindings (wasm-bindgen).

use hokusai::{Brush, BrushSetting};
use wasm_bindgen::prelude::*;

use crate::{Painter, set_brush_rgb};

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

    #[wasm_bindgen(js_name = beginStroke)]
    pub fn begin_stroke(&mut self) {
        self.inner.begin_stroke();
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

    #[wasm_bindgen(js_name = endStroke)]
    pub fn end_stroke(&mut self, brush: &PaintBrush) {
        self.inner.end_stroke(&brush.inner);
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn undo(&mut self) -> bool {
        self.inner.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.inner.redo()
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
