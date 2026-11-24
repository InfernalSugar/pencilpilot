import { describe, expect, it } from 'vitest';
import { nearestInk, nearestOnPaths, nearestPoint, strokeEnds, strokePath } from './ink-snap';
import { STRIDE, type LineStroke } from './lines';

const stroke = (pts: [number, number][], span?: [number, number]): LineStroke =>
	({
		id: 's',
		brush: 'pencil',
		color: '#000000',
		size: 1,
		opacity: 1,
		radius: 2,
		seed: 1,
		pts: Float32Array.from(pts.flatMap(([x, y]) => [x, y, 0.5, 0, 0, 0.008])),
		...(span ? { span } : {})
	}) as unknown as LineStroke;

describe('snap to ink', () => {
	it('a stroke’s ends (a cut piece: the ends it shows)', () => {
		const s = stroke([
			[0, 0],
			[10, 0],
			[20, 5],
			[30, 5]
		]);
		expect(s.pts.length).toBe(4 * STRIDE);
		expect(strokeEnds(s)).toEqual([
			[0, 0],
			[30, 5]
		]);
		expect(
			strokeEnds(
				stroke(
					[
						[0, 0],
						[10, 0],
						[20, 5],
						[30, 5]
					],
					[1, 3]
				)
			)
		).toEqual([
			[10, 0],
			[20, 5]
		]);
		expect(strokePath(stroke([[4, 4]]))).toEqual([[4, 4]]);
	});

	it('the nearest end within reach, else none', () => {
		const ends: [number, number][] = [
			[0, 0],
			[100, 0],
			[104, 3]
		];
		expect(nearestPoint(ends, [103, 4], 10)).toEqual([104, 3]);
		expect(nearestPoint(ends, [50, 0], 10)).toBeNull();
		expect(nearestPoint([], [50, 0], 10)).toBeNull();
	});

	it('the nearest point along a line', () => {
		const paths: [number, number][][] = [
			[
				[0, 0],
				[100, 0]
			],
			[
				[0, 50],
				[100, 50]
			]
		];
		expect(nearestOnPaths(paths, [40, 6], 10)).toEqual([40, 0]);
		expect(nearestOnPaths(paths, [40, 44], 10)).toEqual([40, 50]);
		// past an end: the end
		expect(nearestOnPaths(paths, [105, 3], 10)).toEqual([100, 0]);
		expect(nearestOnPaths(paths, [40, 25], 10)).toBeNull();
	});

	it('the nearest inked pixel within reach', () => {
		const [w, h] = [40, 30];
		const rgba = new Uint8Array(w * h * 4);
		// a line across at row 10, columns 5–34; a faint mark at (20, 20)
		for (let x = 5; x < 35; x++) rgba[(10 * w + x) * 4 + 3] = 255;
		rgba[(20 * w + 20) * 4 + 3] = 20;
		expect(nearestInk(rgba, w, h, [20.5, 16], 8)).toEqual([20.5, 10.5]);
		// the faint mark doesn't count; the line is too far
		expect(nearestInk(rgba, w, h, [20.5, 24], 8)).toBeNull();
		// near the end of the line: its last pixel
		expect(nearestInk(rgba, w, h, [38, 10.5], 8)).toEqual([34.5, 10.5]);
		expect(nearestInk(null, w, h, [20, 16], 8)).toBeNull();
	});
});
