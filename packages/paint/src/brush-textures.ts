/**
 * Brush textures: paper grain, picture tips (brush shapes) and dual brushes
 * (see the engine's `texture` module). Built-in grains are drawn here; pictures
 * come from imported brushes (.abr, .brush / .brushset) or any image, and are
 * kept in this browser. Each brush remembers its own texture settings.
 */

export interface Texture {
	id: string;
	name: string;
	/** grain (repeats over the canvas) or tip (one brush shape) */
	kind: 'grain' | 'tip';
	w: number;
	h: number;
	/** 255 = paint */
	px: Uint8Array;
}

export interface BrushTexture {
	grain: string | null;
	/** canvas px per texel */
	grainScale: number;
	/** 0–1 */
	grainStrength: number;
	tip: string | null;
	dual: string | null;
	/** fractions of the brush radius */
	dualSize: number;
	dualScatter: number;
	dualCount: number;
	/** one stroke paints a bundle of thin strands (none: one line) */
	strands?: BrushStrands | null;
}

/**
 * Strands (see the engine's `strands` module): one stroke paints `count` thin,
 * tapered, roughly parallel lines that follow it, as for hair or fur.
 */
export interface BrushStrands {
	/** strands per stroke, 2–24 */
	count: number;
	/** the bundle's width at full pressure, as a multiple of the brush's size (radius) */
	spread: number;
	/** how much each strand bends and strays, 0–1 */
	wobble: number;
	/** how long the thin ends are, 0–1 */
	taper: number;
}

export const STRANDS_MAX = 24;
export const STRANDS_SPREAD: [number, number] = [1, 30];

export const DEFAULT_STRANDS: BrushStrands = { count: 8, spread: 10, wobble: 0.35, taper: 0.6 };

const clampTo = (v: unknown, lo: number, hi: number, d: number) =>
	typeof v === 'number' && Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : d;

/** Strand settings checked and clamped (from a file or storage); none when they aren't any. */
export function readStrands(v: unknown): BrushStrands | undefined {
	const s = v as BrushStrands;
	if (!s || typeof s !== 'object') return undefined;
	const count = Math.round(clampTo(s.count, 0, STRANDS_MAX, 0));
	if (count < 2) return undefined;
	return {
		count,
		spread: clampTo(s.spread, STRANDS_SPREAD[0], STRANDS_SPREAD[1], DEFAULT_STRANDS.spread),
		wobble: clampTo(s.wobble, 0, 1, DEFAULT_STRANDS.wobble),
		taper: clampTo(s.taper, 0, 1, DEFAULT_STRANDS.taper)
	};
}

export const strandsOn = (t: BrushTexture | null | undefined) =>
	!!t?.strands && t.strands.count >= 2;

/**
 * How far from its path a stroke in strands paints, as a multiple of the brush's radius,
 * given how far its brush paints (`paintReach`): the engine's `Strands::reach` plus that.
 */
export const strandsReach = (s: BrushStrands, paintReach: number) =>
	s.spread * (0.5 + 0.6 * s.wobble) + paintReach;

export const NO_TEXTURE: BrushTexture = {
	grain: null,
	grainScale: 1,
	grainStrength: 0.7,
	tip: null,
	dual: null,
	dualSize: 0.35,
	dualScatter: 0.8,
	dualCount: 3
};

/** whether `t` changes how strokes look at all (grain, a tip, a dual brush or strands) */
export const textureOn = (t: BrushTexture | null | undefined) =>
	!!t && (!!(t.grain && t.grainStrength > 0) || !!t.tip || !!t.dual || strandsOn(t));

/** Built-in brushes that come with settings of these kinds (until changed on this device). */
export const BRUSH_DEFAULTS: Record<string, Partial<BrushTexture>> = {
	'pencilpilot-hair-strands': {
		strands: { count: 6, spread: 24, wobble: 0.3, taper: 0.7 }
	}
};

// ---- built-in grains and tips, drawn from a seeded noise ----

