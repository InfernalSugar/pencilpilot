/**
 * Text layers (Procreate's Add Text, Photoshop's Type tool): what a text is (`TextSpec`,
 * kept on its layer as JSON), how its lines are laid out, and drawing it with the
 * system's fonts (Canvas 2D) into pixels for the paint engine. Fonts are the generic
 * families, common system fonts that are installed, and font files the user imports
 * (kept in the project, so it opens the same elsewhere). Nothing is fetched or bundled.
 */

export type TextAlign = 'left' | 'center' | 'right';

export interface TextSpec {
	text: string;
	/** a font family: generic ("sans-serif"), a system font, or one imported */
	font: string;
	/** canvas px */
	size: number;
	/** "#rrggbb" */
	color: string;
	bold: boolean;
	italic: boolean;
	align: TextAlign;
	/** line height, times the size */
	lineHeight: number;
	/** extra space after each letter, canvas px */
	letterSpacing: number;
	/** the box's top-left corner (canvas px, before turning) */
	x: number;
	y: number;
	/** lines wrap at this width (canvas px); 0: only at line breaks */
	width: number;
	/** degrees clockwise, around the box's centre */
	rotate: number;
}

export const DEFAULT_TEXT: TextSpec = {
	text: '',
	font: 'sans-serif',
	size: 64,
	color: '#16161a',
	bold: false,
	italic: false,
	align: 'left',
	lineHeight: 1.2,
	letterSpacing: 0,
	x: 0,
	y: 0,
	width: 0,
	rotate: 0
};

/** Font sizes and the like stay in these bounds. */
export const SIZE_MIN = 4;
export const SIZE_MAX = 1000;
const MAX_CHARS = 5000;

const num = (v: unknown, lo: number, hi: number, d: number) =>
	typeof v === 'number' && Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : d;

/** A text from its JSON (a layer's), filled in and kept in bounds; null if it isn't one. */
export function parseText(json: string | null | undefined): TextSpec | null {
	let o: unknown;
	try {
		o = JSON.parse(json ?? '');
	} catch {
		return null;
	}
	if (!o || typeof o !== 'object' || Array.isArray(o)) return null;
	const v = o as Record<string, unknown>;
	const d = DEFAULT_TEXT;
	return {
		text: typeof v.text === 'string' ? v.text.slice(0, MAX_CHARS) : '',
		font: typeof v.font === 'string' && v.font.trim() ? v.font.trim().slice(0, 100) : d.font,
		size: num(v.size, SIZE_MIN, SIZE_MAX, d.size),
		color: typeof v.color === 'string' && /^#[0-9a-f]{6}$/i.test(v.color) ? v.color : d.color,
		bold: v.bold === true,
		italic: v.italic === true,
		align: v.align === 'center' || v.align === 'right' ? v.align : 'left',
		lineHeight: num(v.lineHeight, 0.5, 4, d.lineHeight),
		letterSpacing: num(v.letterSpacing, -50, 200, 0),
		x: num(v.x, -1e5, 1e5, 0),
		y: num(v.y, -1e5, 1e5, 0),
		width: num(v.width, 0, 1e5, 0),
		rotate: num(v.rotate, -360, 360, 0)
	};
}

/** The text as its layer keeps it. */
export const textJson = (s: TextSpec) => JSON.stringify(s);

// ---- fonts ----

/** The families every system has. */
export const GENERIC_FONTS = ['sans-serif', 'serif', 'monospace', 'cursive'];

/** Common system fonts, offered when they're installed. */
export const SYSTEM_FONTS = [
	'Arial',
	'Helvetica',
	'Verdana',
	'Tahoma',
	'Trebuchet MS',
	'Segoe UI',
	'Georgia',
	'Times New Roman',
	'Palatino',
	'Garamond',
	'Courier New',
	'Impact',
	'Comic Sans MS',
	'Noto Sans',
	'Noto Serif',
	'DejaVu Sans',
	'DejaVu Serif',
	'Liberation Sans',
	'Liberation Serif',
	'Ubuntu',
	'Cantarell'
];

/** A family as CSS takes it (quoted unless generic), with a fallback. */
export function cssFamily(font: string): string {
	if (GENERIC_FONTS.includes(font)) return font;
	return `"${font.replace(/["\\]/g, '')}", sans-serif`;
}

/** The CSS font for a text (its size in canvas px). */
export const cssFont = (s: TextSpec) =>
	`${s.italic ? 'italic ' : ''}${s.bold ? 'bold ' : ''}${s.size}px ${cssFamily(s.font)}`;

