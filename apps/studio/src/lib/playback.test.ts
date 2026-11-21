import { describe, expect, it } from 'vitest';
import { clampRange, frameAt, framesOf, previewShrink } from './playback';

const ticks = (n: number, f: (i: number) => number | null) =>
	Array.from({ length: n }, (_, i) => f(i));

describe('playback', () => {
	it('plays once, loops, or goes back and forth', () => {
		const r = { from: 2, to: 5 };
		expect(ticks(6, (i) => frameAt(i, r, 'once'))).toEqual([2, 3, 4, 5, null, null]);
		expect(ticks(9, (i) => frameAt(i, r, 'loop'))).toEqual([2, 3, 4, 5, 2, 3, 4, 5, 2]);
		expect(ticks(10, (i) => frameAt(i, r, 'pingpong'))).toEqual([2, 3, 4, 5, 4, 3, 2, 3, 4, 5]);
	});

	it('copes with a single frame', () => {
		const r = { from: 3, to: 3 };
		expect(frameAt(0, r, 'once')).toBe(3);
		expect(frameAt(1, r, 'once')).toBeNull();
		expect(frameAt(7, r, 'pingpong')).toBe(3);
	});

	it('keeps ranges inside the cut', () => {
		expect(clampRange({ from: 9, to: 2 }, 6)).toEqual({ from: 2, to: 5 });
		expect(clampRange({ from: -3, to: 0 }, 6)).toEqual({ from: 0, to: 0 });
		expect(framesOf({ from: 1, to: 3 })).toEqual([1, 2, 3]);
	});

	it('makes previews at most 960 pixels wide', () => {
		expect(previewShrink(800)).toBe(1);
		expect(previewShrink(1920)).toBe(2);
		expect(previewShrink(3840)).toBe(4);
	});
});
