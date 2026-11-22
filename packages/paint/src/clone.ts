/**
 * The Clone tool (Photoshop's Clone Stamp, with Heal for its Healing Brush):
 * Alt+click sets where to copy from, then each stroke copies the picture from
 * there. The pixel work is the engine's (a clone stroke); this is only where
 * each stroke copies from.
 */

export type Point = [number, number];

/**
 * How far from the brush a stroke starting at `at` copies (source − brush), or
 * null with no source yet. Aligned (as Photoshop has it by default): the first
 * stroke sets the offset (`kept`) and later strokes keep it, wherever they start.
 * Not aligned: every stroke starts copying from the source point again.
 */
export function cloneOffset(
	from: Point | null,
	at: Point,
	aligned: boolean,
	kept: Point | null
): Point | null {
	if (!from) return null;
	if (aligned && kept) return kept;
	return [from[0] - at[0], from[1] - at[1]];
}

type Settings = Record<string, { base_value: number; inputs: Record<string, unknown> }>;

/**
 * The .myb source of `myb` as a clone brush: its shape, size, softness and
 * pressure, but copying at full strength (a see-through pencil would otherwise
 * copy only faintly) and never smudging or erasing.
 */
export function cloneBrushSource(myb: string): string {
	const doc = JSON.parse(myb);
	const s = (doc.settings ??= {}) as Settings;
	const set = (k: string, v: number) => (s[k] = { base_value: v, inputs: {} });
	set('opaque', 1);
	set('opaque_linearize', 0);
	set('smudge', 0);
	set('eraser', 0);
	return JSON.stringify(doc);
}
