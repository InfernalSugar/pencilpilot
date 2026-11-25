/**
 * Line art as strokes (the engine's `vectorise`): a drawing's ink as centrelines with a
 * width at each point, for drawing it again through a brush ("Ink this stroke").
 */
import { vectorise as engineVectorise } from 'pencilpilot-brush-wasm';

export interface VPoint {
	x: number;
	y: number;
	/** the line's width there (canvas px) */
	width: number;
}

/** The engine's flat answer as strokes: [count, then each: n, n × (x, y, width)]. */
export function readStrokes(flat: ArrayLike<number>): VPoint[][] {
	const out: VPoint[][] = [];
	let i = 1;
	const n = flat[0] ?? 0;
	for (let s = 0; s < n && i < flat.length; s++) {
		const len = flat[i++];
		const pts: VPoint[] = [];
		for (let k = 0; k < len && i + 2 < flat.length; k++, i += 3)
			pts.push({ x: flat[i], y: flat[i + 1], width: flat[i + 2] });
		out.push(pts);
	}
	return out;
}

/**
 * The strokes of a drawing's ink: `rgba` its pixels (straight alpha), `w` × `h`; ink is
 * more than `threshold` (0–255) covered. Needs the engine loaded (`loadEngine`).
 */
export function vectorise(
	rgba: Uint8Array | Uint8ClampedArray,
	w: number,
	h: number,
	threshold = 96
): VPoint[][] {
	const alpha = new Uint8Array(w * h);
	for (let i = 0; i < alpha.length; i++) alpha[i] = rgba[i * 4 + 3];
	return readStrokes(engineVectorise(alpha, w, h, threshold));
}
