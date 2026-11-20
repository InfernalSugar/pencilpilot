/**
 * Editable QuickShapes (as in Procreate's "Edit Shape"): after a stroke snaps to
 * a clean shape, its points can be dragged until the next stroke.
 */
import type { Shape } from './quickshape';

export type Pt = [number, number];

export type EditShape =
	/** lines, polylines, triangles, rectangles: dragging moves one corner */
	| { kind: 'poly'; pts: Pt[]; closed: boolean; pressure: number }
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

/** The points to draw. */
export function shapePath(s: EditShape): Pt[] {
	if (s.kind === 'poly') return s.closed ? [...s.pts, s.pts[0]] : s.pts;
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
