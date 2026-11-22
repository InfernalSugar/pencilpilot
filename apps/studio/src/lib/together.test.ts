import { describe, expect, it } from 'vitest';
import {
	decodeTile,
	diffTiles,
	encodeTile,
	pack,
	parseEdit,
	Shared,
	TILE_BYTES,
	Together,
	unpack,
	type CanvasPort,
	type TileMap
} from './together';
import { newTimeline, type Timeline } from './timeline';

const A = 'a'.repeat(64);
const B = 'b'.repeat(64);
const C = 'c'.repeat(64);

function tile(v: number) {
	return new Uint8Array(TILE_BYTES).fill(v);
}

/** A canvas: layers with drawings of tiles. */
class FakeCanvas implements CanvasPort {
	layerList: { id: number; name: string; locked: boolean; visible: boolean; drawing: number }[] =
		[];
	drawings = new Map<number, TileMap>();
	selected = 0;
	busy = false;
	#next = 1;
	constructor(names: string[] = ['Layer 1']) {
		for (const n of names) this.addLayer(n);
	}
	addLayer(name: string) {
		const id = this.#next++;
		this.layerList.push({ id, name, locked: false, visible: true, drawing: 0 });
		return id;
	}
	setLayer(id: number, patch: { name?: string; locked?: boolean; visible?: boolean }) {
		Object.assign(
			this.layerList.find((l) => l.id === id)!,
			patch
		);
	}
	/** (as the page does: drawing on an empty cel starts a drawing, on the timeline there) */
	drawingOf(id: number) {
		const l = this.layerList.find((l) => l.id === id)!;
		if (!l.drawing) {
			l.drawing = 1000 + id;
			this.drawings.set(l.drawing, new Map());
			const cut = this.tl.scenes[0].cuts[0];
			if (!cut.tracks[id]?.length) cut.tracks[id] = [{ drawing: l.drawing, length: 1 }];
		}
		return l.drawing;
	}
	async packed(d: number) {
		return pack(this.drawings.get(d) ?? new Map());
	}
	tl: Timeline = newTimeline([]);
	#nextDrawing = 5000;
	newDrawing() {
		const d = this.#nextDrawing++;
		this.drawings.set(d, new Map());
		return d;
	}
	layerOfDrawing(d: number) {
		const showing = this.layerList.find((l) => l.drawing === d);
		if (showing) return showing.id;
		for (const sc of this.tl.scenes)
			for (const c of sc.cuts)
				for (const [l, t] of Object.entries(c.tracks))
					if (t.some((e) => e.drawing === d)) return +l;
		return null;
	}
	drawingsOfLayer(id: number) {
		return this.tl.scenes.flatMap((sc) =>
			sc.cuts.flatMap((c) => (c.tracks[id] ?? []).map((e) => e.drawing).filter((d) => d))
		);
	}
	timeline() {
		return structuredClone(this.tl);
	}
	/** (as the page does: the frame is shown again) */
	setTimeline(tl: Timeline) {
		this.tl = tl;
		this.showFrame(0);
	}
	/** show the drawing on frame `f` of the first cut */
	showFrame(f: number) {
		for (const l of this.layerList) {
			const t = this.tl.scenes[0]?.cuts[0]?.tracks[l.id];
			let at = 0;
			for (const e of t ?? []) {
				if (f < at + e.length) {
					l.drawing = e.drawing;
					break;
				}
				at += e.length;
			}
		}
	}
	put(d: number, p: Uint8Array) {
		if (this.busy) return false;
		this.drawings.set(d, unpack(p));
		return true;
	}
	select(id: number) {
		this.selected = id;
	}
	removeLayer(id: number) {
		if (this.busy) return false;
		this.layerList = this.layerList.filter((l) => l.id !== id);
		return true;
	}
	moveNextTo(id: number, ref: number, above: boolean) {
		if (this.busy) return false;
		const l = this.layerList.find((x) => x.id === id)!;
		this.layerList = this.layerList.filter((x) => x.id !== id);
		const r = this.layerList.findIndex((x) => x.id === ref);
		this.layerList.splice(above ? r + 1 : r, 0, l);
		return true;
	}
	layers() {
		return this.layerList.map((l) => ({ id: l.id, name: l.name }));
	}
	/** draw: set a tile on a layer, if it isn't locked */
	paint(id: number, x: number, y: number, v: number) {
		const l = this.layerList.find((l) => l.id === id)!;
		if (l.locked) return false;
		this.drawings.get(this.drawingOf(id))!.set(`${x},${y}`, tile(v));
		return true;
	}
	pixel(id: number, x: number, y: number) {
		const l = this.layerList.find((l) => l.id === id)!;
		return this.drawings.get(l.drawing)?.get(`${x},${y}`)?.[0];
	}
	byName(name: string) {
		return this.layerList.find((l) => l.name === name)!;
	}
}

