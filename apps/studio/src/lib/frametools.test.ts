import { describe, expect, it } from 'vitest';
import {
	boilFrames,
	copyFrames,
	cycleFrames,
	duplicateFrames,
	exposureRange,
	pasteFrames,
	reverseFrames,
	spell,
	unspell
} from './frametools';
import type { Track } from './timeline';

// drawings A=1, B=2, C=3
const abc = (): Track => [
	{ drawing: 1, length: 2 },
	{ drawing: 2, length: 1 },
	{ drawing: 3, length: 2 }
];

describe('frame tools', () => {
	it('spell tracks out frame by frame and back', () => {
		expect(spell(abc())).toEqual([1, 1, 2, 3, 3]);
		expect(spell([], 2)).toEqual([0, 0]);
		expect(unspell([1, 1, 0, 0, 2, 0])).toEqual([
			{ drawing: 1, length: 2 },
			{ drawing: 0, length: 2 },
			{ drawing: 2, length: 1 }
		]);
	});

	it('copy and paste frames (the same drawings), even mid-hold', () => {
		const t = abc();
		const clip = copyFrames(t, { from: 1, to: 2 }); // A's second frame, B
		expect(clip).toEqual([1, 2]);
		pasteFrames(t, 5, clip);
		expect(spell(t)).toEqual([1, 1, 2, 3, 3, 1, 2]);
		// past the end: empty frames fill the gap
		const u: Track = [];
		pasteFrames(u, 2, [7]);
		expect(spell(u)).toEqual([0, 0, 7]);
	});

	it('duplicate frames as new drawings, each copied once', () => {
		const t: Track = [
			{ drawing: 1, length: 1 },
			{ drawing: 2, length: 1 },
			{ drawing: 1, length: 1 }
		];
		let next = 10;
		const made: number[] = [];
		const at = duplicateFrames(t, { from: 0, to: 2 }, (d) => {
			made.push(d);
			return next++;
		});
		expect(at).toBe(3);
		expect(made).toEqual([1, 2]);
		expect(spell(t)).toEqual([1, 2, 1, 10, 11, 10]);
	});

	it('reverse a range', () => {
		const t = abc();
		reverseFrames(t, { from: 0, to: 4 });
		expect(spell(t)).toEqual([3, 3, 2, 1, 1]);
		reverseFrames(t, { from: 3, to: 2 }); // (either way round)
		expect(spell(t)).toEqual([3, 3, 1, 2, 1]);
	});

	it('cycle a range: loops and back-and-forth, holds kept', () => {
		const loop = abc();
		cycleFrames(loop, { from: 0, to: 4 }, 2, 'loop');
		expect(spell(loop)).toEqual([1, 1, 2, 3, 3, 1, 1, 2, 3, 3, 1, 1, 2, 3, 3]);
		const pp = abc();
		cycleFrames(pp, { from: 0, to: 4 }, 1, 'pingpong');
		expect(spell(pp)).toEqual([1, 1, 2, 3, 3, 2, 1, 1, 2, 3, 3]);
		// cycling nothing does nothing
		const empty: Track = [];
		cycleFrames(empty, { from: 0, to: 3 }, 3, 'loop');
		expect(empty).toEqual([]);
	});

	it('boil held drawings on twos, between the drawing and its copies', () => {
		const t: Track = [
			{ drawing: 1, length: 6 },
			{ drawing: 2, length: 1 },
			{ drawing: 1, length: 3 }
		];
		const asked: [number, number][] = [];
		const made = boilFrames(t, { from: 0, to: 9 }, { variants: 3, step: 2, amount: 1 }, (d, n) => {
			asked.push([d, n]);
			return d * 100 + n;
		});
		// drawing 1 boiled twice (reused in its second hold); 2 is held only one frame
		expect(made).toBe(2);
		expect(asked).toEqual([
			[1, 1],
			[1, 2]
		]);
		expect(spell(t)).toEqual([1, 1, 101, 101, 102, 102, 2, 1, 1, 101]);
		// a range inside a hold boils just that part
		const u: Track = [{ drawing: 5, length: 8 }];
		boilFrames(u, { from: 4, to: 7 }, { variants: 2, step: 2, amount: 1 }, () => 9);
		expect(spell(u)).toEqual([5, 5, 5, 5, 5, 5, 9, 9]);
	});

	it('finds the exposure on a frame', () => {
		expect(exposureRange(abc(), 1)).toEqual({ from: 0, to: 1 });
		expect(exposureRange(abc(), 4)).toEqual({ from: 3, to: 4 });
		expect(exposureRange(abc(), 9)).toBeNull();
	});
});
