import { describe, expect, it } from 'vitest';
import { readStrokes } from './vectorise';

describe('strokes from the engine', () => {
	it('are read from its flat answer', () => {
		expect(readStrokes([2, 2, 1, 2, 3, 4, 5, 6, 1, 7, 8, 9])).toEqual([
			[
				{ x: 1, y: 2, width: 3 },
				{ x: 4, y: 5, width: 6 }
			],
			[{ x: 7, y: 8, width: 9 }]
		]);
		expect(readStrokes([0])).toEqual([]);
		expect(readStrokes([])).toEqual([]);
		// cut short: what's there, no more
		expect(readStrokes([1, 3, 1, 2, 3, 4])).toEqual([[{ x: 1, y: 2, width: 3 }]]);
	});
});