/** A session: numbers edits and hands them to every app, in order. */
class FakeSession {
	seq = 0;
	apps = new Map<string, Together>();
	queue: (() => Promise<void>)[] = [];
	net(me: string) {
		return {
			edit: (body: unknown) => {
				const seq = ++this.seq;
				const copy = structuredClone(body);
				for (const t of this.apps.values()) this.queue.push(() => t.onEdit(seq, me, copy));
			},
			direct: (to: string, body: unknown) => {
				const copy = structuredClone(body);
				this.queue.push(async () => this.apps.get(to)?.onDirect(me, copy));
			}
		};
	}
	async settle() {
		while (this.queue.length) await this.queue.shift()!();
	}
}

describe('drawing together: the shared layers', () => {
	it('takes only the listed edits (no AI, nothing else)', () => {
		expect(parseEdit({ k: 'layer', lid: 'aaaaaaaaaaaa:1', name: 'Hi' })).toEqual({
			k: 'layer',
			lid: 'aaaaaaaaaaaa:1',
			name: 'Hi'
		});
		expect(parseEdit({ k: 'ai-image', lid: 'aaaaaaaaaaaa:1', png: '...' })).toBeNull();
		expect(parseEdit({ k: 'layer', lid: '../etc' })).toBeNull();
		expect(parseEdit({ k: 'grant', lid: 'aaaaaaaaaaaa:1', to: 'nope' })).toBeNull();
		expect(parseEdit(null)).toBeNull();
	});

	it('lets only writers change a layer, and only owners give access', () => {
		const s = new Shared();
		const lid = 'aaaaaaaaaaaa:1';
		expect(s.apply(B, { k: 'layer', lid, name: 'x' })).toBe(false); // not B's id
		expect(s.apply(A, { k: 'layer', lid, name: 'A' })).toBe(true);
		const t = { k: 'tiles' as const, lid, did: lid, set: [], clear: [] };
		expect(s.apply(B, t)).toBe(false);
		expect(s.apply(B, { k: 'ask', lid })).toBe(true);
		expect(s.asks).toEqual([{ lid, from: B }]);
		expect(s.apply(B, { k: 'grant', lid, to: B, handover: false })).toBe(false); // not owner
		expect(s.apply(A, { k: 'grant', lid, to: B, handover: false })).toBe(true);
		expect(s.asks).toEqual([]);
		expect(s.apply(B, t)).toBe(true);
		expect(s.apply(A, { k: 'revoke', lid, to: B })).toBe(true);
		expect(s.apply(B, t)).toBe(false);
		// handing over: B owns it, A can't draw any more
		expect(s.apply(A, { k: 'grant', lid, to: B, handover: true })).toBe(true);
		expect(s.layers.get(lid)!.owner).toBe(B);
		expect(s.canDraw(lid, A)).toBe(false);
		expect(s.apply(A, { k: 'revoke', lid, to: B })).toBe(false);
	});
});

describe('drawing together: tiles', () => {
	it('packs, diffs and travels compressed', async () => {
		const m: TileMap = new Map([
			['0,0', tile(1)],
			['-1,2', tile(2)]
		]);
		expect(unpack(pack(m))).toEqual(m);
		const next: TileMap = new Map([
			['0,0', tile(1)],
			['3,3', tile(9)]
		]);
		const d = diffTiles(m, next);
		expect(d.set.map((s) => [s[0], s[1]])).toEqual([[3, 3]]);
		expect(d.clear).toEqual([[-1, 2]]);
		const enc = await encodeTile(tile(7));
		expect(enc.length).toBeLessThan(200); // a flat tile compresses to almost nothing
		expect(await decodeTile(enc)).toEqual(tile(7));
		expect(await decodeTile('z:garbage!')).toBeNull();
	});
});

