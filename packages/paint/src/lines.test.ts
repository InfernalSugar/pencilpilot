import { describe, expect, it } from 'vitest';
import {
	STRIDE,
	LineStore,
	boilStrokes,
	bounds,
	boxToBox,
	decodeLines,
	diffLines,
	dragHandle,
	encodeLines,
	eraseCut,
	eraseWhole,
	handles,
	hitTest,
	inBox,
	offsetScale,
	patchLines,
	pathOf,
	pointCount,
	rotateAbout,
	strokesOnTiles,
	symmetryStrokes,
	tilesOf,
	transformStroke,
	type LineStroke
} from './lines';

/** a straight stroke from (x0, y0) to (x1, y1), a point every 2 px */
function line(x0: number, y0: number, x1: number, y1: number, extra: Partial<LineStroke> = {}) {
	const n = Math.max(2, Math.ceil(Math.hypot(x1 - x0, y1 - y0) / 2) + 1);
	const pts = new Float32Array(n * STRIDE);
	for (let i = 0; i < n; i++) {
		const t = i / (n - 1);
		pts.set([x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, 0.5, 0.1, -0.1, 0.004], i * STRIDE);
	}
	return {
		id: `s${x0}-${y0}-${x1}-${y1}`,
		pts,
		brush: 'pen',
		color: '#112233',
		size: 1,
		opacity: 1,
		radius: 3,
		seed: 42,
		...extra
	} satisfies LineStroke;
}

describe('bounds and tiles', () => {
	it('covers the path and the brush reach', () => {
		const b = bounds(line(10, 20, 110, 20));
		expect(b.x0).toBeLessThan(10 - 3);
		expect(b.x1).toBeGreaterThan(110 + 3);
		expect(b.y0).toBeLessThan(17);
	});

	it('lists the tiles boxes cover, cut to the canvas', () => {
		const t = tilesOf([{ x0: -10, y0: 60, x1: 70, y1: 70 }], 200, 100);
		const pairs = [...t].reduce<number[][]>((a, v, i) => (i % 2 ? a : [...a, [v, t[i + 1]]]), []);
		expect(pairs.sort()).toEqual([
			[-1, 0],
			[-1, 1],
			[0, 0],
			[0, 1],
			[1, 0],
			[1, 1]
		]);
		expect(tilesOf([{ x0: 500, y0: 500, x1: 600, y1: 600 }], 200, 100).length).toBe(0);
	});

	it('finds the strokes that reach some tiles', () => {
		const a = line(5, 5, 30, 5);
		const b = line(150, 150, 190, 190);
		const t = tilesOf([{ x0: 0, y0: 0, x1: 10, y1: 10 }], 256, 256);
		expect(strokesOnTiles([a, b], t)).toEqual([a]);
	});
});

describe('hit tests', () => {
	const a = line(0, 50, 100, 50, { id: 'a' });
	const b = line(50, 0, 50, 100, { id: 'b' });
	it('picks the topmost stroke under a tap, with its width', () => {
		expect(hitTest([a, b], 20, 52, 0)).toBe(0);
		expect(hitTest([a, b], 50, 50, 0)).toBe(1); // b is on top where they cross
		expect(hitTest([a, b], 20, 60, 0)).toBe(-1);
		expect(hitTest([a, b], 20, 60, 8)).toBe(0); // a tolerance reaches it
	});
	it('a box picks the strokes with path inside it', () => {
		expect(inBox([a, b], { x0: 0, y0: 40, x1: 30, y1: 60 })).toEqual([0]);
		expect(inBox([a, b], { x0: 40, y0: 40, x1: 60, y1: 60 })).toEqual([0, 1]);
	});
});

describe('the eraser', () => {
	const a = line(0, 50, 100, 50, { id: 'a' });
	const b = line(0, 80, 100, 80, { id: 'b' });
	it('whole lines: everything it touches, along its path', () => {
		expect(eraseWhole([a, b], [50, 40], [50, 60], 2)).toEqual([0]);
		expect(eraseWhole([a, b], [50, 0], [50, 100], 2)).toEqual([0, 1]);
		expect(eraseWhole([a, b], [50, 60], [50, 70], 2)).toEqual([]);
	});
	it('normal: cuts a stroke in two where it passes', () => {
		const cut = eraseCut(a, [50, 50], [50, 50], 6)!;
		expect(cut).toHaveLength(2);
		const [l, r] = cut;
		expect(Math.max(...pathOf(l).map((p) => p[0]))).toBeLessThan(45);
		expect(Math.min(...pathOf(r).map((p) => p[0]))).toBeGreaterThan(55);
		expect(l.id).not.toBe(a.id);
		expect(l.id).not.toBe(r.id);
		// the rest keeps its look
		expect([l.color, l.brush, l.size]).toEqual([a.color, a.brush, a.size]);
		expect(pointCount(l) + pointCount(r)).toBeLessThan(pointCount(a));
	});
	it('normal: an end goes, an untouched stroke is left alone, all of it can go', () => {
		expect(eraseCut(a, [0, 50], [0, 50], 6)).toHaveLength(1);
		expect(eraseCut(a, [50, 90], [50, 90], 6)).toBeNull();
		expect(eraseCut(a, [0, 50], [100, 50], 6)).toEqual([]);
	});
});