/** Whether a system font is installed (it measures differently from the fallbacks). */
export function fontInstalled(font: string): boolean {
	if (typeof OffscreenCanvas === 'undefined') return false;
	const g = new OffscreenCanvas(1, 1).getContext('2d');
	if (!g) return false;
	const sample = 'mmmmmmmmmmlli WQ@#0123';
	return ['monospace', 'serif'].some((fallback) => {
		g.font = `40px ${fallback}`;
		const plain = g.measureText(sample).width;
		g.font = `40px "${font}", ${fallback}`;
		return g.measureText(sample).width !== plain;
	});
}

/** A font file the user brought in: the family texts use, and the file as imported. */
export interface ImportedFont {
	family: string;
	/** its name when imported (for the extension) */
	file: string;
	bytes: Uint8Array;
}

const imported = new Map<string, ImportedFont>();

/** Font files this size or smaller are read. */
export const MAX_FONT_BYTES = 32 * 1024 * 1024;
export const FONT_TYPES = '.ttf,.otf,.woff,.woff2';

/** Fonts imported so far (this session or from projects opened). */
export const importedFonts = () => [...imported.values()];

/** A family name from a font file's name: "My-Hand_Bold.ttf" → "My-Hand Bold". */
export function familyFromFile(name: string): string {
	const base = name.replace(/^.*[/\\]/, '').replace(/\.[a-z0-9]+$/i, '');
	return base.replace(/_+/g, ' ').trim().slice(0, 100) || 'Imported font';
}

