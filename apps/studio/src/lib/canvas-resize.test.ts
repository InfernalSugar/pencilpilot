import { describe, expect, it, vi } from 'vitest';
import { defaultGuides } from '@pencilpilot/paint/guides';
import {
	ANCHORS,
	ENGINE_MOST,
	TILE_BYTES,
	changes,
	checkResize,
	fromPx,
	keepAspect,
	moveCameras,
	moveGuides,
	movePoint,
	moveText,
	planCrop,
	planResize,
	toPx
} from './canvas-resize';
import { FrameCache, joinPacket } from './frames';
import { newTimeline } from './timeline';

const HD = { width: 1920, height: 1080 };

describe('where the drawing goes on the new canvas', () => {
	it('the anchor says which part stays put', () => {
		const to = { width: 2120, height: 1280 };
		expect(planResize(HD, to, { scale: false, anchor: [0, 0] })).toMatchObject({ dx: 0, dy: 0 });
		expect(planResize(HD, to, { scale: false, anchor: [0.5, 0.5] })).toMatchObject({
			dx: 100,
			dy: 100
		});
		expect(planResize(HD, to, { scale: false, anchor: [1, 1] })).toMatchObject({
			dx: 200,
			dy: 200
		});
		// smaller: a crop, the drawing moves up and left
		const crop = planResize(HD, { width: 1720, height: 880 }, { scale: false, anchor: [0.5, 1] });
		expect(crop).toMatchObject({ dx: -100, dy: -200 });
		expect(movePoint(crop, [960, 540])).toEqual([860, 340]);
	});

	it('odd differences round to whole pixels', () => {
		const p = planResize(HD, { width: 1921, height: 1083 }, { scale: false, anchor: [0.5, 0.5] });
		expect(Number.isInteger(p.dx) && Number.isInteger(p.dy)).toBe(true);
	});

	it('scaling ignores the anchor and stretches points', () => {
		const p = planResize(HD, { width: 960, height: 540 }, { scale: true, anchor: [1, 1] });
		expect(p).toMatchObject({ dx: 0, dy: 0, scale: true });
		expect(movePoint(p, [1000, 500])).toEqual([500, 250]);
	});

	it('nine anchors, row by row', () => {
		expect(ANCHORS).toHaveLength(9);
		expect(ANCHORS[0]).toEqual([0, 0]);
		expect(ANCHORS[4]).toEqual([0.5, 0.5]);
		expect(ANCHORS[8]).toEqual([1, 1]);
	});

	it('a crop to a box moves the box to the corner', () => {
		const p = planCrop(HD, { x: 100, y: 50, width: 800, height: 600 });
		expect(p).toMatchObject({ to: { width: 800, height: 600 }, dx: -100, dy: -50 });
		expect(movePoint(p, [100, 50])).toEqual([0, 0]);
	});

	it('the same size, in place, changes nothing', () => {
		expect(changes(planResize(HD, HD, { scale: false, anchor: [0.5, 0.5] }))).toBe(false);
		expect(changes(planResize(HD, HD, { scale: true, anchor: [0.5, 0.5] }))).toBe(false);
	});
});

describe('what the new size may be', () => {
	const go = (w: number, h: number, tiles = 0, limit = Infinity) =>
		checkResize(
			planResize(HD, { width: w, height: h }, { scale: false, anchor: [0, 0] }),
			tiles,
			limit
		);

	it('follows the canvas limits (whole pixels, 256 – 8K, not too thin)', () => {
		expect(go(3840, 2160).ok).toBe(true);
		expect(go(100, 1080)).toMatchObject({ ok: false });
		expect(go(9000, 1080)).toMatchObject({ ok: false });
		expect(go(8192, 8192)).toMatchObject({ ok: false });
		expect(go(4000, 1000)).toMatchObject({ ok: false });
		expect(go(1920.5, 1080)).toMatchObject({ ok: false });
		expect(go(1920, 1080)).toMatchObject({ ok: false, why: 'That’s the size it is.' });
	});

	it('warns past the memory limit, and stops where the engine can’t hold it', () => {
		const MB = 2 ** 20;
		const tiles = (300 * MB) / TILE_BYTES;
		expect(go(2000, 1080, tiles, 1024 * MB)).toEqual({ ok: true, note: null });
		const r = go(2000, 1080, tiles, 512 * MB);
		expect(r.ok && r.note).toMatch(/frame memory limit/);
		expect(go(2000, 1080, ENGINE_MOST / TILE_BYTES).ok).toBe(false);
	});

	it('scaling up counts the bigger paint', () => {
		const tiles = (400 * 2 ** 20) / TILE_BYTES;
		const up = planResize(HD, { width: 3840, height: 2160 }, { scale: true, anchor: [0, 0] });
		expect(checkResize(up, tiles, Infinity).ok).toBe(false);
		const keep = planResize(HD, { width: 3840, height: 2160 }, { scale: false, anchor: [0, 0] });
		expect(checkResize(keep, tiles, Infinity).ok).toBe(true);
	});
});