function rng(seed: number) {
	let s = seed >>> 0;
	return () => {
		s = (s + 0x6d2b79f5) >>> 0;
		let t = s;
		t = Math.imul(t ^ (t >>> 15), t | 1);
		t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
}

/** smooth tileable value noise, 0–1, `cells` across */
function valueNoise(n: number, cells: number, seed: number): Float32Array {
	const r = rng(seed);
	const grid = Array.from({ length: cells * cells }, () => r());
	const g = (x: number, y: number) => grid[((y + cells) % cells) * cells + ((x + cells) % cells)];
	const out = new Float32Array(n * n);
	const fade = (t: number) => t * t * (3 - 2 * t);
	for (let y = 0; y < n; y++)
		for (let x = 0; x < n; x++) {
			const fx = (x / n) * cells;
			const fy = (y / n) * cells;
			const [x0, y0] = [Math.floor(fx), Math.floor(fy)];
			const [tx, ty] = [fade(fx - x0), fade(fy - y0)];
			const top = g(x0, y0) * (1 - tx) + g(x0 + 1, y0) * tx;
			const bot = g(x0, y0 + 1) * (1 - tx) + g(x0 + 1, y0 + 1) * tx;
			out[y * n + x] = top * (1 - ty) + bot * ty;
		}
	return out;
}

function toBytes(f: Float32Array, lo = 0, hi = 1): Uint8Array {
	const out = new Uint8Array(f.length);
	for (let i = 0; i < f.length; i++)
		out[i] = Math.round(Math.min(1, Math.max(0, (f[i] - lo) / (hi - lo))) * 255);
	return out;
}

const N = 128;

function paper(): Uint8Array {
	const a = valueNoise(N, 32, 1);
	const b = valueNoise(N, 8, 2);
	const c = valueNoise(N, 64, 3);
	const f = a.map((v, i) => 0.5 * v + 0.3 * b[i] + 0.2 * c[i]);
	return toBytes(f, 0.25, 0.7);
}

function canvasWeave(): Uint8Array {
	const n = valueNoise(N, 64, 4);
	const f = new Float32Array(N * N);
	for (let y = 0; y < N; y++)
		for (let x = 0; x < N; x++) {
			const wx = 0.5 + 0.5 * Math.sin((x / N) * Math.PI * 2 * 32);
			const wy = 0.5 + 0.5 * Math.sin((y / N) * Math.PI * 2 * 32);
			const over = ((x >> 2) + (y >> 2)) % 2 ? wx : wy;
			f[y * N + x] = 0.75 * over + 0.25 * n[y * N + x];
		}
	return toBytes(f, 0.15, 0.85);
}

function grit(): Uint8Array {
	const r = rng(5);
	const f = new Float32Array(N * N).map(() => r());
	return toBytes(f, 0.35, 0.65);
}

function hatch(): Uint8Array {
	const n = valueNoise(N, 32, 6);
	const f = new Float32Array(N * N);
	for (let y = 0; y < N; y++)
		for (let x = 0; x < N; x++) {
			const d = ((x + y) % 8) / 8;
			f[y * N + x] = (d < 0.45 ? 1 : 0.1) * (0.8 + 0.2 * n[y * N + x]);
		}
	return toBytes(f);
}

function sponge(): Uint8Array {
	const a = valueNoise(N, 16, 7);
	const b = valueNoise(N, 32, 8);
	return toBytes(
		a.map((v, i) => (0.6 * v + 0.4 * b[i] > 0.5 ? 1 : 0.05)),
		0,
		1
	);
}

/** a soft-edged round tip with a rough edge (chalk) */
function chalkTip(): Uint8Array {
	const n = valueNoise(64, 8, 9);
	const g = valueNoise(64, 32, 10);
	const out = new Float32Array(64 * 64);
	for (let y = 0; y < 64; y++)
		for (let x = 0; x < 64; x++) {
			const d = Math.hypot(x - 31.5, y - 31.5) / 32;
			const edge = 0.75 + 0.25 * n[y * 64 + x];
			out[y * 64 + x] = d < edge ? 0.55 + 0.45 * g[y * 64 + x] : 0;
		}
	return toBytes(out);
}

/** a flat, slanted nib */
function flatTip(): Uint8Array {
	const out = new Float32Array(64 * 64);
	for (let y = 0; y < 64; y++)
		for (let x = 0; x < 64; x++) {
			const u = (x - 31.5) / 32;
			const v = (y - 31.5) / 32;
			const a = (u + v) / Math.SQRT2;
			const b = (u - v) / Math.SQRT2;
			out[y * 64 + x] = Math.abs(a) < 0.95 && Math.abs(b) < 0.22 ? 1 : 0;
		}
	return toBytes(out);
}

/** speckles: a cluster of small dots */
function speckleTip(): Uint8Array {
	const r = rng(11);
	const out = new Float32Array(64 * 64);
	for (let k = 0; k < 26; k++) {
		const a = r() * Math.PI * 2;
		const d = Math.sqrt(r()) * 26;
		const [cx, cy] = [31.5 + Math.cos(a) * d, 31.5 + Math.sin(a) * d];
		const rad = 1.5 + r() * 3;
		for (let y = 0; y < 64; y++)
			for (let x = 0; x < 64; x++) if (Math.hypot(x - cx, y - cy) < rad) out[y * 64 + x] = 1;
	}
	return toBytes(out);
}

let builtIns: Texture[] | null = null;

/**
 * The textures that come with the app: the drawn ones (made once, on first use) and the
 * picture ones (papers and tips from texture packs, loaded on first use: listeners hear
 * when they're in).
 */
export function builtInTextures(): Texture[] {
	if (!picturesLoad && typeof document !== 'undefined') void loadPictureTextures();
	return [...drawnTextures(), ...pictures];
}

function drawnTextures(): Texture[] {
	builtIns ??= [
		{ id: 'grain-paper', name: 'Paper', kind: 'grain', w: N, h: N, px: paper() },
		{ id: 'grain-canvas', name: 'Canvas', kind: 'grain', w: N, h: N, px: canvasWeave() },
		{ id: 'grain-grit', name: 'Grit', kind: 'grain', w: N, h: N, px: grit() },
		{ id: 'grain-hatch', name: 'Hatching', kind: 'grain', w: N, h: N, px: hatch() },
		{ id: 'grain-sponge', name: 'Sponge', kind: 'grain', w: N, h: N, px: sponge() },
		{ id: 'tip-chalk', name: 'Chalk', kind: 'tip', w: 64, h: 64, px: chalkTip() },
		{ id: 'tip-flat', name: 'Flat nib', kind: 'tip', w: 64, h: 64, px: flatTip() },
		{ id: 'tip-speckle', name: 'Speckles', kind: 'tip', w: 64, h: 64, px: speckleTip() }
	];
	return builtIns;
}

// ---- picture textures that come with the app (tools/brush-pack/textures.mjs makes them) ----

export interface PictureEntry {
	id: string;
	name: string;
	kind: 'grain' | 'tip';
	file: string;
	w: number;
	h: number;
}

/** The picture textures' list as served: anything unusable left out. */
export function readPictureList(raw: unknown): PictureEntry[] {
	if (!Array.isArray(raw)) return [];
	return raw.flatMap((e) => {
		const ok =
			e &&
			typeof e.id === 'string' &&
			e.id.startsWith('pic-') &&
			typeof e.name === 'string' &&
			(e.kind === 'grain' || e.kind === 'tip') &&
			typeof e.file === 'string' &&
			/^[a-z0-9-]+\.png$/.test(e.file) &&
			Number.isInteger(e.w) &&
			Number.isInteger(e.h) &&
			e.w > 0 &&
			e.h > 0 &&
			e.w <= 1024 &&
			e.h <= 1024;
		return ok
			? [{ id: e.id, name: e.name.slice(0, 40), kind: e.kind, file: e.file, w: e.w, h: e.h }]
			: [];
	});
}

let pictures: Texture[] = [];
let picturesLoad: Promise<void> | null = null;

/** A grey picture's values (its red channel), `w` × `h`; null when it isn't that size. */
async function greyOf(blob: Blob, w: number, h: number): Promise<Uint8Array | null> {
	const bmp = await createImageBitmap(blob);
	try {
		if (bmp.width !== w || bmp.height !== h) return null;
		const c = document.createElement('canvas');
		[c.width, c.height] = [w, h];
		const g = c.getContext('2d', { willReadFrequently: true })!;
		g.drawImage(bmp, 0, 0);
		const d = g.getImageData(0, 0, w, h).data;
		const px = new Uint8Array(w * h);
		for (let i = 0; i < px.length; i++) px[i] = d[i * 4];
		return px;
	} finally {
		bmp.close();
	}
}

/** Load the app's picture textures (once): from /brushes/textures, as the brushes are. */
export function loadPictureTextures(fetcher: typeof fetch = fetch): Promise<void> {
	picturesLoad ??= (async () => {
		const r = await fetcher('/brushes/textures/manifest.json');
		if (!r.ok) return;
		const out: Texture[] = [];
		for (const e of readPictureList(await r.json())) {
			try {
				const res = await fetcher(`/brushes/textures/${e.file}`);
				const px = res.ok ? await greyOf(await res.blob(), e.w, e.h) : null;
				if (px) out.push({ id: e.id, name: e.name, kind: e.kind, w: e.w, h: e.h, px });
			} catch {
				/* one that won't load: the rest still do */
			}
		}
		pictures = out;
		for (const l of listeners) l();
	})().catch(() => {
		/* none (offline, say): the drawn ones are still there */
	});
	return picturesLoad;
}

// ---- imported pictures, kept in this browser ----

const IMG_KEY = 'pp.brushTextures';
const SET_KEY = 'pp.brushTextureSettings';
const listeners = new Set<() => void>();
let saved: Texture[] | null = null;

const b64 = (u: Uint8Array) => {
	let s = '';
	for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode(...u.subarray(i, i + 0x8000));
	return btoa(s);
};
const unb64 = (s: string) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));

