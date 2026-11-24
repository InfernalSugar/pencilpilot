/**
 * Drawing guides: a grid, perspective (1/2/3 point), a ruler and composition overlays, plus the
 * "assist" that keeps a stroke on a guide direction (like Procreate's
 * Drawing Assist, turned on per layer). All positions are canvas pixels.
 */
import {
	defShapes,
	flipped,
	moved,
	PHI_CUT,
	readCompositionDef,
	spiral,
	type Arc,
	type CompositionDef,
	type Seg,
	type Shapes
} from './composition-elements';
export { arcsPath, PHI_CUT } from './composition-elements';
export type { Arc, Ellipse, Seg, Shapes } from './composition-elements';

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
	/** a composition overlay fitted to the canvas (only shown, never drawn into the picture) */
	composition: Composition;
}

export const COMPOSITIONS = [
	'off',
	'thirds',
	'phi',
	'spiral',
	'triangles',
	'diagonals',
	'cross',
	'armature',
	'safe'
] as const;
export type CompositionPreset = (typeof COMPOSITIONS)[number];
export interface Composition {
	/** a built-in guide, or 'custom': one of your own (`custom`) */
	preset: CompositionPreset | 'custom';
	/** mirrored left to right / top to bottom (the spiral and triangles) */
	flipX: boolean;
	flipY: boolean;
	/**
	 * Where it sits: its middle moved from the canvas's middle by this share of the
	 * canvas's width and height, and its size as a share of the canvas's (it fills the
	 * canvas when left out).
	 */
	offset?: Pt;
	scale?: number;
	/** a guide of your own, kept whole here so the project opens the same anywhere */
	custom?: CompositionDef;
}

/** no composition guide */
export const NO_COMPOSITION: Composition = {
	preset: 'off',
	flipX: false,
	flipY: false,
	offset: [0, 0],
	scale: 1
};

export const SYMMETRY_MODES = ['off', 'vertical', 'horizontal', 'quadrant', 'radial'] as const;

export function defaultGuides(w: number, h: number): GuideSettings {
	return {
		grid: { on: false, kind: 'square', size: Math.round(Math.min(w, h) / 8) },
		perspective: { points: 0, vps: vpLayout(1, w, h) },
		ruler: { on: false, a: [w * 0.2, h * 0.5], b: [w * 0.8, h * 0.5] },
		symmetry: { mode: 'off', n: 6, c: [w / 2, h / 2] },
		composition: { ...NO_COMPOSITION, offset: [0, 0] }
	};
}

/**
 * Where the vanishing points start, centred on the canvas: 1-point in the middle;
 * 2-point either side of the middle, as far from it; 3-point adds one below the middle,
 * just inside the canvas's bottom edge (further down it would start out of sight).
 */
export function vpLayout(points: number, w: number, h: number): Pt[] {
	return [
		points >= 2 ? [w * 0.15, h * 0.4] : [w * 0.5, h * 0.4],
		[w * 0.85, h * 0.4],
		[w * 0.5, h * 0.9]
	];
}

/** Where 3-point's third point used to start (below the canvas): still counts as not moved. */
const oldLayout = (w: number, h: number): Pt[] => [
	[w * 0.15, h * 0.4],
	[w * 0.85, h * 0.4],
	[w * 0.5, h * 1.6]
];

/**
 * A handle for a point off the canvas: pinned to the canvas edge (`inset` inside it) on
 * the line from the middle to the point, with the direction it lies in. Null when on the canvas.
 */
export function edgePin(p: Pt, w: number, h: number, inset = 0): { at: Pt; angle: number } | null {
	if (p[0] >= 0 && p[0] <= w && p[1] >= 0 && p[1] <= h) return null;
	const [cx, cy] = [w / 2, h / 2];
	const [dx, dy] = [p[0] - cx, p[1] - cy];
	const hx = Math.max(0, w / 2 - inset);
	const hy = Math.max(0, h / 2 - inset);
	const t = Math.min(dx ? hx / Math.abs(dx) : Infinity, dy ? hy / Math.abs(dy) : Infinity);
	return { at: [cx + dx * t, cy + dy * t], angle: Math.atan2(dy, dx) };
}

