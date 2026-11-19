//! CLIP ViT-B/32 (OpenAI, MIT) as ONNX, run with `tract` (pure Rust), for
//! **sorting reference pictures only**: which category and topic a picture
//! belongs to (ADR-0009). Never used for scoring, grading or critiquing
//! drawings (ADR-0002). See README.md for provenance.
//!
//! Two towers: [`ClipVision`] turns a picture into a unit-length 512-d
//! embedding, [`ClipText`] does the same for a short prompt. Their dot
//! product is the cosine similarity used for zero-shot matching. The text
//! tower is only needed once, to embed the prompts, and can then be dropped.

use std::collections::HashMap;
use std::path::Path;

use image::{DynamicImage, imageops::FilterType};
use tract_onnx::prelude::*;

use crate::FilterError;

/// Network input side (centre crop after resizing the short side to this).
pub const INPUT: usize = 224;
/// Per-channel mean / std of the CLIP preprocessing (RGB, values 0–1).
pub const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
pub const STD: [f32; 3] = [0.268_629_54, 0.261_302_6, 0.275_777_1];
/// Text context length; prompts are padded with end-of-text to this.
pub const CONTEXT: usize = 77;
/// Embedding size.
pub const DIM: usize = 512;

pub const VISION_FILE: &str = "vision_model.onnx";
pub const TEXT_FILE: &str = "text_model.onnx";
pub const VOCAB_FILE: &str = "vocab.json";
pub const MERGES_FILE: &str = "merges.txt";

const SOT: i64 = 49406;
const EOT: i64 = 49407;

type Plan = std::sync::Arc<TypedSimplePlan>;

fn model_err(e: impl std::fmt::Display) -> FilterError {
    FilterError::Model(format!("CLIP: {e:#}"))
}

fn need(path: &Path) -> Result<(), FilterError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(FilterError::ModelMissing(path.to_path_buf()))
    }
}

/// Load one tower, keeping only the `output` embedding. The exports have a
/// symbolic batch size: declare the input with the model's own symbol, then
/// fix it to 1 so tract can pick its fast kernels.
fn load_plan(
    path: &Path,
    output: &str,
    fact: impl Fn(TDim) -> InferenceFact,
) -> Result<Plan, FilterError> {
    need(path)?;
    let m = tract_onnx::onnx().model_for_path(path).map_err(model_err)?;
    let out = m
        .output_outlets()
        .map_err(model_err)?
        .iter()
        .position(|o| m.outlet_label(*o) == Some(output) || m.node(o.node).name == output)
        .unwrap_or(0);
    let batch = m.symbols.sym("batch_size");
    m.with_input_fact(0, fact(batch.to_dim()))
        .and_then(|m| {
            let o = m.output_outlets()?[out];
            m.with_output_outlets(&[o])
        })
        .and_then(|m| m.into_typed())
        .and_then(|m| m.set_symbols(&[(batch, 1.to_dim())].into_iter().collect()))
        .and_then(|m| m.into_optimized())
        .and_then(|m| m.into_runnable())
        .map_err(model_err)
}

fn unit_output(out: TVec<TValue>) -> Result<Vec<f32>, FilterError> {
    let v = out[0].to_plain_array_view::<f32>().map_err(model_err)?;
    let v: Vec<f32> = v.iter().copied().collect();
    if v.len() != DIM {
        return Err(model_err(format!(
            "embedding has {} values, want {DIM}",
            v.len()
        )));
    }
    Ok(normalise(v))
}

/// The image tower. `Send + Sync`: load once and share.
pub struct ClipVision {
    plan: Plan,
}

impl ClipVision {
    /// Load `VISION_FILE` from `dir`.
    pub fn load(dir: &Path) -> Result<Self, FilterError> {
        let plan = load_plan(&dir.join(VISION_FILE), "image_embeds", |b| {
            f32::fact([b, 3.to_dim(), INPUT.to_dim(), INPUT.to_dim()]).into()
        })?;
        Ok(Self { plan })
    }

    /// Unit-length embedding of a picture.
    pub fn embed(&self, img: &DynamicImage) -> Result<Vec<f32>, FilterError> {
        unit_output(
            self.plan
                .run(tvec!(preprocess(img).into_tvalue()))
                .map_err(model_err)?,
        )
    }
}

/// The text tower plus its tokenizer.
pub struct ClipText {
    plan: Plan,
    tokenizer: Tokenizer,
}

