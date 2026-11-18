/**
 * Plain-maths stroke metrics for the line drills (no AI, see ADR-0002).
 *
 * - **straightness**: the largest distance of any point from the straight chord
 *   between the stroke's ends, divided by the chord length. 0 = ruler-straight.
 * - **endpoint error**: how far the stroke's two ends are from the target dots
 *   (best pairing, mean distance), divided by the dot-to-dot distance.
 * - **angle error**: degrees between the stroke's direction (least-squares line
 *   through all its points) and the target line, ignoring direction (0–90°).
 * - **taper**: how the stroke's pressure (≈ its width) changes from the thick end
 *   to the thin end: the drop from the first quarter to the last quarter, and how
 *   steadily it falls (see `taperMetrics`).
 *
 * Each is turned into a 0–1 score with a fixed, documented tolerance, then blended
 * with the grader's 1–10 score in `roundScore`.
 */
import type { Stroke, StrokePoint } from './paint/strokes';
import type { Pt } from './drills';

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
const dist = (a: Pt, b: Pt) => Math.hypot(b.x - a.x, b.y - a.y);

export function pathLength(pts: readonly Pt[]) {
	let s = 0;
	for (let i = 1; i < pts.length; i++) s += dist(pts[i - 1], pts[i]);
	return s;
}

/** The longest visible (non-eraser) stroke's points, with pressure. */
export function mainStrokePoints(strokes: readonly Stroke[]): StrokePoint[] | null {
	let best: StrokePoint[] | null = null;
	let bestLen = 0;
	for (const s of strokes) {
		if (s.eraser || s.points.length < 2) continue;
		const len = pathLength(s.points);
		if (len > bestLen) [best, bestLen] = [s.points, len];
	}
	return best;
}

/** The longest visible (non-eraser) stroke: the one the student meant as "the line". */
export function mainStroke(strokes: readonly Stroke[]): Pt[] | null {
	return mainStrokePoints(strokes)?.map(({ x, y }) => ({ x, y })) ?? null;
}

/** Max deviation from the end-to-end chord ÷ chord length (0 = perfectly straight). */
export function straightness(pts: readonly Pt[]): number {
	if (pts.length < 2) return 1;
	const a = pts[0];
	const b = pts[pts.length - 1];
	const len = dist(a, b);
	if (len < 1) return 1;
	let max = 0;
	for (const p of pts) {
		const d = Math.abs((b.x - a.x) * (a.y - p.y) - (a.x - p.x) * (b.y - a.y)) / len;
		if (d > max) max = d;
	}
	return max / len;
}

/** Mean distance from the stroke's ends to the two dots (either order) ÷ dot distance. */
export function endpointError(pts: readonly Pt[], a: Pt, b: Pt): number {
	if (pts.length < 1) return 1;
	const s = pts[0];
	const e = pts[pts.length - 1];
	const span = Math.max(1, dist(a, b));
	const forward = (dist(s, a) + dist(e, b)) / 2;
	const backward = (dist(s, b) + dist(e, a)) / 2;
	return Math.min(forward, backward) / span;
}

/** Direction of the least-squares line through the points, degrees in [0, 180), screen coords. */
export function strokeAngle(pts: readonly Pt[]): number {
	const k = pts.length;
	const mx = pts.reduce((s, p) => s + p.x, 0) / k;
	const my = pts.reduce((s, p) => s + p.y, 0) / k;
	let sxx = 0;
	let syy = 0;
	let sxy = 0;
	for (const p of pts) {
		sxx += (p.x - mx) ** 2;
		syy += (p.y - my) ** 2;
		sxy += (p.x - mx) * (p.y - my);
	}
	const deg = (0.5 * Math.atan2(2 * sxy, sxx - syy) * 180) / Math.PI;
	return ((deg % 180) + 180) % 180;
}

/** Smallest difference between two undirected line angles, 0–90°. */
export function angleError(a: number, b: number): number {
	const d = Math.abs((((a - b) % 180) + 180) % 180);
	return Math.min(d, 180 - d);
}

/** Tolerances: at or below `good` scores 1, at or beyond `bad` scores 0, linear between. */
export const TOLERANCE = {
	straightness: { good: 0.01, bad: 0.08 },
	endpoint: { good: 0.02, bad: 0.22 },
	angle: { good: 1, bad: 20 },
	/** pressure drop, first quarter → last quarter, as a share of the start pressure (higher is better) */
	taperDrop: { good: 0.6, bad: 0.05 }
} as const;

const fromTolerance = (v: number, t: { good: number; bad: number }) =>
	clamp01(1 - (v - t.good) / (t.bad - t.good));

export interface TaperMetrics {
	/** (mean start pressure − mean end pressure) ÷ mean start pressure; ≤ 0 means no taper (or the wrong way) */
	drop: number;
	/** 0–1: share of the smoothed pressure changes that go down (1 = it only ever gets lighter) */
	steadiness: number;
	/** 0–1: 75% drop + 25% steadiness */
	score: number;
}

