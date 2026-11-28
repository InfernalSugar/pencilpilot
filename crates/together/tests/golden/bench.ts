/**
 * How long a host takes to share a big project, and a joiner to take it in: 20 layers
 * of a 4K canvas (60 × 34 tiles each, painted all over with soft strokes), the studio's
 * own together.ts. Timings only (no fixtures):
 *
 *   cd apps/studio && node --no-warnings --import ../../crates/together/tests/golden/loader.mjs \
 *     ../../crates/together/tests/golden/bench.ts
 */
import {
  TILE_BYTES,
  Together,
} from "../../../../apps/studio/src/lib/together.ts";
import { FakeCanvas } from "./fake.ts";

Math.random = () => 0;
const A = "a".repeat(64);
const B = "b".repeat(64);
const LAYERS = Number(process.argv[2] ?? 20);
const [COLS, ROWS] = [60, 34];

/** a soft-edged, mostly opaque tile, different for each place and layer */
function paintTile(l: number, x: number, y: number) {
  const px = new Uint8Array(TILE_BYTES);
  for (let i = 0; i < 64 * 64; i++) {
    const u = i % 64;
    const v = (i / 64) | 0;
    const a = Math.max(
      0,
      Math.min(
        255,
        128 + 127 * Math.sin((u + x * 7 + l) / 9) * Math.cos((v + y * 5) / 11),
      ),
    );
    px.set([(l * 37 + u) & 255, (x * 13 + v) & 255, (y * 29) & 255, a], i * 4);
  }
  return px;
}

const hc = new FakeCanvas(
  Array.from({ length: LAYERS }, (_, i) => `Layer ${i + 1}`),
);
for (const l of hc.layerList) {
  const d = hc.drawingOf(l.id);
  const m = hc.drawings.get(d)!;
  for (let y = 0; y < ROWS; y++)
    for (let x = 0; x < COLS; x++) m.set(`${x},${y}`, paintTile(l.id, x, y));
}
const edits: unknown[] = [];
const directs: unknown[] = [];
const host = new Together(A, "Ana", hc, {
  edit: (b) => edits.push(b),
  direct: (_to, b) => directs.push(structuredClone(b)),
});
let t = performance.now();
await host.startHost();
const hostMs = performance.now() - t;
// (the session numbers and hands back the host's own edits)
for (const [i, e] of edits.entries()) await host.onEdit(i + 1, A, e);
// a joiner right away (the tiles not compressed ahead yet), then one later
t = performance.now();
await host.onDirect(B, { k: "snap.req" });
const snapMs = performance.now() - t;
const first = directs.splice(0);
t = performance.now();
await host.onDirect(B, { k: "snap.req" });
const snapAgainMs = performance.now() - t;
const bc = new FakeCanvas([]);
const bo = new Together(B, "Bo", bc, { edit: () => {}, direct: () => {} });
bo.startJoin(A);
t = performance.now();
// (as the page hands them in: each as it comes, none awaited)
await Promise.all(first.map((m) => bo.onDirect(A, m)));
const joinMs = performance.now() - t;
const tiles = LAYERS * COLS * ROWS;
console.log(
  JSON.stringify({
    layers: LAYERS,
    tiles,
    hostShareMs: Math.round(hostMs),
    snapshotMs: Math.round(snapMs),
    snapshotAgainMs: Math.round(snapAgainMs),
    tilesKept: [...bc.drawings.values()].reduce((n, m) => n + m.size, 0),
    joinerTakesMs: Math.round(joinMs),
    synced: bo.synced,
    messages: first.length,
  }),
);