impl ClipText {
    /// Load `TEXT_FILE`, `VOCAB_FILE` and `MERGES_FILE` from `dir`.
    pub fn load(dir: &Path) -> Result<Self, FilterError> {
        need(&dir.join(VOCAB_FILE))?;
        need(&dir.join(MERGES_FILE))?;
        let plan = load_plan(&dir.join(TEXT_FILE), "text_embeds", |b| {
            i64::fact([b, CONTEXT.to_dim()]).into()
        })?;
        let tokenizer = Tokenizer::load(&dir.join(VOCAB_FILE), &dir.join(MERGES_FILE))?;
        Ok(Self { plan, tokenizer })
    }

    /// Unit-length embedding of a short text prompt.
    pub fn embed(&self, prompt: &str) -> Result<Vec<f32>, FilterError> {
        let ids = self.tokenizer.encode(prompt);
        let t = tract_ndarray::Array2::from_shape_vec((1, CONTEXT), ids)
            .expect("CONTEXT ids")
            .into_tensor();
        unit_output(self.plan.run(tvec!(t.into_tvalue())).map_err(model_err)?)
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }
}

/// True if every file the sorter needs is in `dir`.
pub fn present(dir: &Path) -> bool {
    [VISION_FILE, TEXT_FILE, VOCAB_FILE, MERGES_FILE]
        .iter()
        .all(|f| dir.join(f).is_file())
}

fn normalise(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

/// CLIP preprocessing (`CLIPImageProcessor`): transparency onto white, RGB,
/// bicubic resize so the short side is 224, centre crop 224×224, scale to
/// 0–1, subtract `MEAN`, divide by `STD`. NCHW float32 `[1, 3, 224, 224]`.
#[allow(clippy::needless_range_loop)]
pub fn preprocess(img: &DynamicImage) -> Tensor {
    let rgb = crate::flatten_alpha(img).to_rgb8();
    let (w, h) = (rgb.width().max(1), rgb.height().max(1));
    let s = INPUT as f32 / w.min(h) as f32;
    let (nw, nh) = (
        ((w as f32 * s).round() as u32).max(INPUT as u32),
        ((h as f32 * s).round() as u32).max(INPUT as u32),
    );
    let resized = image::imageops::resize(&rgb, nw, nh, FilterType::CatmullRom);
    let (ox, oy) = ((nw - INPUT as u32) / 2, (nh - INPUT as u32) / 2);
    let mut data = vec![0f32; 3 * INPUT * INPUT];
    for y in 0..INPUT {
        for x in 0..INPUT {
            let p = resized.get_pixel(ox + x as u32, oy + y as u32).0;
            for c in 0..3 {
                data[c * INPUT * INPUT + y * INPUT + x] = (p[c] as f32 / 255.0 - MEAN[c]) / STD[c];
            }
        }
    }
    tract_ndarray::Array4::from_shape_vec((1, 3, INPUT, INPUT), data)
        .expect("shape matches data length")
        .into_tensor()
}

/// CLIP's byte-level BPE tokenizer (`openai/CLIP` `simple_tokenizer.py`),
/// enough for short plain-English prompts: lower-cased, whitespace
/// collapsed, no ftfy.
pub struct Tokenizer {
    vocab: HashMap<String, i64>,
    ranks: HashMap<(String, String), usize>,
    byte_chars: [char; 256],
}

impl Tokenizer {
    pub fn load(vocab: &Path, merges: &Path) -> Result<Self, FilterError> {
        let read = |p: &Path| {
            std::fs::read_to_string(p).map_err(|e| model_err(format!("{}: {e}", p.display())))
        };
        let vocab: HashMap<String, i64> = serde_json::from_str(&read(vocab)?).map_err(model_err)?;
        Self::new(vocab, &read(merges)?)
    }

    pub fn new(vocab: HashMap<String, i64>, merges: &str) -> Result<Self, FilterError> {
        let ranks = merges
            .lines()
            .filter(|l| !l.starts_with("#version") && !l.trim().is_empty())
            .enumerate()
            .filter_map(|(i, l)| {
                let (a, b) = l.split_once(' ')?;
                Some(((a.to_string(), b.to_string()), i))
            })
            .collect();
        Ok(Self {
            vocab,
            ranks,
            byte_chars: bytes_to_unicode(),
        })
    }

    /// Token ids with start/end markers, padded with end-of-text (or
    /// truncated) to `CONTEXT`.
    pub fn encode(&self, text: &str) -> Vec<i64> {
        let mut ids = vec![SOT];
        let clean = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        for word in pre_tokenize(&clean) {
            let chars: String = word.bytes().map(|b| self.byte_chars[b as usize]).collect();
            for piece in self.bpe(&chars) {
                if let Some(&id) = self.vocab.get(&piece) {
                    ids.push(id);
                }
            }
        }
        ids.truncate(CONTEXT - 1);
        ids.push(EOT);
        ids.resize(CONTEXT, EOT);
        ids
    }

    fn bpe(&self, word: &str) -> Vec<String> {
        let chars: Vec<char> = word.chars().collect();
        if chars.is_empty() {
            return vec![];
        }
        let mut parts: Vec<String> = chars.iter().map(|c| c.to_string()).collect();
        let last = parts.len() - 1;
        parts[last].push_str("</w>");
        loop {
            let best = parts
                .windows(2)
                .enumerate()
                .filter_map(|(i, w)| {
                    self.ranks
                        .get(&(w[0].clone(), w[1].clone()))
                        .map(|r| (*r, i))
                })
                .min();
            let Some((_, _)) = best else { break };
            let (a, b) = {
                let i = best.unwrap().1;
                (parts[i].clone(), parts[i + 1].clone())
            };
            // merge every occurrence of the pair, left to right
            let mut merged = Vec::with_capacity(parts.len());
            let mut i = 0;
            while i < parts.len() {
                if i + 1 < parts.len() && parts[i] == a && parts[i + 1] == b {
                    merged.push(format!("{a}{b}"));
                    i += 2;
                } else {
                    merged.push(parts[i].clone());
                    i += 1;
                }
            }
            parts = merged;
            if parts.len() == 1 {
                break;
            }
        }
        parts
    }
}

/// CLIP's split pattern: `'s|'t|'re|'ve|'m|'ll|'d|[\p{L}]+|[\p{N}]|[^\s\p{L}\p{N}]+`.
fn pre_tokenize(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '\'' {
            let rest: String = chars[i + 1..].iter().take(2).collect();
            let suffix = ["re", "ve", "ll", "s", "t", "m", "d"]
                .into_iter()
                .find(|s| rest.starts_with(s));
            if let Some(s) = suffix {
                out.push(format!("'{s}"));
                i += 1 + s.len();
                continue;
            }
        }
        let start = i;
        if c.is_alphabetic() {
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
        } else if c.is_numeric() {
            i += 1;
        } else {
            while i < chars.len()
                && !chars[i].is_whitespace()
                && !chars[i].is_alphabetic()
                && !chars[i].is_numeric()
            {
                i += 1;
            }
        }
        out.push(chars[start..i].iter().collect());
    }
    out
}

