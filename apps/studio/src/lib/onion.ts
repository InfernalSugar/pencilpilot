/**
 * Onion skin and light table: which drawings show faintly over the canvas, in what
 * colour. Onion skin counts drawings, not frames (a drawing held for three frames is one
 * step back): the ones before in red, after in green, fainter the farther they are. The
 * light table shows any frames you pin, in blue.
 */
import { exposureAt, type Cut, type Track } from './timeline';

export interface OnionSettings {
	on: boolean;
	/** drawings shown before and after (0–5) */
	before: number;
	after: number;
	/** the nearest one's opacity (0–1); farther ones fade */
	opacity: number;
	beforeColour: string;
	afterColour: string;
	/** the active layer's drawings, or every layer's */
	layers: 'active' | 'all';
}

export const ONION_DEFAULTS: OnionSettings = {
	on: false,
	before: 1,
	after: 1,
	opacity: 0.6,
	beforeColour: '#e0453a',
	afterColour: '#22a05a',
	layers: 'active'
};

export const PIN_COLOUR = '#3d6bff';

/** Saved settings, cleaned. */
export function cleanOnion(raw: unknown): OnionSettings {
	const o = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
	const n = (v: unknown, d: number, lo: number, hi: number) =>
		typeof v === 'number' && Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : d;
	const colour = (v: unknown, d: string) =>
		typeof v === 'string' && /^#[0-9a-f]{6}$/i.test(v) ? v : d;
	const D = ONION_DEFAULTS;
	return {
		on: typeof o.on === 'boolean' ? o.on : D.on,
		before: Math.round(n(o.before, D.before, 0, 5)),
		after: Math.round(n(o.after, D.after, 0, 5)),
		opacity: n(o.opacity, D.opacity, 0.05, 1),
		beforeColour: colour(o.beforeColour, D.beforeColour),
		afterColour: colour(o.afterColour, D.afterColour),
		layers: o.layers === 'all' ? 'all' : 'active'
	};
}

export interface OnionEntry {
	drawing: number;
	colour: string;
	opacity: number;
}

/** The drawings before and after `frame` on a track: nearest first. */
export function neighbours(t: Track, frame: number, before: number, after: number) {
	const at = exposureAt(t, frame);
	// past the end (or on an empty cel past it): everything is "before"
	const here = at ? at.index : t.length;
	const prev: number[] = [];
	for (let i = here - 1; i >= 0 && prev.length < before; i--)
		if (t[i].drawing) prev.push(t[i].drawing);
	const next: number[] = [];
	for (let i = here + 1; i < t.length && next.length < after; i++)
		if (t[i].drawing) next.push(t[i].drawing);
	return { prev, next };
}

/** Farther drawings fade: the nearest at full opacity, the farthest at half. */
const fade = (opacity: number, i: number, n: number) => opacity * (1 - (0.5 * i) / Math.max(1, n));

/**
 * What the onion skin and light table show on `frame`: farthest first (each laid over
 * the ones before), never a drawing that's showing.
 */
export function onionEntries(
	cut: Cut,
	layers: number[],
	active: number,
	frame: number,
	s: OnionSettings,
	pinned: number[] = []
): OnionEntry[] {
	const which = s.layers === 'all' ? layers : layers.filter((l) => l === active);
	const showing = new Set(
		which.map((l) => {
			const t = cut.tracks[l];
			const at = t && exposureAt(t, frame);
			return at ? t[at.index].drawing : 0;
		})
	);
	const out: OnionEntry[] = [];
	const seen = new Set<number>();
	const add = (drawing: number, colour: string, opacity: number) => {
		if (!drawing || showing.has(drawing) || seen.has(drawing)) return;
		seen.add(drawing);
		out.push({ drawing, colour, opacity });
	};
	// the light table underneath everything
	for (const f of pinned)
		for (const l of which) {
			const t = cut.tracks[l];
			const at = t && exposureAt(t, f);
			if (at) add(t[at.index].drawing, PIN_COLOUR, s.opacity);
		}
	if (s.on)
		for (const l of which) {
			const t = cut.tracks[l];
			if (!t) continue;
			const { prev, next } = neighbours(t, frame, s.before, s.after);
			// farthest first, before and after alternating towards the nearest
			for (let i = Math.max(prev.length, next.length) - 1; i >= 0; i--) {
				if (prev[i]) add(prev[i], s.beforeColour, fade(s.opacity, i, s.before));
				if (next[i]) add(next[i], s.afterColour, fade(s.opacity, i, s.after));
			}
		}
	return out;
}
