# pencilpilot-brush

The PencilPilot painting engine, in plain Rust: [hokusai](https://github.com/reearth/hokusai)
(a pure-Rust port of libmypaint) plus everything a drawing app needs on top of it. That
includes layers, groups, masks, blend modes, adjustment layers, undo/redo, selections,
transform / warp / liquify, fills, brush textures, canvas resizing and dirty-rect
flattening to RGBA8.

- **The app** (`apps/studio`, planned) uses it natively.
- **The website** uses it through [`pencilpilot-brush-wasm`](../brush-wasm), the
  wasm-bindgen bindings (`pnpm wasm:brush`).

```sh
cargo test -p pencilpilot-brush   # unit tests + every brush in the pack
```

The entry point is [`Painter`](src/lib.rs).