/// GPT-2/CLIP byte → printable unicode char table.
fn bytes_to_unicode() -> [char; 256] {
    let mut table = ['\0'; 256];
    let mut n = 0u32;
    for b in 0..=255u32 {
        let printable =
            (33..=126).contains(&b) || (161..=172).contains(&b) || (174..=255).contains(&b);
        table[b as usize] = if printable {
            char::from_u32(b).unwrap()
        } else {
            n += 1;
            char::from_u32(255 + n).unwrap()
        };
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_table_matches_gpt2() {
        let t = bytes_to_unicode();
        assert_eq!(t[b'a' as usize], 'a');
        assert_eq!(t[b' ' as usize], 'Ġ');
        assert_eq!(t[0], 'Ā');
    }

    #[test]
    fn pre_tokenizer_splits_like_clip() {
        assert_eq!(
            pre_tokenize("a cat's 3d-face, ok"),
            ["a", "cat", "'s", "3", "d", "-", "face", ",", "ok"]
        );
    }

    #[test]
    fn bpe_with_a_tiny_vocab() {
        let vocab: HashMap<String, i64> = [
            ("c", 1),
            ("a", 2),
            ("t</w>", 3),
            ("ca", 4),
            ("cat</w>", 5),
            ("a</w>", 6),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let tok = Tokenizer::new(vocab, "#version: 0.2\nc a\nca t</w>\n").unwrap();
        let ids = tok.encode("A  CAT");
        assert_eq!(&ids[..4], &[SOT, 6, 5, EOT]);
        assert_eq!(ids.len(), CONTEXT);
        assert!(ids[4..].iter().all(|&i| i == EOT));
    }

    #[test]
    #[allow(clippy::needless_range_loop)]
    fn preprocessing_layout() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            640,
            480,
            image::Rgb([255, 0, 128]),
        ));
        let t = preprocess(&img);
        assert_eq!(t.shape(), &[1, 3, INPUT, INPUT]);
        let v = t.to_plain_array_view::<f32>().unwrap();
        let want = [
            (1.0 - MEAN[0]) / STD[0],
            (0.0 - MEAN[1]) / STD[1],
            (128.0 / 255.0 - MEAN[2]) / STD[2],
        ];
        for c in 0..3 {
            assert!((v[[0, c, 100, 100]] - want[c]).abs() < 0.02, "channel {c}");
        }
    }
}