describe('edits', () => {
	const a = line(10, 10, 50, 10, {
		id: 'a',
		shape: { kind: 'ellipse', c: [30, 10], a: 20, b: 10, theta: 0, circle: false, pressure: 0.5 }
	});
	it('moves, scales and turns strokes (and their brush, and their shape)', () => {
		const moved = transformStroke(a, [1, 0, 0, 1, 5, 7]);
		expect(pathOf(moved)[0]).toEqual([15, 17]);
		expect(moved.id).toBe(a.id);
		expect(moved).not.toBe(a);
		expect(a.pts[0]).toBe(10); // the original is untouched
		expect(moved.shape).toMatchObject({ c: [35, 17], a: 20, b: 10 });
		const big = transformStroke(a, [2, 0, 0, 2, 0, 0]);
		expect(big.size).toBe(2);
		expect(big.radius).toBe(6);
		expect(big.shape).toMatchObject({ a: 40, b: 20 });
		const turned = transformStroke(a, rotateAbout(Math.PI / 2, 30, 10));
		const [x, y] = pathOf(turned)[0];
		expect(x).toBeCloseTo(30);
		expect(y).toBeCloseTo(-10);
		expect(turned.size).toBeCloseTo(1);
		// pressure, tilt and timing ride along
		expect([...turned.pts.slice(2, 6)]).toEqual([...a.pts.slice(2, 6)]);
	});
	it('maps one box onto another', () => {
		const m = boxToBox({ x0: 0, y0: 0, x1: 10, y1: 10 }, { x0: 5, y0: 5, x1: 25, y1: 15 });
		const s = transformStroke(line(0, 0, 10, 10), m);
		expect(pathOf(s)[0]).toEqual([5, 5]);
		expect(pathOf(s).at(-1)).toEqual([25, 15]);
	});
	it('offsets and scales a whole drawing (a canvas resize)', () => {
		const [s] = offsetScale([a], 10, 20, 0.5);
		expect(pathOf(s)[0]).toEqual([10, 15]);
	});
	it('drags a handle: the point all the way, its neighbours less, beyond untouched', () => {
		// an L: a corner in the middle
		const pts = new Float32Array(41 * STRIDE);
		for (let i = 0; i <= 20; i++) pts.set([i * 2, 0, 0.5, 0, 0, 0.004], i * STRIDE);
		for (let i = 21; i <= 40; i++) pts.set([40, (i - 20) * 2, 0.5, 0, 0, 0.004], i * STRIDE);
		const L = { ...line(0, 0, 1, 1), pts };
		const hs = handles(L);
		expect(hs).toEqual([0, 20, 40]);
		const d = dragHandle(L, hs, 1, 10, -10);
		const p = pathOf(d);
		expect(p[20]).toEqual([50, -10]);
		expect(p[0]).toEqual([0, 0]);
		expect(p[40]).toEqual([40, 40]);
		expect(p[10][0]).toBeGreaterThan(20);
		expect(p[10][0]).toBeLessThan(30);
		expect(d.shape).toBeUndefined();
	});
	it('keeps handles few on long wiggly strokes', () => {
		const n = 2000;
		const pts = new Float32Array(n * STRIDE);
		for (let i = 0; i < n; i++) pts.set([i, Math.sin(i / 5) * 30, 0.5, 0, 0, 0.004], i * STRIDE);
		expect(handles({ ...line(0, 0, 1, 1), pts }).length).toBeLessThanOrEqual(24);
	});
});