/** Make a font file usable by texts (as `family`). False if it isn't a font. */
export async function addFont(family: string, file: string, bytes: Uint8Array): Promise<boolean> {
	if (typeof FontFace === 'undefined' || typeof document === 'undefined') return false;
	if (imported.get(family)?.bytes.length === bytes.length) return true;
	try {
		const face = new FontFace(family.replace(/["\\]/g, ''), bytes.slice().buffer);
		await face.load();
		document.fonts.add(face);
	} catch {
		return false;
	}
	imported.set(family, { family, file, bytes });
	return true;
}

/** Import a font file the user picked; its family, or null if it isn't one we can read. */
export async function importFontFile(f: File): Promise<string | null> {
	if (f.size > MAX_FONT_BYTES) return null;
	const bytes = new Uint8Array(await f.arrayBuffer());
	let family = familyFromFile(f.name);
	// another font by that name: number this one
	for (let n = 2; imported.has(family) && imported.get(family)!.bytes.length !== bytes.length; n++)
		family = `${familyFromFile(f.name)} ${n}`;
	return (await addFont(family, f.name, bytes)) ? family : null;
}

/** The imported fonts these texts use (what a project keeps). */
export function fontsUsed(specs: (TextSpec | null)[]): ImportedFont[] {
	const used = new Set(specs.map((s) => s?.font));
	return importedFonts().filter((f) => used.has(f.family));
}

// ---- layout ----

export interface TextLine {
	text: string;
	/** left of the line in the box */
	x: number;
	/** top of the line in the box */
	y: number;
	width: number;
}

export interface TextLayout {
	lines: TextLine[];
	/** the box (before turning) */
	width: number;
	height: number;
}

/**
 * Lines of a text, wrapped to its width (words; a word too long is cut) and aligned.
 * `measure` gives a string's width without letter spacing.
 */
export function layoutText(s: TextSpec, measure: (t: string) => number): TextLayout {
	const wide = (t: string) => measure(t) + s.letterSpacing * [...t].length;
	const rows: string[] = [];
	const push = (line: string) => {
		// a word wider than the box: cut it where it runs out
		let chars = [...line];
		while (chars.length > 1 && wide(chars.join('')) > s.width) {
			let n = chars.length - 1;
			while (n > 1 && wide(chars.slice(0, n).join('')) > s.width) n--;
			rows.push(chars.slice(0, n).join(''));
			chars = chars.slice(n);
		}
		rows.push(chars.join(''));
	};
	for (const para of s.text.split('\n')) {
		if (!(s.width > 0)) {
			rows.push(para);
			continue;
		}
		let line = '';
		for (const word of para.split(/(\s+)/)) {
			if (!word) continue;
			const next = line + word;
			if (!line || wide(next.trimEnd()) <= s.width) {
				line = next;
				continue;
			}
			push(line.trimEnd());
			line = /^\s+$/.test(word) ? '' : word;
		}
		push(line);
	}
	const widths = rows.map(wide);
	const width = s.width > 0 ? s.width : Math.max(0, ...widths);
	const step = s.size * s.lineHeight;
	return {
		lines: rows.map((text, i) => ({
			text,
			width: widths[i],
			y: i * step,
			x:
				s.align === 'center' ? (width - widths[i]) / 2 : s.align === 'right' ? width - widths[i] : 0
		})),
		width,
		height: rows.length * step
	};
}

/** The box's corners on the canvas (turned): top-left, top-right, bottom-right, bottom-left. */
export function boxCorners(s: TextSpec, w: number, h: number, pad = 0): [number, number][] {
	const cx = s.x + w / 2;
	const cy = s.y + h / 2;
	const a = (s.rotate * Math.PI) / 180;
	const [c, sn] = [Math.cos(a), Math.sin(a)];
	const hw = w / 2 + pad;
	const hh = h / 2 + pad;
	return (
		[
			[-hw, -hh],
			[hw, -hh],
			[hw, hh],
			[-hw, hh]
		] as [number, number][]
	).map(([x, y]) => [cx + x * c - y * sn, cy + x * sn + y * c]);
}

/** Whether canvas point (px, py) is in the box (turned), with `pad` px around it. */
export function inBox(s: TextSpec, w: number, h: number, px: number, py: number, pad = 0) {
	const cx = s.x + w / 2;
	const cy = s.y + h / 2;
	const a = (-s.rotate * Math.PI) / 180;
	const dx = px - cx;
	const dy = py - cy;
	const x = dx * Math.cos(a) - dy * Math.sin(a);
	const y = dx * Math.sin(a) + dy * Math.cos(a);
	return Math.abs(x) <= w / 2 + pad && Math.abs(y) <= h / 2 + pad;
}

// ---- drawing ----

function context(w: number, h: number) {
	if (typeof OffscreenCanvas === 'undefined') return null;
	return new OffscreenCanvas(Math.max(1, w), Math.max(1, h)).getContext('2d', {
		willReadFrequently: true
	});
}

/** A text's lines, measured with its font. */
export function measureText(s: TextSpec): TextLayout {
	const g = context(1, 1);
	if (!g) return layoutText(s, (t) => t.length * s.size * 0.55);
	g.font = cssFont(s);
	return layoutText(s, (t) => g.measureText(t).width);
}

/** A text drawn: straight-alpha RGBA `w`×`h` placed at (x, y), cut to the canvas. */
export interface TextPixels {
	x: number;
	y: number;
	w: number;
	h: number;
	rgba: Uint8Array;
	/** the box, measured */
	layout: TextLayout;
}

/** Draw a text for a `cw`×`ch` canvas. No pixels (w = 0) when none land on it. */
export function renderText(s: TextSpec, cw: number, ch: number): TextPixels {
	const layout = measureText(s);
	// room for letters that reach past their line (italics, tall accents)
	const corners = boxCorners(s, layout.width, layout.height, s.size * 0.6);
	const xs = corners.map((p) => p[0]);
	const ys = corners.map((p) => p[1]);
	const x = Math.max(0, Math.floor(Math.min(...xs)));
	const y = Math.max(0, Math.floor(Math.min(...ys)));
	const w = Math.min(cw, Math.ceil(Math.max(...xs))) - x;
	const h = Math.min(ch, Math.ceil(Math.max(...ys))) - y;
	const none = { x: 0, y: 0, w: 0, h: 0, rgba: new Uint8Array(0), layout };
	if (w <= 0 || h <= 0 || !s.text.trim()) return none;
	const g = context(w, h);
	if (!g) return none;
	g.translate(-x, -y);
	g.translate(s.x + layout.width / 2, s.y + layout.height / 2);
	g.rotate((s.rotate * Math.PI) / 180);
	g.translate(-layout.width / 2, -layout.height / 2);
	g.font = cssFont(s);
	g.fillStyle = s.color;
	g.textBaseline = 'middle';
	// (older canvases have no letter spacing: then a letter at a time)
	const spaced = typeof (g as { letterSpacing?: unknown }).letterSpacing === 'string';
	if (spaced) g.letterSpacing = `${s.letterSpacing}px`;
	const step = s.size * s.lineHeight;
	for (const line of layout.lines) {
		const mid = line.y + step / 2;
		if (spaced || !s.letterSpacing) {
			g.fillText(line.text, line.x, mid);
			continue;
		}
		let at = line.x;
		for (const c of line.text) {
			g.fillText(c, at, mid);
			at += g.measureText(c).width + s.letterSpacing;
		}
	}
	const data = g.getImageData(0, 0, w, h).data;
	return { x, y, w, h, rgba: new Uint8Array(data.buffer, data.byteOffset, data.length), layout };
}

/** Wait for a text's font to be ready (an imported one may still be loading). */
export async function fontReady(s: TextSpec) {
	try {
		await document.fonts.load(cssFont(s), s.text || 'A');
	} catch {
		// (a font that won't load falls back to the generic family)
	}
}
