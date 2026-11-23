import { describe, expect, it } from 'vitest';
import {
	curvable,
	curveThrough,
	editable,
	moveHandle,
	shapeHandles,
	shapePath,
	type Pt
} from './shape-edit';

const ring = (cx: number, cy: number, a: number, b: number): Pt[] =>
	Array.from({ length: 73 }, (_, i) => {
		const t = (i / 72) * Math.PI * 2;
		return [cx + a * Math.cos(t), cy + b * Math.sin(t)];
	});

describe('editable shapes', () => {
	it('a rectangle keeps its four corners and closes again', () => {
		const pts: Pt[] = [
			[0, 0],
			[10, 0],
			[10, 5],
			[0, 5],
			[0, 0]
		];
		const s = editable({ kind: 'rectangle', points: pts }, 0.5);
		expect(shapeHandles(s)).toHaveLength(4);
		const moved = moveHandle(s, 2, [20, 15]);
		expect(shapePath(moved)).toEqual([
			[0, 0],
			[10, 0],
			[20, 15],
			[0, 5],
			[0, 0]
		]);
	});

	it('a line has two ends', () => {
		const s = editable(
			{
				kind: 'line',
				points: [
					[0, 0],
					[10, 10]
				]
			},
			0.4
		);
		expect(shapePath(moveHandle(s, 0, [5, 0]))).toEqual([
			[5, 0],
			[10, 10]
		]);
	});

	it('an ellipse is rebuilt from its centre and axes', () => {
		const s = editable({ kind: 'ellipse', points: ring(50, 40, 30, 10) }, 0.5);
		expect(s.kind).toBe('ellipse');
		if (s.kind !== 'ellipse') return;
		expect(s.c[0]).toBeCloseTo(50, 0);
		expect(s.a).toBeCloseTo(30, 0);
		expect(s.b).toBeCloseTo(10, 0);
		// drag the second axis out: taller
		const t = moveHandle(s, 2, [50, 60]);
		expect(t.kind === 'ellipse' && t.b).toBeCloseTo(20, 0);
		// move the centre: everything moves with it
		const m = moveHandle(s, 0, [60, 40]);
		expect(shapePath(m)[0][0]).toBeCloseTo(90, 0);
	});

	it('a circle stays round', () => {
		const s = editable({ kind: 'circle', points: ring(0, 0, 20, 20) }, 0.5);
		const t = moveHandle(s, 1, [40, 0]);
		expect(t.kind === 'ellipse' && t.a === t.b).toBe(true);
	});
});

describe('perfect shapes', () => {
	it('make an ellipse a circle, a rectangle a square, a line a 15° step', async () => {
		const { perfect, shapeName } = await import('./shape-edit');
		const oval = perfect({
			kind: 'ellipse',
			c: [50, 50],
			a: 40,
			b: 20,
			theta: 0.3,
			circle: false,
			pressure: 0.5
		});
		expect(oval).toMatchObject({ c: [50, 50], a: 30, b: 30, circle: true });
		expect(shapeName(oval)).toBe('circle');
		const rect = perfect({
			kind: 'poly',
			pts: [
				[0, 0],
				[40, 0],
				[40, 20],
				[0, 20]
			],
			closed: true,
			pressure: 0.5
		});
		expect(shapeName(rect)).toBe('square');
		if (rect.kind !== 'poly') throw new Error('poly');
		const c = rect.pts.reduce((t, p) => [t[0] + p[0] / 4, t[1] + p[1] / 4], [0, 0]);
		expect(c[0]).toBeCloseTo(20);
		expect(c[1]).toBeCloseTo(10);
		const line = perfect({
			kind: 'poly',
			pts: [
				[0, 0],
				[100, 9]
			],
			closed: false,
			pressure: 0.5
		});
		if (line.kind !== 'poly') throw new Error('poly');
		// 5.1° → 0°, the same length
		expect(line.pts[1][1]).toBeCloseTo(0);
		expect(line.pts[1][0]).toBeCloseTo(Math.hypot(100, 9));
		const tri = perfect({
			kind: 'poly',
			pts: [
				[0, 0],
				[50, 5],
				[10, 40]
			],
			closed: true,
			pressure: 0.5
		});
		if (tri.kind !== 'poly') throw new Error('poly');
		const sides = tri.pts.map((p, i) =>
			Math.hypot(p[0] - tri.pts[(i + 1) % 3][0], p[1] - tri.pts[(i + 1) % 3][1])
		);
		expect(Math.max(...sides) - Math.min(...sides)).toBeLessThan(1e-6);
	});
});

describe('curved shapes', () => {
	const v: Pt[] = [
		[0, 0],
		[50, 100],
		[100, 0]
	];
	it('go through every corner, from the first to the last', () => {
		const c = curveThrough(v, false);
		expect(c[0]).toEqual([0, 0]);
		expect(c.at(-1)).toEqual([100, 0]);
		const near = (p: Pt) => Math.min(...c.map((q) => Math.hypot(q[0] - p[0], q[1] - p[1])));
		expect(near([50, 100])).toBeLessThan(0.01);
	});

	it('bend smoothly instead of turning at a corner', () => {
		const c = curveThrough(v, false, 32);
		// halfway along the first line the straight version is at (25, 50); the curve bulges out
		const mid = c[16];
		expect(Math.hypot(mid[0] - 25, mid[1] - 50)).toBeGreaterThan(2);
		// no sharp turn anywhere: neighbouring directions differ by less than 30°
		for (let i = 1; i < c.length - 1; i++) {
			const a = Math.atan2(c[i][1] - c[i - 1][1], c[i][0] - c[i - 1][0]);
			const b = Math.atan2(c[i + 1][1] - c[i][1], c[i + 1][0] - c[i][0]);
			const turn = Math.abs(Math.atan2(Math.sin(b - a), Math.cos(b - a)));
			expect(turn).toBeLessThan(Math.PI / 6);
		}
	});

	it('close back round for closed shapes, and only bend shapes with corners', () => {
		const sq: Pt[] = [
			[0, 0],
			[10, 0],
			[10, 10],
			[0, 10]
		];
		const c = shapePath({ kind: 'poly', pts: sq, closed: true, pressure: 1, smooth: true });
		expect(c[0]).toEqual([0, 0]);
		expect(c.at(-1)).toEqual([0, 0]);
		expect(c.length).toBeGreaterThan(20);
		expect(curvable({ kind: 'poly', pts: sq, closed: true, pressure: 1 })).toBe(true);
		const line: Pt[] = [
			[0, 0],
			[5, 5]
		];
		expect(curvable({ kind: 'poly', pts: line, closed: false, pressure: 1 })).toBe(false);
		expect(
			shapePath({ kind: 'poly', pts: line, closed: false, pressure: 1, smooth: true })
		).toEqual(line);
	});
});
