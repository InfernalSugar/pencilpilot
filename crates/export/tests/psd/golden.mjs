// The PSD fixture written by the current app's writer (packages/paint/src/psd.ts), for the
// native writer's byte-for-byte check (crates/export/src/psd/tests.rs builds the same
// document). Regenerate with:
//
//   node crates/export/tests/psd/golden.mjs crates/export/tests/psd/ts.psd
import { writeFileSync } from 'node:fs';
import { writePsd } from '../../../../packages/paint/src/psd.ts';

const W = 150;
const H = 40;
const canvas = (f) => {
	const a = new Uint8Array(W * H * 4);
	for (let y = 0; y < H; y++)
		for (let x = 0; x < W; x++) {
			const p = f(x, y);
			if (p) a.set(p, (y * W + x) * 4);
		}
	return a;
};
const layer = (name, blend, opacity, visible, clip, rgba, mask) => ({
	kind: 'layer', name, blend, opacity, visible, clip, rgba, ...(mask ? { mask } : {})
});
const noise = (x, y) => (Math.imul(x, 73856093) ^ Math.imul(y, 19349663)) & 255;
const mask = new Uint8Array(W * H);
for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) mask[y * W + x] = x < 75 ? 255 : (y * 6) & 255;

const doc = {
	width: W,
	height: H,
	composite: canvas((x, y) => [(x * 3 + y) & 255, (x ^ y) & 255, (x * y) & 255, 255]),
	items: [
		layer('Paper', 'normal', 1, true, false, canvas(() => [240, 235, 220, 255])),
		layer('Ink ✏️ é', 'multiply', 0.7, true, false, canvas((x, y) =>
			x >= 10 && x < 90 && y >= 5 && y < 30
				? [(x * 7) & 255, (y * 11) & 255, ((x + y) * 5) & 255, (x + y) % 3 === 0 ? 128 : 255]
				: null)),
		layer('Shade', 'screen', 0.333, true, true, canvas((x, y) => {
			if (y < 20) return null;
			const n = noise(x, y);
			return [n, (n * 3) & 255, (n * 7) & 255, n | 1];
		})),
		{
			kind: 'group', name: 'Folder', opacity: 0.5, visible: false,
			children: [
				layer('Masked', 'overlay', 1, true, false,
					canvas((x) => [((x >> 1) & 1) * 200, 50, ((x >> 2) & 1) * 100, 255]), mask),
				layer('Empty', 'lighten', 1, true, false, canvas(() => null))
			]
		},
		layer('Top add', 'add', 0.9, false, false, canvas((x, y) => (x === 149 && y === 39 ? [1, 2, 3, 4] : null))),
		layer('Unknown ' + 'é'.repeat(260), 'hue', 1, true, false, canvas((x, y) => (y === 0 ? [x, x, x, x] : null)))
	]
};
const out = writePsd(doc);
writeFileSync(process.argv[2] ?? 'ts.psd', out);
console.log(out.length);
