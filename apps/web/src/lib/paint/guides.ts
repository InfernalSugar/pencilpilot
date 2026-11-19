/**
 * Drawing guides: a grid, perspective (1/2/3 point) and a ruler, plus the
 * "assist" that keeps a stroke on a guide direction (like Procreate's
 * Drawing Assist). All positions are canvas pixels.
 */
export type Pt = [number, number];

export interface GuideSettings {
	grid: { on: boolean; kind: 'square' | 'isometric'; size: number };
	perspective: {
		/** 0 = off */
		points: 0 | 1 | 2 | 3;
		/** vanishing points (the first `points` are used) */
		vps: Pt[];
		/** strokes snap to the perspective directions */
		assist: boolean;
	};
	ruler: { on: boolean; a: Pt; b: Pt };
	/** strokes mirrored / rotated live about `c` */
	symmetry: { mode: 'off' | 'vertical' | 'horizontal' | 'quadrant' | 'radial'; n: number; c: Pt };
}

export const SYMMETRY_MODES = ['off', 'vertical', 'horizontal', 'quadrant', 'radial'] as const;

export function defaultGuides(w: number, h: number): GuideSettings {
	return {
		grid: { on: false, kind: 'square', size: Math.round(Math.min(w, h) / 8) },
		perspective: {
			points: 0,
			vps: [
				[w * 0.5, h * 0.4],
				[w * 1.1, h * 0.4],
				[w * 0.5, h * 1.6]
			],
			assist: true
		},
		ruler: { on: false, a: [w * 0.2, h * 0.5], b: [w * 0.8, h * 0.5] },
		symmetry: { mode: 'off', n: 6, c: [w / 2, h / 2] }
	};
}

export const anyGuide = (g: GuideSettings | null | undefined) =>
	!!g && (g.grid.on || g.perspective.points > 0 || g.ruler.on || symmetryOn(g));

export const symmetryOn = (g: GuideSettings | null | undefined) =>
	!!g?.symmetry && g.symmetry.mode !== 'off';

/** The symmetry axes to draw, as [x1, y1, x2, y2] (long lines through the centre). */
export function symmetryAxes(
	g: GuideSettings,
	w: number,
	h: number
): [number, number, number, number][] {
	if (!symmetryOn(g)) return [];
	const [cx, cy] = g.symmetry.c;
	const far = 2 * Math.hypot(w, h);
	const at = (a: number): [number, number, number, number] => [
		cx - Math.cos(a) * far,
		cy - Math.sin(a) * far,
		cx + Math.cos(a) * far,
		cy + Math.sin(a) * far
	];
	switch (g.symmetry.mode) {
		case 'vertical':
			return [at(Math.PI / 2)];
		case 'horizontal':
			return [at(0)];
		case 'quadrant':
			return [at(0), at(Math.PI / 2)];
		default: {
			// spokes from the centre
			const n = Math.max(2, Math.min(24, g.symmetry.n));
			return Array.from({ length: n }, (_, k) => {
				const a = (k / n) * Math.PI * 2 - Math.PI / 2;
				return [cx, cy, cx + Math.cos(a) * far, cy + Math.sin(a) * far];
			});
		}
	}
}

/** Unit directions a stroke starting at `p` may snap to (perspective assist). */
export function snapDirections(g: GuideSettings, p: Pt): Pt[] {
	const out: Pt[] = [];
	const n = g.perspective.points;
	if (n > 0 && g.perspective.assist) {
		for (const vp of g.perspective.vps.slice(0, n)) {
			const [dx, dy] = [vp[0] - p[0], vp[1] - p[1]];
			const l = Math.hypot(dx, dy);
			if (l > 1) out.push([dx / l, dy / l]);
		}
		// one point: flat sides are horizontal and vertical; two point: verticals stay vertical
		if (n === 1) out.push([1, 0], [0, 1]);
		if (n === 2) out.push([0, 1]);
	}
	return out;
}

/** The direction (of `dirs`, either way along it) closest to the movement `d`. */
export function closestDirection(dirs: Pt[], d: Pt): Pt | null {
	const l = Math.hypot(d[0], d[1]);
	if (!dirs.length || l === 0) return null;
	let best: Pt | null = null;
	let bestDot = -1;
	for (const u of dirs) {
		const dot = Math.abs((u[0] * d[0] + u[1] * d[1]) / l);
		if (dot > bestDot) [best, bestDot] = [u, dot];
	}
	return best;
}

/** p projected onto the line through `origin` along unit `u`. */
export const project = (origin: Pt, u: Pt, p: Pt): Pt => {
	const t = (p[0] - origin[0]) * u[0] + (p[1] - origin[1]) * u[1];
	return [origin[0] + t * u[0], origin[1] + t * u[1]];
};

/** If `p` is within `reach` of the ruler, the ruler line (origin on it + unit direction). */
export function rulerLine(g: GuideSettings, p: Pt, reach: number): { origin: Pt; u: Pt } | null {
	if (!g.ruler.on) return null;
	const { a, b } = g.ruler;
	const [dx, dy] = [b[0] - a[0], b[1] - a[1]];
	const l = Math.hypot(dx, dy);
	if (l < 1) return null;
	const u: Pt = [dx / l, dy / l];
	const t = (p[0] - a[0]) * u[0] + (p[1] - a[1]) * u[1];
	if (t < -reach || t > l + reach) return null;
	const foot: Pt = [a[0] + t * u[0], a[1] + t * u[1]];
	return Math.hypot(p[0] - foot[0], p[1] - foot[1]) <= reach ? { origin: foot, u } : null;
}

/** Grid lines as [x1, y1, x2, y2] segments covering the canvas. */
export function gridLines(
	g: GuideSettings,
	w: number,
	h: number
): [number, number, number, number][] {
	const s = Math.max(4, g.grid.size);
	const out: [number, number, number, number][] = [];
	if (g.grid.kind === 'square') {
		for (let x = s; x < w; x += s) out.push([x, 0, x, h]);
		for (let y = s; y < h; y += s) out.push([0, y, w, y]);
		return out;
	}
	// isometric: verticals plus lines at ±30°
	const step = s;
	for (let x = step; x < w; x += step) out.push([x, 0, x, h]);
	const k = Math.tan(Math.PI / 6);
	const span = w * k + h;
	for (let c = -span; c < span + h; c += step / Math.cos(Math.PI / 6)) {
		out.push([0, c, w, c - w * k]);
		out.push([0, c - w * k, w, c]);
	}
	return out;
}

/** Rays from a vanishing point, for drawing the perspective guide. */
export function perspectiveRays(
	vp: Pt,
	w: number,
	h: number,
	n = 28
): [number, number, number, number][] {
	const far = 2 * Math.hypot(w, h);
	return Array.from({ length: n }, (_, i) => {
		const a = (i / n) * Math.PI * 2;
		return [vp[0], vp[1], vp[0] + Math.cos(a) * far, vp[1] + Math.sin(a) * far];
	});
}
