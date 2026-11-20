import { describe, expect, it } from 'vitest';
import { corner, dragPart, hitPath, pathD, samplePath, type PenPath } from './pen-path';

const line: PenPath = { anchors: [corner([0, 0]), corner([100, 0])], closed: false };

describe('pen paths', () => {
	it('a straight segment is sampled evenly from end to end', () => {
		const pts = samplePath(line, 10);
		expect(pts[0]).toEqual([0, 0]);
		expect(pts.at(-1)).toEqual([100, 0]);
		expect(pts.length).toBeGreaterThanOrEqual(10);
		for (let i = 1; i < pts.length - 1; i++)
			expect(Math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1])).toBeCloseTo(10, 0);
	});

	it('dragging out a handle bends the curve, mirrored on the other side', () => {
		const bent = dragPart(line, 0, 'hout', [0, 60]);
		expect(bent.anchors[0].hin).toEqual([0, -60]);
		const ys = samplePath(bent, 5).map((p) => p[1]);
		expect(Math.max(...ys)).toBeGreaterThan(10);
	});

	it('moving a point carries its handles', () => {
		const bent = dragPart(line, 0, 'hout', [0, 60]);
		const moved = dragPart(bent, 0, 'p', [10, 10]);
		expect(moved.anchors[0].hout).toEqual([10, 70]);
	});

	it('closing adds the last segment', () => {
		const tri: PenPath = {
			anchors: [corner([0, 0]), corner([100, 0]), corner([50, 80])],
			closed: true
		};
		expect(pathD(tri)).toMatch(/Z$/);
		expect(samplePath(tri, 5).at(-1)).toEqual([0, 0]);
	});

	it('finds points and handles under the pointer', () => {
		const bent = dragPart(line, 1, 'hout', [140, 0]);
		expect(hitPath(bent, [101, 1], 4)).toEqual({ index: 1, part: 'p' });
		expect(hitPath(bent, [139, 0], 4)).toEqual({ index: 1, part: 'hout' });
		expect(hitPath(bent, [50, 50], 4)).toBeNull();
	});
});