/** The same guides (which are on, grid kind and size) with every point back in its centred place. */
export function centredGuides(g: GuideSettings, w: number, h: number): GuideSettings {
	const d = defaultGuides(w, h);
	return {
		grid: { ...g.grid },
		perspective: { ...g.perspective, vps: vpLayout(g.perspective.points, w, h) },
		ruler: { ...g.ruler, a: d.ruler.a, b: d.ruler.b },
		symmetry: { ...g.symmetry, c: d.symmetry.c },
		composition: { ...(g.composition ?? d.composition) }
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
		near(g.perspective.vps, vpLayout(points ? 1 : 2, w, h)) ||
		near(g.perspective.vps, oldLayout(w, h));
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
				symmetry: { mode: g.symmetry.mode, n: g.symmetry.n, c: [g.symmetry.c[0], g.symmetry.c[1]] },
				composition: readComposition(g.composition)
			}
		: null;
}

/**
 * A saved composition overlay (projects from before it, or anything odd: off). A guide of
 * your own is checked element by element; where it sits is held to sensible limits.
 */
export function readComposition(v: unknown): Composition {
	const c = v as Partial<Composition> | null;
	const custom = c?.preset === 'custom' ? readCompositionDef(c.custom) : null;
	const ok =
		!!c &&
		typeof c === 'object' &&
		(COMPOSITIONS.includes(c.preset as CompositionPreset) || !!custom);
	const num = (v: unknown, a: number, b: number, d: number) =>
		typeof v === 'number' && isFinite(v) ? Math.max(a, Math.min(b, v)) : d;
	const o = ok && Array.isArray(c.offset) ? c.offset : [0, 0];
	return {
		preset: ok ? c.preset! : 'off',
		flipX: ok && c.flipX === true,
		flipY: ok && c.flipY === true,
		offset: [num(o[0], -1, 1, 0), num(o[1], -1, 1, 0)],
		scale: ok ? num(c.scale, MIN_SCALE, MAX_SCALE, 1) : 1,
		...(custom ? { custom } : {})
	};
}

/** how small or big a composition guide can be made on the canvas (of the canvas's size) */
export const MIN_SCALE = 0.1;
export const MAX_SCALE = 4;

/** the guide has been moved or resized on the canvas */
export const compositionPlaced = (c: Composition | null | undefined) =>
	!!c && (!!c.offset?.[0] || !!c.offset?.[1] || (c.scale ?? 1) !== 1);

/** The rectangle a composition guide fills on a w × h canvas: [x, y, width, height]. */
export function compositionFrame(
	c: Composition,
	w: number,
	h: number
): [number, number, number, number] {
	const s = c.scale ?? 1;
	const [ox, oy] = c.offset ?? [0, 0];
	const [fw, fh] = [w * s, h * s];
	return [w * (0.5 + ox) - fw / 2, h * (0.5 + oy) - fh / 2, fw, fh];
}

export const anyGuide = (g: GuideSettings | null | undefined) =>
	drawingGuide(g) || compositionOn(g);

/** a guide of the Guides row on (grid, perspective, ruler, symmetry; not composition) */
export const drawingGuide = (g: GuideSettings | null | undefined) =>
	!!g && (g.grid.on || g.perspective.points > 0 || g.ruler.on || symmetryOn(g));

export const compositionOn = (g: GuideSettings | null | undefined) =>
	!!g?.composition &&
	g.composition.preset !== 'off' &&
	(g.composition.preset !== 'custom' || !!g.composition.custom);

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
	// isometric: verticals plus lines at ±30°, through the same points as `snapToGrid`
	for (let x = s; x < w; x += s) out.push([x, 0, x, h]);
	const k = Math.tan(Math.PI / 6);
	const d = isoStep(s);
	// falling lines y = c - x·k cross the canvas for c in [0, h + w·k]; rising y = c + x·k for [-w·k, h]
	for (let n = 0; n * d <= h + w * k; n++) out.push([0, n * d, w, n * d - w * k]);
	for (let n = Math.ceil((-w * k) / d); n * d <= h; n++) out.push([0, n * d, w, n * d + w * k]);
	return out;
}

/** an isometric grid's distance between crossings along its upright lines (its triangles' side) */
const isoStep = (s: number) => s / Math.cos(Math.PI / 6);

