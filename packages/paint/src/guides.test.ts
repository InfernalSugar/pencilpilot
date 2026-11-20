import { describe, expect, it } from 'vitest';
import {
	closestDirection,
	defaultGuides,
	gridLines,
	project,
	rulerLine,
	snapDirections
} from './guides';

describe('guides', () => {
	it('perspective assist offers lines to the vanishing points (+ flat sides)', () => {
		const g = defaultGuides(800, 600);
		g.perspective.points = 1;
		g.perspective.vps[0] = [400, 200];
		const dirs = snapDirections(g, [100, 200]);
		expect(dirs).toHaveLength(3);
		expect(dirs[0][0]).toBeCloseTo(1); // straight to the VP (along the horizon)
		// moving roughly towards the VP picks it; moving down picks vertical
		expect(closestDirection(dirs, [10, 1])).toEqual(dirs[0]);
		expect(closestDirection(dirs, [1, 10])).toEqual([0, 1]);
		g.perspective.assist = false;
		expect(snapDirections(g, [100, 200])).toEqual([]);
	});

	it('projects onto a direction and finds the ruler within reach', () => {
		expect(project([0, 0], [1, 0], [5, 3])).toEqual([5, 0]);
		const g = defaultGuides(800, 600);
		g.ruler = { on: true, a: [100, 300], b: [700, 300] };
		const r = rulerLine(g, [400, 310], 20)!;
		expect(r.origin).toEqual([400, 300]);
		expect(r.u).toEqual([1, 0]);
		expect(rulerLine(g, [400, 360], 20)).toBeNull();
		expect(rulerLine(g, [760, 300], 20)).toBeNull();
	});

	it('draws grids', () => {
		const g = defaultGuides(800, 600);
		g.grid.size = 100;
		expect(gridLines(g, 800, 600)).toHaveLength(7 + 5);
		g.grid.kind = 'isometric';
		expect(gridLines(g, 800, 600).length).toBeGreaterThan(20);
	});
});
