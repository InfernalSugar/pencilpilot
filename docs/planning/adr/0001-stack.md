# ADR-0001 — Stack: SvelteKit + Rust grader compiled to WebAssembly

**Status:** Accepted (2026-11-18)

## Decision
- Website: **SvelteKit** (TypeScript, Svelte 5 runes) in `apps/web`.
- Grading and feedback logic: **Rust** in `crates/`, compiled to **WebAssembly** with wasm-pack and loaded by the site as the `pencilpilot-grader-wasm` workspace package.
- Monorepo: pnpm workspace + cargo workspace.

## Why
- Grading is number-crunching over images. Rust is fast and, running in the browser through WASM, it gives instant results with no server cost per drawing.
- The same Rust crates can run natively in tools (`ref-importer`, `calibrate`) and in tests, so grading is exactly the same everywhere.
- SvelteKit is small and fast, and makes playful, animated UI (the "Fun" theme) easy.

## Consequences
- Contributors need Rust + wasm-pack as well as Node.
- `crates/grader-wasm/pkg` is a build output (git-ignored). `pnpm dev`/`pnpm build` rebuild it first.