export function savedTextures(): Texture[] {
	if (saved) return saved;
	try {
		const v = JSON.parse(localStorage.getItem(IMG_KEY) ?? '[]');
		saved = Array.isArray(v)
			? v
					.filter((t) => t && typeof t.px === 'string' && t.w > 0 && t.h > 0)
					.map((t) => ({ ...t, px: unb64(t.px) }))
					.filter((t) => t.px.length === t.w * t.h)
			: [];
	} catch {
		saved = [];
	}
	return saved!;
}

/** Keep a picture as a texture (replacing one with the same id). Returns it. */
export function saveTexture(t: Omit<Texture, 'id'> & { id?: string }): Texture {
	const tex: Texture = {
		...t,
		id: t.id ?? `img-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`,
		name: t.name.slice(0, 40) || 'Picture'
	};
	const list = [...savedTextures().filter((x) => x.id !== tex.id), tex];
	saved = list;
	try {
		localStorage.setItem(IMG_KEY, JSON.stringify(list.map((x) => ({ ...x, px: b64(x.px) }))));
	} catch {
		/* full or blocked: kept for this visit */
	}
	for (const l of listeners) l();
	return tex;
}

export function deleteTexture(id: string) {
	saved = savedTextures().filter((t) => t.id !== id);
	try {
		localStorage.setItem(IMG_KEY, JSON.stringify(saved.map((x) => ({ ...x, px: b64(x.px) }))));
	} catch {
		/* blocked */
	}
	for (const l of listeners) l();
}