describe('symmetry and boil', () => {
	it('mirrors a stroke with the seeds the engine paints its copies with', () => {
		const a = line(10, 10, 40, 30);
		const [m] = symmetryStrokes(a, 'vertical', 0, 50, 50);
		expect(pathOf(m)[0]).toEqual([90, 10]);
		expect(m.seed).toBe(43);
		expect(m.pts[3]).toBe(0); // no tilt on copies
		const quad = symmetryStrokes(a, 'quadrant', 0, 50, 50);
		expect(quad.map((s) => pathOf(s)[0])).toEqual([
			[90, 10],
			[10, 90],
			[90, 90]
		]);
		const radial = symmetryStrokes(a, 'radial', 4, 50, 50);
		expect(radial).toHaveLength(3);
		expect(pathOf(radial[1])[0][0]).toBeCloseTo(90);
		expect(pathOf(radial[1])[0][1]).toBeCloseTo(90);
	});
	it('boils strokes a little, the same for the same seed', () => {
		const a = line(0, 0, 200, 0);
		const [b1] = boilStrokes([a], 3, 7);
		const [b2] = boilStrokes([a], 3, 7);
		const [b3] = boilStrokes([a], 3, 8);
		expect([...b1.pts]).toEqual([...b2.pts]);
		expect([...b1.pts]).not.toEqual([...b3.pts]);
		for (const [x, y] of pathOf(b1)) {
			expect(Math.abs(y)).toBeLessThanOrEqual(3.001);
			expect(Number.isFinite(x)).toBe(true);
		}
		expect(b1.id).not.toBe(a.id);
	});
});

describe('saving and sharing', () => {
	it('round-trips strokes exactly, settings and shapes included', () => {
		const a = line(1.25, 2.5, 99.125, 3, {
			id: 'abc',
			texture: {
				grain: 'paper',
				grainScale: 2,
				grainStrength: 0.5,
				tip: null,
				dual: null,
				dualSize: 0.35,
				dualScatter: 0,
				dualCount: 1
			},
			shape: {
				kind: 'poly',
				pts: [
					[1, 2],
					[3, 4],
					[5, 6]
				],
				closed: true,
				pressure: 0.5,
				smooth: true
			}
		});
		const b = line(0, 0, 0, 0, { id: 'dot', color: '#ABCDEF', seed: 4294967295 });
		const back = decodeLines(encodeLines([a, b]));
		expect(back).toHaveLength(2);
		expect([...back[0].pts]).toEqual([...a.pts]);
		expect(back[0].texture).toEqual(a.texture);
		expect(back[0].shape).toEqual(a.shape);
		expect(back[1].color).toBe('#abcdef');
		expect(back[1].seed).toBe(4294967295);
		expect(decodeLines(encodeLines([]))).toEqual([]);
	});
	it('refuses what isn’t line data', () => {
		expect(() => decodeLines(new Uint8Array([1, 2, 3]))).toThrow();
		const bytes = encodeLines([line(0, 0, 10, 0)]);
		expect(() => decodeLines(bytes.subarray(0, bytes.length - 4))).toThrow();
		const bad = encodeLines([line(0, 0, 10, 0, { color: 'red' })]);
		expect(() => decodeLines(bad)).toThrow();
		const nan = encodeLines([line(0, 0, 10, 0)]);
		new DataView(nan.buffer).setFloat32(nan.length - 4, NaN, true);
		expect(() => decodeLines(nan)).toThrow();
	});
	it('diffs a drawing into deletions and placed strokes, and patches it back', () => {
		const [a, b, c] = [
			line(0, 0, 1, 1, { id: 'a' }),
			line(0, 0, 2, 2, { id: 'b' }),
			line(0, 0, 3, 3, { id: 'c' })
		];
		const before = [a, b, c];
		const b2 = transformStroke(b, [1, 0, 0, 1, 4, 4]);
		const d = line(9, 9, 9, 9, { id: 'd' });
		const after = [d, a, b2];
		const diff = diffLines(before, after);
		expect(diff.del.sort()).toEqual(['b', 'c']);
		expect(diff.put.map(([i, s]) => [i, s.id])).toEqual([
			[0, 'd'],
			[2, 'b']
		]);
		expect(patchLines(before, diff.del, diff.put)).toEqual(after);
		expect(diffLines(after, after)).toEqual({ del: [], put: [] });
	});
});

describe('the store', () => {
	it('keeps lists by drawing, copies them, and hears changes', () => {
		const st = new LineStore();
		const heard: number[] = [];
		st.onchange = (d) => heard.push(d);
		const a = line(0, 0, 1, 1);
		st.set(3, [a]);
		st.copy(3, 4);
		expect(st.get(4)).toEqual([a]);
		st.set(3, []);
		expect(st.has(3)).toBe(false);
		expect(st.get(3)).toEqual([]);
		st.keepOnly(new Set([9]));
		expect(st.drawings()).toEqual([]);
		expect(heard).toEqual([3, 4, 3]);
	});
});
