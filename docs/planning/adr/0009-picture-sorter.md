# ADR-0009: A local model sorts reference pictures (and nothing else)

- Status: accepted (2026-11-19, user decision)
- Changes: ADR-0005 (AI was allowed only as an NSFW guard). ADR-0002 still holds.

## Context

Reference pictures were filed by where they were found, not by what they show:

- the live service filed every Pixabay hit under the topic whose search query found it;
- the importer used the first topic whose tags matched the uploader's tags.

So anime landscapes landed in Anime › Characters, cartoon couriers in
Real life › Vehicles, and building photos in Animals › Birds. Tags and search
terms can't fix this: uploaders tag freely.

## Decision

**The AI rule.** AI is allowed in exactly two places:

1. **Safety:** the NSFW guard (ADR-0005).
2. **Sorting reference pictures:** deciding which category and topic a
   reference picture belongs to.

AI is **never** used to grade, score, rank, critique or give feedback on a
student's drawing (ADR-0002). The sorter never sees drawings.

**The model.** OpenAI CLIP ViT-B/32, a general image/text matching model:

- Licence: MIT.
- Files: the ONNX export by Xenova (transformers.js) of
  `openai/clip-vit-base-patch32` on Hugging Face, pinned to one revision and
  checked by SHA-256: vision tower 335 MiB, text tower 242 MiB, plus the
  tokenizer files.
- Fetched by `crates/ref-filters/fetch-model.sh` into
  `crates/ref-filters/models/clip-vit-b32/`. The files are git-ignored and
  never committed.
- Runtime: `tract` (pure Rust, no native libraries), inside ref-service, the
  same way as the NSFW guard. About 0.1 s per picture on one core.

**How it sorts (zero-shot).**

- Each topic in `content/topics.json` has a few short `prompts`, for example
  "an anime portrait of a character's face" and "an anime landscape scene".
- The `_sorting` block holds style prompts (photo, anime, cartoon, drawing),
  which styles each category accepts, which categories may swap pictures (a
  `group`), and the cut-offs.
- Style check: a picture is rejected if its category's allowed styles get
  less than `minStyle`. Example: a drawing in Real life.
- Topic check, against every topic in the same group:
  - Keep the picture if its own topic wins.
  - Re-file it if another topic wins by at least `margin` with at least
    `minBest` probability.
  - Reject it if its own topic gets less than `rejectBelow` and nothing
    claims it confidently.
  - Otherwise keep it: the model must be sure before it moves a picture.
- One drawable subject (added 2026-11-19, user rule: "Characters should
  only be close-up pictures, or characters posing"). A reference must show
  one clear subject, or one coherent scene for scenery-type topics. Not a
  sheet of twenty small heads, an icon or sticker set, a pattern, text or a
  logo. Checked on the topic the picture ends up in:
  - Each topic has a kind: `"subject": "single"` (the default) or
    `"scene"` (anime scenery and the environment topics).
  - The keep class is the mean of the topic's own prompts and its kind's
    prompts. It competes with the reject classes in `_sorting.subject`
    (sheet, icons, pattern, text, logo) and with each prompt in the topic's
    `avoid` list (for example "a drawing of a single eye" for Characters).
  - Reject if the strongest of those beats the keep class by the kind's
    margin: 0.35 for `single`, 0.45 for `scene`.
- Tag rules and search queries stay as the cheap first guess. The model has
  the final say.

**Where it runs.**

- In the refill path, on every new picture after the plain-maths checks.
- At startup, ref-service checks the pool pictures it hasn't checked yet.
  The pool stores a fingerprint of the rules (prompts, cut-offs, rule
  version) with its `sorted` set. When the rules change, the next start
  re-checks the whole pool once.
- `ref-service recheck [--dry-run]` re-checks the existing live pool. Stop the
  server first.
- `ref-importer sort [--dry-run]` re-checks the committed manifests. Removed
  ids go to `rejected.json`.
- ref-service refuses to start without the model unless
  `REF_ALLOW_NO_SORTER=1` (dev only). CI has no model: the tests use a fake
  scorer, and tests that need the real files skip when they are missing.

## Why this model

- Small enough for a server, permissive licence, the standard model for
  zero-shot image/text matching.
- The fp32 export runs on tract unchanged.
- The int8 export runs 50 times slower on tract (5 s per picture), so we use
  fp32.
- We checked the Rust pipeline (preprocessing and BPE tokenizer) against
  onnxruntime with the Hugging Face tokenizer. Logits agree to ±0.02, and the
  token ids are identical.

## Consequences

- About 580 MiB more to download on the server, and about 350 MiB of memory.
  The text tower is dropped once the prompts are embedded.
- Changing a topic's prompts changes how pictures are sorted. Re-run the dry
  runs after editing them. The live pool is re-checked on the next restart.
- The subject check was calibrated on 988 live pool pictures and 124
  manifest pictures (see crates/ref-filters/README.md). On the pool it
  rejected 27 pictures: 23 clearly right, 4 borderline, none clearly wrong.
  It caught 20 of 23 hand-labelled sheets and sets.
- The privacy page and README promises stay true: no AI looks at drawings.
