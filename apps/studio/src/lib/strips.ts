/**
 * The drawings of one layer in a cut, in order, as the new layout's slim strips show
 * them (Draw's frame strip, Colour's drawings to colour): each exposure of a drawing,
 * with its number on the layer and the frames it covers.
 */
import { drawingNumbers, startOf, type Track } from './timeline';

export interface StripDrawing {
	/** the exposure's place on the track */
	index: number;
	/** the drawing's id */
	drawing: number;
	/** its number on the layer (1, 2, 3…) */
	n: number;
	/** the first frame (0-based) and how many it's held */
	start: number;
	length: number;
}

export function stripDrawings(t: Track): StripDrawing[] {
	const nums = drawingNumbers(t);
	const out: StripDrawing[] = [];
	t.forEach((e, index) => {
		if (!e.drawing) return;
		out.push({
			index,
			drawing: e.drawing,
			n: nums.get(e.drawing) ?? 0,
			start: startOf(t, index),
			length: e.length
		});
	});
	return out;
}

/** the drawing showing on this frame (null: none) */
export const stripAt = (list: StripDrawing[], frame: number) =>
	list.find((d) => frame >= d.start && frame < d.start + d.length) ?? null;

/** frames 19–24, or frame 7 */
export const framesLabel = (d: StripDrawing) =>
	d.length > 1 ? `frames ${d.start + 1}–${d.start + d.length}` : `frame ${d.start + 1}`;

/** 0:02.50 */
export function clock(frames: number, fps: number): string {
	const s = frames / fps;
	const m = Math.floor(s / 60);
	return `${m}:${(s - m * 60).toFixed(2).padStart(5, '0')}`;
}
