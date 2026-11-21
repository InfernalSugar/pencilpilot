/**
 * Playback timing: which frame shows on each tick of a fixed-fps clock, for a range
 * played once, looped, or back and forth (ping-pong).
 */

export interface PlayRange {
	/** first and last frame played (inclusive) */
	from: number;
	to: number;
}

export type PlayMode = 'once' | 'loop' | 'pingpong';

/** The frame on tick `n` (0 = the first), or null when a single play is over. */
export function frameAt(n: number, r: PlayRange, mode: PlayMode): number | null {
	const len = r.to - r.from + 1;
	if (len <= 1) return mode === 'once' && n > 0 ? null : r.from;
	if (mode === 'once') return n < len ? r.from + n : null;
	if (mode === 'loop') return r.from + (n % len);
	// back and forth, without showing the ends twice: 0 1 2 3 2 1 0 1 …
	const period = 2 * (len - 1);
	const k = n % period;
	return r.from + (k < len ? k : period - k);
}

/** The frames a range plays, each once (to prepare them). */
export function framesOf(r: PlayRange): number[] {
	return Array.from({ length: Math.max(0, r.to - r.from + 1) }, (_, i) => r.from + i);
}

/** A range kept inside a cut `length` frames long, `from` ≤ `to`. */
export function clampRange(r: PlayRange, length: number): PlayRange {
	const last = Math.max(0, length - 1);
	const a = Math.min(Math.max(0, r.from), last);
	const b = Math.min(Math.max(0, r.to), last);
	return { from: Math.min(a, b), to: Math.max(a, b) };
}

/** How many times smaller a preview frame is, so it's at most `max` pixels wide. */
export const previewShrink = (width: number, max = 960) => Math.max(1, Math.ceil(width / max));
