import { describe, expect, it } from 'vitest';
import { Stabilizer, smoothTool, stringLength, type TipPoint } from './stabilizer';

/** feed points through, keeping where the tip went */
function run(s: Stabilizer, pts: [number, number, number][]) {
	const out: TipPoint[] = [];
	for (const [x, y, p] of pts) {
		const t = s.push(x, y, p);
		if (t) out.push(t);
	}
	return out;
}

describe('stroke smoothing (pulled string)', () => {
	it('with no string, the tip is the pointer (no lag)', () => {
		const s = new Stabilizer(0);
		s.reset();
		const pts: [number, number, number][] = [
			[0, 0, 0.2],
			[5, 3, 0.4],
			[9, -2, 0.9]
		];
		expect(run(s, pts)).toEqual(pts.map(([x, y, p]) => ({ x, y, p })));
		expect(s.catchUp()).toEqual([]);
	});

	it('the tip stays put while the string is slack, then trails by its length', () => {
		const s = new Stabilizer(10);
		s.reset();
		expect(s.push(0, 0, 0.5)).toEqual({ x: 0, y: 0, p: 0.5 });
		expect(s.push(6, 0, 0.5)).toBeNull();
		expect(s.push(0, 8, 0.5)).toBeNull();
		const t = s.push(30, 0, 0.5)!;
		expect(t.x).toBeCloseTo(20);
		expect(t.y).toBeCloseTo(0);
		// pulled at an angle: straight towards the pointer, the string's length short of it
		const u = s.push(50, 20, 0.5)!;
		expect(Math.hypot(50 - u.x, 20 - u.y)).toBeCloseTo(10);
	});

	it('steadies a wobbly line', () => {
		const s = new Stabilizer(12);
		s.reset();
		const wobbly: [number, number, number][] = Array.from({ length: 60 }, (_, i) => [
			i * 4,
			i % 2 ? 6 : -6,
			0.5
		]);
		const tips = run(s, wobbly);
		const ys = tips.slice(5).map((t) => t.y);
		expect(Math.max(...ys) - Math.min(...ys)).toBeLessThan(12 * 0.25);
		// and it still gets where the pointer went
		expect(tips.at(-1)!.x).toBeGreaterThan(220);
	});

	it('smooths pressure as the tip moves', () => {
		const s = new Stabilizer(10);
		s.reset();
		s.push(0, 0, 0.1);
		const t = s.push(20, 0, 0.9)!;
		// the tip moved 10, the string's length: half way to the new pressure
		expect(t.p).toBeCloseTo(0.5);
		const ps = run(
			s,
			Array.from({ length: 20 }, (_, i) => [30 + i * 2, 0, i % 2 ? 1 : 0.2])
		).map((q) => q.p);
		for (let i = 1; i < ps.length; i++) expect(Math.abs(ps[i] - ps[i - 1])).toBeLessThan(0.3);
	});

	it('catches up on pen-up, ending where the pen lifted', () => {
		const s = new Stabilizer(20);
		s.reset();
		run(s, [
			[0, 0, 0.5],
			[50, 0, 0.5],
			[60, 10, 0.2]
		]);
		const tail = s.catchUp(4);
		expect(tail.length).toBeGreaterThan(1);
		expect(tail.at(-1)).toEqual({ x: 60, y: 10, p: 0.2 });
		for (let i = 1; i < tail.length; i++)
			expect(Math.hypot(tail[i].x - tail[i - 1].x, tail[i].y - tail[i - 1].y)).toBeLessThan(4.01);
		expect(s.catchUp()).toEqual([]); // already there
	});

	it('carries on from a point (after a Shift+click line)', () => {
		const s = new Stabilizer(10);
		s.reset({ x: 100, y: 100, p: 0.7 });
		expect(s.push(105, 100, 0.7)).toBeNull();
		expect(s.catchUp(10)).toEqual([{ x: 105, y: 100, p: 0.7 }]);
	});

	it('maps the amount to a string length, gently at first', () => {
		expect(stringLength(0)).toBe(0);
		expect(stringLength(1)).toBe(80);
		expect(stringLength(2)).toBe(80);
		expect(stringLength(0.25)).toBeLessThan(80 * 0.25);
		expect(stringLength(0.5)).toBeGreaterThan(stringLength(0.25));
	});

	it('each painting tool keeps its own smoothing', () => {
		expect(smoothTool('brush', false)).toBe('brush');
		expect(smoothTool('brush', true)).toBe('eraser');
		expect(smoothTool('smudge', true)).toBe('smudge');
		expect(smoothTool('fill', false)).toBeNull();
	});
});
