import { describe, expect, it } from 'vitest';
import { apply, boxToQuad, fromAffine, usableQuad, type Pt, type Quad } from './homography';

const close = (a: Pt | null, b: Pt) => {
	expect(a).not.toBeNull();
	expect(a![0]).toBeCloseTo(b[0], 4);
	expect(a![1]).toBeCloseTo(b[1], 4);
};

describe('homography', () => {
	const box: [number, number, number, number] = [10, 20, 110, 70];
	const corners: Quad = [
		[10, 20],
		[110, 20],
		[110, 70],
		[10, 70]
	];

	it('the box onto itself is the identity', () => {
		const h = boxToQuad(box, corners);
		close(apply(h, [37, 41]), [37, 41]);
	});

	it('pins all four corners, also for a real perspective quad', () => {
		const q: Quad = [
			[30, 10],
			[90, 25],
			[120, 90],
			[0, 80]
		];
		const h = boxToQuad(box, q);
		corners.forEach((c, i) => close(apply(h, c), q[i]));
		// the centre lands inside the quad, not on the plain average (it's projective)
		const m = apply(h, [60, 45])!;
		expect(m[0]).toBeGreaterThan(0);
		expect(m[0]).toBeLessThan(120);
	});

	it('a parallelogram is affine (no perspective terms)', () => {
		const q: Quad = [
			[10, 20],
			[110, 30],
			[120, 80],
			[20, 70]
		];
		const h = boxToQuad(box, q);
		expect(h[6]).toBeCloseTo(0, 9);
		expect(h[7]).toBeCloseTo(0, 9);
	});

	it('affine matrices convert', () => {
		close(apply(fromAffine([1, 0, 0, 1, 5, -3]), [1, 1]), [6, -2]);
	});

	it('refuses folded or flat quads', () => {
		expect(usableQuad(corners)).toBe(true);
		const bowtie: Quad = [
			[0, 0],
			[10, 10],
			[10, 0],
			[0, 10]
		];
		expect(usableQuad(bowtie)).toBe(false);
		const flat: Quad = [
			[0, 0],
			[5, 0],
			[10, 0],
			[0, 0]
		];
		expect(usableQuad(flat)).toBe(false);
		const concave: Quad = [
			[0, 0],
			[10, 0],
			[3, 3],
			[0, 10]
		];
		expect(usableQuad(concave)).toBe(false);
	});
});
