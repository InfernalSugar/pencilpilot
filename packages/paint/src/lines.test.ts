import { describe, expect, it } from 'vitest';
import {
	STRIDE,
	LineStore,
	bakeClosure,
	bakePlan,
	boilStrokes,
	compose,
	cutStroke,
	decodeLineData,
	detach,
	ownStroke,
	pasteCopies,
	pasteOffset,
	pickable,
	spanOf,
	encodeLineData,
	invert,
	linkedCopies,
	mirrored,
	mirroredDelta,
	overLimit,
	replayGroups,
	smudgeArea,
	strokeTiles,
	strokesBytes,
	strokesOnKeys,
	twinMap,
	twins,
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
	it('keeps a stroke’s strands, checked', () => {
		const texture = {
			grain: null,
			grainScale: 1,
			grainStrength: 0.7,
			tip: null,
			dual: null,
			dualSize: 0.35,
			dualScatter: 0.8,
			dualCount: 3,
			strands: { count: 9, spread: 9, wobble: 0.35, taper: 0.7 }
		};
		const a = line(1, 2, 50, 3, { id: 'hair', texture });
		const b = line(1, 2, 50, 3, {
			id: 'odd',
			texture: { ...texture, strands: { count: 500, spread: -4, wobble: 9, taper: 0.5 } }
		});
		const c = line(1, 2, 50, 3, { id: 'one', texture: { ...texture, strands: null } });
		const [ra, rb, rc] = decodeLines(encodeLines([a, b, c]));
		expect(ra.texture).toEqual(texture);
		expect(rb.texture?.strands).toEqual({ count: 24, spread: 1, wobble: 1, taper: 0.5 });
		expect(rc.texture?.strands).toBeUndefined();
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

describe('linked mirror strokes', () => {
	const pts = (s: LineStroke) => [...s.pts].filter((_, i) => i % STRIDE < 2);

	it('links a stroke with its copies, and maps each onto the others', () => {
		const s = line(10, 20, 60, 40);
		for (const [mode, n] of [
			['vertical', 6],
			['horizontal', 6],
			['quadrant', 6],
			['radial', 5]
		] as const) {
			const all = linkedCopies(s, mode, n, 100, 80);
			expect(all).toHaveLength(symmetryStrokes(s, mode, n, 100, 80).length + 1);
			const g = all[0].link!.g;
			expect(all.every((x, k) => x.link?.g === g && x.link.k === k)).toBe(true);
			expect(all[0].id).toBe(s.id);
			// twin j is twin i mapped
			for (const a of all)
				for (const b of all) {
					const mapped = transformStroke(a, twinMap(a.link!, b.link!));
					pts(mapped).forEach((v, i) => expect(v).toBeCloseTo(pts(b)[i], 2));
				}
		}
		expect(twins(linkedCopies(s, 'quadrant', 6, 0, 0), s)).toHaveLength(1);
	});

	it('carries an edit to the twins mirrored', () => {
		const [a, b] = linkedCopies(line(10, 20, 60, 20), 'vertical', 6, 100, 0);
		// a moves right and down: its mirror twin moves left and down
		const m = mirrored([1, 0, 0, 1, 5, 7], a.link!, b.link!);
		const moved = transformStroke(b, m);
		expect(moved.pts[0]).toBeCloseTo(b.pts[0] - 5);
		expect(moved.pts[1]).toBeCloseTo(b.pts[1] + 7);
		expect(mirroredDelta(5, 7, a.link!, b.link!)).toEqual([-5, 7]);
		// a turn about a's middle turns b the other way about b's
		const r = rotateAbout(0.3, 35, 20);
		const turned = transformStroke(b, mirrored(r, a.link!, b.link!));
		const want = transformStroke(b, rotateAbout(-0.3, 165, 20));
		pts(turned).forEach((v, i) => expect(v).toBeCloseTo(pts(want)[i], 3));
		// radial: a move of copy 0 turns for copy 1
		const [c0, c1] = linkedCopies(line(110, 20, 160, 20), 'radial', 4, 100, 100);
		const [dx, dy] = mirroredDelta(10, 0, c0.link!, c1.link!);
		expect(dx).toBeCloseTo(0);
		expect(dy).toBeCloseTo(10);
		expect(compose(invert(r), r).map((v) => Math.round(v * 1e6) / 1e6)).toEqual([1, 0, 0, 1, 0, 0]);
	});

	it('keeps links in the saved bytes, and drops bad ones', () => {
		const all = linkedCopies(line(10, 20, 60, 40), 'radial', 6, 100, 80);
		const back = decodeLines(encodeLines(all));
		expect(back.map((s) => s.link)).toEqual(all.map((s) => s.link));
		const bad = { ...all[0], link: { ...all[0].link!, mode: 'spiral' } } as unknown as LineStroke;
		expect(decodeLines(encodeLines([bad]))[0].link).toBeUndefined();
	});
});

describe('editable strokes on plain layers', () => {
	// tiles are 64 px: a (0–1 across), b crosses a, c far away, d over b only
	const a = { ...line(10, 30, 120, 30), id: 'a' };
	const b = { ...line(100, 10, 100, 200), id: 'b' };
	const c = { ...line(400, 300, 500, 300), id: 'c' };
	const d = { ...line(90, 180, 110, 180), id: 'd' };
	const list = [a, b, c, d];

	it('bakes the strokes and every older one sharing a tile', () => {
		expect([...bakeClosure(list, [1])].sort()).toEqual([0, 1]);
		expect([...bakeClosure(list, [0])]).toEqual([0]);
		expect([...bakeClosure(list, [3])].sort()).toEqual([0, 1, 3]);
		expect([...bakeClosure(list, [2])]).toEqual([2]);
		expect(strokesOnKeys(list, new Set(['7,4']))).toEqual([2]);
	});

	it('plans where the baked go into the base and which tiles it forgets', () => {
		// a alone: its tiles where b stays are replayed, the rest forgotten
		const p = bakePlan(list, [0]);
		expect(p.baked.map((s) => s.id)).toEqual(['a']);
		expect(p.keep.map((s) => s.id)).toEqual(['b', 'c', 'd']);
		const pairs = (t: Int32Array) =>
			[...t].reduce<string[]>((o, v, i) => (i % 2 ? o : [...o, `${v},${t[i + 1]}`]), []);
		const replay = pairs(p.replay);
		const drop = pairs(p.drop);
		expect(replay.length).toBeGreaterThan(0);
		for (const t of replay)
			expect(strokeTiles(b).includes(t) || strokeTiles(d).includes(t)).toBe(true);
		for (const t of drop) expect(strokeTiles(b).includes(t)).toBe(false);
		expect(new Set([...replay, ...drop])).toEqual(new Set(strokeTiles(a)));
		// all of them: nothing to replay
		const all = bakePlan(list, [0, 1, 2, 3]);
		expect(all.keep).toEqual([]);
		expect(all.replay.length).toBe(0);
	});

	it('bakes the strokes over one it can’t replay instead', () => {
		const p = bakePlan(list, [0], (s) => s.id !== 'a');
		// (b shares a's tiles; d only b's, which no baked stroke needs replayed under it)
		expect(p.baked.map((s) => s.id)).toEqual(['a', 'b']);
		expect(p.keep.map((s) => s.id)).toEqual(['c', 'd']);
	});

	it('keeps the newest up to the limit, and counts their memory', () => {
		expect(overLimit(list, 2)).toEqual([0, 1]);
		expect(overLimit(list, 10)).toEqual([]);
		expect(overLimit(list, 0)).toEqual([0, 1, 2, 3]);
		expect(strokesBytes([a])).toBe(a.pts.byteLength + 240);
	});

	it('saves the pixels under them after the strokes; older readers see only strokes', () => {
		const base = new Uint8Array([1, 2, 3, 4, 5]);
		const bytes = encodeLineData(list, base);
		expect(decodeLines(bytes).map((s) => s.id)).toEqual(['a', 'b', 'c', 'd']);
		const back = decodeLineData(bytes);
		expect(back.strokes.map((s) => s.id)).toEqual(['a', 'b', 'c', 'd']);
		expect([...back.base!]).toEqual([1, 2, 3, 4, 5]);
		expect(decodeLineData(encodeLineData(list)).base).toBeNull();
		expect(decodeLineData(encodeLineData([], new Uint8Array())).base).toEqual(new Uint8Array());
		expect(() => decodeLineData(bytes.subarray(0, bytes.length - 2))).toThrow();
	});

	it('keeps which drawings lie over pixels in the store', () => {
		const st = new LineStore();
		st.setPlain(3);
		st.set(3, [a]);
		st.copy(3, 4);
		expect(st.isPlain(4)).toBe(true);
		st.keepOnly(new Set([4]));
		expect(st.isPlain(3)).toBe(false);
		st.setPlain(4, false);
		expect(st.isPlain(4)).toBe(false);
	});
});

describe('strokes that smudge, and snapped shapes with symmetry', () => {
	it('takes in all a smudging stroke reaches, and its twins', () => {
		const plain = { ...line(10, 10, 50, 10), id: 'p' };
		const smudge = { ...line(100, 100, 400, 100), id: 's', smudge: true, reach: 6 };
		const [a, b] = linkedCopies(
			{ ...line(600, 300, 640, 300), smudge: true },
			'vertical',
			6,
			800,
			0
		);
		const list = [plain, smudge, a, b];
		const keysOf = (t: Int32Array) => {
			const out = new Set<string>();
			for (let i = 0; i + 1 < t.length; i += 2) out.add(`${t[i]},${t[i + 1]}`);
			return out;
		};
		// a tile on the smudging stroke: all it reaches comes along
		const one = new Int32Array([3, 1]);
		const area = keysOf(smudgeArea(list, one, 1600, 800));
		for (const t of strokeTiles(smudge)) if (!t.startsWith('-')) expect(area.has(t)).toBe(true);
		expect(area.has('0,0')).toBe(false);
		// one twin's tile: the other twin comes too
		const twinArea = smudgeArea(list, new Int32Array([9, 4]), 1600, 800);
		expect(strokesOnTiles(list, twinArea).map((s) => s.id)).toEqual(
			expect.arrayContaining([a.id, b.id])
		);
		// nothing smudges: as it was
		expect(smudgeArea([plain], one, 1600, 800)).toBe(one);
		// a far reach is cut to the biggest canvas
		expect(strokeTiles({ ...smudge, reach: 1000 }).length).toBeLessThan(140 * 140);
	});

	it('numbers the strokes made together, to replay them together', () => {
		const [a, b] = linkedCopies(line(10, 10, 50, 10), 'vertical', 6, 100, 0);
		const [c, d] = linkedCopies(line(10, 40, 50, 40), 'vertical', 6, 100, 0);
		const p = line(0, 0, 9, 9);
		expect(replayGroups([p, a, b, c, d, p])).toEqual([0, 1, 1, 2, 2, 0]);
		expect(replayGroups([a, p, b])).toEqual([1, 0, 2]);
	});

	it('gives a snapped shape’s copies the shape, mirrored; reach and smudge are kept', () => {
		const shape = {
			kind: 'poly' as const,
			pts: [
				[10, 10],
				[30, 40],
				[50, 10]
			] as [number, number][],
			closed: false,
			pressure: 0.5
		};
		const [a, b] = linkedCopies({ ...line(10, 10, 50, 10), shape }, 'vertical', 6, 100, 0);
		expect(a.shape).toEqual(shape);
		expect(b.shape?.kind === 'poly' && b.shape.pts).toEqual([
			[190, 10],
			[170, 40],
			[150, 10]
		]);
		expect(decodeLines(encodeLines([{ ...a, reach: 4.5, smudge: true }]))[0]).toMatchObject({
			reach: 4.5,
			smudge: true
		});
	});
});

const link = (k: number) => ({ g: 'g', k, mode: 'vertical' as const, n: 2, cx: 100, cy: 100 });

describe('the eraser cutting strokes exactly', () => {
	const mark = (x: number, extra: Partial<LineStroke> = {}) =>
		line(x, 0, x, 100, { id: `e${x}`, erase: true, radius: 8, brush: 'rubber', ...extra });
	/** dabs as the engine lists them (straight strokes): every `step` px, trailing `lag` px */
	const dabsOf = (s: LineStroke, step = 1, lag = 0) => {
		const n = pointCount(s);
		const [ax, ay] = [s.pts[0], s.pts[1]];
		const [bx, by] = [s.pts[(n - 1) * STRIDE], s.pts[(n - 1) * STRIDE + 1]];
		const len = Math.hypot(bx - ax, by - ay);
		const out: number[] = [];
		for (let t = 0; t < len; t += step) {
			const f = Math.max(0, t - lag) / (len || 1);
			const i = Math.min(n - 1, Math.floor((t / (len || 1)) * (n - 1)) + 1);
			out.push(i, ax + (bx - ax) * f, ay + (by - ay) * f, s.radius);
		}
		return new Float32Array(out);
	};
	const cut = (s: LineStroke, marks: LineStroke[], core = 1, whole = false) =>
		cutStroke(s, marks, core, whole, { stroke: dabsOf(s), marks: marks.map((m) => dabsOf(m)) });

	it('cuts a stroke into pieces that draw its own dabs, each its part', () => {
		const s = line(0, 50, 200, 50);
		const n = pointCount(s);
		const all = dabsOf(s).length / 4;
		const pieces = cut(s, [mark(100)])!;
		expect(pieces).toHaveLength(2);
		const [a, b] = pieces;
		// the same points and seed: each piece draws its part of the whole stroke's dabs
		expect(a.pts).toBe(s.pts);
		expect(b.seed).toBe(s.seed);
		expect(a.id).toBe(s.id);
		expect(b.id).not.toBe(s.id);
		expect(a.dabs![0]).toBe(0);
		expect(b.dabs![1]).toBe(all);
		// what the eraser took isn't drawn: dabs within its reach (5 px each side)
		const gap = b.dabs![0] - a.dabs![1];
		expect(gap).toBeGreaterThan(8);
		expect(gap).toBeLessThan(12);
		// they show only their side: picking, boxes, handles, the path
		expect(spanOf(a)[0]).toBe(0);
		expect(spanOf(b)[1]).toBe(n);
		expect(hitTest(pieces, 100, 50, 1)).toBe(-1);
		expect(hitTest(pieces, 150, 50, 1)).toBe(1);
		expect(pathOf(b).every(([x]) => x > 100)).toBe(true);
		expect(pathOf(a).every(([x]) => x < 100)).toBe(true);
		expect(handles(b).every((i) => i >= spanOf(b)[0] && i < spanOf(b)[1])).toBe(true);
		// (their paint can reach as far as the whole stroke's)
		expect(bounds(a)).toEqual(bounds(s));
	});

	it('goes by where the dabs fall: a brush trailing behind the pen', () => {
		const s = line(0, 50, 200, 50);
		const marks = [mark(100)];
		const [a, b] = cutStroke(s, marks, 1, false, {
			stroke: dabsOf(s, 1, 6),
			marks: marks.map((m) => dabsOf(m))
		})!;
		const [c, d] = cut(s, marks)!;
		expect(a.dabs![1]).toBeGreaterThan(c.dabs![1]);
		expect(b.dabs![0]).toBeGreaterThan(d.dabs![0]);
		// no dabs listed: not cut (it stays whole under the eraser)
		expect(cutStroke(s, marks)).toBeNull();
	});

	it('keeps every dab between the pieces when asked (`whole`), split across the gap', () => {
		const s = line(0, 50, 200, 50);
		const [a, b] = cut(s, [mark(100)], 1, true)!;
		expect(a.dabs![0]).toBe(0);
		expect(a.dabs![1]).toBe(b.dabs![0]);
		expect(b.dabs![1]).toBe(dabsOf(s).length / 4);
	});

	it('leaves strokes it does not cut through, or does not reach', () => {
		// a thick stroke under a thin eraser: notched, not cut
		expect(cut(line(0, 50, 200, 50, { radius: 12 }), [mark(100)])).toBeNull();
		expect(cut(line(0, 50, 60, 50), [mark(150)])).toBeNull();
		// an eraser over the whole of it: nothing left
		expect(cut(line(97, 50, 103, 50), [mark(100)])).toEqual([]);
		// the end: one piece left
		const [one, more] = cut(line(0, 50, 100, 50), [mark(100)])!;
		expect(more).toBeUndefined();
		expect(one.dabs![0]).toBe(0);
	});

	it('cuts a piece again within its part', () => {
		const s = line(0, 50, 200, 50);
		const [, b] = cut(s, [mark(60)])!;
		const [c, d] = cut(b, [mark(140)])!;
		expect(c.dabs![0]).toBe(b.dabs![0]);
		expect(d.dabs![1]).toBe(b.dabs![1]);
		expect(c.dabs![1]).toBeLessThan(d.dabs![0]);
	});

	it('a piece reshaped or boiled becomes a stroke of its own', () => {
		const [, b] = cut(line(0, 50, 200, 50), [mark(100)])!;
		const [from, to] = spanOf(b);
		const own = ownStroke(b);
		expect(own.dabs).toBeUndefined();
		expect(own.span).toBeUndefined();
		expect(pointCount(own)).toBe(to - from);
		expect(own.pts[0]).toBe(b.pts[from * STRIDE]);
		const hs = handles(b);
		const moved = dragHandle(b, hs, 1, 0, 20);
		expect(moved.dabs).toBeUndefined();
		expect(pointCount(moved)).toBe(to - from);
		expect(Math.max(...pathOf(moved).map(([, y]) => y))).toBeCloseTo(70);
		const [boiled] = boilStrokes([b], 2, 1);
		expect(boiled.dabs).toBeUndefined();
		expect(pointCount(boiled)).toBe(to - from);
	});

	it('erase marks are never picked, boxed, erased whole or counted', () => {
		const s = line(0, 50, 200, 50);
		const m = mark(100);
		const list = [s, m];
		expect(hitTest(list, 100, 20, 2)).toBe(-1);
		expect(inBox(list, { x0: 90, y0: 0, x1: 110, y1: 30 })).toEqual([]);
		expect(eraseWhole(list, [100, 10], [100, 20], 3)).toEqual([]);
		expect(pickable(list)).toEqual([s]);
		expect(overLimit([s, m, line(0, 0, 5, 5)], 1)).toEqual([0]);
	});

	it('detaches moving strokes: the marks over them come along, and they move up past them', () => {
		const s = line(0, 50, 200, 50);
		const other = line(0, 300, 200, 300, { id: 'other' });
		const m = mark(100);
		const later = line(0, 70, 200, 70, { id: 'later' });
		const list = [s, other, m, later];
		const out = detach(list, new Set([s.id]));
		expect(out.map((x) => x.id)).toEqual(['other', m.id, s.id, 'later']);
		expect(out[2].clip).toEqual([m]);
		// moved by a transform, the clip goes with it
		const t = transformStroke(out[2], [1, 0, 0, 1, 30, 0]);
		expect(t.clip![0].pts[0]).toBeCloseTo(130);
		// only stepping over (not taken): no clip
		const kept = detach(list, new Set([s.id]), true);
		expect(kept[2].clip).toBeUndefined();
		// nothing over it: unchanged
		expect(detach(list, new Set(['other']))).toBe(list);
		// a stroke with its own erasers is replayed on its own
		const groups = replayGroups([
			{ ...s, link: link(0), clip: [m] },
			{ ...s, link: link(1) }
		]);
		expect(groups).toEqual([0, 1]);
	});

	it('saves and shares marks, pieces and clips', () => {
		const s = line(0, 50, 200, 50);
		const [a, b] = cut(s, [mark(100)])!;
		const moved = { ...b, clip: [mark(100), mark(120)] };
		const bytes = encodeLines([a, moved, mark(100)]);
		// (a new version: apps that don't know erase marks turn it down)
		expect([...bytes.slice(0, 4)]).toEqual([0x50, 0x50, 0x4c, 0x32]);
		expect([...encodeLines([s]).slice(0, 4)]).toEqual([0x50, 0x50, 0x4c, 0x31]);
		const back = decodeLines(bytes);
		expect(back[0].dabs).toEqual(a.dabs);
		expect(back[0].span).toEqual(a.span);
		expect(back[1].dabs).toEqual(b.dabs);
		expect(back[1].clip!.map((c) => c.id)).toEqual(['e100', 'e120']);
		expect(back[1].clip![1].pts).toEqual(mark(120).pts);
		expect(back[2].erase).toBe(true);
		expect(back[2].pts).toEqual(mark(100).pts);
		expect(strokesBytes([moved])).toBeGreaterThan(strokesBytes([b]));
		// and with a base after them
		const data = encodeLineData([moved, mark(1)], new Uint8Array([1, 2, 3]));
		const { strokes, base } = decodeLineData(data);
		expect(strokes[0].clip).toHaveLength(2);
		expect([...base!]).toEqual([1, 2, 3]);
	});
});

describe('pasting strokes', () => {
	it('gives copies new ids, keeping twins linked together', () => {
		const a = line(10, 10, 50, 10, { id: 'a', link: link(0) });
		const b = line(190, 10, 150, 10, { id: 'b', link: link(1) });
		const [x, y] = pasteCopies([a, b], 12, 12);
		expect(x.id).not.toBe('a');
		expect(x.link!.g).not.toBe('g');
		expect(x.link!.g).toBe(y.link!.g);
		expect(x.link!.cx).toBe(112);
		expect(x.pts[0]).toBeCloseTo(22);
		// another paste: another group
		const [z] = pasteCopies([a]);
		expect(z.link!.g).not.toBe(x.link!.g);
		expect(z.pts).toBe(a.pts);
	});

	it('goes where it was copied from, or further on when copies are there', () => {
		const a = line(10, 10, 50, 10, { id: 'a' });
		expect(pasteOffset([], [a])).toBe(0);
		expect(pasteOffset([a], [a])).toBe(12);
		const once = pasteCopies([a], 12, 12);
		expect(pasteOffset([a, ...once], [a])).toBe(24);
	});
});
