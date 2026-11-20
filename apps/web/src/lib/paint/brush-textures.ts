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
}

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

export const textureOn = (t: BrushTexture | null | undefined) =>
	!!t && (!!(t.grain && t.grainStrength > 0) || !!t.tip || !!t.dual);

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

/** The textures that come with the app (made once, on first use). */
export function builtInTextures(): Texture[] {
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
	return { ...NO_TEXTURE, ...allSettings()[brushId] };
}

export function setBrushTexture(brushId: string, t: BrushTexture | null) {
	const all = { ...allSettings() };
	if (t && textureOn(t)) all[brushId] = t;
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
