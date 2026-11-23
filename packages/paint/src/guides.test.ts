import { describe, expect, it } from 'vitest';
import {
	centredGuides,
	closestDirection,
	defaultGuides,
	gridLines,
	readGuides,
	project,
	rulerLine,
	snapDirections,
	vpLayout,
	withPerspective
} from './guides';

describe('guides', () => {
	it('perspective assist offers lines to the vanishing points (+ flat sides)', () => {
		const g = defaultGuides(800, 600);
		g.perspective.points = 1;
		g.perspective.vps[0] = [400, 200];
		const dirs = snapDirections(g, [100, 200]);
		// on the horizon the line to the VP is the flat side across, so it's there once
		expect(dirs).toHaveLength(2);
		expect(dirs[0][0]).toBeCloseTo(1); // straight to the VP (along the horizon)
		expect(snapDirections(g, [100, 300])).toHaveLength(3);
		// moving roughly towards the VP picks it; moving down picks vertical
		expect(closestDirection(dirs, [10, 1])).toEqual(dirs[0]);
		expect(closestDirection(dirs, [1, 10])).toEqual([0, 1]);
	});

	it('assist on a square grid keeps strokes across or down', () => {
		const g = defaultGuides(800, 600);
		expect(snapDirections(g, [100, 200])).toEqual([]); // no guide, nothing to snap to
		g.grid.on = true;
		const dirs = snapDirections(g, [100, 200]);
		expect(dirs).toEqual([
			[1, 0],
			[0, 1]
		]);
		expect(closestDirection(dirs, [10, -3])).toEqual([1, 0]);
		expect(closestDirection(dirs, [-2, -10])).toEqual([0, 1]);
		// a wobbly line across ends up level
		const u = closestDirection(dirs, [50, 4])!;
		expect(project([100, 200], u, [150, 204])).toEqual([150, 200]);
		// with one-point perspective too, across and down only count once
		g.perspective.points = 1;
		g.perspective.vps[0] = [400, 0];
		expect(snapDirections(g, [100, 200])).toHaveLength(3);
	});

	it('assist on an isometric grid snaps to its three axes', () => {
		const g = defaultGuides(800, 600);
		g.grid = { on: true, kind: 'isometric', size: 40 };
		const dirs = snapDirections(g, [0, 0]);
		expect(dirs).toHaveLength(3);
		const angle = (d: [number, number]) => {
			const u = closestDirection(dirs, d)!;
			return Math.round((Math.atan2(u[1], u[0]) * 180) / Math.PI);
		};
		expect(angle([1, 12])).toBe(90); // nearly upright
		expect(angle([10, -5])).toBe(-30); // up and right
		expect(angle([10, 7])).toBe(30); // down and right
		expect(angle([-10, 5])).toBe(-30); // down and left: the same axis as up and right
		// the axes match the grid's slanted lines
		const [x1, y1, x2, y2] = gridLines(g, 800, 600).find(([, a, , b]) => b < a)!;
		const s = closestDirection(dirs, [x2 - x1, y2 - y1])!;
		expect(Math.abs(s[0] * (y2 - y1) - s[1] * (x2 - x1))).toBeLessThan(1e-6);
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

describe('guides in the middle', () => {
	it('start centred: the mirror line, 1-point and 2-point evenly either side', () => {
		const g = defaultGuides(1000, 800);
		expect(g.symmetry.c).toEqual([500, 400]);
		expect(g.perspective.vps[0]).toEqual([500, 320]);
		const two = vpLayout(2, 1000, 800);
		expect(two[0][0] + two[1][0]).toBeCloseTo(1000);
		expect(two[0][1]).toBe(two[1][1]);
	});

	it('Centre guides puts every point back, keeping which guides are on', () => {
		const moved = {
			...defaultGuides(1000, 800),
			symmetry: { mode: 'vertical' as const, n: 6, c: [700, 100] as [number, number] },
			perspective: {
				points: 2 as const,
				vps: [
					[10, 10],
					[20, 20],
					[30, 30]
				] as [number, number][]
			}
		};
		const c = centredGuides(moved, 1000, 800);
		expect(c.symmetry).toEqual({ mode: 'vertical', n: 6, c: [500, 400] });
		expect(c.perspective.points).toBe(2);
		expect(c.perspective.vps).toEqual(vpLayout(2, 1000, 800));
	});

	it('switching perspective lays the points out again unless they were moved', () => {
		const g = defaultGuides(1000, 800);
		const two = withPerspective(withPerspective(g, 1, 1000, 800), 2, 1000, 800);
		expect(two.perspective.vps).toEqual(vpLayout(2, 1000, 800));
		const moved = {
			...two,
			perspective: {
				...two.perspective,
				vps: [
					[1, 2],
					[3, 4],
					[5, 6]
				] as [number, number][]
			}
		};
		expect(withPerspective(moved, 3, 1000, 800).perspective.vps).toEqual(moved.perspective.vps);
	});

	it('read back from a project only when whole', () => {
		const g = defaultGuides(100, 100);
		expect(readGuides(JSON.parse(JSON.stringify(g)))).toEqual(g);
		expect(readGuides({ ...g, symmetry: { mode: 'sideways', n: 2, c: [0, 0] } })).toBeNull();
		expect(readGuides({ ...g, ruler: { on: true, a: [0, 'x'], b: [1, 1] } })).toBeNull();
		expect(readGuides(null)).toBeNull();
	});
});
