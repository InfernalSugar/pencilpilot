/**
 * Composition guides of your own: a list of elements (a grid, a line at a ratio, a line
 * between two points, diagonals, a spiral, a frame inset, a circle or ellipse), each
 * drawn solid or dashed, with one colour and opacity for the whole guide. Here: the
 * shapes they make for a frame of a given size, the built-in presets as elements, and
 * reading them back (from storage, a project or a file) defensively.
 * Positions inside a guide are fractions of its frame (0–1).
 */
export type Pt = [number, number];
export type Seg = [number, number, number, number];
/** A quarter of an ellipse from `from` to `to` (radii `rx`, `ry`), as an SVG arc draws it. */
export interface Arc {
	from: Pt;
	to: Pt;
	rx: number;
	ry: number;
	/** SVG's sweep flag: 1 turns clockwise on screen */
	sweep: 0 | 1;
}
export interface Ellipse {
	c: Pt;
	rx: number;
	ry: number;
}
/** what a guide draws: lines, arcs (spirals) and ellipses, in pixels */
export interface Shapes {
	lines: Seg[];
	arcs: Arc[];
	ellipses: Ellipse[];
}

/** 1 / φ: the golden ratio's long part of 1 */
export const PHI_CUT = (Math.sqrt(5) - 1) / 2;

export type LineStyle = 'solid' | 'dashed';
interface Common {
	on: boolean;
	style: LineStyle;
}
export type CompElement = Common &
	(
		| { kind: 'grid'; cols: number; rows: number }
		| {
				kind: 'ratio';
				ratio: RatioName;
				/** the fraction, for a custom ratio */
				at: number;
				/** measured from the left / top (start) or the right / bottom (end) */
				from: 'start' | 'end';
				/** a vertical line (across the width) or a horizontal one */
				dir: 'vertical' | 'horizontal';
		  }
		| { kind: 'line'; a: Pt; b: Pt }
		| {
				kind: 'diagonals';
				/** corner to corner */
				corners: boolean;
				/** 45° from each corner */
				angled: boolean;
		  }
		| { kind: 'spiral'; ratio: number; flipX: boolean; flipY: boolean }
		/** a frame inset from the edges by `inset` % (safe areas) */
		| { kind: 'frame'; inset: number }
		/** a circle (radius `rx` of the frame's short side) or an ellipse (`rx`, `ry` of its sides) */
		| { kind: 'ellipse'; c: Pt; rx: number; ry: number; circle: boolean }
	);
export type ElementKind = CompElement['kind'];

export const RATIOS = { golden: PHI_CUT, third: 1 / 3, root2: Math.SQRT1_2, custom: 0.5 } as const;
export type RatioName = keyof typeof RATIOS;

/** A composition guide of your own. */
export interface CompositionDef {
	id: string;
	name: string;
	/** '#rrggbb' */
	color: string;
	/** 0.05 – 1 */
	opacity: number;
	elements: CompElement[];
}

export const MAX_ELEMENTS = 64;
export const DEFAULT_COLOR = '#e8492f';
export const DEFAULT_OPACITY = 0.6;
export const ELEMENT_KINDS: ElementKind[] = [
	'grid',
	'ratio',
	'line',
	'diagonals',
	'spiral',
	'frame',
	'ellipse'
];

/** A new element of a kind, as the creator adds it. */
export function newElement(kind: ElementKind): CompElement {
	const c: Common = { on: true, style: 'solid' };
	switch (kind) {
		case 'grid':
			return { ...c, kind, cols: 3, rows: 3 };
		case 'ratio':
			return { ...c, kind, ratio: 'golden', at: 0.5, from: 'start', dir: 'vertical' };
		case 'line':
			return { ...c, kind, a: [0.2, 0.8], b: [0.8, 0.2] };
		case 'diagonals':
			return { ...c, kind, corners: true, angled: false };
		case 'spiral':
			return { ...c, kind, ratio: PHI_CUT, flipX: false, flipY: false };
		case 'frame':
			return { ...c, kind, inset: 10 };
		case 'ellipse':
			return { ...c, kind, c: [0.5, 0.5], rx: 0.3, ry: 0.3, circle: true };
	}
}