describe('units and the aspect ratio', () => {
	it('cm and inches at the pixels per inch', () => {
		expect(toPx(10, 'in', 300)).toBe(3000);
		expect(toPx(2.54, 'cm', 300)).toBe(300);
		expect(toPx(1919.6, 'px', 300)).toBe(1920);
		expect(fromPx(3000, 'in', 300)).toBe(10);
		expect(fromPx(300, 'cm', 300)).toBe(2.54);
		expect(fromPx(1920, 'px', 72)).toBe(1920);
	});

	it('a locked ratio sets the other side', () => {
		expect(keepAspect(HD, 'width', 3840)).toEqual({ width: 3840, height: 2160 });
		expect(keepAspect(HD, 'height', 540)).toEqual({ width: 960, height: 540 });
	});
});

describe('everything else kept in canvas pixels', () => {
	it('every cut’s camera frames the same part of the drawing', () => {
		const tl = newTimeline([{ id: 1, drawing: 1 }]);
		tl.scenes[0].cuts.push({
			id: 2,
			name: 'Cut 2',
			tracks: {},
			camera: { x: 160, y: 90, width: 1600, height: 900 }
		});
		tl.scenes.push({
			id: 2,
			name: 'Scene 2',
			cuts: [{ id: 3, name: 'Cut 1', tracks: {}, camera: { x: 0, y: 0, width: 960, height: 540 } }]
		});
		const more = planResize(
			HD,
			{ width: 2120, height: 1280 },
			{ scale: false, anchor: [0.5, 0.5] }
		);
		const moved = moveCameras(tl, more);
		expect(moved.scenes[0].cuts[0].camera).toBeUndefined();
		expect(moved.scenes[0].cuts[1].camera).toEqual({ x: 260, y: 190, width: 1600, height: 900 });
		expect(moved.scenes[1].cuts[0].camera).toEqual({ x: 100, y: 100, width: 960, height: 540 });
		// the timeline itself isn't changed (undo keeps the one before)
		expect(tl.scenes[0].cuts[1].camera!.x).toBe(160);
		const half = planResize(HD, { width: 960, height: 540 }, { scale: true, anchor: [0, 0] });
		expect(moveCameras(tl, half).scenes[0].cuts[1].camera).toEqual({
			x: 80,
			y: 45,
			width: 800,
			height: 450
		});
		// cropped past a camera: it's pulled back onto the canvas
		const crop = planCrop(HD, { x: 1000, y: 500, width: 600, height: 400 });
		const c = moveCameras(tl, crop).scenes[0].cuts[1].camera!;
		expect(c.x >= 0 && c.y >= 0 && c.x + c.width <= 600 && c.y + c.height <= 400).toBe(true);
	});

	it('a text box moves (and resizes when scaling), keeping the rest', () => {
		const spec = JSON.stringify({
			text: 'Hi',
			size: 64,
			x: 100,
			y: 200,
			width: 400,
			font: 'serif'
		});
		const shift = planResize(
			HD,
			{ width: 2120, height: 1280 },
			{ scale: false, anchor: [0.5, 0.5] }
		);
		expect(JSON.parse(moveText(spec, shift)!)).toEqual({
			text: 'Hi',
			size: 64,
			x: 200,
			y: 300,
			width: 400,
			font: 'serif'
		});
		const half = planResize(HD, { width: 960, height: 540 }, { scale: true, anchor: [0, 0] });
		expect(JSON.parse(moveText(spec, half)!)).toMatchObject({
			x: 50,
			y: 100,
			size: 32,
			width: 200
		});
		expect(moveText('not json', half)).toBeNull();
	});

	it('the guides stay on the drawing', () => {
		const g = defaultGuides(1920, 1080);
		const more = planResize(
			HD,
			{ width: 2120, height: 1280 },
			{ scale: false, anchor: [0.5, 0.5] }
		);
		const m = moveGuides(g, more);
		expect(m.symmetry.c).toEqual([1060, 640]);
		expect(m.ruler.a).toEqual([g.ruler.a[0] + 100, g.ruler.a[1] + 100]);
		expect(m.grid.size).toBe(g.grid.size);
		const half = planResize(HD, { width: 960, height: 540 }, { scale: true, anchor: [0, 0] });
		expect(moveGuides(g, half).grid.size).toBe(Math.round(g.grid.size / 2));
	});
});

// ---- the frame store's side: every drawing there at once, then back to the store ----

const store = new Map<number, Uint8Array>();
const calls: string[] = [];
vi.mock('@tauri-apps/api/core', () => ({
	invoke: async (cmd: string, args: unknown) => {
		calls.push(cmd);
		if (cmd === 'frames_get') {
			const ids = (args as { ids: number[] }).ids.filter((id) => store.has(id));
			return joinPacket(ids.map((id) => ({ id, packed: store.get(id)! }))).buffer;
		}
		if (cmd === 'frames_put') {
			const b = args as Uint8Array;
			const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
			let at = 4;
			for (let i = 0, n = v.getUint32(0, true); i < n; i++) {
				const id = v.getUint32(at, true);
				store.set(id, b.slice(at + 4, at + 8));
				at += 8;
			}
		}
		if (cmd === 'frames_swap') {
			const ids = (args as { pairs: number[] }).pairs;
			for (let i = 0; i + 1 < ids.length; i += 2) {
				const [a, b] = [store.get(ids[i]), store.get(ids[i + 1])];
				store.delete(ids[i]);
				store.delete(ids[i + 1]);
				if (b) store.set(ids[i], b);
				if (a) store.set(ids[i + 1], a);
			}
		}
		return null;
	}
}));