// ---- Shift: constraints while dragging ----

const D = Math.SQRT1_2;
/** across, down and the two diagonals: Shift + drag keeps a stroke or a move to these */
export const STRAIGHT_DIRS: Pt[] = [
	[1, 0],
	[0, 1],
	[D, D],
	[D, -D]
];

/**
 * The grid crossing nearest `p` (with the grid off, `p` itself). A square grid crosses at
 * every (m·s, n·s); an isometric one at (m·s, (j + m/2)·d) with d = s / cos 30°.
 */
export function snapToGrid(g: GuideSettings | null | undefined, p: Pt): Pt {
	if (!g?.grid.on) return p;
	const s = Math.max(4, g.grid.size);
	if (g.grid.kind === 'square') return [Math.round(p[0] / s) * s, Math.round(p[1] / s) * s];
	const d = isoStep(s);
	let best: Pt = p;
	let bd = Infinity;
	// (the nearest crossing is always in one of the two columns either side)
	for (const m of [Math.floor(p[0] / s), Math.floor(p[0] / s) + 1]) {
		const q: Pt = [m * s, (Math.round(p[1] / d - m / 2) + m / 2) * d];
		const dist = Math.hypot(q[0] - p[0], q[1] - p[1]);
		if (dist < bd) [best, bd] = [q, dist];
	}
	return best;
}

/**
 * `p` held to the line through `origin` along unit `u`, and on it the nearest grid crossing
 * when the grid is on and the line passes through crossings near there.
 */
export function snapAlong(g: GuideSettings | null | undefined, origin: Pt, u: Pt, p: Pt): Pt {
	const q = project(origin, u, p);
	if (!g?.grid.on) return q;
	const s = Math.max(4, g.grid.size);
	const step = g.grid.kind === 'square' ? s : isoStep(s);
	const c = snapToGrid(g, q);
	let best: Pt = q;
	let bd = Infinity;
	for (let i = -2; i <= 2; i++)
		for (let j = -2; j <= 2; j++) {
			const r = snapToGrid(g, [c[0] + i * s, c[1] + j * step]);
			// on the line?
			const off = Math.abs((r[0] - origin[0]) * u[1] - (r[1] - origin[1]) * u[0]);
			if (off > 1e-6 * s) continue;
			const dist = Math.hypot(r[0] - q[0], r[1] - q[1]);
			if (dist < bd) [best, bd] = [r, dist];
		}
	return best;
}

/** `p` turned about `origin` to the nearest `stepDeg` step, as far from it as before. */
export function snapAngle(origin: Pt, p: Pt, stepDeg: number): Pt {
	const [dx, dy] = [p[0] - origin[0], p[1] - origin[1]];
	const r = Math.hypot(dx, dy);
	if (r === 0) return p;
	const step = (stepDeg * Math.PI) / 180;
	const a = Math.round(Math.atan2(dy, dx) / step) * step;
	return [origin[0] + r * Math.cos(a), origin[1] + r * Math.sin(a)];
}

/**
 * Shift on a move: the offset `d` (from where the drag started) held across, down or at 45°,
 * whichever is closest, and a whole number of grid cells along it when the grid is on.
 */
export function shiftOffset(g: GuideSettings | null | undefined, d: Pt): Pt {
	const u = closestDirection(STRAIGHT_DIRS, d);
	return u ? snapAlong(g, [0, 0], u, d) : d;
}

/**
 * Shift on a dragged point (it started at `from`, the pointer is at `to`): with the grid on,
 * the nearest grid crossing; otherwise held across, down or at 45° from where it started.
 */