/** Where a ratio element's line falls, 0 – 1 across (or down) its frame. */
export function ratioAt(e: Extract<CompElement, { kind: 'ratio' }>): number {
	const k = e.ratio === 'custom' ? e.at : RATIOS[e.ratio];
	return e.from === 'end' ? 1 - k : k;
}

const empty = (): Shapes => ({ lines: [], arcs: [], ellipses: [] });

/** The shapes one element makes in a w × h frame (at 0, 0). Off: nothing. */
export function elementShapes(e: CompElement, w: number, h: number): Shapes {
	const out = empty();
	if (!e.on) return out;
	const { lines } = out;
	switch (e.kind) {
		case 'grid':
			for (let i = 1; i < e.cols; i++) lines.push([(w * i) / e.cols, 0, (w * i) / e.cols, h]);
			for (let j = 1; j < e.rows; j++) lines.push([0, (h * j) / e.rows, w, (h * j) / e.rows]);
			break;
		case 'ratio': {
			const k = ratioAt(e);
			lines.push(e.dir === 'vertical' ? [w * k, 0, w * k, h] : [0, h * k, w, h * k]);
			break;
		}
		case 'line':
			lines.push([e.a[0] * w, e.a[1] * h, e.b[0] * w, e.b[1] * h]);
			break;
		case 'diagonals': {
			if (e.corners) lines.push([0, 0, w, h], [w, 0, 0, h]);
			if (e.angled) {
				const m = Math.min(w, h);
				lines.push([0, 0, m, m], [w, 0, w - m, m], [0, h, m, h - m], [w, h, w - m, h - m]);
			}
			break;
		}
		case 'spiral': {
			const s = empty();
			spiral(w, h, s.lines, s.arcs, e.ratio);
			const f = flipped(s, w, h, e.flipX, e.flipY);
			out.lines.push(...f.lines);
			out.arcs.push(...f.arcs);
			break;
		}
		case 'frame': {
			const k = e.inset / 100;
			const [x0, y0, x1, y1] = [w * k, h * k, w * (1 - k), h * (1 - k)];
			lines.push([x0, y0, x1, y0], [x1, y0, x1, y1], [x1, y1, x0, y1], [x0, y1, x0, y0]);
			break;
		}
		case 'ellipse': {
			const r = Math.min(w, h);
			out.ellipses.push({
				c: [e.c[0] * w, e.c[1] * h],
				rx: e.circle ? e.rx * r : e.rx * w,
				ry: e.circle ? e.rx * r : e.ry * h
			});
			break;
		}
	}
	return out;
}

/** A guide's shapes in a w × h frame, the solid ones and the dashed ones apart. */
export function defShapes(
	d: Pick<CompositionDef, 'elements'>,
	w: number,
	h: number
): { solid: Shapes; dashed: Shapes } {
	const solid = empty();
	const dashed = empty();
	for (const e of d.elements) {
		const s = elementShapes(e, w, h);
		const to = e.style === 'dashed' ? dashed : solid;
		to.lines.push(...s.lines);
		to.arcs.push(...s.arcs);
		to.ellipses.push(...s.ellipses);
	}
	return { solid, dashed };
}

/** Shapes mirrored inside a w × h frame (a spiral mirrored once turns the other way). */
export function flipped(s: Shapes, w: number, h: number, fx: boolean, fy: boolean): Shapes {
	if (!fx && !fy) return s;
	const f = (p: Pt): Pt => [fx ? w - p[0] : p[0], fy ? h - p[1] : p[1]];
	const turn = fx !== fy;
	return {
		lines: s.lines.map(([x1, y1, x2, y2]) => [...f([x1, y1]), ...f([x2, y2])] as Seg),
		arcs: s.arcs.map((a) => ({
			...a,
			from: f(a.from),
			to: f(a.to),
			sweep: turn ? ((1 - a.sweep) as 0 | 1) : a.sweep
		})),
		ellipses: s.ellipses.map((e) => ({ ...e, c: f(e.c) }))
	};
}

