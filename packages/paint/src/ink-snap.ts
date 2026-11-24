/**
 * Snap to ink: a stroke's start and end (or a dragged point) go onto ink close by, so
 * lines meet with no gap. The end of a nearby stroke first; else the nearest point on a
 * line. All positions are canvas pixels.
 */
import { STRIDE, spanOf, type LineStroke } from './lines';

export type Pt = [number, number];

/** how near (screen px) ink has to be to snap to it */
export const INK_SNAP_PX = 10;

/** The two ends of a stroke (of the points it shows). */
export function strokeEnds(s: LineStroke): Pt[] {
	const [from, to] = spanOf(s);
	if (to <= from) return [];
	const at = (i: number): Pt => [s.pts[i * STRIDE], s.pts[i * STRIDE + 1]];
	return to - from === 1 ? [at(from)] : [at(from), at(to - 1)];
}

/** A stroke's path (the points it shows). */
export function strokePath(s: LineStroke): Pt[] {
	const [from, to] = spanOf(s);
	const out: Pt[] = [];
	for (let i = from; i < to; i++) out.push([s.pts[i * STRIDE], s.pts[i * STRIDE + 1]]);
	return out;
}

/** The point of `pts` nearest `p`, if it is within `r`. */
export function nearestPoint(pts: readonly Pt[], p: Pt, r: number): Pt | null {
	let best: Pt | null = null;
	let bd = r;
	for (const q of pts) {
		const d = Math.hypot(q[0] - p[0], q[1] - p[1]);
		if (d <= bd) [best, bd] = [q, d];
	}
	return best;
}

/** The nearest point on any of the polylines `paths`, if it is within `r` of `p`. */
export function nearestOnPaths(paths: readonly Pt[][], p: Pt, r: number): Pt | null {
	let best: Pt | null = null;
	let bd = r;
	for (const path of paths) {
		if (path.length === 1) {
			const d = Math.hypot(path[0][0] - p[0], path[0][1] - p[1]);
			if (d <= bd) [best, bd] = [path[0], d];
		}
		for (let i = 1; i < path.length; i++) {
			const [a, b] = [path[i - 1], path[i]];
			const [dx, dy] = [b[0] - a[0], b[1] - a[1]];
			const l2 = dx * dx + dy * dy;
			const t = l2 ? Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2)) : 0;
			const q: Pt = [a[0] + dx * t, a[1] + dy * t];
			const d = Math.hypot(q[0] - p[0], q[1] - p[1]);
			if (d <= bd) [best, bd] = [q, d];
		}
	}
	return best;
}

/**
 * The nearest inked pixel (its alpha at least `minAlpha`) of an RGBA picture `w`×`h`
 * within `r` of `p`: the middle of that pixel, or null.
 */
export function nearestInk(
	rgba: ArrayLike<number> | null | undefined,
	w: number,
	h: number,
	p: Pt,
	r: number,
	minAlpha = 96
): Pt | null {
	if (!rgba || rgba.length < w * h * 4) return null;
	const x0 = Math.max(0, Math.floor(p[0] - r));
	const x1 = Math.min(w - 1, Math.ceil(p[0] + r));
	const y0 = Math.max(0, Math.floor(p[1] - r));
	const y1 = Math.min(h - 1, Math.ceil(p[1] + r));
	let best: Pt | null = null;
	let bd = r;
	for (let y = y0; y <= y1; y++)
		for (let x = x0; x <= x1; x++) {
			if (rgba[(y * w + x) * 4 + 3] < minAlpha) continue;
			const c: Pt = [x + 0.5, y + 0.5];
			const d = Math.hypot(c[0] - p[0], c[1] - p[1]);
			if (d <= bd) [best, bd] = [c, d];
		}
	return best;
}
