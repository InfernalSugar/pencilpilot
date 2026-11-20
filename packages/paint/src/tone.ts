/**
 * Curves, Levels and Gradient Map (Photoshop / Procreate adjustments), turned
 * into the look-up tables the paint engine applies: a 256-entry table per
 * channel for curves and levels, and 256 colours for a gradient map.
 */

/** A curve point: input → output, both 0–1. */
export type CurvePt = [number, number];

export interface Curves {
	/** all three channels */
	rgb: CurvePt[];
	r: CurvePt[];
	g: CurvePt[];
	b: CurvePt[];
}

export interface Levels {
	/** input black / white points and midtone gamma (1 = unchanged) */
	inBlack: number;
	inWhite: number;
	gamma: number;
	/** output black / white */
	outBlack: number;
	outWhite: number;
}

export interface MapStop {
	/** 0 (darkest) – 1 (lightest) */
	at: number;
	color: string;
}

/** Everything an adjustment (or adjustment layer) can do. */
export interface AdjustSpec {
	hue: number;
	saturation: number;
	brightness: number;
	contrast: number;
	curves: Curves | null;
	levels: Levels | null;
	map: MapStop[] | null;
}

export const LINE: CurvePt[] = [
	[0, 0],
	[1, 1]
];
export const NO_CURVES: Curves = { rgb: LINE, r: LINE, g: LINE, b: LINE };
export const NO_LEVELS: Levels = { inBlack: 0, inWhite: 1, gamma: 1, outBlack: 0, outWhite: 1 };
export const NO_SPEC: AdjustSpec = {
	hue: 0,
	saturation: 0,
	brightness: 0,
	contrast: 0,
	curves: null,
	levels: null,
	map: null
};

export const MAP_PRESETS: { id: string; name: string; stops: MapStop[] }[] = [
	{
		id: 'mono',
		name: 'Black & white',
		stops: [
			{ at: 0, color: '#000000' },
			{ at: 1, color: '#ffffff' }
		]
	},
	{
		id: 'sepia',
		name: 'Sepia',
		stops: [
			{ at: 0, color: '#1e120a' },
			{ at: 0.55, color: '#a0703f' },
			{ at: 1, color: '#fbf0d9' }
		]
	},
	{
		id: 'dusk',
		name: 'Dusk',
		stops: [
			{ at: 0, color: '#1b1340' },
			{ at: 0.5, color: '#c4436b' },
			{ at: 1, color: '#ffd59a' }
		]
	},
	{
		id: 'teal-orange',
		name: 'Teal & orange',
		stops: [
			{ at: 0, color: '#0b2730' },
			{ at: 0.5, color: '#4f8a8b' },
			{ at: 1, color: '#ffb36b' }
		]
	},
	{
		id: 'blueprint',
		name: 'Blueprint',
		stops: [
			{ at: 0, color: '#0a2a6b' },
			{ at: 1, color: '#e8f1ff' }
		]
	}
];

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/** Points sorted, kept inside 0–1, with no two at the same input. */
export function cleanCurve(pts: CurvePt[]): CurvePt[] {
	const out = pts
		.map(([x, y]) => [clamp01(x), clamp01(y)] as CurvePt)
		.sort((a, b) => a[0] - b[0])
		.filter((p, i, a) => i === 0 || p[0] - a[i - 1][0] > 1e-3);
	return out.length >= 2 ? out : LINE;
}

/**
 * A smooth curve through the points that never overshoots (monotone cubic,
 * Fritsch–Carlson), as 256 values 0–255. Flat before the first point and after
 * the last.
 */
export function curveTable(pts: CurvePt[]): Uint8Array {
	const p = cleanCurve(pts);
	const n = p.length;
	const dx: number[] = [];
	const m: number[] = [];
	for (let i = 0; i < n - 1; i++) {
		dx.push(p[i + 1][0] - p[i][0]);
		m.push((p[i + 1][1] - p[i][1]) / dx[i]);
	}
	// tangents
	const t: number[] = [m[0]];
	for (let i = 1; i < n - 1; i++) t.push(m[i - 1] * m[i] <= 0 ? 0 : (m[i - 1] + m[i]) / 2);
	t.push(m[n - 2]);
	for (let i = 0; i < n - 1; i++) {
		if (m[i] === 0) {
			t[i] = t[i + 1] = 0;
			continue;
		}
		const a = t[i] / m[i];
		const b = t[i + 1] / m[i];
		const h = a * a + b * b;
		if (h > 9) {
			const k = 3 / Math.sqrt(h);
			t[i] = k * a * m[i];
			t[i + 1] = k * b * m[i];
		}
	}
	const out = new Uint8Array(256);
	let seg = 0;
	for (let i = 0; i < 256; i++) {
		const x = i / 255;
		let y: number;
		if (x <= p[0][0]) y = p[0][1];
		else if (x >= p[n - 1][0]) y = p[n - 1][1];
		else {
			while (seg < n - 2 && x > p[seg + 1][0]) seg++;
			const h = dx[seg];
			const s = (x - p[seg][0]) / h;
			const s2 = s * s;
			const s3 = s2 * s;
			y =
				(2 * s3 - 3 * s2 + 1) * p[seg][1] +
				(s3 - 2 * s2 + s) * h * t[seg] +
				(-2 * s3 + 3 * s2) * p[seg + 1][1] +
				(s3 - s2) * h * t[seg + 1];
		}
		out[i] = Math.round(clamp01(y) * 255);
	}
	return out;
}