/** Shapes moved by (dx, dy). */
export function moved(s: Shapes, dx: number, dy: number): Shapes {
	if (!dx && !dy) return s;
	const m = (p: Pt): Pt => [p[0] + dx, p[1] + dy];
	return {
		lines: s.lines.map(([x1, y1, x2, y2]) => [x1 + dx, y1 + dy, x2 + dx, y2 + dy] as Seg),
		arcs: s.arcs.map((a) => ({ ...a, from: m(a.from), to: m(a.to) })),
		ellipses: s.ellipses.map((e) => ({ ...e, c: m(e.c) }))
	};
}

/**
 * A spiral stretched to the rectangle: a cut of `s` (the golden one by default) taken off
 * one side after another (going round clockwise), a quarter ellipse across each piece,
 * joined end to end. The cuts are drawn too.
 */
export function spiral(w: number, h: number, lines: Seg[], arcs: Arc[], s = PHI_CUT) {
	let [x, y, rw, rh] = [0, 0, w, h];
	// the first cut across the long side: the left on a wide canvas, the top on a tall one
	let side = w >= h ? 0 : 1; // 0 left, 1 top, 2 right, 3 bottom
	for (let i = 0; i < 24 && rw > 1 && rh > 1; i++) {
		if (side === 0) {
			const pw = rw * s;
			arcs.push({ from: [x, y + rh], to: [x + pw, y], rx: pw, ry: rh, sweep: 1 });
			lines.push([x + pw, y, x + pw, y + rh]);
			[x, rw] = [x + pw, rw - pw];
		} else if (side === 1) {
			const ph = rh * s;
			arcs.push({ from: [x, y], to: [x + rw, y + ph], rx: rw, ry: ph, sweep: 1 });
			lines.push([x, y + ph, x + rw, y + ph]);
			[y, rh] = [y + ph, rh - ph];
		} else if (side === 2) {
			const pw = rw * s;
			arcs.push({ from: [x + rw, y], to: [x + rw - pw, y + rh], rx: pw, ry: rh, sweep: 1 });
			lines.push([x + rw - pw, y, x + rw - pw, y + rh]);
			rw -= pw;
		} else {
			const ph = rh * s;
			arcs.push({ from: [x + rw, y + rh], to: [x, y + rh - ph], rx: rw, ry: ph, sweep: 1 });
			lines.push([x, y + rh - ph, x + rw, y + rh - ph]);
			rh -= ph;
		}
		side = (side + 1) % 4;
	}
}

/** Arcs as one SVG path (a jump to where an arc starts when it doesn't follow on). */
export function arcsPath(arcs: Arc[]): string {
	if (!arcs.length) return '';
	const n = (v: number) => +v.toFixed(2);
	let d = '';
	let at: Pt | null = null;
	for (const a of arcs) {
		if (!at || Math.hypot(a.from[0] - at[0], a.from[1] - at[1]) > 0.01)
			d += `${d ? ' ' : ''}M${n(a.from[0])} ${n(a.from[1])}`;
		d += ` A${n(a.rx)} ${n(a.ry)} 0 0 ${a.sweep} ${n(a.to[0])} ${n(a.to[1])}`;
		at = a.to;
	}
	return d;
}

/** A short name for an element, as the creator's list shows it. */
export function elementLabel(e: CompElement): string {
	const pct = (v: number) => `${Math.round(v * 100)}%`;
	switch (e.kind) {
		case 'grid':
			return `Grid ${e.cols} × ${e.rows}`;
		case 'ratio': {
			const name = { golden: 'Golden', third: '1/3', root2: '√2', custom: pct(e.at) }[e.ratio];
			const side =
				e.dir === 'vertical'
					? e.from === 'start'
						? 'from the left'
						: 'from the right'
					: e.from === 'start'
						? 'from the top'
						: 'from the bottom';
			return `${name} line, ${side}`;
		}
		case 'line':
			return 'Line';
		case 'diagonals':
			return e.corners && e.angled
				? 'Diagonals and 45°'
				: e.angled
					? '45° from corners'
					: 'Diagonals';
		case 'spiral':
			return Math.abs(e.ratio - PHI_CUT) < 0.002 ? 'Golden spiral' : `Spiral ${e.ratio.toFixed(2)}`;
		case 'frame':
			return `Frame inset ${Math.round(e.inset)}%`;
		case 'ellipse':
			return e.circle ? 'Circle' : 'Ellipse';
	}
}

