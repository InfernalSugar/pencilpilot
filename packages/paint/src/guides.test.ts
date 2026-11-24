import { describe, expect, it } from 'vitest';
import {
	anyGuide,
	arcsPath,
	centredGuides,
	closestDirection,
	COMPOSITIONS,
	compositionFlips,
	compositionShapes,
	defaultGuides,
	edgePin,
	gridLines,
	PHI_CUT,
	readGuides,
	project,
	rulerLine,
	shiftOffset,
	shiftPoint,
	snapAlong,
	snapAngle,
	snapDirections,
	snapToGrid,
	vpLayout,
	withPerspective,
	type CompositionPreset
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

describe('3-point perspective', () => {
	it('starts with all three points on the canvas, so each can be seen and dragged', () => {
		const [w, h] = [1000, 800];
		for (const from of [0, 1, 2] as const) {
			const g = withPerspective(withPerspective(defaultGuides(w, h), from, w, h), 3, w, h);
			expect(g.perspective.points).toBe(3);
			for (const [x, y] of g.perspective.vps) {
				expect(x).toBeGreaterThan(0);
				expect(x).toBeLessThan(w);
				expect(y).toBeGreaterThan(0);
				expect(y).toBeLessThan(h);
				expect(edgePin([x, y], w, h)).toBeNull();
			}
		}
		// the third point below the other two, in the middle
		const v = vpLayout(3, w, h);
		expect(v[2][0]).toBe(w / 2);
		expect(v[2][1]).toBeGreaterThan(v[0][1]);
	});

	it('the old start below the canvas (kept by older projects) is laid out again', () => {
		const g = defaultGuides(1000, 800);
		g.perspective = {
			points: 0,
			vps: [
				[150, 320],
				[850, 320],
				[500, 1280]
			]
		};
		expect(withPerspective(g, 3, 1000, 800).perspective.vps).toEqual(vpLayout(3, 1000, 800));
	});

	it('a point moved off the canvas gets a handle pinned to its edge, pointing at it', () => {
		const pin = edgePin([500, 1600], 1000, 800, 10)!;
		expect(pin.at[0]).toBeCloseTo(500);
		expect(pin.at[1]).toBeCloseTo(790);
		expect(pin.angle).toBeCloseTo(Math.PI / 2);
		const side = edgePin([3000, 400], 1000, 800)!;
		expect(side.at).toEqual([1000, 400]);
		expect(side.angle).toBeCloseTo(0);
		// up and to the left: on the top edge, along the line from the middle
		const corner = edgePin([-500, -1000], 1000, 800)!;
		expect(corner.at[1]).toBeCloseTo(0);
		expect((corner.at[0] - 500) / (corner.at[1] - 400)).toBeCloseTo(1000 / 1400);
	});
});

describe('composition overlays', () => {
	const W = 1200;
	const H = 800;
	const on = (preset: CompositionPreset, flipX = false, flipY = false) =>
		compositionShapes({ preset, flipX, flipY }, W, H);
	const inside = ([x1, y1, x2, y2]: number[]) =>
		[x1, x2].every((x) => x >= -1e-6 && x <= W + 1e-6) &&
		[y1, y2].every((y) => y >= -1e-6 && y <= H + 1e-6);

	it('every preset fits the canvas; off draws nothing', () => {
		expect(on('off')).toEqual({ lines: [], arcs: [], ellipses: [] });
		for (const p of COMPOSITIONS.slice(1)) {
			const s = on(p);
			expect(s.lines.length, p).toBeGreaterThan(0);
			expect(s.lines.every(inside), p).toBe(true);
		}
	});

	it('thirds and the phi grid', () => {
		const thirds = on('thirds').lines.map((l) => l.map(Math.round));
		expect(thirds).toEqual([
			[400, 0, 400, 800],
			[800, 0, 800, 800],
			[0, 267, 1200, 267],
			[0, 533, 1200, 533]
		]);
		const phi = on('phi').lines;
		expect(phi).toHaveLength(4);
		expect(phi[0][0]).toBeCloseTo(1200 * 0.382, 0);
		expect(phi[1][0]).toBeCloseTo(1200 * 0.618, 0);
		expect(phi[1][0] / phi[0][0]).toBeCloseTo(PHI_CUT / (1 - PHI_CUT)); // φ
	});

	it('centre lines with diagonals, the diagonal method, safe areas', () => {
		expect(on('cross').lines).toContainEqual([600, 0, 600, 800]);
		expect(on('cross').lines).toContainEqual([0, 0, 1200, 800]);
		// 45° from each corner
		for (const [x1, y1, x2, y2] of on('diagonals').lines)
			expect(Math.abs(x2 - x1)).toBeCloseTo(Math.abs(y2 - y1));
		expect(on('diagonals').lines).toHaveLength(4);
		const safe = on('safe').lines;
		expect(safe).toHaveLength(8);
		expect(safe[0]).toEqual([60, 40, 1140, 40]); // 90%
		expect(safe[4]).toEqual([120, 80, 1080, 80]); // 80%
	});

	it('the armature of the rectangle has its 14 lines', () => {
		const a = on('armature').lines;
		expect(a).toHaveLength(14);
		expect(new Set(a.map((l) => l.join())).size).toBe(14);
	});

	it('golden triangles: a diagonal and two lines meeting it square on, flippable', () => {
		const [d, l1, l2] = on('triangles').lines;
		expect(d).toEqual([0, 0, 1200, 800]);
		for (const [x1, y1, x2, y2] of [l1, l2])
			expect((x2 - x1) * 1200 + (y2 - y1) * 800).toBeCloseTo(0);
		expect(on('triangles', true).lines[0]).toEqual([1200, 0, 0, 800]);
		expect(compositionFlips('triangles')).toBe('x');
		expect(compositionFlips('spiral')).toBe('xy');
		expect(compositionFlips('thirds')).toBe('none');
	});

	it('the golden spiral: joined quarter turns, winding in, in four ways round', () => {
		const { arcs, lines } = on('spiral');
		expect(arcs.length).toBeGreaterThan(5);
		expect(lines).toHaveLength(arcs.length);
		expect(arcs[0].from).toEqual([0, 800]);
		expect(arcs[0].to[0]).toBeCloseTo(1200 * PHI_CUT);
		for (let i = 1; i < arcs.length; i++) {
			expect(arcs[i].from[0]).toBeCloseTo(arcs[i - 1].to[0]);
			expect(arcs[i].from[1]).toBeCloseTo(arcs[i - 1].to[1]);
			expect(arcs[i].rx * arcs[i].ry).toBeLessThan(arcs[i - 1].rx * arcs[i - 1].ry);
		}
		expect(arcsPath(arcs)).toMatch(/^M0 800 A/);
		// where it winds to (its last point), for each way round: four different places
		const ways = [
			[false, false],
			[true, false],
			[false, true],
			[true, true]
		] as const;
		const ends = ways.map(([fx, fy]) => {
			const s = on('spiral', fx, fy);
			expect(s.arcs.every((a) => a.sweep === (fx !== fy ? 0 : 1))).toBe(true);
			const e = s.arcs.at(-1)!.to;
			return [Math.sign(e[0] - W / 2), Math.sign(e[1] - H / 2)].join();
		});
		expect(new Set(ends).size).toBe(4);
		// a tall canvas starts with a cut across the top
		const tall = compositionShapes({ preset: 'spiral', flipX: false, flipY: false }, 600, 1000);
		expect(tall.arcs[0].from).toEqual([0, 0]);
	});

	it('kept with the guides: older projects without it read back as off', () => {
		const g = defaultGuides(100, 100);
		g.composition = { preset: 'spiral', flipX: true, flipY: false, offset: [0, 0], scale: 1 };
		expect(readGuides(JSON.parse(JSON.stringify(g)))).toEqual(g);
		const old: Record<string, unknown> = { ...g };
		delete old.composition;
		expect(readGuides(old)?.composition).toEqual({
			preset: 'off',
			flipX: false,
			flipY: false,
			offset: [0, 0],
			scale: 1
		});
		expect(readGuides({ ...g, composition: { preset: 'nope' } })?.composition.preset).toBe('off');
		expect(anyGuide(defaultGuides(100, 100))).toBe(false);
		expect(anyGuide(g)).toBe(true);
	});
});

describe('Shift: snapping while dragging', () => {
	const grid = (kind: 'square' | 'isometric', size = 40) => {
		const g = defaultGuides(800, 600);
		g.grid = { on: true, kind, size };
		return g;
	};
	const close = (p: [number, number], q: [number, number]) => {
		expect(p[0]).toBeCloseTo(q[0], 6);
		expect(p[1]).toBeCloseTo(q[1], 6);
	};

	it('a square grid: the nearest crossing', () => {
		const g = grid('square');
		expect(snapToGrid(g, [57, 21])).toEqual([40, 40]);
		expect(snapToGrid(g, [61, 19])).toEqual([80, 0]);
		expect(snapToGrid(g, [-15, 99])).toEqual([-0, 80]);
		// the grid off: the point stays put
		g.grid.on = false;
		expect(snapToGrid(g, [57, 21])).toEqual([57, 21]);
		expect(snapToGrid(null, [57, 21])).toEqual([57, 21]);
	});

	it('an isometric grid: the nearest crossing, on all three of its lines', () => {
		const g = grid('isometric');
		const d = 40 / Math.cos(Math.PI / 6);
		close(snapToGrid(g, [3, d - 2]), [0, d]);
		// the next column is half a step down
		close(snapToGrid(g, [38, d / 2 + 3]), [40, d / 2]);
		// every crossing is on an upright line and on both slanted ones that gridLines draws
		const lines = gridLines(g, 800, 600);
		const on = (p: [number, number]) =>
			lines.filter(([x1, y1, x2, y2]) => {
				const cross = (x2 - x1) * (p[1] - y1) - (y2 - y1) * (p[0] - x1);
				return Math.abs(cross) / Math.hypot(x2 - x1, y2 - y1) < 1e-6;
			}).length;
		for (const q of [
			[200, 300],
			[333, 123],
			[517, 401]
		] as [number, number][])
			expect(on(snapToGrid(g, q))).toBe(3);
	});

	it('along a line: the nearest crossing on it, else just on the line', () => {
		const g = grid('square');
		close(snapAlong(g, [40, 40], [1, 0], [97, 52]), [80, 40]);
		const D = Math.SQRT1_2;
		close(snapAlong(g, [0, 0], [D, D], [70, 95]), [80, 80]);
		// the line misses every crossing: on the line, wherever the pointer is
		close(snapAlong(g, [0, 10], [1, 0], [97, 52]), [97, 10]);
		// no grid: just held to the line
		close(snapAlong(null, [0, 0], [1, 0], [97, 52]), [97, 0]);
		// isometric: across, the crossings come every two columns
		close(snapAlong(grid('isometric'), [0, 0], [1, 0], [70, 5]), [80, 0]);
	});

	it('angles turn to the nearest step, as far out as before', () => {
		const p = snapAngle([10, 10], [20, 11], 15);
		close(p, [10 + Math.hypot(10, 1), 10]);
		const q = snapAngle([0, 0], [10, 9], 45);
		close(q, [Math.hypot(10, 9) * Math.SQRT1_2, Math.hypot(10, 9) * Math.SQRT1_2]);
		const r = snapAngle([0, 0], [10, 2], 15);
		expect((Math.atan2(r[1], r[0]) * 180) / Math.PI).toBeCloseTo(15);
		expect(snapAngle([5, 5], [5, 5], 15)).toEqual([5, 5]);
	});

	it('a Shift move goes across, down or at 45°; whole cells with the grid on', () => {
		close(shiftOffset(null, [50, 8]), [50, 0]);
		close(shiftOffset(null, [-6, 70]), [0, 70]);
		close(shiftOffset(null, [30, -34]), [32, -32]);
		const g = grid('square');
		close(shiftOffset(g, [50, 8]), [40, 0]);
		close(shiftOffset(g, [65, 61]), [80, 80]);
	});

	it('a Shift-dragged point: on the grid when it is on, else held to an axis from its start', () => {
		close(shiftPoint(null, [100, 100], [160, 107]), [160, 100]);
		close(shiftPoint(null, [100, 100], [96, 40]), [100, 40]);
		close(shiftPoint(grid('square'), [100, 100], [163, 107]), [160, 120]);
	});
});