/** Levels as 256 values 0–255. */
export function levelsTable(l: Levels): Uint8Array {
	const out = new Uint8Array(256);
	const lo = clamp01(l.inBlack);
	const hi = Math.max(lo + 1 / 255, clamp01(l.inWhite));
	const g = Math.min(9.99, Math.max(0.1, l.gamma));
	for (let i = 0; i < 256; i++) {
		const v = clamp01((i / 255 - lo) / (hi - lo)) ** (1 / g);
		out[i] = Math.round(clamp01(l.outBlack + v * (l.outWhite - l.outBlack)) * 255);
	}
	return out;
}

const isLine = (c: CurvePt[]) =>
	c.length === 2 && c[0][0] === 0 && c[0][1] === 0 && c[1][0] === 1 && c[1][1] === 1;

const isNoLevels = (l: Levels) =>
	l.inBlack === 0 && l.inWhite === 1 && l.gamma === 1 && l.outBlack === 0 && l.outWhite === 1;

/**
 * The R, G, B tables (768 bytes) for the curves and levels of `spec` (levels
 * first, then the master curve, then each channel's curve), or an empty array
 * when they change nothing.
 */
export function toneTables(spec: Pick<AdjustSpec, 'curves' | 'levels'>): Uint8Array {
	const c = spec.curves;
	const l = spec.levels;
	const curvesOn = !!c && [c.rgb, c.r, c.g, c.b].some((x) => !isLine(x));
	const levelsOn = !!l && !isNoLevels(l);
	if (!curvesOn && !levelsOn) return new Uint8Array(0);
	const lv = levelsOn ? levelsTable(l!) : null;
	const master = curvesOn ? curveTable(c!.rgb) : null;
	const out = new Uint8Array(768);
	(['r', 'g', 'b'] as const).forEach((ch, k) => {
		const own = curvesOn ? curveTable(c![ch]) : null;
		for (let i = 0; i < 256; i++) {
			let v = lv ? lv[i] : i;
			if (master) v = master[v];
			if (own) v = own[v];
			out[k * 256 + i] = v;
		}
	});
	return out;
}

const rgbOf = (hex: string): [number, number, number] => {
	const n = parseInt(hex.replace('#', ''), 16);
	return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};

/** A gradient map as 256 RGB colours (768 bytes), dark → light; empty for none. */
export function mapTable(stops: MapStop[] | null): Uint8Array {
	if (!stops || stops.length === 0) return new Uint8Array(0);
	const s = [...stops]
		.filter((x) => /^#[0-9a-f]{6}$/i.test(x.color))
		.map((x) => ({ at: clamp01(x.at), rgb: rgbOf(x.color) }))
		.sort((a, b) => a.at - b.at);
	if (!s.length) return new Uint8Array(0);
	const out = new Uint8Array(768);
	for (let i = 0; i < 256; i++) {
		const t = i / 255;
		let k = 0;
		while (k < s.length - 1 && t > s[k + 1].at) k++;
		const a = s[k];
		const b = s[Math.min(k + 1, s.length - 1)];
		const f = t <= a.at ? 0 : t >= b.at || b.at === a.at ? 1 : (t - a.at) / (b.at - a.at);
		for (let c = 0; c < 3; c++) out[i * 3 + c] = Math.round(a.rgb[c] + (b.rgb[c] - a.rgb[c]) * f);
	}
	return out;
}

/** Changes anything at all. */
export function specActive(s: AdjustSpec): boolean {
	return (
		s.hue !== 0 ||
		s.saturation !== 0 ||
		s.brightness !== 0 ||
		s.contrast !== 0 ||
		toneTables(s).length > 0 ||
		mapTable(s.map).length > 0
	);
}

/** A short name for an adjustment layer. */
export function specName(s: AdjustSpec): string {
	if (s.map) return 'Gradient map';
	if (s.curves && toneTables({ curves: s.curves, levels: null }).length) return 'Curves';
	if (s.levels && toneTables({ curves: null, levels: s.levels }).length) return 'Levels';
	if (s.hue || s.saturation) return 'Hue / saturation';
	if (s.brightness || s.contrast) return 'Brightness / contrast';
	return 'Adjustment';
}

/** An adjustment layer's saved spec (from the engine's layer list); NO_SPEC if unreadable. */
export function parseSpec(json: string | null | undefined): AdjustSpec {
	try {
		const o = JSON.parse(json ?? '');
		return { ...NO_SPEC, ...(o && typeof o === 'object' ? o : {}) };
	} catch {
		return { ...NO_SPEC };
	}
}
