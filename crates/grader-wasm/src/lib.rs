//! WebAssembly bindings for the PencilPilot grader.

use serde::Serialize;
use wasm_bindgen::prelude::*;

fn js_err(e: impl std::fmt::Debug) -> JsError {
    JsError::new(&format!("{e:?}"))
}

fn to_js<T: Serialize>(v: &T) -> Result<JsValue, JsError> {
    v.serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Grade a drawing against a reference. Both are grayscale, `width * height`
/// bytes (0 = black, 255 = white).
///
/// `options` is `undefined`/`null`, an object, or a JSON string:
/// `{ category?, weights?, curve?, strokes?, keypoints? }` (camelCase, see
/// `grader::GradeOptions`). Returns the `grader::Grade` as a plain JS object
/// (`None` becomes `null`).
#[wasm_bindgen]
pub fn grade(
    width: u32,
    height: u32,
    drawing: Vec<u8>,
    reference: Vec<u8>,
    options: JsValue,
) -> Result<JsValue, JsError> {
    let opts: grader::GradeOptions = if options.is_undefined() || options.is_null() {
        grader::GradeOptions::default()
    } else if let Some(s) = options.as_string() {
        serde_json::from_str(&s).map_err(|e| JsError::new(&format!("bad options: {e}")))?
    } else {
        serde_wasm_bindgen::from_value(options)
            .map_err(|e| JsError::new(&format!("bad options: {e}")))?
    };
    let d = grader::Gray::new(width, height, drawing).map_err(js_err)?;
    let r = grader::Gray::new(width, height, reference).map_err(js_err)?;
    let g = grader::grade_with(&d, &r, &opts).map_err(js_err)?;
    to_js(&g)
}

/// Default weights for a category id (`anime`, `real-life`, `animals`,
/// `environment`, `fundamentals`).
#[wasm_bindgen(js_name = defaultWeights)]
pub fn default_weights(category: &str) -> Result<JsValue, JsError> {
    let c: grader::Category = serde_json::from_value(serde_json::Value::String(category.into()))
        .map_err(|e| JsError::new(&format!("unknown category: {e}")))?;
    to_js(&grader::Weights::for_category(c))
}

#[derive(serde::Deserialize)]
struct NamedValue {
    name: String,
    value: f32,
}

/// Turn a grade's `measurements` (`[{name, value}]`) into ranked tips:
/// `{ fixes: Item[], praise: Item | null }`. `seed` picks tip wordings
/// (use something stable per attempt, e.g. its timestamp).
#[wasm_bindgen]
pub fn feedback(measurements: JsValue, category: &str, seed: u32) -> Result<JsValue, JsError> {
    let ms: Vec<NamedValue> =
        serde_wasm_bindgen::from_value(measurements).map_err(|e| JsError::new(&e.to_string()))?;
    let pairs: Vec<(String, f32)> = ms.into_iter().map(|m| (m.name, m.value)).collect();
    let f = feedback::feedback(feedback::Library::builtin(), &pairs, category, seed);
    f.serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsError::new(&e.to_string()))
}