/** Pressures that barely change (a mouse reports a flat 0.5) can't show a taper. */
export const MIN_PRESSURE_RANGE = 0.03;

/**
 * Taper of one stroke, read from its recorded pen pressure (the brush width follows it).
 * The stroke is oriented so it starts at the end nearest `thick` (when given), then split
 * by length: the drop compares the mean pressure of the first and last quarters, and the
 * steadiness looks at every step of a 5-point moving average. Returns null when the
 * pressure is flat (no pressure-sensitive device).
 */
export function taperMetrics(points: readonly StrokePoint[], thick?: Pt): TaperMetrics | null {
	if (points.length < 4) return null;
	let pts = [...points];
	if (thick && dist(pts.at(-1)!, thick) < dist(pts[0], thick)) pts = pts.reverse();
	const ps = pts.map((p) => p.pressure);
	if (Math.max(...ps) - Math.min(...ps) < MIN_PRESSURE_RANGE) return null;
	// arc length at each point, to split the stroke into quarters by distance, not sample count
	const along = [0];
	for (let i = 1; i < pts.length; i++) along.push(along[i - 1] + dist(pts[i - 1], pts[i]));
	const total = along.at(-1)! || 1;
	const mean = (from: number, to: number) => {
		const v = ps.filter((_, i) => along[i] / total >= from && along[i] / total <= to);
		return v.length ? v.reduce((s, x) => s + x, 0) / v.length : 0;
	};
	const start = mean(0, 0.25);
	const end = mean(0.75, 1);
	const drop = start > 0 ? (start - end) / start : 0;
	const smooth = ps.map((_, i) => {
		const win = ps.slice(Math.max(0, i - 2), i + 3);
		return win.reduce((s, x) => s + x, 0) / win.length;
	});
	let down = 0;
	let up = 0;
	for (let i = 1; i < smooth.length; i++) {
		const d = smooth[i] - smooth[i - 1];
		if (d < 0) down -= d;
		else up += d;
	}
	const steadiness = down + up > 0 ? down / (down + up) : 0;
	const dropScore = clamp01(
		(drop - TOLERANCE.taperDrop.bad) / (TOLERANCE.taperDrop.good - TOLERANCE.taperDrop.bad)
	);
	return { drop, steadiness, score: 0.75 * dropScore + 0.25 * steadiness };
}

export interface LineMetrics {
	straightness: number;
	endpointError?: number;
	angleError?: number;
	/** line-weight drills: null when the device reports no pressure */
	taper?: TaperMetrics | null;
	/** 0–1 blend of the above */
	score: number;
}

/**
 * Line drill metrics for the main stroke.
 * ghosted-lines: 50% straightness + 50% endpoints. angle-matching: 65% angle + 35% straightness.
 * line-weight-pressure: 50% taper + 25% straightness + 25% endpoints; without pressure data
 * (a mouse) it falls back to 50% straightness + 50% endpoints.
 */
export function lineMetrics(
	strokes: readonly Stroke[],
	target: { segment?: { a: Pt; b: Pt }; angle?: number; taper?: { a: Pt; b: Pt } }
): LineMetrics | null {
	const raw = mainStrokePoints(strokes);
	if (!raw) return null;
	const pts = raw.map(({ x, y }) => ({ x, y }));
	const s = straightness(pts);
	const sScore = fromTolerance(s, TOLERANCE.straightness);
	if (target.taper) {
		const e = endpointError(pts, target.taper.a, target.taper.b);
		const eScore = fromTolerance(e, TOLERANCE.endpoint);
		const taper = taperMetrics(raw, target.taper.a);
		return {
			straightness: s,
			endpointError: e,
			taper,
			score: taper ? 0.5 * taper.score + 0.25 * sScore + 0.25 * eScore : 0.5 * sScore + 0.5 * eScore
		};
	}
	if (target.segment) {
		const e = endpointError(pts, target.segment.a, target.segment.b);
		return {
			straightness: s,
			endpointError: e,
			score: 0.5 * sScore + 0.5 * fromTolerance(e, TOLERANCE.endpoint)
		};
	}
	if (target.angle !== undefined) {
		const err = angleError(strokeAngle(pts), target.angle);
		return {
			straightness: s,
			angleError: err,
			score: 0.65 * fromTolerance(err, TOLERANCE.angle) + 0.35 * sScore
		};
	}
	return { straightness: s, score: sScore };
}

/** Share of the round score that comes from the line metrics (the rest is the grader). */
export const LINE_METRIC_SHARE = 0.6;

/** Round score 1–10: the grader's score, blended with the line metrics when there are any. */
export function roundScore(graderScore: number, line: LineMetrics | null | undefined): number {
	const s = line
		? (1 - LINE_METRIC_SHARE) * graderScore + LINE_METRIC_SHARE * (1 + 9 * line.score)
		: graderScore;
	return Math.round(Math.min(10, Math.max(1, s)) * 10) / 10;
}
