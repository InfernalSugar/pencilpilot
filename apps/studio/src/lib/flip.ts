/**
 * Flipping, as animators flip paper: hold a key and the frames around this one roll by
 * (back and forth), or the keys and breakdowns of the cut do; let go and you're back.
 */
import { startOf, type Cut, type DrawingKind } from './timeline';

/** Frames a second while flipping. */
export const FLIP_FPS = 8;

/** Frames where a new drawing starts on any of `layers` (static layers aside), in order. */
export function drawingStarts(cut: Cut, layers: number[], statics: number[] = []): number[] {
	const out = new Set<number>();
	for (const l of layers) {
		if (statics.includes(l)) continue;
		const t = cut.tracks[l] ?? [];
		t.forEach((e, i) => e.drawing && out.add(startOf(t, i)));
	}
	return [...out].sort((a, b) => a - b);
}

/** The drawings around `frame`: `n` changes before and after it (and where it is). */
export function rollFrames(starts: number[], frame: number, n = 2): number[] {
	let here = -1;
	for (let i = 0; i < starts.length; i++) if (starts[i] <= frame) here = i;
	if (here < 0) return starts.slice(0, n + 1);
	return starts.slice(Math.max(0, here - n), here + n + 1);
}

/** Frames where a key or breakdown starts on any of `layers`, in order. */
export function keyFrames(
	cut: Cut,
	layers: number[],
	kinds: Record<number, DrawingKind>,
	statics: number[] = []
): number[] {
	const out = new Set<number>();
	for (const l of layers) {
		if (statics.includes(l)) continue;
		const t = cut.tracks[l] ?? [];
		t.forEach((e, i) => {
			const k = kinds[e.drawing];
			if (k === 'key' || k === 'breakdown') out.add(startOf(t, i));
		});
	}
	return [...out].sort((a, b) => a - b);
}

/** Which of `n` frames shows at step `i` of a flip: back and forth (0 1 2 1 0 1 …). */
export function flipIndex(i: number, n: number): number {
	if (n <= 1) return 0;
	const period = 2 * (n - 1);
	const k = i % period;
	return k < n ? k : period - k;
}
