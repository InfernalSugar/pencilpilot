# @pencilpilot/paint

The drawing studio, shared by the website (`apps/web`) and the app (`apps/studio`, planned):
the canvas (`PaintCanvas.svelte`, over the [paint engine](../../crates/brush)), the studio
layout, tool rail, panels (layers, brushes, colour, history, textures, adjustments) and
their logic (strokes, QuickShape, guides, warp, pen paths, tone curves, brush import,
timelapse, PSD export…).

```ts
import PaintCanvas from '@pencilpilot/paint/PaintCanvas.svelte';
import { StrokeLog } from '@pencilpilot/paint/strokes';
```

- Svelte source, compiled by each app. Needs the `pencilpilot-brush-wasm` package
  (`pnpm wasm:brush`).
- Brushes load from `/brushes/` (`manifest.json`, `*.myb`, previews), which the website
  serves from `apps/web/static/brushes`. The app will serve the same files (S1).
- `pnpm test`, `pnpm check` and `pnpm lint` run here; the root scripts run them too.