describe('drawing together: apps', () => {
	it('shares the host’s drawing with a joiner, then each draws on their own layer', async () => {
		const session = new FakeSession();
		const hc = new FakeCanvas(['Sky']);
		const host = new Together(A, 'Ana', hc, session.net(A));
		session.apps.set(A, host);
		hc.paint(hc.byName('Sky').id, 0, 0, 50);
		await host.startHost();
		await session.settle();

		// Bo joins: gets the project
		const bc = new FakeCanvas(['Bo’s old work']);
		bc.paint(1, 5, 5, 99);
		const bo = new Together(B, 'Bo', bc, session.net(B));
		session.apps.set(B, bo);
		const tilesFrom: string[] = [];
		host.ontiles = (from) => tilesFrom.push(from);
		bo.startJoin(A);
		await session.settle();
		expect(bo.synced).toBe(true);
		const sky = bc.byName('Sky');
		expect(bc.pixel(sky.id, 0, 0)).toBe(50);
		expect(sky.locked).toBe(true);
		// Bo's own work before joining stays here, hidden, not shared
		expect(bc.layerList[0]).toMatchObject({ visible: false, locked: true });
		// Bo got a layer of his own, selected, and the host has it too
		const boLayer = bc.byName('Bo’s layer');
		expect(bc.selected).toBe(boLayer.id);
		expect(boLayer.locked).toBe(false);
		expect(hc.byName('Bo’s layer').locked).toBe(true);

		// Bo draws on his layer: the host sees it
		bc.paint(boLayer.id, 1, 1, 77);
		// (the page sends its timeline when a drawing starts on it, as here)
		bo.timelineChanged();
		await bo.localChanged();
		await session.settle();
		expect(hc.pixel(hc.byName('Bo’s layer').id, 1, 1)).toBe(77);
		// (and the host was told Bo's real pixels came, for clearing his live preview)
		expect(tilesFrom).toEqual([B]);
		// Bo can't draw on the host's layer
		expect(bc.paint(sky.id, 2, 2, 1)).toBe(false);

		// the same layer order everywhere
		expect(hc.layers().map((l) => l.name)).toEqual(['Sky', 'Bo’s layer']);
		expect(
			bc
				.layers()
				.slice(1)
				.map((l) => l.name)
		).toEqual(['Sky', 'Bo’s layer']);
	});

	it('asks, grants and takes back access to a layer', async () => {
		const session = new FakeSession();
		const hc = new FakeCanvas(['Sky']);
		const host = new Together(A, 'Ana', hc, session.net(A));
		session.apps.set(A, host);
		await host.startHost();
		await session.settle();
		const bc = new FakeCanvas();
		const bo = new Together(B, 'Bo', bc, session.net(B));
		session.apps.set(B, bo);
		bo.startJoin(A);
		await session.settle();
		const skyLid = host.lidOf(hc.byName('Sky').id)!;

		bo.ask(skyLid);
		await session.settle();
		expect(host.shared.asks).toEqual([{ lid: skyLid, from: B }]);
		host.grant(skyLid, B);
		await session.settle();
		const sky = bc.byName('Sky');
		expect(sky.locked).toBe(false);
		bc.paint(sky.id, 3, 3, 33);
		await bo.localChanged();
		await session.settle();
		expect(hc.pixel(hc.byName('Sky').id, 3, 3)).toBe(33);

		host.revoke(skyLid, B);
		await session.settle();
		expect(bc.byName('Sky').locked).toBe(true);
	});

	it('ignores tiles from someone who may not draw there, and retries when busy', async () => {
		const session = new FakeSession();
		const hc = new FakeCanvas(['Sky']);
		const host = new Together(A, 'Ana', hc, session.net(A));
		session.apps.set(A, host);
		await host.startHost();
		await session.settle();
		const skyLid = host.lidOf(hc.byName('Sky').id)!;
		// a forged change from C (not a writer)
		const evil = {
			k: 'tiles',
			lid: skyLid,
			did: skyLid,
			set: [[0, 0, await encodeTile(tile(1))]],
			clear: []
		};
		await host.onEdit(99, C, evil);
		expect(hc.pixel(hc.byName('Sky').id, 0, 0)).toBeUndefined();

		// a joiner whose canvas is busy (mid-stroke) gets the tiles once it's free
		const bc = new FakeCanvas();
		const bo = new Together(B, 'Bo', bc, session.net(B));
		session.apps.set(B, bo);
		bo.startJoin(A);
		await session.settle();
		hc.paint(hc.byName('Sky').id, 4, 4, 44);
		bc.busy = true;
		await host.localChanged();
		await session.settle();
		expect(bc.pixel(bc.byName('Sky').id, 4, 4)).toBeUndefined();
		bc.busy = false;
		await bo.localChanged();
		expect(bc.pixel(bc.byName('Sky').id, 4, 4)).toBe(44);
	});

	it('deletes and moves shared layers for everyone (only their owner can)', async () => {
		const session = new FakeSession();
		const hc = new FakeCanvas(['Sky', 'Hills']);
		const host = new Together(A, 'Ana', hc, session.net(A));
		session.apps.set(A, host);
		await host.startHost();
		await session.settle();
		const bc = new FakeCanvas();
		const bo = new Together(B, 'Bo', bc, session.net(B));
		session.apps.set(B, bo);
		bo.startJoin(A);
		await session.settle();
		const names = (c: FakeCanvas) => c.layerList.filter((l) => l.visible).map((l) => l.name);
		expect(names(hc)).toEqual(['Sky', 'Hills', 'Bo’s layer']);
		expect(names(bc)).toEqual(['Sky', 'Hills', 'Bo’s layer']);

		// Bo can't move or delete Ana's layers
		const sky = host.lidOf(hc.byName('Sky').id)!;
		bo.net.edit({ k: 'remove', lid: sky });
		await session.settle();
		expect(names(bc)).toContain('Sky');

		// Ana moves Sky to the top, everywhere
		host.moveOwn(sky, 2);
		await session.settle();
		expect(names(hc)).toEqual(['Hills', 'Bo’s layer', 'Sky']);
		expect(names(bc)).toEqual(['Hills', 'Bo’s layer', 'Sky']);

		// and deletes Hills; Bo's canvas is mid-stroke, so it goes once he's done
		bc.busy = true;
		host.removeOwn(host.lidOf(hc.byName('Hills').id)!);
		await session.settle();
		expect(names(hc)).toEqual(['Bo’s layer', 'Sky']);
		expect(names(bc)).toContain('Hills');
		bc.busy = false;
		await bo.localChanged();
		expect(names(bc)).toEqual(['Bo’s layer', 'Sky']);
	});

	it('shares the timeline and every frame’s drawing', async () => {
		const session = new FakeSession();
		const hc = new FakeCanvas(['Ball']);
		const ball = hc.byName('Ball').id;
		// two frames: drawing 1 for 2 frames, drawing 2 for 2 frames
		const d1 = hc.newDrawing();
		const d2 = hc.newDrawing();
		hc.drawings.get(d1)!.set('0,0', tile(11));
		hc.drawings.get(d2)!.set('1,0', tile(22));
		hc.tl = newTimeline([{ id: ball, drawing: d1 }]);
		hc.tl.scenes[0].cuts[0].tracks[ball] = [
			{ drawing: d1, length: 2 },
			{ drawing: d2, length: 2 }
		];
		hc.showFrame(0);
		const host = new Together(A, 'Ana', hc, session.net(A));
		session.apps.set(A, host);
		await host.startHost();
		await session.settle();

		const bc = new FakeCanvas();
		const bo = new Together(B, 'Bo', bc, session.net(B));
		session.apps.set(B, bo);
		bo.startJoin(A);
		await session.settle();
		const bBall = bc.byName('Ball').id;
		const track = bc.tl.scenes[0].cuts[0].tracks[bBall];
		expect(track.map((e) => e.length)).toEqual([2, 2]);
		// both frames' pixels arrived, though only the first was showing on the host
		expect(bc.drawings.get(track[0].drawing)!.get('0,0')?.[0]).toBe(11);
		expect(bc.drawings.get(track[1].drawing)!.get('1,0')?.[0]).toBe(22);

		// the host times frame 1 on threes: Bo's timeline follows
		hc.tl.scenes[0].cuts[0].tracks[ball][0].length = 3;
		host.timelineChanged();
		await session.settle();
		expect(bc.tl.scenes[0].cuts[0].tracks[bBall].map((e) => e.length)).toEqual([3, 2]);

		// Ana draws on frame 2 (showing it): Bo's frame 2 changes, not frame 1
		hc.showFrame(3);
		hc.paint(ball, 2, 2, 33);
		await host.localChanged();
		await session.settle();
		expect(bc.drawings.get(track[1].drawing)!.get('2,2')?.[0]).toBe(33);
		expect(bc.drawings.get(track[0].drawing)!.has('2,2')).toBe(false);
	});
});