export const allTextures = () => [...builtInTextures(), ...savedTextures()];
export const textureById = (id: string | null) =>
	id ? allTextures().find((t) => t.id === id) : undefined;

// ---- per-brush settings ----

let settings: Record<string, BrushTexture> | null = null;

function allSettings(): Record<string, BrushTexture> {
	if (settings) return settings;
	try {
		const v = JSON.parse(localStorage.getItem(SET_KEY) ?? '{}');
		settings = v && typeof v === 'object' ? v : {};
	} catch {
		settings = {};
	}
	return settings!;
}

export function brushTexture(brushId: string): BrushTexture {
	const t = { ...NO_TEXTURE, ...BRUSH_DEFAULTS[brushId], ...allSettings()[brushId] };
	const strands = readStrands(t.strands);
	if (strands) t.strands = strands;
	else delete t.strands;
	return t;
}

/** Keep `t` as `brushId`'s settings (null: back to its own). */
export function setBrushTexture(brushId: string, t: BrushTexture | null) {
	const all = { ...allSettings() };
	// (a built-in brush's own settings switched off are kept as off)
	if (t && (textureOn(t) || BRUSH_DEFAULTS[brushId]))
		all[brushId] = { ...t, strands: strandsOn(t) ? t.strands : null };
	else delete all[brushId];
	settings = all;
	try {
		localStorage.setItem(SET_KEY, JSON.stringify(all));
	} catch {
		/* blocked */
	}
	for (const l of listeners) l();
}

