# ADR-0011 — AI assistance in the PencilPilot app

**Status:** Accepted (2026-11-21). Applies to the app only. The website keeps
ADR-0002 (no AI grading or critique) and ADR-0009 (AI only to vet reference pictures).

## Decision
The app may use generative and vision models to help the artist make frames: keyframe and
in-between guides, reference and model-sheet creation, guided drawing, colour guides, and a
"director" that critiques timing and poses. The following rules hold:

1. **The artist draws the art.** AI output lands on a **Guide layer**: shown like a light
   table, never exported and never merged by itself. Getting AI work into a frame takes an
   explicit action:
   - tracing it;
   - "ink this stroke", which replays a vector stroke through *the artist's own brush*, so it
     becomes a real, editable stroke;
   - "use as layer", for colour guides and backgrounds.
2. **Provenance.** Every frame records which guides it used and how its pixels were made
   (hand-drawn, traced, auto-inked, AI pixels). Export can embed a C2PA / XMP note. The
   project view shows a per-shot "made by hand" share.
3. **Style comes from the artist's own work only.** A style pack is the artist's frames, model
   sheets and brushes. The app never offers "in the style of <a named artist>". It refuses to
   train or condition on files the user marks as not theirs.
4. **Bring your own key first.** Provider keys live in the OS keychain. Each action shows its
   estimated cost before it runs, and a running cost meter per project. A hosted
   "credits" proxy is optional and comes later (A11).
5. **Nothing leaves the machine unasked.** Drawing, animating, exporting, line extraction,
   vectorising, lip-sync and upscaling all run locally. Only generate / colour-guide /
   director actions call a provider, and only with the frames shown in the request.
6. **Swappable providers.** Every capability (generate-from-references, edit-region,
   first-last-frame video, colourise, critique) is an interface with several back ends, so
   price changes, API changes or a refusal can fall back to another provider. Local back ends
   (ComfyUI, ONNX) count as providers.
7. **Licences are checked per back end.** Only models whose terms allow commercial use of
   output are on by default. Non-commercial weights (e.g. FLUX.2 dev, NoobAI) are hidden
   behind a "personal projects" switch that is labelled as such.

## Why
AI drawing "for" the artist would defeat the point of a drawing app built around practice.
Here AI scaffolds (poses, spacing, construction, colour plans) and the artist's own strokes
make the frame, and the style stays theirs because it is literally their brush and their
hand.

## First providers (2026-11-21)
The owner's choice: **ByteDance's models through BytePlus**, with one key covering both:
- **Seedance 2.5** for motion: in-between guides between two keys (first / last frame),
  sampled at the artist's spacing, then cleaned up into line guides (S8).
- **Seedream** (image model) for keyframe guides, region edits and references (S6).

Both are tested on the owner's own frames before S6 / S8 commit to them (their content
policies on anime characters, line quality, cost). Other back ends stay behind the same
interfaces (§6) as fallbacks.
