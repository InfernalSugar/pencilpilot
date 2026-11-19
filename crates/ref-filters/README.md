# ref-filters

Auto-vets reference pictures fetched live from Pixabay before students see
them. Used by `crates/ref-service`.

Everything is plain image maths (ADR-0002) **except two learned models**, and
neither ever affects scores, tiers or feedback on drawings:

* an **NSFW classifier, used only as a safety guard** (ADR-0005);
* **CLIP, used only to sort reference pictures** into category and topic
  (`clip` and `sort` modules, ADR-0009; see [Picture sorter](#picture-sorter-clip-sorting-only)).

Get both with `crates/ref-filters/fetch-model.sh` (or `fetch-model.sh nsfw` /
`fetch-model.sh clip`).

```rust
use ref_filters::{Filters, Thresholds, decode, hamming};

let filters = Filters::new("crates/ref-filters/models/open_nsfw.onnx".as_ref())?
    .with_thresholds(Thresholds::default());
let v = filters.check(&decode(&bytes)?);
if !v.ok { /* v.reasons, e.g. ["text_heavy"] */ }
if hamming(v.phash, other.phash) <= 10 { /* near-duplicate */ }
```

`Filters` is `Send + Sync`: load it once and share it (e.g. in an `Arc`).
`Filters::without_nsfw()` skips the model for tests and CI: `nsfw` is 0,
`nsfw_checked` is false and `reasons` contains the non-blocking
`nsfw_unchecked`.

## Checks

| Reason | Hard? | How (default threshold) |
|---|---|---|
| `tiny` | yes | short side < 400 px |
| `extreme_aspect` | yes | long ÷ short > 3 |
| `blank` | yes | `blank` ≥ 0.8. `blank` = max(flat, edgeless); flat = 1 − gray std/24, edgeless = 1 − (grader edge density ÷ picture area)/0.02 |
| `too_faint` | yes | grader difficulty score < 10 |
| `too_busy` | yes | grader difficulty score > 82, or grader edge density > 0.28 (the score saturates around 80 for pure texture) |
| `text_heavy` | yes | `text` ≥ 0.5: rows of glyph-sized, baseline-aligned, similar-height connected components (see `src/text.rs`) |
| `nsfw` | yes | model probability ≥ 0.3 |
| `nsfw_error` | yes | the model failed on this image (fails closed) |
| `nsfw_unchecked` | no | built with `without_nsfw()` |

Report-only metrics:

* `smoothness` (0–1): mean |Laplacian| (with a 3-gray-level floor for 8-bit and
  JPEG jitter) ÷ mean |gradient|, mapped to `1/(1+r)`. Airbrushed and AI-style
  renders score high. Soft clouds and shallow depth-of-field photos do too, so
  this is a hint and never a reason to reject.
* `phash`: standard 64-bit DCT pHash (32×32 box-filtered gray, 8×8
  low-frequency block including DC, bit = above median). Use `hamming(a, b)`;
  ≤ 10 means near-duplicate.
* `difficulty`: `grader::difficulty` on the picture fitted into the 800×600
  canvas on white, exactly like `ref-importer`'s `fitted_gray`.

Transparent pictures are composited onto white first (`flatten_alpha`), the
way the site shows them. Many Pixabay line-art PNGs keep their ink only in the
alpha channel, and dropping alpha makes them look blank.

All cut-offs live in `Thresholds` (serde, camelCase, `Default`).

## NSFW model (safety guard only)

| | |
|---|---|
| Model | Yahoo **open_nsfw** (ResNet-50 "thin", 2-class), via the Keras port in [`bhky/opennsfw2`](https://github.com/bhky/opennsfw2), converted to ONNX with tf2onnx 1.8.4 (opset 9) |
| Licence | **BSD-2-Clause** (weights: [yahoo/open_nsfw](https://github.com/yahoo/open_nsfw); the opennsfw2 code is MIT) |
| File | <https://huggingface.co/bluefoxcreation/open-nsfw/resolve/0fd53306e9c78e89b216aea7d30f65842a66d591/open-nsfw.onnx> (repository licence: bsd-2-clause) |
| Size | 23,590,724 bytes (22.5 MiB) |
| SHA-256 | `864bb37bf8863564b87eb330ab8c785a79a773f4e7c43cb96db52ed8611305fa` |
| Input | `input:0`, float32 NHWC `[1, 224, 224, 3]` |
| Output | `outputs`, float32 `[1, 2]` softmax: `[sfw, nsfw]`; we report index 1 |
| Runtime | `tract-onnx` 0.23 (pure Rust, no native libraries), single thread |

Get it with `crates/ref-filters/fetch-model.sh`. It downloads to
`crates/ref-filters/models/open_nsfw.onnx` (git-ignored, **never commit it**)
and verifies the SHA-256. Tests that need it skip when it is missing.

**Preprocessing** (`nsfw_preprocess`), the same as `opennsfw2.preprocess_image`
with `Preprocessing.YAHOO`:

1. Composite onto white if transparent, then RGB.
2. Resize to 256×256 (bilinear/triangle, aspect not kept).
3. JPEG round trip at quality 75 (the reference does this to match the
   original Caffe pipeline).
4. Centre crop 224×224 (offset 16, 16).
5. RGB → BGR, subtract the means `[104, 117, 123]`. No scaling to 0–1.

We checked this against the reference Python pipeline (PIL + scikit-image +
onnxruntime) on the 159 samples: mean |Δp| = 0.006, max 0.14 (a single
borderline image; the JPEG and resize implementations differ slightly).

**Threshold: 0.3.** Yahoo's guidance is that < 0.2 is "likely safe" and
> 0.8 "likely NSFW". We reject anything above the safe band (conservative:
another picture is cheap, a bad one is not). On the 159 approved, safe
samples: median 0.003, p95 0.104, max 0.711. Four samples (2.5 %) score ≥ 0.3
and are rejected: two skin-heavy close-ups of hands (0.71, 0.53), a thin
line-art PNG of cartoon cats (0.49) and an ink drawing of a man and a pig
(0.35). All are false positives, and the only cost is one fewer candidate
picture. The model is trained on photos, so it is weaker on drawn or anime
content. Pixabay SafeSearch stays on as the first guard.

No explicit imagery was downloaded or used to build or test this crate.

### Candidates considered

* **Yahoo open_nsfw / opennsfw2** (BSD-2): chosen. A ready ONNX file under
  30 MB (pinned by revision and SHA-256), runs on tract unchanged, and the
  reference preprocessing is documented.
* **GantMan nsfw_model** (MIT, MobileNetV2 1.4, 5 classes including
  `hentai` and `drawings`): smaller, and a better fit for anime and cartoons,
  but published only as Keras, SavedModel or TF.js (a 100 MB zip). There is
  no official ONNX, so we would need a TensorFlow + tf2onnx conversion step
  whose output hash is not reproducible. Worth revisiting if drawn content
  slips through.
* **Falconsai/nsfw_image_detection** (Apache-2.0, ViT-B/16): 343 MB as fp32 ONNX
  and 87 MB as int8 (onnx-community export), with several times the compute of
  open_nsfw. Too big.

### tract vs ort

tract loads and runs the model as-is (Conv, Relu, MaxPool, AveragePool,
MatMul, Softmax, opset 9), so we do not need `ort` and its native
onnxruntime library.

## Performance

`check` on a 1280 px image, release build, pinned to one core
(`taskset -c 3`, Ryzen 7 5700X), over the 159 samples: **median 79 ms,
p95 96 ms, max 100 ms** with the model, and 41 ms median for the maths
checks alone. Decoding is not included.

## Calibration

```sh
cargo run --release -p pencilpilot-ref-filters --example scan -- \
    apps/web/static/references crates/ref-filters/models/open_nsfw.onnx
REF_SAMPLES_DIR=$PWD/apps/web/static/references \
    cargo test --release -p pencilpilot-ref-filters --test samples -- --nocapture
```

Results on the 159 approved samples: 0 blank, 0 too_faint, 0 too_busy,
0 text_heavy (max `text` 0.35: a forest and its reflection), 0 tiny or
extreme_aspect, and 4 nsfw (see above). Rendered test posters (a quote,
an infographic, a document page, two-column small print, a paragraph over a
photo, text tilted 4°) were all flagged as `text_heavy`. A photo with a
single two-word title was not. pHash: resizing to 640 px and re-encoding as
JPEG moves a sample by at most 4 bits. The closest two different samples are
14 bits apart, and 99.8 % of pairs are ≥ 20.

## Picture sorter (CLIP, sorting only)

Used **only** to decide which category and topic a reference picture belongs
to (ADR-0009). It never sees students' drawings and never affects scores,
tiers or feedback.

| | |
|---|---|
| Model | OpenAI **CLIP ViT-B/32** (`openai/clip-vit-base-patch32`) |
| Licence | **MIT** |
| Files | ONNX export by Xenova (transformers.js): <https://huggingface.co/Xenova/clip-vit-base-patch32>, revision `6b7c523703ae26e39be4b03774966f8571c7f523` |
| Size | `onnx/vision_model.onnx` 351,685,709 B, `onnx/text_model.onnx` 254,058,553 B, plus `vocab.json` and `merges.txt` (≈ 580 MiB in total) |
| SHA-256 | pinned in `fetch-model.sh` |
| Runtime | `tract-onnx` 0.23, batch fixed to 1. ≈ 0.1 s per picture on one core. The text tower is dropped after the prompts are embedded |

`fetch-model.sh clip` downloads to `models/clip-vit-b32/` (git-ignored,
**never commit it**). Tests that need the files skip when they are missing.

We use the fp32 export because the int8 export runs about 50 times slower on
tract (≈ 5 s per picture). The Rust pipeline matches onnxruntime with the
Hugging Face tokenizer: logits agree to ±0.02, and the token ids are identical.

* Preprocessing (`clip::preprocess`): composite onto white, bicubic resize
  so the short side is 224, centre crop 224×224, scale to 0–1, CLIP
  mean/std, NCHW.
* Tokenizer (`clip::Tokenizer`): CLIP's byte-level BPE (lower-case, no ftfy),
  padded with end-of-text to 77.

`sort::Sorter` does zero-shot matching against the `prompts` in
content/topics.json and the `_sorting` block there. That block holds the
style prompts, the allowed styles and group of each category, and the
cut-offs. The decision rules are documented in `src/sort.rs`.

Calibrate with:

```sh
# list.tsv: id <TAB> category <TAB> topic <TAB> image path
cargo run --release -p pencilpilot-ref-filters --example sort_scan -- \
    crates/ref-filters/models/clip-vit-b32 content/topics.json list.tsv
```

Calibration (2026-11-19) on 609 live pool pictures and 127 manifest pictures:

* We dropped the `render`, `painting` and `graphic` styles. They claimed
  many real studio photos (clean backgrounds) and isolated cartoon objects.
* Anime and cartoon accept each other's styles plus `drawing`. Whether a
  picture is anime or cartoon is settled by the cross-group topic match, not
  by the style gate.
* Most re-files were right on visual review. For example, anime landscapes
  went from Characters to Scenery, cartoon kittens from Anime › Chibi to
  Cartoon › Animals, and a building photo from Animals › Birds to
  Environment › City.
* A few re-files are debatable, such as hair clip art moved to Characters.

Subject check (2026-11-19, `_sorting.subject`): a reference must show one
clear subject, or one coherent scene for scenery-type topics (topic
`"subject": "scene"`). Reject classes (sheet, icons, pattern, text, logo) and
each topic's `avoid` prompts compete with a keep class made of the topic's
own prompts plus its kind's prompts. Calibrated on 988 live pool pictures
(55 hand-labelled near the cut-off) and 124 manifest pictures:

* Keeping the topic's prompts in the keep class matters. Generic "one
  subject" prompts alone lose to "a set of icons" on most single cartoon
  pictures (300+ false rejects).
* Margin 0.35 for `single`, 0.45 for `scene`: wide scenes have repeated
  motifs (stars, windows) that look a bit like a pattern.
* Pool: 27 rejected; 23 were clearly right, 4 borderline (a row of 3
  lollipops, a row of sushi, a cat on a leaf pattern, a group of 6 girls),
  none clearly wrong. 20 of the 23 hand-labelled sheets/sets were caught; a
  2×3 cat sheet, a set of stamps and city skylines with captions slipped
  through.
* Manifests: 9 rejected, 7 right, 1 borderline, 1 wrong (a cartoon man
  holding a chart, as "logo"). Tighter logo prompts fixed that one but
  rejected three good pool pictures, so they stayed.
* A plain-maths "count similar blobs on a plain background" heuristic was
  tried and dropped: line drawings and faces split into many blobs, so it
  flagged good pictures as often as it found missed sheets.
