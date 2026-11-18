//! WebAssembly bindings for the PencilPilot grader.

use wasm_bindgen::prelude::*;

/// Grade a drawing against a reference. Both are grayscale, `width * height` bytes.
/// Returns `{ score, measurements }` as a JS object.
#[wasm_bindgen]
pub fn grade(
    width: u32,
    height: u32,
    drawing: Vec<u8>,
    reference: Vec<u8>,
) -> Result<JsValue, JsError> {
    let d =
        grader::Gray::new(width, height, drawing).map_err(|e| JsError::new(&format!("{e:?}")))?;
    let r =
        grader::Gray::new(width, height, reference).map_err(|e| JsError::new(&format!("{e:?}")))?;
    let g = grader::grade(&d, &r).map_err(|e| JsError::new(&format!("{e:?}")))?;
    serde_wasm_bindgen::to_value(&g).map_err(|e| JsError::new(&e.to_string()))
}
