/**
 * Drawing guides: a grid, perspective (1/2/3 point) and a ruler, plus the
 * "assist" that keeps a stroke on a guide direction (like Procreate's
 * Drawing Assist, turned on per layer). All positions are canvas pixels.
 */
export type Pt = [number, number];

export interface GuideSettings {
	grid: { on: boolean; kind: 'square' | 'isometric'; size: number };
	perspective: {
		/** 0 = off */
		points: 0 | 1 | 2 | 3;
		/** vanishing points (the first `points` are used) */
		vps: Pt[];
	};
	ruler: { on: boolean; a: Pt; b: Pt };
	/** strokes mirrored / rotated live about `c` */
	symmetry: { mode: 'off' | 'vertical' | 'horizontal' | 'quadrant' | 'radial'; n: number; c: Pt };
}

export const SYMMETRY_MODES = ['off', 'vertical', 'horizontal', 'quadrant', 'radial'] as const;

export function defaultGuides(w: number, h: number): GuideSettings {
	return {
		grid: { on: false, kind: 'square', size: Math.round(Math.min(w, h) / 8) },
		perspective: { points: 0, vps: vpLayout(1, w, h) },
		ruler: { on: false, a: [w * 0.2, h * 0.5], b: [w * 0.8, h * 0.5] },
		symmetry: { mode: 'off', n: 6, c: [w / 2, h / 2] }
	};
}

/**
 * Where the vanishing points start, centred on the canvas: 1-point in the middle;
 * 2-point either side of the middle, as far from it; 3-point adds one below the middle.
 */
export function vpLayout(points: number, w: number, h: number): Pt[] {
	return [
		points >= 2 ? [w * 0.15, h * 0.4] : [w * 0.5, h * 0.4],
		[w * 0.85, h * 0.4],
		[w * 0.5, h * 1.6]
	];
}

/** The same guides (which are on, grid kind and size) with every point back in its centred place. */
export function centredGuides(g: GuideSettings, w: number, h: number): GuideSettings {
	const d = defaultGuides(w, h);
	return {
		grid: { ...g.grid },
		perspective: { ...g.perspective, vps: vpLayout(g.perspective.points, w, h) },
		ruler: { ...g.ruler, a: d.ruler.a, b: d.ruler.b },
		symmetry: { ...g.symmetry, c: d.symmetry.c }
	};
}

/** Switching perspective: points still where the old mode put them move to the new mode's places. */
export function withPerspective(
	g: GuideSettings,
	points: 0 | 1 | 2 | 3,
	w: number,
	h: number
): GuideSettings {
	const near = (a: Pt[], b: Pt[]) =>
		a.length === b.length && a.every((p, i) => Math.hypot(p[0] - b[i][0], p[1] - b[i][1]) < 1);
	const untouched =
		near(g.perspective.vps, vpLayout(g.perspective.points, w, h)) ||
		near(g.perspective.vps, vpLayout(points ? 1 : 2, w, h));
	return {
		...g,
		perspective: {
			points,
			vps: untouched ? vpLayout(points, w, h) : g.perspective.vps
		}
	};
}

/** Guides from a saved project, checked (none: anything missing or not a number). */
export function readGuides(v: unknown): GuideSettings | null {
	const pt = (p: unknown): p is Pt =>
		Array.isArray(p) && p.length === 2 && p.every((n) => typeof n === 'number' && isFinite(n));
	const g = v as GuideSettings | null;
	if (!g || typeof g !== 'object') return null;
	const ok =
		typeof g.grid?.on === 'boolean' &&
		(g.grid.kind === 'square' || g.grid.kind === 'isometric') &&
		typeof g.grid.size === 'number' &&
		g.grid.size > 0 &&
		[0, 1, 2, 3].includes(g.perspective?.points) &&
		Array.isArray(g.perspective.vps) &&
		g.perspective.vps.length === 3 &&
		g.perspective.vps.every(pt) &&
		typeof g.ruler?.on === 'boolean' &&
		pt(g.ruler.a) &&
		pt(g.ruler.b) &&
		(SYMMETRY_MODES as readonly string[]).includes(g.symmetry?.mode) &&
		typeof g.symmetry.n === 'number' &&
		pt(g.symmetry.c);
	return ok
		? {
				grid: { ...g.grid },
				perspective: {
					points: g.perspective.points,
					vps: g.perspective.vps.map((p) => [p[0], p[1]])
				},
				ruler: { on: g.ruler.on, a: [g.ruler.a[0], g.ruler.a[1]], b: [g.ruler.b[0], g.ruler.b[1]] },
				symmetry: { mode: g.symmetry.mode, n: g.symmetry.n, c: [g.symmetry.c[0], g.symmetry.c[1]] }
			}
		: null;
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

const ISO_X = Math.cos(Math.PI / 6);
const ISO_Y = Math.sin(Math.PI / 6);

/**
 * Unit directions a stroke starting at `p` may snap to, on a layer with drawing assist:
 * a square grid's across and down, an isometric grid's three axes (upright and ±30°),
 * and the lines to the vanishing points.
 */
export function snapDirections(g: GuideSettings, p: Pt): Pt[] {
	const out: Pt[] = [];
	if (g.grid.on && g.grid.kind === 'square') out.push([1, 0], [0, 1]);
	if (g.grid.on && g.grid.kind === 'isometric') out.push([0, 1], [ISO_X, -ISO_Y], [ISO_X, ISO_Y]);
	const n = g.perspective.points;
	if (n > 0) {
		for (const vp of g.perspective.vps.slice(0, n)) {
			const [dx, dy] = [vp[0] - p[0], vp[1] - p[1]];
			const l = Math.hypot(dx, dy);
			if (l > 1) out.push([dx / l, dy / l]);
		}
		// one point: flat sides are horizontal and vertical; two point: verticals stay vertical
		if (n === 1) out.push([1, 0], [0, 1]);
		if (n === 2) out.push([0, 1]);
	}
	// the same direction twice (a square grid and one-point perspective) only once
	return out.filter((u, i) => out.findIndex((v) => v[0] === u[0] && v[1] === u[1]) === i);
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
