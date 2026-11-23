/**
 * Editable QuickShapes (as in Procreate's "Edit Shape"): after a stroke snaps to
 * a clean shape, its points can be dragged until the next stroke.
 */
import type { Shape } from './quickshape';

export type Pt = [number, number];

export type EditShape =
	/** lines, polylines, triangles, rectangles: dragging moves one corner */
	| {
			kind: 'poly';
			pts: Pt[];
			closed: boolean;
			pressure: number;
			/** a smooth curve through the corners instead of straight lines */
			smooth?: boolean;
	  }
	/** circles and ellipses: centre, end of the first axis, end of the second */
	| {
			kind: 'ellipse';
			c: Pt;
			a: number;
			b: number;
			/** radians, the first axis's direction */
			theta: number;
			circle: boolean;
			pressure: number;
	  };

const dist = (p: Pt, q: Pt) => Math.hypot(p[0] - q[0], p[1] - q[1]);

/** The editable form of a snapped shape. */
export function editable(s: Shape, pressure: number): EditShape {
	if (s.kind === 'circle' || s.kind === 'ellipse') {
		// QuickShape draws 73 points from t = 0 (end of the first axis) round to 2π
		const ring = s.points.slice(0, -1);
		const c: Pt = [
			ring.reduce((t, p) => t + p[0], 0) / ring.length,
			ring.reduce((t, p) => t + p[1], 0) / ring.length
		];
		const p0 = s.points[0];
		const q = s.points[Math.round(s.points.length / 4)];
		return {
			kind: 'ellipse',
			c,
			a: dist(c, p0),
			b: dist(c, q),
			theta: Math.atan2(p0[1] - c[1], p0[0] - c[0]),
			circle: s.kind === 'circle',
			pressure
		};
	}
	const closed = s.kind === 'triangle' || s.kind === 'rectangle';
	return {
		kind: 'poly',
		pts: (closed ? s.points.slice(0, -1) : s.points).map((p) => [p[0], p[1]] as Pt),
		closed,
		pressure
	};
}

/** Whether the shape can be curved (it has a corner between lines). */
export const curvable = (s: EditShape) => s.kind === 'poly' && s.pts.length >= 3;

/**
 * A smooth curve through the points (centripetal Catmull–Rom: no loops or overshoot at
 * sharp corners), `per` points along each piece; a closed one comes back round to the start.
 */
export function curveThrough(pts: Pt[], closed: boolean, per = 16): Pt[] {
	const n = pts.length;
	if (n < 3) return closed && n ? [...pts, pts[0]] : pts;
	const at = (i: number): Pt =>
		closed
			? pts[(i + n) % n]
			: i < 0
				? mirror(pts[0], pts[1])
				: i >= n
					? mirror(pts[n - 1], pts[n - 2])
					: pts[i];
	const out: Pt[] = [];
	const pieces = closed ? n : n - 1;
	for (let i = 0; i < pieces; i++) {
		const [p0, p1, p2, p3] = [at(i - 1), at(i), at(i + 1), at(i + 2)];
		const t0 = 0;
		const t1 = t0 + knot(p0, p1);
		const t2 = t1 + knot(p1, p2);
		const t3 = t2 + knot(p2, p3);
		for (let k = 0; k < per; k++) {
			const t = t1 + ((t2 - t1) * k) / per;
			out.push(cr(p0, p1, p2, p3, t0, t1, t2, t3, t));
		}
	}
	out.push(closed ? pts[0] : pts[n - 1]);
	return out;
}
/** an extra point beyond an open end, so the curve leaves the end along its line */
const mirror = (end: Pt, next: Pt): Pt => [2 * end[0] - next[0], 2 * end[1] - next[1]];
const knot = (p: Pt, q: Pt) => Math.max(1e-6, Math.sqrt(dist(p, q)));
function cr(
	p0: Pt,
	p1: Pt,
	p2: Pt,
	p3: Pt,
	t0: number,
	t1: number,
	t2: number,
	t3: number,
	t: number
): Pt {
	const lerp = (a: Pt, b: Pt, ta: number, tb: number): Pt => {
		const u = (t - ta) / (tb - ta);
		return [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u];
	};
	const a1 = lerp(p0, p1, t0, t1);
	const a2 = lerp(p1, p2, t1, t2);
	const a3 = lerp(p2, p3, t2, t3);
	const b1 = lerp(a1, a2, t0, t2);
	const b2 = lerp(a2, a3, t1, t3);
	return lerp(b1, b2, t1, t2);
}

