import { describe, expect, it } from 'vitest';
import { drawingStarts, flipIndex, keyFrames, rollFrames } from './flip';
import { nextKind, setKind, kindsFromFile, kindsToFile, type Cut, type Timeline } from './timeline';
import { cleanOnion, onionEntries } from './onion';

// layer 1: drawings 1 (2 frames), 2, 3 (3 frames), 4; layer 2 static
const cut: Cut = {
	id: 1,
	name: 'C',
	tracks: {
		1: [
			{ drawing: 1, length: 2 },
			{ drawing: 2, length: 1 },
			{ drawing: 0, length: 1 },
			{ drawing: 3, length: 3 },
			{ drawing: 4, length: 1 }
		],
		2: [{ drawing: 9, length: 1 }]
	}
};

describe('flipping', () => {
	it('finds where drawings start, and rolls around this frame', () => {
		const starts = drawingStarts(cut, [1, 2], [2]);
		expect(starts).toEqual([0, 2, 4, 7]);
		expect(rollFrames(starts, 5, 1)).toEqual([2, 4, 7]);
		expect(rollFrames(starts, 0, 2)).toEqual([0, 2, 4]);
		expect(rollFrames(starts, 9, 2)).toEqual([2, 4, 7]);
	});

	it('flips through keys and breakdowns', () => {
		expect(keyFrames(cut, [1], { 1: 'key', 3: 'breakdown', 4: 'inbetween' })).toEqual([0, 4]);
	});

	it('goes back and forth', () => {
		expect([...Array(9).keys()].map((i) => flipIndex(i, 3))).toEqual([0, 1, 2, 1, 0, 1, 2, 1, 0]);
		expect(flipIndex(5, 1)).toBe(0);
	});
});

describe('drawing marks', () => {
	it('cycle key → breakdown → in-between → none, and go through the file', () => {
		expect(nextKind(undefined)).toBe('key');
		expect(nextKind('key')).toBe('breakdown');
		expect(nextKind('breakdown')).toBe('inbetween');
		expect(nextKind('inbetween')).toBe(null);
		const tl: Timeline = { scenes: [], statics: [] };
		setKind(tl, 3, 'key');
		setKind(tl, 5, 'breakdown');
		expect(kindsToFile(tl, [3])).toEqual({ D3: 'key' });
		setKind(tl, 3, null);
		setKind(tl, 5, null);
		expect(tl.kinds).toBeUndefined();
		expect(kindsFromFile({ D2: 'key', D4: 'wobbly', X: 'key' })).toEqual({ 2: 'key' });
	});

	it('onion skin can show only keys and breakdowns', () => {
		const s = { ...cleanOnion(null), on: true, before: 1, after: 1, keysOnly: true };
		// on frame 4 (drawing 3): the nearest key before is 1, no key after
		const e = onionEntries(cut, [1], 1, 4, s, [], { 1: 'key', 2: 'inbetween', 4: 'inbetween' });
		expect(e.map((x) => x.drawing)).toEqual([1]);
		const all = onionEntries(cut, [1], 1, 4, { ...s, keysOnly: false });
		expect(all.map((x) => x.drawing).sort()).toEqual([2, 4]);
	});
});