// ---- the built-in guides as elements (to start a guide of your own from one) ----

export const PRESET_NAMES = {
	thirds: 'Thirds',
	phi: 'Golden grid',
	spiral: 'Golden spiral',
	triangles: 'Golden triangles',
	diagonals: 'Diagonals',
	cross: 'Centre lines',
	armature: 'Armature',
	safe: 'Safe areas'
} as const;
export type BuiltIn = keyof typeof PRESET_NAMES;

/**
 * A built-in guide as elements that draw the same lines. `aspect` (width / height) is the
 * frame's: the golden triangles' lines meet the diagonal square on only for that shape.
 */
export function presetElements(
	p: BuiltIn,
	aspect = 1.5,
	flipX = false,
	flipY = false
): CompElement[] {
	const c: Common = { on: true, style: 'solid' };
	const line = (a: Pt, b: Pt): CompElement => {
		const f = (q: Pt): Pt => [flipX ? 1 - q[0] : q[0], flipY ? 1 - q[1] : q[1]];
		return { ...c, kind: 'line', a: f(a), b: f(b) };
	};
	const ratio = (dir: 'vertical' | 'horizontal', from: 'start' | 'end'): CompElement => ({
		...c,
		kind: 'ratio',
		ratio: 'golden',
		at: 0.5,
		from,
		dir
	});
	switch (p) {
		case 'thirds':
			return [{ ...c, kind: 'grid', cols: 3, rows: 3 }];
		case 'phi':
			return [
				ratio('vertical', 'end'),
				ratio('vertical', 'start'),
				ratio('horizontal', 'end'),
				ratio('horizontal', 'start')
			];
		case 'spiral':
			return [{ ...c, kind: 'spiral', ratio: PHI_CUT, flipX, flipY }];
		case 'triangles': {
			// in a frame 1 × 1 / aspect, scaled back to fractions
			const [w, h] = [1, 1 / aspect];
			const foot = (q: Pt): Pt => {
				const t = (q[0] * w + q[1] * h) / (w * w + h * h);
				return [t, t];
			};
			return [line([0, 0], [1, 1]), line([1, 0], foot([w, 0])), line([0, 1], foot([0, h]))];
		}
		case 'diagonals':
			return [{ ...c, kind: 'diagonals', corners: false, angled: true }];
		case 'cross':
			return [
				{ ...c, kind: 'grid', cols: 2, rows: 2 },
				{ ...c, kind: 'diagonals', corners: true, angled: false }
			];
		case 'armature':
			return [
				{ ...c, kind: 'diagonals', corners: true, angled: false },
				line([0.5, 0], [1, 0.5]),
				line([1, 0.5], [0.5, 1]),
				line([0.5, 1], [0, 0.5]),
				line([0, 0.5], [0.5, 0]),
				line([0, 0], [1, 0.5]),
				line([0, 0], [0.5, 1]),
				line([1, 0], [0, 0.5]),
				line([1, 0], [0.5, 1]),
				line([0, 1], [1, 0.5]),
				line([0, 1], [0.5, 0]),
				line([1, 1], [0, 0.5]),
				line([1, 1], [0.5, 0])
			];
		case 'safe':
			return [
				{ ...c, kind: 'frame', inset: 5 },
				{ ...c, kind: 'frame', inset: 10 }
			];
	}
}

// ---- reading back: anything odd is clamped, unknown things are left off ----

const clamp = (v: unknown, a: number, b: number, d: number) =>
	typeof v === 'number' && isFinite(v) ? Math.max(a, Math.min(b, v)) : d;
const int = (v: unknown, a: number, b: number, d: number) => Math.round(clamp(v, a, b, d));
const pick = <T extends string>(v: unknown, of: readonly T[], d: T): T =>
	of.includes(v as T) ? (v as T) : d;
const pt = (v: unknown, d: Pt): Pt =>
	Array.isArray(v) ? [clamp(v[0], 0, 1, d[0]), clamp(v[1], 0, 1, d[1])] : [...d];

