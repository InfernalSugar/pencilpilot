import { describe, expect, it } from 'vitest';
import { filmFps, filmPlan, thin, thinIndices } from './timelapse';

const frame = (n: number) => ({
	n,
	closed: false,
	close() {
		this.closed = true;
	}
});

describe('timelapse', () => {
	it('thinning keeps every other frame and the last, closing the rest', () => {
		const f = Array.from({ length: 6 }, (_, i) => frame(i));
		const kept = thin(f);
		expect(kept.map((x) => x.n)).toEqual([0, 2, 4, 5]);
		expect(f.filter((x) => x.closed).map((x) => x.n)).toEqual([1, 3]);
	});

	it('films last about 15 seconds, at 4 to 30 frames a second', () => {
		expect(filmFps(300)).toBe(20);
		expect(filmFps(10)).toBe(4);
		expect(filmFps(5000)).toBe(30);
	});

	it('thins by index the same way', () => {
		expect(thinIndices(6)).toEqual([0, 2, 4, 5]);
		expect(thinIndices(5)).toEqual([0, 2, 4]);
	});

	it('plans a film of a given length, then holds the finished picture', () => {
		// 4 frames over 1 s at 8 fps: each held twice, then a 0.5 s hold
		expect(filmPlan(4, 8, 1, 0.5)).toEqual([0, 0, 1, 1, 2, 2, 3, 3, 3, 3, 3, 3]);
		// 10 frames in 0.5 s at 8 fps: some are skipped, the order is kept
		expect(filmPlan(10, 8, 0.5, 0)).toEqual([0, 2, 5, 7]);
		expect(filmPlan(0, 8, 1)).toEqual([]);
	});
});
