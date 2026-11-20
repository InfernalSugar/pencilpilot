/**
 * The Smudge tool (Procreate's Smudge, Photoshop's Smudge / Mixer Brush): any
 * brush, turned into one that pushes the paint already on the layer around
 * instead of laying down colour. MyPaint brushes do this with their `smudge`
 * settings: `smudge` 1 paints only with the picked-up colour, and
 * `smudge_length` sets how long that colour lasts along the stroke.
 */

type Settings = Record<string, { base_value: number; inputs: Record<string, unknown> }>;

/** 0 (a short, soft blend) – 1 (drags paint a long way) */
export const SMUDGE_DEFAULT = 0.6;

/** The .myb source of `myb` as a smudge brush of the given strength (0–1). */
export function smudgeSource(myb: string, strength = SMUDGE_DEFAULT): string {
	const doc = JSON.parse(myb);
	const s = (doc.settings ??= {}) as Settings;
	const set = (k: string, v: number) => (s[k] = { base_value: v, inputs: {} });
	const k = Math.min(1, Math.max(0, strength));
	set('smudge', 1);
	// how slowly the picked-up colour is replaced: longer = paint carried further
	set('smudge_length', 0.4 + 0.58 * k);
	// pick up from a little more than the dab, so colours mix smoothly
	set('smudge_radius_log', 0.3);
	set('eraser', 0);
	// a smudge carries the paint at full strength, however see-through the brush
	// is when painting (pencils are): the strength is how far it carries it
	set('opaque', 1);
	set('opaque_multiply', 1);
	set('opaque_linearize', 0);
	return JSON.stringify(doc);
}
