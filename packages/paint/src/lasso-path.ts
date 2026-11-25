/**
 * The lasso fill's outline: freehand runs mixed with straight edges between clicked
 * corners, smoothed (freehand parts only, corners kept sharp) before it fills.
 * Pure geometry, no canvas: `PaintCanvas` keeps the points and fills the outline.
 */

export type Pt = [number, number];

/** A point of the outline: a clicked corner (straight edges meet there) or freehand. */
export interface LassoPoint {
	p: Pt;
	corner: boolean;
}

const dist = (a: Pt, b: Pt) => Math.hypot(b[0] - a[0], b[1] - a[1]);

/** Points along a polyline every `step` (its ends kept). */
export function resample(line: Pt[], step: number): Pt[] {
	if (line.length < 2 || step <= 0) return line.slice();
	const out: Pt[] = [line[0]];
	let carry = 0;
	for (let i = 1; i < line.length; i++) {
		const [a, b] = [line[i - 1], line[i]];
		const d = dist(a, b);
		let t = step - carry;
		while (t <= d) {
			out.push([a[0] + ((b[0] - a[0]) * t) / d, a[1] + ((b[1] - a[1]) * t) / d]);
			t += step;
		}
		carry = d - (t - step);
	}
	const last = line.at(-1)!;
	if (dist(out.at(-1)!, last) > step * 0.25) out.push(last);
	else out[out.length - 1] = last;
	return out;
}

/** Each point averaged with `k` neighbours a side (Gaussian weights); ends stay put. */
function smoothOpen(pts: Pt[], k: number): Pt[] {
	const n = pts.length;
	return pts.map((p, i) => {
		// fewer neighbours near the ends, the same on both sides: the ends don't move
		const r = Math.min(k, i, n - 1 - i);
		if (r <= 0) return p;
		return average(pts, i, r, (j) => pts[j]);
	});
}

/** The same around a closed loop. */
function smoothClosed(pts: Pt[], k: number): Pt[] {
	const n = pts.length;
	const r = Math.min(k, Math.floor((n - 1) / 2));
	if (r <= 0) return pts.slice();
	return pts.map((_, i) => average(pts, i, r, (j) => pts[((j % n) + n) % n]));
}

function average(pts: Pt[], i: number, r: number, at: (j: number) => Pt): Pt {
	const s = r / 2;
	let [x, y, w] = [0, 0, 0];
	for (let j = i - r; j <= i + r; j++) {
		const g = Math.exp(-((j - i) ** 2) / (2 * s * s));
		const q = at(j);
		x += q[0] * g;
		y += q[1] * g;
		w += g;
	}
	return [x / w, y / w];
}

/**
 * The closed outline to fill. `radius` (canvas px, 0 = off) smooths the freehand runs;
 * straight edges and corners stay as they are. With no corners at all (one freehand
 * loop) the whole loop is smoothed, round where it closes too.
 */
export function lassoOutline(pts: LassoPoint[], radius: number): Pt[] {
	if (radius <= 0 || pts.length < 3) return pts.map((q) => q.p);
	const step = Math.max(0.5, radius / 8);
	const k = Math.max(1, Math.round(radius / step));
	const first = pts.findIndex((q) => q.corner);
	if (first < 0) {
		const loop = resample([...pts.map((q) => q.p), pts[0].p], step);
		loop.pop(); // (back at the start)
		return smoothClosed(loop, k);
	}
	// from the first corner round to it again, a chain at a time
	const ring = [...pts.slice(first), ...pts.slice(0, first), pts[first]];
	const out: Pt[] = [];
	let from = 0;
	for (let i = 1; i < ring.length; i++) {
		if (!ring[i].corner && i < ring.length - 1) continue;
		const chain = ring.slice(from, i + 1).map((q) => q.p);
		const part = chain.length > 2 ? smoothOpen(resample(chain, step), k) : chain;
		out.push(...part.slice(0, -1));
		from = i;
	}
	return out;
}

/** Backspace: the last corner goes, or the whole freehand run that led to it. */
export function lassoBack(pts: LassoPoint[]): LassoPoint[] {
	const out = pts.slice(0, -1);
	while (out.length > 1 && !out.at(-1)!.corner) out.pop();
	return out;
}

/** Hard edges: every pixel all in or all out (half covered or more is in). */
export function hardEdges(coverage: Uint8Array): Uint8Array {
	for (let i = 0; i < coverage.length; i++) coverage[i] = coverage[i] >= 128 ? 255 : 0;
	return coverage;
}
