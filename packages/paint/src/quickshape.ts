/**
 * QuickShape (as in Procreate): hold at the end of a stroke and it becomes the
 * clean shape it was trying to be: a line, polyline, triangle, rectangle,
 * circle or ellipse. Plain geometry: path simplification + an ellipse fit.
 */
export type Pt = [number, number];
export interface Shape {
	kind: 'line' | 'polyline' | 'triangle' | 'rectangle' | 'circle' | 'ellipse';
	/** the shape as a path (closed shapes end where they start) */
	points: Pt[];
}

const dist = (a: Pt, b: Pt) => Math.hypot(a[0] - b[0], a[1] - b[1]);

/** distance from p to segment ab */
function segDist(p: Pt, a: Pt, b: Pt) {
	const [dx, dy] = [b[0] - a[0], b[1] - a[1]];
	const l2 = dx * dx + dy * dy;
	const t = l2 ? Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2)) : 0;
	return Math.hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy);
}

/** Ramer–Douglas–Peucker */
export function simplify(pts: Pt[], eps: number): Pt[] {
	if (pts.length < 3) return pts.slice();
	let [far, idx] = [0, 0];
	for (let i = 1; i < pts.length - 1; i++) {
		const d = segDist(pts[i], pts[0], pts[pts.length - 1]);
		if (d > far) [far, idx] = [d, i];
	}
	if (far <= eps) return [pts[0], pts[pts.length - 1]];
	return [...simplify(pts.slice(0, idx + 1), eps).slice(0, -1), ...simplify(pts.slice(idx), eps)];
}

/** points at equal spacing along a path */
export function resample(pts: Pt[], step: number): Pt[] {
	const out: Pt[] = [pts[0]];
	let carry = 0;
	for (let i = 1; i < pts.length; i++) {
		const [a, b] = [pts[i - 1], pts[i]];
		const len = dist(a, b);
		let d = step - carry;
		while (d <= len) {
			const t = d / len;
			out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
			d += step;
		}
		carry = len - (d - step);
	}
	if (dist(out[out.length - 1], pts[pts.length - 1]) > 0.5) out.push(pts[pts.length - 1]);
	return out;
}

const pathLength = (pts: Pt[]) => pts.slice(1).reduce((s, p, i) => s + dist(pts[i], p), 0);

function angleAt(a: Pt, b: Pt, c: Pt) {
	const v1 = [a[0] - b[0], a[1] - b[1]];
	const v2 = [c[0] - b[0], c[1] - b[1]];
	const cos =
		(v1[0] * v2[0] + v1[1] * v2[1]) / (Math.hypot(v1[0], v1[1]) * Math.hypot(v2[0], v2[1]) || 1);
	return (Math.acos(Math.max(-1, Math.min(1, cos))) * 180) / Math.PI;
}

function ellipse(pts: Pt[]): Shape | null {
	const even = resample(pts, Math.max(1, pathLength(pts) / 120));
	const n = even.length;
	const cx = even.reduce((s, p) => s + p[0], 0) / n;
	const cy = even.reduce((s, p) => s + p[1], 0) / n;
	let [sxx, syy, sxy] = [0, 0, 0];
	for (const [x, y] of even) {
		sxx += (x - cx) ** 2;
		syy += (y - cy) ** 2;
		sxy += (x - cx) * (y - cy);
	}
	[sxx, syy, sxy] = [sxx / n, syy / n, sxy / n];
	const tr = sxx + syy;
	const det = sxx * syy - sxy * sxy;
	const l1 = tr / 2 + Math.sqrt(Math.max(0, (tr * tr) / 4 - det));
	const l2 = tr / 2 - Math.sqrt(Math.max(0, (tr * tr) / 4 - det));
	const theta = 0.5 * Math.atan2(2 * sxy, sxx - syy);
	// points spread evenly on an ellipse have variance a²/2 along the major axis
	let a = Math.sqrt(2 * l1);
	let b = Math.sqrt(2 * Math.max(l2, 1e-6));
	if (b < 3) return null;
	const [c, s] = [Math.cos(theta), Math.sin(theta)];
	let err = 0;
	for (const [x, y] of even) {
		const [u, v] = [(x - cx) * c + (y - cy) * s, -(x - cx) * s + (y - cy) * c];
		err += Math.abs(Math.hypot(u / a, v / b) - 1);
	}
	if (err / n > 0.16) return null;
	const round = a / b < 1.15;
	if (round) a = b = (a + b) / 2;
	const out: Pt[] = [];
	for (let i = 0; i <= 72; i++) {
		const t = (i / 72) * Math.PI * 2;
		const [u, v] = [a * Math.cos(t), b * Math.sin(t)];
		out.push([cx + u * c - v * s, cy + u * s + v * c]);
	}
	return { kind: round ? 'circle' : 'ellipse', points: out };
}

/** The clean shape a stroke was trying to be, or null if it isn't clear. */
export function recognise(raw: Pt[]): Shape | null {
	if (raw.length < 4) return null;
	const len = pathLength(raw);
	const xs = raw.map((p) => p[0]);
	const ys = raw.map((p) => p[1]);
	const diag = Math.hypot(Math.max(...xs) - Math.min(...xs), Math.max(...ys) - Math.min(...ys));
	if (len < 20 || diag < 12) return null;
	const [first, last] = [raw[0], raw[raw.length - 1]];
	const closed = dist(first, last) < Math.max(12, 0.18 * diag) && len > 2.2 * diag;
	const simple = simplify(raw, Math.max(3, 0.06 * diag));

	if (!closed) {
		const dev = Math.max(...raw.map((p) => segDist(p, first, last)));
		if (dev <= 0.07 * dist(first, last)) return { kind: 'line', points: [first, last] };
		if (simple.length <= 6) {
			const mean =
				raw.reduce(
					(s, p) => s + Math.min(...simple.slice(1).map((q, i) => segDist(p, simple[i], q))),
					0
				) / raw.length;
			if (mean < 0.035 * diag) return { kind: 'polyline', points: simple };
		}
		return null;
	}
	// closed: corners of the simplified loop (drop the closing duplicate)
	const loop =
		dist(simple[0], simple[simple.length - 1]) < Math.max(12, 0.18 * diag)
			? simple.slice(0, -1)
			: simple;
	const corner = (i: number) =>
		angleAt(loop[(i - 1 + loop.length) % loop.length], loop[i], loop[(i + 1) % loop.length]);
	if (loop.length === 3) return { kind: 'triangle', points: [...loop, loop[0]] };
	if (loop.length === 4 && [0, 1, 2, 3].every((i) => Math.abs(corner(i) - 90) < 25))
		return { kind: 'rectangle', points: [...loop, loop[0]] };
	return ellipse(raw);
}
