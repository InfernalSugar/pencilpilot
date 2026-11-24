import { describe, expect, it } from 'vitest';
import {
	hardEdges,
	lassoBack,
	lassoOutline,
	resample,
	type LassoPoint,
	type Pt
} from './lasso-path';

const corner = (x: number, y: number): LassoPoint => ({ p: [x, y], corner: true });
const free = (x: number, y: number): LassoPoint => ({ p: [x, y], corner: false });

/** a wobbly freehand line from (0,0) to (100,0) */
const wobble = (): LassoPoint[] =>
	Array.from({ length: 49 }, (_, i) => free(2 + i * 2, i % 2 ? 6 : -6));

const has = (out: Pt[], p: Pt) => out.some((q) => q[0] === p[0] && q[1] === p[1]);

describe('resample', () => {
	it('spaces points evenly and keeps both ends', () => {
		const out = resample(
			[
				[0, 0],
				[10, 0],
				[10, 10]
			],
			2
		);
		expect(out[0]).toEqual([0, 0]);
		expect(out.at(-1)).toEqual([10, 10]);
		expect(out).toHaveLength(11);
		for (let i = 1; i < out.length; i++)
			expect(Math.hypot(out[i][0] - out[i - 1][0], out[i][1] - out[i - 1][1])).toBeCloseTo(2, 5);
	});
});

describe('lassoOutline', () => {
	it('is the points as drawn with smoothing off', () => {
		const pts = [corner(0, 0), free(5, 3), corner(10, 0), corner(5, 10)];
		expect(lassoOutline(pts, 0)).toEqual([
			[0, 0],
			[5, 3],
			[10, 0],
			[5, 10]
		]);
	});

	it('smooths the freehand part and keeps the corners sharp', () => {
		const pts = [corner(0, 0), ...wobble(), corner(100, 0), corner(100, 80), corner(0, 80)];
		const out = lassoOutline(pts, 12);
		// every clicked corner is still there, exactly
		for (const p of [
			[0, 0],
			[100, 0],
			[100, 80],
			[0, 80]
		] as Pt[])
			expect(has(out, p)).toBe(true);
		// the wobble is ironed out along the top (away from its ends)
		const top = out.filter((q) => q[0] > 20 && q[0] < 80 && q[1] < 40);
		expect(top.length).toBeGreaterThan(5);
		for (const q of top) expect(Math.abs(q[1])).toBeLessThan(2);
		// the straight edges stay straight: nothing extra along the right and bottom
		expect(out.filter((q) => q[0] > 99 && q[1] > 1 && q[1] < 79)).toHaveLength(0);
	});

	it('smooths a freehand loop all the way round', () => {
		// a jagged circle, no corners
		const pts = Array.from({ length: 60 }, (_, i) => {
			const a = (i / 60) * Math.PI * 2;
			const r = 50 + (i % 2 ? 4 : -4);
			return free(100 + Math.cos(a) * r, 100 + Math.sin(a) * r);
		});
		const out = lassoOutline(pts, 10);
		const radii = out.map((q) => Math.hypot(q[0] - 100, q[1] - 100));
		expect(Math.max(...radii) - Math.min(...radii)).toBeLessThan(3);
	});
});

describe('lassoBack', () => {
	it('takes away the last corner', () => {
		const pts = [corner(0, 0), corner(10, 0), corner(10, 10)];
		expect(lassoBack(pts)).toEqual(pts.slice(0, 2));
	});
	it('takes away a whole freehand run back to the corner it began at', () => {
		const pts = [corner(0, 0), corner(10, 0), free(12, 3), free(14, 6), corner(15, 10)];
		expect(lassoBack(pts)).toEqual(pts.slice(0, 2));
	});
	it('never takes the first point', () => {
		expect(lassoBack([corner(0, 0)])).toEqual([]);
		expect(lassoBack([corner(0, 0), free(3, 3)])).toEqual([corner(0, 0)]);
	});
});

describe('hardEdges', () => {
	it('makes every pixel all in or all out', () => {
		expect([...hardEdges(Uint8Array.from([0, 40, 127, 128, 200, 255]))]).toEqual([
			0, 0, 0, 255, 255, 255
		]);
	});
});