/** One element, checked (null: not an element this version knows). */
export function readElement(v: unknown): CompElement | null {
	if (!v || typeof v !== 'object') return null;
	const o = v as Record<string, unknown>;
	if (!ELEMENT_KINDS.includes(o.kind as ElementKind)) return null;
	const d = newElement(o.kind as ElementKind);
	const c: Common = { on: o.on === true, style: pick(o.style, ['solid', 'dashed'], 'solid') };
	switch (d.kind) {
		case 'grid':
			return {
				...c,
				kind: d.kind,
				cols: int(o.cols, 1, 24, d.cols),
				rows: int(o.rows, 1, 24, d.rows)
			};
		case 'ratio':
			return {
				...c,
				kind: d.kind,
				ratio: pick(o.ratio, Object.keys(RATIOS) as RatioName[], 'golden'),
				at: clamp(o.at, 0, 1, d.at),
				from: pick(o.from, ['start', 'end'], 'start'),
				dir: pick(o.dir, ['vertical', 'horizontal'], 'vertical')
			};
		case 'line':
			return { ...c, kind: d.kind, a: pt(o.a, d.a), b: pt(o.b, d.b) };
		case 'diagonals':
			return { ...c, kind: d.kind, corners: o.corners === true, angled: o.angled === true };
		case 'spiral':
			return {
				...c,
				kind: d.kind,
				ratio: clamp(o.ratio, 0.3, 0.8, d.ratio),
				flipX: o.flipX === true,
				flipY: o.flipY === true
			};
		case 'frame':
			return { ...c, kind: d.kind, inset: clamp(o.inset, 0, 45, d.inset) };
		case 'ellipse':
			return {
				...c,
				kind: d.kind,
				c: pt(o.c, d.c),
				rx: clamp(o.rx, 0.01, 1, d.rx),
				ry: clamp(o.ry, 0.01, 1, d.ry),
				circle: o.circle === true
			};
	}
}

export const ID = /^mc-[a-z0-9]{1,24}$/;

/** A composition guide, checked (null: not one). Its id is '' when it had none. */
export function readCompositionDef(v: unknown): CompositionDef | null {
	if (!v || typeof v !== 'object') return null;
	const o = v as Record<string, unknown>;
	if (!Array.isArray(o.elements)) return null;
	const name = typeof o.name === 'string' ? o.name.trim().slice(0, 40) : '';
	return {
		id: typeof o.id === 'string' && ID.test(o.id) ? o.id : '',
		name: name || 'My composition',
		color:
			typeof o.color === 'string' && /^#[0-9a-f]{6}$/i.test(o.color)
				? o.color.toLowerCase()
				: DEFAULT_COLOR,
		opacity: clamp(o.opacity, 0.05, 1, DEFAULT_OPACITY),
		elements: o.elements
			.slice(0, MAX_ELEMENTS)
			.map(readElement)
			.filter((e): e is CompElement => !!e)
	};
}

// ---- files: .ppcomp.json ----

export const FILE_EXT = '.ppcomp.json';
/** the most guides one file brings in */
export const MAX_IN_FILE = 50;

/** One or more guides as a file's text (without their ids: each import gets new ones). */
export function compositionFile(defs: Omit<CompositionDef, 'id'>[]): string {
	return JSON.stringify(
		{
			kind: 'pencilpilot-composition',
			version: 1,
			compositions: defs.map(({ name, color, opacity, elements }) => ({
				name,
				color,
				opacity,
				elements
			}))
		},
		null,
		1
	);
}

/** The guides in a file's text (a file of several, or a single one); [] if none can be read. */
export function readCompositionFile(text: string): CompositionDef[] {
	let v: unknown;
	try {
		v = JSON.parse(text);
	} catch {
		return [];
	}
	const o = v as Record<string, unknown> | null;
	const list = Array.isArray(o?.compositions) ? o.compositions : Array.isArray(v) ? v : [v];
	return list
		.slice(0, MAX_IN_FILE)
		.map(readCompositionDef)
		.filter((d): d is CompositionDef => !!d)
		.map((d) => ({ ...d, id: '' }));
}

/** A file name for a guide: its name made safe, with the extension. */
export function compositionFileName(name: string): string {
	const base =
		name
			.replace(/[^\p{L}\p{N} _-]+/gu, '')
			.replace(/\s+/g, ' ')
			.trim() || 'composition';
	return `${base}${FILE_EXT}`;
}
