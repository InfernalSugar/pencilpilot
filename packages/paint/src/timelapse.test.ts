import { describe, expect, it } from 'vitest';
import { filmFps, thin } from './timelapse';

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
});
