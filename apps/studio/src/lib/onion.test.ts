import { describe, expect, it } from 'vitest';
import { cleanOnion, neighbours, onionEntries, ONION_DEFAULTS, PIN_COLOUR } from './onion';
import type { Cut } from './timeline';

const cut: Cut = {
	id: 1,
	name: 'Cut 1',
	tracks: {
		// drawings 1–4, drawing 2 held for 3 frames, an empty cel before drawing 4
		1: [
			{ drawing: 1, length: 1 },
			{ drawing: 2, length: 3 },
			{ drawing: 3, length: 1 },
			{ drawing: 0, length: 1 },
			{ drawing: 4, length: 1 }
		],
		2: [{ drawing: 9, length: 7 }]
	}
};
const on = { ...ONION_DEFAULTS, on: true };

describe('onion skin', () => {
	it('counts drawings, not frames', () => {
		// frame 3 is the last frame of drawing 2
		expect(neighbours(cut.tracks[1], 3, 2, 2)).toEqual({ prev: [1], next: [3, 4] });
		// on the empty cel: drawing 3 before, 4 after
		expect(neighbours(cut.tracks[1], 5, 1, 1)).toEqual({ prev: [3], next: [4] });
		// past the end: everything before
		expect(neighbours(cut.tracks[1], 20, 2, 2)).toEqual({ prev: [4, 3], next: [] });
	});

	it('shows red before, green after, farthest first and fainter', () => {
		const e = onionEntries(cut, [1, 2], 1, 4, { ...on, before: 2, after: 1 });
		expect(e.map((x) => x.drawing)).toEqual([1, 2, 4]);
		expect(e[0]).toMatchObject({ colour: ONION_DEFAULTS.beforeColour });
		expect(e[0].opacity).toBeLessThan(e[1].opacity);
		expect(e[2]).toMatchObject({ colour: ONION_DEFAULTS.afterColour, opacity: on.opacity });
	});

	it('uses the active layer, or all of them', () => {
		expect(onionEntries(cut, [1, 2], 2, 2, on)).toEqual([]);
		const all = onionEntries(cut, [1, 2], 2, 2, { ...on, layers: 'all' });
		expect(all.map((x) => x.drawing)).toEqual([1, 3]);
	});

	it('pins frames on the light table, under the onion skin, never what shows', () => {
		const e = onionEntries(cut, [1, 2], 1, 2, on, [0, 2, 6]);
		expect(e.map((x) => [x.drawing, x.colour])).toEqual([
			[1, PIN_COLOUR],
			[4, PIN_COLOUR],
			[3, ONION_DEFAULTS.afterColour]
		]);
		// off: just the pins
		expect(onionEntries(cut, [1, 2], 1, 2, ONION_DEFAULTS, [6]).length).toBe(1);
	});

	it('cleans saved settings', () => {
		expect(cleanOnion(null)).toEqual(ONION_DEFAULTS);
		expect(cleanOnion({ before: 9, opacity: 3, beforeColour: 'red', layers: 'all' })).toMatchObject(
			{ before: 5, opacity: 1, beforeColour: ONION_DEFAULTS.beforeColour, layers: 'all' }
		);
	});
});
