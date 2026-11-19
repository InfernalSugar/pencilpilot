# ✏️ PencilPilot

**Learn to draw by matching references, and get a score and real tips without any AI.**

Pick a reference (Anime, Real life, Animals, Environment, Cartoon…), draw it on a
fixed-size canvas, and submit. PencilPilot measures your drawing against the
reference (proportions, angles, shapes, light and dark, line confidence), gives you a
score out of 10, and tells you the 2–3 most useful things to fix, with lessons and
drills to practise them.

> 🚧 Early development. See the [plan and milestones](docs/planning/README.md).

## How it works

- **Website:** SvelteKit (TypeScript), in `apps/web`
- **Grader:** Rust compiled to WebAssembly (`crates/grader`, `crates/grader-wasm`). It runs in your browser, so grading is instant and free.
- **Feedback:** a Rust rules engine (`crates/feedback`) turns measurements into tips written by people (`content/`)
- **No AI critique.** Scores and tips come from measurements and hand-written rules ([ADR-0002](docs/planning/adr/0002-no-ai-grading.md)).

## Develop

Requirements: Rust (with the `wasm32-unknown-unknown` target), [wasm-pack](https://rustwasm.github.io/wasm-pack/), Node 22+, pnpm 10.

```sh
pnpm install
pnpm dev          # builds the WASM grader, then starts the site on http://localhost:5173
cargo test        # Rust tests
pnpm test         # web unit tests
pnpm --filter @pencilpilot/web test:e2e   # browser tests
```

## Configuration

One git-ignored `.env` at the repo root:

| Variable | What for |
|---|---|
| `PIXABAY_API_KEY` | reference importer + Endless reference service |
| `DATABASE_PATH` | optional; SQLite file for accounts (default `data/pencilpilot.db`) |
| `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET` | optional; enables "Continue with Google" (redirect URI: `<site>/auth/google/callback`) |
| `COMMUNITY` | optional; `on` enables community critique (beta, [ADR-0006](docs/planning/adr/0006-community-critique.md)). Needs the ref-service running with its NSFW model, or sharing is refused |
| `REF_SERVICE_URL` | optional; where the web server reaches the ref-service (default `http://127.0.0.1:8787`) |

## Licences

- Code: [MIT](LICENSE)
- Teaching content in `content/`: [CC BY-SA 4.0](content/README.md)
- Reference images: each keeps its own licence and is credited on the site. See [LICENSING.md](docs/planning/LICENSING.md).