/** Get told when textures or texture settings change (returns an unsubscribe). */
export function onTextures(f: () => void) {
	listeners.add(f);
	return () => listeners.delete(f);
}

/**
 * Any picture as a grey texture of at most `max` px a side. Dark, opaque
 * pixels paint (like Photoshop brush tips); with `alpha`, opaque ones do
 * (Procreate shapes are white on black, so `invert` flips that).
 */
export function grayFromImage(
	img: ImageData,
	max = 128,
	opts: { invert?: boolean } = {}
): { w: number; h: number; px: Uint8Array } {
	const k = Math.min(1, max / Math.max(img.width, img.height));
	const w = Math.max(1, Math.round(img.width * k));
	const h = Math.max(1, Math.round(img.height * k));
	const px = new Uint8Array(w * h);
	const sx = img.width / w;
	const sy = img.height / h;
	for (let y = 0; y < h; y++)
		for (let x = 0; x < w; x++) {
			// average the source pixels this one covers
			let sum = 0;
			let n = 0;
			for (let yy = Math.floor(y * sy); yy < Math.max(Math.floor(y * sy) + 1, (y + 1) * sy); yy++)
				for (
					let xx = Math.floor(x * sx);
					xx < Math.max(Math.floor(x * sx) + 1, (x + 1) * sx);
					xx++
				) {
					const i = (Math.min(img.height - 1, yy) * img.width + Math.min(img.width - 1, xx)) * 4;
					const lum =
						(0.299 * img.data[i] + 0.587 * img.data[i + 1] + 0.114 * img.data[i + 2]) / 255;
					const a = img.data[i + 3] / 255;
					const ink = opts.invert ? lum * a : (1 - lum) * a;
					sum += ink;
					n++;
				}
			px[y * w + x] = Math.round((sum / Math.max(1, n)) * 255);
		}
	return { w, h, px };
}

/** The engine calls that texture a canvas's strokes (`PaintCanvas` from the brush engine). */
interface TexturedCanvas {
	setTexture(id: number, w: number, h: number, px: Uint8Array): boolean;
	setDabStyle(
		grain: number,
		grainScale: number,
		grainStrength: number,
		tip: number,
		dual: number,
		dualSize: number,
		dualScatter: number,
		dualCount: number
	): void;
	setStrands(count: number, spread: number, wobble: number, taper: number): void;
}

/** Strand the engine canvas's next strokes as `t` says (none: one line a stroke). */
export function strandStrokes(paint: Pick<TexturedCanvas, 'setStrands'>, t?: BrushTexture | null) {
	const s = strandsOn(t) ? t!.strands! : null;
	if (!s) return paint.setStrands(0, 0, 0, 0);
	paint.setStrands(s.count, s.spread, s.wobble, s.taper);
}

/**
 * Texture an engine canvas's next strokes with `t` (none: plain dabs). `ids`
 * remembers which pictures that canvas already has (app id → engine id).
 */
export function styleStrokes(
	paint: TexturedCanvas,
	t: BrushTexture | null | undefined,
	ids: Map<string, number>
) {
	const engineId = (id: string | null) => {
		if (!id) return 0;
		const known = ids.get(id);
		if (known) return known;
		const tex = textureById(id);
		if (!tex) return 0;
		const n = ids.size + 1;
		if (!paint.setTexture(n, tex.w, tex.h, tex.px)) return 0;
		ids.set(id, n);
		return n;
	};
	strandStrokes(paint, t);
	if (!t || !textureOn(t)) return paint.setDabStyle(0, 1, 0, 0, 0, 0, 0, 1);
	paint.setDabStyle(
		engineId(t.grain),
		t.grainScale,
		t.grainStrength,
		engineId(t.tip),
		engineId(t.dual),
		t.dualSize,
		t.dualScatter,
		t.dualCount
	);
}