export function shiftPoint(g: GuideSettings | null | undefined, from: Pt, to: Pt): Pt {
	if (g?.grid.on) return snapToGrid(g, to);
	const d = shiftOffset(null, [to[0] - from[0], to[1] - from[1]]);
	return [from[0] + d[0], from[1] + d[1]];
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

// ---- composition overlays: lines and arcs fitted to the canvas rectangle ----

/** The flips each preset can take (none, across only, or both ways). */
export function compositionFlips(preset: Composition['preset']): 'none' | 'x' | 'xy' {
	return preset === 'spiral' || preset === 'custom' ? 'xy' : preset === 'triangles' ? 'x' : 'none';
}

/**
 * A composition overlay's lines, arcs and ellipses in canvas pixels for a w × h canvas
 * (flipped, then moved and sized as placed), the solid ones and the dashed ones apart.
 */
export function compositionParts(
	c: Composition,
	w: number,
	h: number
): { solid: Shapes; dashed: Shapes } {
	const [x, y, fw, fh] = compositionFrame(c, w, h);
	const parts =
		c.preset === 'custom'
			? c.custom
				? defShapes(c.custom, fw, fh)
				: { solid: none(), dashed: none() }
			: { solid: { ...presetShapes(c.preset, fw, fh), ellipses: [] }, dashed: none() };
	const place = (s: Shapes) => moved(flipped(s, fw, fh, c.flipX, c.flipY), x, y);
	return { solid: place(parts.solid), dashed: place(parts.dashed) };
}

const none = (): Shapes => ({ lines: [], arcs: [], ellipses: [] });

/** All of a composition overlay's shapes, in canvas pixels for w × h (as `compositionParts`). */
export function compositionShapes(c: Composition, w: number, h: number): Shapes {
	const { solid, dashed } = compositionParts(c, w, h);
	return {
		lines: [...solid.lines, ...dashed.lines],
		arcs: [...solid.arcs, ...dashed.arcs],
		ellipses: [...solid.ellipses, ...dashed.ellipses]
	};
}

/** A built-in guide's lines (and the spiral's arcs) for a w × h frame. */
function presetShapes(
	preset: CompositionPreset,
	w: number,
	h: number
): { lines: Seg[]; arcs: Arc[] } {
	const lines: Seg[] = [];
	const arcs: Arc[] = [];
	const across = (k: number) => lines.push([w * k, 0, w * k, h]);
	const down = (k: number) => lines.push([0, h * k, w, h * k]);
	const box = (k: number) => {
		const [x0, y0, x1, y1] = [w * k, h * k, w * (1 - k), h * (1 - k)];
		lines.push([x0, y0, x1, y0], [x1, y0, x1, y1], [x1, y1, x0, y1], [x0, y1, x0, y0]);
	};
	switch (preset) {
		case 'thirds':
			for (const k of [1 / 3, 2 / 3]) across(k);
			for (const k of [1 / 3, 2 / 3]) down(k);
			break;
		case 'phi':
			for (const k of [1 - PHI_CUT, PHI_CUT]) across(k);
			for (const k of [1 - PHI_CUT, PHI_CUT]) down(k);
			break;
		case 'cross':
			across(0.5);
			down(0.5);
			lines.push([0, 0, w, h], [w, 0, 0, h]);
			break;
		case 'diagonals': {
			// 45° from each corner, as far as the far side
			const m = Math.min(w, h);
			lines.push([0, 0, m, m], [w, 0, w - m, m], [0, h, m, h - m], [w, h, w - m, h - m]);
			break;
		}
		case 'armature': {
			// the two diagonals, the diamond through the sides' middles, and from each
			// corner to the middles of the two sides across from it: 14 lines
			const [mx, my] = [w / 2, h / 2];
			lines.push([0, 0, w, h], [w, 0, 0, h]);
			lines.push([mx, 0, w, my], [w, my, mx, h], [mx, h, 0, my], [0, my, mx, 0]);
			lines.push([0, 0, w, my], [0, 0, mx, h], [w, 0, 0, my], [w, 0, mx, h]);
			lines.push([0, h, w, my], [0, h, mx, 0], [w, h, 0, my], [w, h, mx, 0]);
			break;
		}
		case 'safe':
			box(0.05); // action safe: 90%
			box(0.1); // title safe: 80%
			break;
		case 'triangles': {
			// a diagonal, and from the other two corners straight across to it
			const foot = (p: Pt): Pt => {
				const t = (p[0] * w + p[1] * h) / (w * w + h * h);
				return [t * w, t * h];
			};
			const [a, b] = [foot([w, 0]), foot([0, h])];
			lines.push([0, 0, w, h], [w, 0, a[0], a[1]], [0, h, b[0], b[1]]);
			break;
		}
		case 'spiral':
			spiral(w, h, lines, arcs);
			break;
	}
	return { lines, arcs };
}
