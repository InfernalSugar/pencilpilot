/**
 * Warp: a 4×4 grid of control points shapes a smooth (bicubic Bézier) patch over
 * the lifted box, like Procreate's Warp. The engine draws it as a fine mesh of
 * small cells, each with its own projective map.
 */
export type Pt = [number, number];

/** how many cells across and down the engine draws */
export const WARP_N = 12;

const bern = (t: number): [number, number, number, number] => {
	const u = 1 - t;
	return [u * u * u, 3 * u * u * t, 3 * u * t * t, t * t * t];
};

/** The patch point at (u, v) in 0–1 for 16 control points, row by row. */
export function patchPoint(ctrl: Pt[], u: number, v: number): Pt {
	const bu = bern(u);
	const bv = bern(v);
	let [x, y] = [0, 0];
	for (let r = 0; r < 4; r++)
		for (let c = 0; c < 4; c++) {
			const k = bv[r] * bu[c];
			x += ctrl[r * 4 + c][0] * k;
			y += ctrl[r * 4 + c][1] * k;
		}
	return [x, y];
}

/** The (n+1)×(n+1) mesh corner points as x, y pairs, row by row, for the engine. */
export function meshNodes(ctrl: Pt[], n = WARP_N): Float32Array {
	const out = new Float32Array(2 * (n + 1) * (n + 1));
	let i = 0;
	for (let r = 0; r <= n; r++)
		for (let c = 0; c <= n; c++) {
			const [x, y] = patchPoint(ctrl, c / n, r / n);
			out[i++] = x;
			out[i++] = y;
		}
	return out;
}

/** Points along the patch's grid lines (u or v = 0, ⅓, ⅔, 1), for drawing the warp grid. */
export function meshLines(ctrl: Pt[], steps = 24): Pt[][] {
	const lines: Pt[][] = [];
	for (const k of [0, 1 / 3, 2 / 3, 1]) {
		lines.push(Array.from({ length: steps + 1 }, (_, i) => patchPoint(ctrl, i / steps, k)));
		lines.push(Array.from({ length: steps + 1 }, (_, i) => patchPoint(ctrl, k, i / steps)));
	}
	return lines;
}
