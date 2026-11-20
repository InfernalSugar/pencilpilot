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
	{ id: 'large', label: 'Large 4:3', width: 1600, height: 1200 },
	{ id: 'fhd', label: 'Full HD', width: 1920, height: 1080 },
	{ id: 'uhd4k', label: '4K', width: 3840, height: 2160 },
	{ id: 'uhd8k', label: '8K', width: 7680, height: 4320 }
];

export const DEFAULT_CANVAS: CanvasSize = { width: 800, height: 600 };
/**
 * Limits for custom sizes. Memory is the real limit: every layer keeps its own
 * pixels (8 bytes each where painted, ~66 MB for a full 4K layer, ~265 MB at
 * 8K) plus undo copies, so the cap is 8K UHD and big canvases get a warning.
 */
export const SIDE_MIN = 256;
export const SIDE_MAX = 8192;
export const AREA_MAX = 7680 * 4320;
/** above this (about 4K) drawing still works but memory use is high */
export const AREA_BIG = 3840 * 2160;
export const ASPECT_MAX = 3;

/** A valid canvas size, or null with why not. */
export function checkSize(
	w: number,
	h: number
): { ok: true; size: CanvasSize } | { ok: false; why: string } {
	if (!Number.isInteger(w) || !Number.isInteger(h)) return { ok: false, why: 'Use whole pixels.' };
	if (w < SIDE_MIN || h < SIDE_MIN) return { ok: false, why: `At least ${SIDE_MIN} px each side.` };
	if (w > SIDE_MAX || h > SIDE_MAX) return { ok: false, why: `At most ${SIDE_MAX} px each side.` };
	if (w * h > AREA_MAX) return { ok: false, why: 'That’s too big (8K, 7680 × 4320, at most).' };
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

/** A note for very big canvases (null if it's an everyday size). */
export function sizeNote(s: CanvasSize): string | null {
	const area = s.width * s.height;
	if (area > AREA_BIG)
		return 'Very big canvas: it uses a lot of memory. Keep to a few layers, and expect fills and filters to take a moment.';
	if (area > 1920 * 1080 * 1.5)
		return (
			'Big canvas: each full layer takes about ' + Math.round((area * 8) / 1e6) + ' MB of memory.'
		);
	return null;
}