/** The points to draw. */
export function shapePath(s: EditShape): Pt[] {
	if (s.kind === 'poly') {
		if (s.smooth && s.pts.length >= 3) return curveThrough(s.pts, s.closed);
		return s.closed ? [...s.pts, s.pts[0]] : s.pts;
	}
	const [cs, sn] = [Math.cos(s.theta), Math.sin(s.theta)];
	return Array.from({ length: 73 }, (_, i) => {
		const t = (i / 72) * Math.PI * 2;
		const [u, v] = [s.a * Math.cos(t), s.b * Math.sin(t)];
		return [s.c[0] + u * cs - v * sn, s.c[1] + u * sn + v * cs] as Pt;
	});
}

/** Where the drag handles sit. */
export function shapeHandles(s: EditShape): Pt[] {
	if (s.kind === 'poly') return s.pts;
	const [cs, sn] = [Math.cos(s.theta), Math.sin(s.theta)];
	return [s.c, [s.c[0] + s.a * cs, s.c[1] + s.a * sn], [s.c[0] - s.b * sn, s.c[1] + s.b * cs]];
}

/** The shape with handle `i` dragged to `p`. */
export function moveHandle(s: EditShape, i: number, p: Pt): EditShape {
	if (s.kind === 'poly') return { ...s, pts: s.pts.map((q, k) => (k === i ? p : q)) };
	if (i === 0) return { ...s, c: p };
	const r = Math.max(2, dist(s.c, p));
	if (i === 1) {
		const theta = Math.atan2(p[1] - s.c[1], p[0] - s.c[0]);
		return { ...s, theta, a: r, b: s.circle ? r : s.b };
	}
	return { ...s, b: r, a: s.circle ? r : s.a };
}

/** What a shape is, in words (for its Edit Shape buttons). */
export function shapeName(
	s: EditShape
): 'circle' | 'ellipse' | 'square' | 'rectangle' | 'triangle' | 'line' | 'polyline' {
	if (s.kind === 'ellipse') return s.circle ? 'circle' : 'ellipse';
	if (!s.closed) return s.pts.length === 2 ? 'line' : 'polyline';
	if (s.pts.length === 3) return 'triangle';
	if (s.pts.length === 4) {
		const e = s.pts.map((p, i) => dist(p, s.pts[(i + 1) % 4]));
		return Math.max(...e) - Math.min(...e) < 0.5 ? 'square' : 'rectangle';
	}
	return 'polyline';
}

/**
 * The perfect form of a snapped shape (Procreate: tap with another finger while
 * holding): an ellipse becomes a circle, a rectangle a square, a triangle an
 * equilateral one, and a line snaps to the nearest 15°. Centres stay where they are.
 */
export function perfect(s: EditShape): EditShape {
	if (s.kind === 'ellipse') {
		const r = (s.a + s.b) / 2;
		return { ...s, a: r, b: r, circle: true };
	}
	const n = s.pts.length;
	if (!s.closed && n === 2) {
		const [p, q] = s.pts;
		const len = dist(p, q);
		const step = Math.PI / 12;
		const ang = Math.round(Math.atan2(q[1] - p[1], q[0] - p[0]) / step) * step;
		return { ...s, pts: [p, [p[0] + len * Math.cos(ang), p[1] + len * Math.sin(ang)]] };
	}
	if (!s.closed || (n !== 3 && n !== 4)) return s;
	// a regular shape on the same centre, as big on average, turned like the first corner
	const c: Pt = [s.pts.reduce((t, p) => t + p[0], 0) / n, s.pts.reduce((t, p) => t + p[1], 0) / n];
	const r = s.pts.reduce((t, p) => t + dist(c, p), 0) / n;
	const a0 = Math.atan2(s.pts[0][1] - c[1], s.pts[0][0] - c[0]);
	return {
		...s,
		pts: Array.from({ length: n }, (_, i) => {
			const a = a0 + (i * 2 * Math.PI) / n;
			return [c[0] + r * Math.cos(a), c[1] + r * Math.sin(a)] as Pt;
		})
	};
}
