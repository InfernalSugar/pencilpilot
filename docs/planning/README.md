# PencilPilot planning

Start here.

| Doc | What it covers |
|---|---|
| [MILESTONES.md](MILESTONES.md) | Milestones M0–M9 and every task (they match the GitHub milestones and issues) |
| [ADR-0001 Stack](adr/0001-stack.md) | SvelteKit + Rust/WASM grader, monorepo layout |
| [ADR-0002 No AI grading](adr/0002-no-ai-grading.md) | Why grading and feedback are maths + hand-written rules |
| [ADR-0003 Reference images](adr/0003-reference-images.md) | Where images come from and how they stay licence-clean and SFW |
| [ADR-0005 Endless references](adr/0005-endless-references.md) | Live, auto-vetted Pixabay pool; AI only as an NSFW guard |
| [LICENSING.md](LICENSING.md) | Licence rules for code, content, and reference images |
| [CONTENT_GUIDE.md](CONTENT_GUIDE.md) | How to write rules, tips, lessons and drills |

## The product in one paragraph

A drawing practice site. You pick a reference from a category, draw it on a fixed-size
canvas, and submit. The grader measures the drawing against the reference and gives a score
out of 10. The feedback engine picks the 2–3 biggest problems and one thing you did well,
shows them on your drawing (arrows, boxes, a ghost outline, a fade slider), and links each
problem to a mini-lesson and a drill. Over time it spots recurring mistakes and adjusts which
references and lessons it suggests. The theme is **Fun**: bright, bouncy, encouraging.

## Workflow

- Default branch `dev`. Every task gets a branch and a PR into `dev`, squash-merged once CI is green.
- `main` receives releases only.
- Issue titles follow `Mx-Tyy — description`, under the matching GitHub milestone.
- Labels: `web`, `grader`, `feedback`, `content`, `design`, `infra`, `legal`.

## Estimates (rough)

| Point | When |
|---|---|
| Public prototype (end of M5) | ~6–8 weeks |
| Solid v1.0 (end of M9) | ~4–6 months |

Content writing (rules, tips, lessons) is the slowest part and keeps going after v1.
