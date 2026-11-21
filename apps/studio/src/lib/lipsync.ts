/**
 * Lip-sync: Rhubarb Lip Sync (in the app's Rust side) listens to a line and says which
 * mouth shape shows when; here that becomes exposures on the mouth layer, from the
 * drawings of its mouth chart.
 *
 * Timing follows the house style: on twos by default (a mouth changes at most 12 times
 * a second at 24 fps), but a closed mouth (M, B, P) is never lost to it, because lips
 * that never meet read as mumbling.
 */
import { spell, unspell } from './frametools';
import { MOUTH_SHAPES, type MouthChart, type MouthShape, type Track } from './timeline';

/** What each shape is for (Rhubarb's chart). */
export const SHAPE_INFO: Record<MouthShape, { label: string; hint: string; extended?: true }> = {
	A: { label: 'M B P', hint: 'Closed, lips pressed' },
	B: { label: 'K S T, ee', hint: 'Slightly open, teeth together' },
	C: { label: 'eh, a', hint: 'Open' },
	D: { label: 'aa', hint: 'Wide open' },
	E: { label: 'o, er', hint: 'Slightly rounded' },
	F: { label: 'oo, w', hint: 'Puckered' },
	G: { label: 'F V', hint: 'Top teeth on the lower lip', extended: true },
	H: { label: 'L', hint: 'Tongue up behind the teeth', extended: true },
	X: { label: 'rest', hint: 'Closed and relaxed (pauses)', extended: true }
};

/** A shape from `start` to `end` seconds (Rhubarb's answer). */
export interface Cue {
	start: number;
	end: number;
	value: string;
}

export interface LipSyncSettings {
	/** English words (better); otherwise sounds only, for any language */
	english: boolean;
	/** frames each mouth holds at least (2 = on twos) */
	step: number;
	/** frames the mouth moves ahead of the sound (it reads better a little early) */
	lead: number;
}

export const LIPSYNC_DEFAULTS: LipSyncSettings = { english: true, step: 2, lead: 0 };

/** Nearest shapes to stand in for one the chart doesn't have, best first. */
const STAND_INS: Record<MouthShape, MouthShape[]> = {
	A: ['X', 'B'],
	B: ['C', 'A'],
	C: ['B', 'D', 'E'],
	D: ['C', 'H', 'E'],
	E: ['C', 'F', 'D'],
	F: ['E', 'A'],
	G: ['B', 'A'],
	H: ['C', 'D'],
	X: ['A', 'B']
};

/** The drawing for a shape: its own, a near one, or any the chart has (0: none). */
export function drawingFor(chart: MouthChart, shape: MouthShape): number {
	const s = chart.shapes;
	for (const k of [shape, ...STAND_INS[shape]]) if (s[k]) return s[k]!;
	for (const k of MOUTH_SHAPES) if (s[k]) return s[k]!;
	return 0;
}

/** The extended shapes the chart has drawings for (Rhubarb uses only those). */
export const extendedOf = (chart: MouthChart) =>
	(['G', 'H', 'X'] as const).filter((k) => chart.shapes[k]).join('');

/** Fill a chart's empty shapes from a layer's drawings, in the order A–F, then G, H, X. */
export function autoChart(chart: MouthChart, drawings: number[]) {
	const used = new Set(Object.values(chart.shapes));
	const free = drawings.filter((d) => d && !used.has(d));
	for (const k of MOUTH_SHAPES)
		if (!chart.shapes[k] && free.length) chart.shapes[k] = free.shift()!;
}

/**
 * The shape on each of `frames` frames, from cues timed from frame 0. Frames go in
 * blocks of `step`; each block shows the shape that covers most of it, or the closed
 * mouth (A) if it's there for at least a quarter of a frame.
 */
export function shapeFrames(
	cues: Cue[],
	fps: number,
	frames: number,
	step = 2,
	lead = 0
): MouthShape[] {
	const valid = (v: string): v is MouthShape => (MOUTH_SHAPES as readonly string[]).includes(v);
	const shift = lead / fps;
	const out: MouthShape[] = [];
	const rest: MouthShape = cues.some((c) => c.value === 'X') ? 'X' : 'A';
	const n = Math.max(1, Math.round(step));
	for (let b = 0; b < frames; b += n) {
		const t0 = b / fps;
		const t1 = Math.min(b + n, frames) / fps;
		const cover = new Map<MouthShape, number>();
		for (const c of cues) {
			if (!valid(c.value)) continue;
			const o = Math.min(t1, c.end - shift) - Math.max(t0, c.start - shift);
			if (o > 0) cover.set(c.value, (cover.get(c.value) ?? 0) + o);
		}
		let pick: MouthShape = rest;
		if ((cover.get('A') ?? 0) >= 0.25 / fps) pick = 'A';
		else {
			let best = 0;
			for (const [k, v] of cover)
				if (v > best + 1e-9) {
					best = v;
					pick = k;
				}
		}
		for (let f = b; f < Math.min(b + n, frames); f++) out.push(pick);
	}
	return out;
}

/**
 * Write mouth shapes onto a track from frame `from` (in place: what was on those frames
 * goes; frames before and after stay).
 */
export function writeMouths(t: Track, from: number, shapes: MouthShape[], chart: MouthChart) {
	const f = spell(t, from + shapes.length);
	shapes.forEach((s, i) => (f[from + i] = drawingFor(chart, s)));
	t.splice(0, t.length, ...unspell(f));
}

/** The dialogue notes on a cut's frames, in order (what's said, to help recognition). */
export function dialogueText(notes: Record<number, string> | undefined, from = 0, to = Infinity) {
	return Object.entries(notes ?? {})
		.filter(([f]) => +f >= from && +f < to)
		.sort(([a], [b]) => +a - +b)
		.map(([, t]) => t.trim())
		.join(' ')
		.replace(/-\s+/g, '')
		.trim();
}
