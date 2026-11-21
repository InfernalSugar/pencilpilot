/**
 * Frame tools: copy, paste, duplicate, reverse, cycles and boil, on a range of frames of
 * one track. They work frame by frame (a track spelled out as the drawing on each
 * frame) and write the track back as exposures, so a range can start or end in the
 * middle of a held drawing.
 *
 * - Paste and cycles reuse the same drawings (change one, it changes everywhere it's
 *   exposed, as on an exposure sheet). Duplicate makes new drawings to change.
 * - Boil swaps a held drawing, every few frames, between it and slightly wobbled copies
 *   of it (`boil`), so a still drawing looks alive without moving.
 */
import { trackLength, type Exposure, type Track } from './timeline';

export interface FrameRange {
	from: number;
	to: number;
}

/** The drawing on each frame (0: empty), `length` frames (at least the track's). */
export function spell(t: Track, length = 0): number[] {
	const out: number[] = [];
	for (const e of t) for (let i = 0; i < e.length; i++) out.push(e.drawing);
	while (out.length < length) out.push(0);
	return out;
}

/** Frames back into exposures (a drawing on frames in a row is one exposure). */
export function unspell(frames: number[]): Track {
	const t: Track = [];
	for (const d of frames) {
		const last = t.at(-1);
		if (last && last.drawing === d) last.length++;
		else t.push({ drawing: d, length: 1 });
	}
	while (t.length && t.at(-1)!.drawing === 0) t.pop();
	return t;
}

/** Write frames into a track in place (so it works on `$state`). */
function write(t: Track, frames: number[]) {
	t.splice(0, t.length, ...unspell(frames));
}

const span = (r: FrameRange) => r.to - r.from + 1;
const clean = (r: FrameRange): FrameRange => ({
	from: Math.max(0, Math.min(r.from, r.to)),
	to: Math.max(0, r.from, r.to)
});

/** The drawings on a range of frames (a clip to paste). */
export function copyFrames(t: Track, r: FrameRange): number[] {
	const c = clean(r);
	return spell(t, c.to + 1).slice(c.from, c.to + 1);
}

/** Put frames in before `at` (later frames move along). */
export function pasteFrames(t: Track, at: number, clip: number[]) {
	if (!clip.length) return;
	const f = spell(t, at);
	f.splice(at, 0, ...clip);
	write(t, f);
}

/**
 * The range again right after it, as new drawings (`copy` makes one: the same drawing
 * twice in the range is copied once). Returns where the copy starts.
 */
export function duplicateFrames(t: Track, r: FrameRange, copy: (d: number) => number): number {
	const c = clean(r);
	const made = new Map<number, number>();
	const clip = copyFrames(t, c).map((d) => {
		if (!d) return 0;
		if (!made.has(d)) made.set(d, copy(d) || d);
		return made.get(d)!;
	});
	pasteFrames(t, c.to + 1, clip);
	return c.to + 1;
}

/** The range played backwards. */
export function reverseFrames(t: Track, r: FrameRange) {
	const c = clean(r);
	const f = spell(t, c.to + 1);
	const mid = f.slice(c.from, c.to + 1).reverse();
	f.splice(c.from, span(c), ...mid);
	write(t, f);
}

/**
 * Repeat the range `times` more times after it, the same drawings: a loop (A B C A B C)
 * or back and forth (A B C B A B C), each drawing held as it was.
 */
export function cycleFrames(t: Track, r: FrameRange, times: number, mode: 'loop' | 'pingpong') {
	const c = clean(r);
	const clip = copyFrames(t, c);
	const once = unspell(clip).length ? clip : [];
	if (!once.length || times < 1) return;
	let unit = once;
	if (mode === 'pingpong') {
		// back through the drawings between the ends, then forwards again
		const ex: Exposure[] = [];
		for (const d of once) {
			const last = ex.at(-1);
			if (last && last.drawing === d) last.length++;
			else ex.push({ drawing: d, length: 1 });
		}
		unit = [...ex.slice(1, -1).reverse(), ...ex].flatMap((e) =>
			Array<number>(e.length).fill(e.drawing)
		);
	}
	const more: number[] = [];
	for (let i = 0; i < Math.min(times, 99); i++) more.push(...unit);
	pasteFrames(t, c.to + 1, more);
}

export interface BoilSettings {
	/** drawings to swap between: the original and (n − 1) boiled copies */
	variants: number;
	/** frames each one shows (2: "on twos") */
	step: number;
	/** how far lines move, in pixels on a 1080-pixel-high canvas */
	amount: number;
}

/** Subtle, on twos: lines that breathe, not wriggle. */
export const BOIL_DEFAULTS: BoilSettings = { variants: 3, step: 2, amount: 1 };

/**
 * Boil the held drawings in the range: each hold longer than a step swaps, every
 * `step` frames, between the drawing and its boiled copies (`boil(drawing, n)` makes
 * copy n; a drawing held in several places reuses its copies). Returns how many
 * copies were made.
 */
export function boilFrames(
	t: Track,
	r: FrameRange,
	s: BoilSettings,
	boil: (drawing: number, n: number) => number
): number {
	const c = clean(r);
	const f = spell(t, c.to + 1);
	const variants = new Map<number, number[]>();
	let made = 0;
	const of = (d: number) => {
		if (!variants.has(d)) {
			const v = [d];
			for (let n = 1; n < Math.max(2, s.variants); n++) {
				const b = boil(d, n);
				if (b) {
					v.push(b);
					made++;
				}
			}
			variants.set(d, v);
		}
		return variants.get(d)!;
	};
	const step = Math.max(1, Math.round(s.step));
	let i = c.from;
	while (i <= c.to) {
		const d = f[i];
		let j = i;
		while (j + 1 <= c.to && f[j + 1] === d) j++;
		// a hold (within the range) longer than a step
		if (d && j - i + 1 > step) {
			const v = of(d);
			for (let k = i; k <= j; k++) f[k] = v[Math.floor((k - i) / step) % v.length];
		}
		i = j + 1;
	}
	if (made) write(t, f);
	return made;
}

/** The frames a range covers on a track, for the timeline to mark. */
export const inRange = (r: FrameRange | null, f: number) =>
	!!r && f >= Math.min(r.from, r.to) && f <= Math.max(r.from, r.to);

/** The range of the exposure on `frame` (none past the end). */
export function exposureRange(t: Track, frame: number): FrameRange | null {
	let start = 0;
	for (const e of t) {
		if (frame < start + e.length) return { from: start, to: start + e.length - 1 };
		start += e.length;
	}
	return frame < trackLength(t) ? { from: frame, to: frame } : null;
}
