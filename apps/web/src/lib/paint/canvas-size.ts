/** Canvas shapes students can pick before they start drawing. */
export interface CanvasSize {
	width: number;
	height: number;
}

export const CANVAS_PRESETS: { id: string; label: string; width: number; height: number }[] = [
	{ id: 'classic', label: 'Landscape 4:3', width: 800, height: 600 },
	{ id: 'portrait', label: 'Portrait 3:4', width: 600, height: 800 },
	{ id: 'square', label: 'Square', width: 720, height: 720 },
	{ id: 'wide', label: 'Wide 16:9', width: 1024, height: 576 },
	{ id: 'a4p', label: 'A4 portrait', width: 707, height: 1000 },
	{ id: 'a4l', label: 'A4 landscape', width: 1000, height: 707 },
	{ id: 'large', label: 'Large 4:3', width: 1600, height: 1200 }
];

export const DEFAULT_CANVAS: CanvasSize = { width: 800, height: 600 };
/** Limits for custom sizes: memory (every layer stores its pixels) and sanity. */
export const SIDE_MIN = 256;
export const SIDE_MAX = 2048;
export const AREA_MAX = 2048 * 1536;
export const ASPECT_MAX = 3;

/** A valid canvas size, or null with why not. */
export function checkSize(
	w: number,
	h: number
): { ok: true; size: CanvasSize } | { ok: false; why: string } {
	if (!Number.isInteger(w) || !Number.isInteger(h)) return { ok: false, why: 'Use whole pixels.' };
	if (w < SIDE_MIN || h < SIDE_MIN) return { ok: false, why: `At least ${SIDE_MIN} px each side.` };
	if (w > SIDE_MAX || h > SIDE_MAX) return { ok: false, why: `At most ${SIDE_MAX} px each side.` };
	if (w * h > AREA_MAX) return { ok: false, why: 'That’s too big (about 3 megapixels at most).' };
	if (Math.max(w / h, h / w) > ASPECT_MAX)
		return { ok: false, why: 'Too long and thin (3:1 at most).' };
	return { ok: true, size: { width: w, height: h } };
}

const KEY = 'pp.canvasSize';

export function savedSize(): CanvasSize {
	try {
		const v = JSON.parse(localStorage.getItem(KEY) ?? 'null');
		const c = v && checkSize(v.width, v.height);
		if (c && c.ok) return c.size;
	} catch {
		/* storage blocked or bad value */
	}
	return DEFAULT_CANVAS;
}

export function saveSize(s: CanvasSize) {
	try {
		localStorage.setItem(KEY, JSON.stringify(s));
	} catch {
		/* storage blocked: the choice lasts for this page only */
	}
}

export const presetOf = (s: CanvasSize) =>
	CANVAS_PRESETS.find((p) => p.width === s.width && p.height === s.height)?.id ?? 'custom';
