import { describe, expect, it } from 'vitest';
import { editable, moveHandle, shapeHandles, shapePath, type Pt } from './shape-edit';

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
