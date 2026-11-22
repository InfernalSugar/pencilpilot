/**
 * Filters for the Adjust panel (Procreate's Adjustments, Photoshop's Filter menu).
 * The engine does the pixel work (crates/brush/src/filters.rs); this is each
 * filter's settings as the panel shows them.
 */

export type FilterKind =
	'sharpen' | 'motion' | 'noise' | 'halftone' | 'chromatic' | 'bloom' | 'glitch';

/** A filter and its settings, as the engine takes them (`a`, `b`, `c` per filter). */
export interface FilterSpec {
	kind: FilterKind;
	a: number;
	b: number;
	c: number;
	/** noise and halftone: grey instead of colour */
	mono: boolean;
	/** noise and glitch: which pattern */
	seed: number;
}

export interface FilterSlider {
	key: 'a' | 'b' | 'c';
	label: string;
	min: number;
	max: number;
	/** shown value = setting × scale */
	scale: number;
	unit: string;
}

export interface FilterInfo {
	id: FilterKind;
	label: string;
	sliders: FilterSlider[];
	/** settings when picked (something shows straight away) */
	start: { a: number; b: number; c: number };
	/** has a grey / colour switch */
	mono?: boolean;
	/** has a "new pattern" button */
	seeded?: boolean;
}

const pct = (key: FilterSlider['key'], label: string, max = 100): FilterSlider => ({
	key,
	label,
	min: 0,
	max,
	scale: 100,
	unit: '%'
});
const px = (key: FilterSlider['key'], label: string, min: number, max: number): FilterSlider => ({
	key,
	label,
	min,
	max,
	scale: 1,
	unit: ' px'
});

export const FILTERS: FilterInfo[] = [
	{
		id: 'sharpen',
		label: 'Sharpen',
		sliders: [pct('a', 'Amount', 300), px('b', 'Radius', 1, 20)],
		start: { a: 1, b: 3, c: 0 }
	},
	{
		id: 'motion',
		label: 'Motion blur',
		sliders: [
			{ key: 'a', label: 'Angle', min: -180, max: 180, scale: 1, unit: '°' },
			px('b', 'Distance', 0, 100)
		],
		start: { a: 0, b: 20, c: 0 }
	},
	{
		id: 'noise',
		label: 'Noise',
		sliders: [pct('a', 'Amount')],
		start: { a: 0.2, b: 0, c: 0 },
		mono: true,
		seeded: true
	},
	{
		id: 'halftone',
		label: 'Halftone',
		sliders: [px('a', 'Dot size', 2, 40)],
		start: { a: 8, b: 0, c: 0 },
		mono: true
	},
	{
		id: 'chromatic',
		label: 'Chromatic aberration',
		sliders: [px('a', 'Amount', 0, 60)],
		start: { a: 8, b: 0, c: 0 }
	},
	{
		id: 'bloom',
		label: 'Bloom',
		sliders: [pct('a', 'Threshold'), px('b', 'Size', 1, 80), pct('c', 'Strength', 200)],
		start: { a: 0.6, b: 20, c: 1 }
	},
	{
		id: 'glitch',
		label: 'Glitch',
		sliders: [pct('a', 'Amount'), px('b', 'Block size', 2, 80)],
		start: { a: 0.4, b: 12, c: 0 },
		seeded: true
	}
];

export const filterInfo = (kind: FilterKind): FilterInfo =>
	FILTERS.find((f) => f.id === kind) ?? FILTERS[0];

/** A filter with its starting settings (keeping the pattern and grey switch). */
export function pickFilter(kind: FilterKind, was?: FilterSpec): FilterSpec {
	return { kind, ...filterInfo(kind).start, mono: was?.mono ?? false, seed: was?.seed ?? 1 };
}

/** Another pattern for noise and glitch. */
export const reseed = (f: FilterSpec): FilterSpec => ({ ...f, seed: (f.seed % 9973) + 1 });
