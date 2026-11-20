import { describe, expect, it } from 'vitest';
import { meshLines, meshNodes, patchPoint, type Pt } from './warp';

/** an evenly spaced 4×4 grid over (x0, y0)–(x1, y1) */
const grid = (x0: number, y0: number, x1: number, y1: number): Pt[] =>
	Array.from({ length: 16 }, (_, i) => [
		x0 + ((x1 - x0) * (i % 4)) / 3,
		y0 + ((y1 - y0) * Math.floor(i / 4)) / 3
	]);

describe('warp patch', () => {
	it('an even grid is the plain box (no warp)', () => {
		const g = grid(10, 20, 100, 80);
		const [x, y] = patchPoint(g, 0.25, 0.5);
		expect(x).toBeCloseTo(10 + 90 * 0.25, 5);
		expect(y).toBeCloseTo(50, 5);
	});

	it('passes through its corner points', () => {
		const g = grid(0, 0, 90, 90);
		g[15] = [120, 130];
		expect(patchPoint(g, 1, 1)).toEqual([120, 130]);
		expect(patchPoint(g, 0, 0)).toEqual([0, 0]);
	});

	it('pulling an inner point bends the middle, not the corners', () => {
		const g = grid(0, 0, 90, 90);
		g[5] = [30, -30]; // row 1, column 1 pulled up
		const mid = patchPoint(g, 1 / 3, 1 / 3);
		expect(mid[1]).toBeLessThan(30);
		expect(patchPoint(g, 0, 0)).toEqual([0, 0]);
	});

	it('mesh nodes and grid lines have the right sizes', () => {
		const g = grid(0, 0, 90, 90);
		expect(meshNodes(g, 4)).toHaveLength(2 * 5 * 5);
		const n = meshNodes(g, 4);
		expect([n[0], n[1]]).toEqual([0, 0]);
		expect(n.at(-2)).toBeCloseTo(90, 4);
		expect(meshLines(g, 10)).toHaveLength(8);
	});
});