/** an engine that holds drawings by id (each an empty packing: no tiles) */
function fakeEngine(showing: number[]) {
	const kept = new Set<number>();
	const has = (id: number) => kept.has(id) || showing.includes(id);
	return {
		kept,
		canvas: {
			hasDrawing: has,
			putDrawingTiles: (id: number) => void kept.add(id),
			drawingTilesPacked: () => new Uint8Array(4),
			keptDrawings: () => [...kept],
			forgetDrawing: (id: number) => kept.delete(id)
		}
	};
}

describe('a canvas size frame by frame through the store', () => {
	it('resizes a batch at a time, keeping only what shows; the step swaps in the store', async () => {
		store.clear();
		calls.length = 0;
		for (let id = 1; id <= 20; id++) store.set(id, new Uint8Array(4));
		const e = fakeEngine([1]);
		const cache = new FrameCache(() => e.canvas as never, true);
		cache.stored([...store.keys()]);
		cache.keep = 2;
		let next = 100;
		let most = 0;
		const progress: number[] = [];
		const pairs = await cache.resizeEach(
			[...store.keys(), 1, 0],
			new Set([1]),
			(from, to) => {
				e.kept.add(to);
				most = Math.max(most, e.kept.size);
				return e.canvas.hasDrawing(from);
			},
			() => ++next,
			(done) => progress.push(done),
			5
		);
		expect(pairs).toHaveLength(20);
		expect(progress).toEqual([5, 10, 15, 20]);
		// never more than a batch and its new sizes in the engine at once (and what shows)
		expect(most).toBeLessThanOrEqual(11);
		// each batch sent once: the new sizes (the drawings were in the store already)
		expect(calls.filter((c) => c === 'frames_put')).toHaveLength(4);
		expect(store.size).toBe(40);
		// the engine keeps the drawing showing and its new size (held through a trim)
		const other = pairs.find(([d]) => d === 1)![1];
		cache.trim();
		expect([...e.kept]).toEqual([other]);
		// the step: the store swaps every pair, the engine (here, `go`) does too
		const was = new Map(store);
		let went = 0;
		expect(await cache.swapStep(pairs, new Set([1]), () => ++went > 0)).toBe(true);
		expect(went).toBe(1);
		for (const [d, to] of pairs) {
			expect(store.get(d)).toBe(was.get(to));
			expect(store.get(to)).toBe(was.get(d));
		}
		// the other size is let go: it waits in the store for undo
		expect(e.kept.size).toBe(0);
		expect(cache.version(5)).toBe(1);

		// undo: the showing drawing's other size comes from the store first
		let had = false;
		expect(
			await cache.swapStep(pairs, new Set([1]), () => (had = e.canvas.hasDrawing(other)))
		).toBe(true);
		expect(had).toBe(true);
		for (const [d] of pairs) expect(store.get(d)).toBe(was.get(d));
		// one that doesn't happen swaps the store back
		expect(await cache.swapStep(pairs, new Set([1]), () => false)).toBe(false);
		for (const [d] of pairs) expect(store.get(d)).toBe(was.get(d));
	});
});

describe('a canvas size across the frame store', () => {
	it('holds every drawing in the engine while it changes, then sends them all back', async () => {
		store.clear();
		calls.length = 0;
		for (let id = 1; id <= 30; id++) store.set(id, new Uint8Array(4));
		const e = fakeEngine([1]);
		const cache = new FrameCache(() => e.canvas as never, true);
		cache.stored([...store.keys()]);
		cache.keep = 2;
		const ids = [...store.keys()];
		await cache.hold(ids);
		expect(ids.every((id) => e.canvas.hasDrawing(id))).toBe(true);
		// trimming meanwhile (a frame change finishing) lets none of them go
		cache.trim();
		expect(e.kept.size).toBe(29);
		// all changed: to the store a batch at a time, then the engine keeps its usual few
		store.clear();
		await cache.release(ids, 10);
		expect(calls.filter((c) => c === 'frames_put')).toHaveLength(3);
		expect(store.size).toBe(30);
		expect(e.kept.size).toBe(2);
	});

	it('a failed change lets them go unchanged (nothing sent)', async () => {
		calls.length = 0;
		const e = fakeEngine([]);
		const cache = new FrameCache(() => e.canvas as never, true);
		cache.stored([1, 2, 3]);
		cache.keep = 0;
		await cache.hold([1, 2, 3]);
		await cache.release();
		expect(calls).not.toContain('frames_put');
		expect(e.kept.size).toBe(0);
	});
});
