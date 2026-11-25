import { describe, expect, it } from 'vitest';
import { NO_STYLE, type LayerStyle } from '@pencilpilot/paint/layer-style';
import {
	decodeTile,
	diffTiles,
	encodeTile,
	pack,
	parseEdit,
	Shared,
	TILE_BYTES,
	readSavedShare,
	Together,
	unpack,
	type CanvasPort,
	type TileMap
} from './together';
import { followTiming, newTimeline, setStage, type Timeline } from './timeline';
import { STRIDE, transformStroke, type LineStroke } from '@pencilpilot/paint/lines';
import {
	applyLineEdit,
	fromShared,
	lineEdits,
	readLineEdit,
	splitBase,
	strokeFromShare,
	strokeToShare,
	toShared,
	withBase
} from './together';

const A = 'a'.repeat(64);
const B = 'b'.repeat(64);
const C = 'c'.repeat(64);

function tile(v: number) {
	return new Uint8Array(TILE_BYTES).fill(v);
}

/** A canvas: layers with drawings of tiles. */
class FakeCanvas implements CanvasPort {
	layerList: {
		id: number;
		name: string;
		locked: boolean;
		visible: boolean;
		drawing: number;
		opacity: number;
		blend: string;
		adjust: string | null;
		mask: TileMap | null;
		group: number;
		style: LayerStyle | null;
	}[] = [];
	groupList: { id: number; name: string; visible: boolean; opacity: number }[] = [];
	#nextGroup = 1;
	drawings = new Map<number, TileMap>();
	selected = 0;
	busy = false;
	#next = 1;
	constructor(names: string[] = ['Layer 1']) {
		for (const n of names) this.addLayer(n);
	}
	addLayer(name: string, adjust?: string) {
		const id = this.#next++;
		this.layerList.push({
			id,
			name,
			locked: false,
			visible: true,
			drawing: 0,
			opacity: 1,
			blend: 'normal',
			adjust: adjust ?? null,
			mask: null,
			group: 0,
			style: null
		});
		return id;
	}
	place(id: number, ref: number, above: boolean, group: number) {
		if (this.busy) return false;
		if (ref && ref !== id) this.moveNextTo(id, ref, above);
		const l = this.layerList.find((x) => x.id === id);
		if (l) l.group = this.groupList.some((g) => g.id === group) ? group : 0;
		return true;
	}
	addGroup(layer: number) {
		if (this.busy) return 0;
		const id = this.#nextGroup++;
		this.groupList.push({ id, name: `Group ${id}`, visible: true, opacity: 1 });
		this.layerList.find((l) => l.id === layer)!.group = id;
		return id;
	}
	setGroup(id: number, s: { name: string; visible: boolean; opacity: number }) {
		const g = this.groupList.find((x) => x.id === id);
		if (g) Object.assign(g, s);
	}
	ungroup(id: number) {
		if (this.busy) return false;
		for (const l of this.layerList) if (l.group === id) l.group = 0;
		this.groupList = this.groupList.filter((g) => g.id !== id);
		return true;
	}
	/** (the upper layer's tiles over the lower's) */
	mergeDown(id: number) {
		const i = this.layerList.findIndex((l) => l.id === id);
		if (this.busy || i < 1) return false;
		const [lower, upper] = [this.layerList[i - 1], this.layerList[i]];
		const into = this.drawings.get(this.drawingOf(lower.id))!;
		for (const [k, px] of this.drawings.get(this.drawingOf(upper.id)) ?? []) into.set(k, px);
		this.layerList.splice(i, 1);
		return true;
	}
	mergeGroup(id: number) {
		const m = this.layerList.filter((l) => l.group === id);
		const g = this.groupList.find((x) => x.id === id);
		if (this.busy || !m.length || !g) return 0;
		const into = this.drawings.get(this.drawingOf(m[0].id))!;
		for (const l of m.slice(1))
			for (const [k, px] of this.drawings.get(this.drawingOf(l.id)) ?? []) into.set(k, px);
		this.layerList = this.layerList.filter((l) => !m.slice(1).includes(l));
		Object.assign(m[0], { name: g.name, group: 0, opacity: g.opacity });
		this.groupList = this.groupList.filter((x) => x !== g);
		return m[0].id;
	}
	groups() {
		return structuredClone(this.groupList);
	}
	setAdjust(id: number, spec: string) {
		if (this.busy) return false;
		this.layerList.find((l) => l.id === id)!.adjust = spec;
		return true;
	}
	maskPacked(id: number) {
		const m = this.layerList.find((l) => l.id === id)!.mask;
		return m ? pack(m) : new Uint8Array();
	}
	putMask(id: number, packed: Uint8Array) {
		if (this.busy) return false;
		this.layerList.find((l) => l.id === id)!.mask = packed.length ? unpack(packed) : null;
		return true;
	}
	setLayer(
		id: number,
		patch: {
			name?: string;
			locked?: boolean;
			visible?: boolean;
			opacity?: number;
			blend?: string;
			style?: LayerStyle | null;
		}
	) {
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
	sounds = new Map<string, { bytes: Uint8Array; name: string }>();
	sound(file: string) {
		return this.sounds.get(file) ?? null;
	}
	async putSound(file: string, bytes: Uint8Array, name: string) {
		this.sounds.set(file, { bytes, name });
	}
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
		return this.layerList.map((l) => ({
			id: l.id,
			name: l.name,
			adjust: l.adjust,
			group: l.group,
			look: {
				visible: l.visible,
				opacity: l.opacity,
				blend: l.blend,
				alphaLock: false,
				clip: false,
				style: l.style
			}
		}));
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
	// line layers: which layers, and each drawing's strokes
	lineLayers = new Set<number>();
	strokes = new Map<number, readonly LineStroke[]>();
	isLineLayer(id: number) {
		return this.lineLayers.has(id);
	}
	setLineLayer(id: number, on: boolean) {
		if (on) this.lineLayers.add(id);
		else this.lineLayers.delete(id);
	}
	lineStrokes(d: number) {
		return this.strokes.get(d) ?? [];
	}
	setLineStrokes(d: number, list: readonly LineStroke[], plain?: boolean) {
		this.strokes.set(d, list);
		if (plain) this.plain.add(d);
	}
	// plain layers' editable strokes: which drawings, their base changes and all of it
	plain = new Set<number>();
	baseOut = new Map<number, Uint8Array>();
	baseFull = new Map<number, Uint8Array>();
	baseIn: [number, Uint8Array][] = [];
	plainStrokes(d: number) {
		return this.plain.has(d);
	}
	baseChanges(d: number) {
		const b = this.baseOut.get(d) ?? null;
		this.baseOut.delete(d);
		return b;
	}
	baseAll(d: number) {
		return this.baseFull.get(d) ?? null;
	}
	putBaseChanges(d: number, bytes: Uint8Array) {
		this.baseIn.push([d, bytes]);
	}
}

/** a packed base (as the engine packs it): `keys` there (clear), `gone` keys gone */
function basePack(keys: [number, number][], gone: [number, number][] = []) {
	const out = new Uint8Array(4 + keys.length * 8 + 4 + 4 + gone.length * 8);
	const v = new DataView(out.buffer);
	let o = 0;
	v.setUint32(o, keys.length, true);
	o += 4;
	for (const [x, y] of keys) {
		v.setInt32(o, x, true);
		v.setInt32(o + 4, y, true);
		o += 8;
	}
	v.setUint32(o, 0, true);
	o += 4;
	v.setUint32(o, gone.length, true);
	o += 4;
	for (const [x, y] of gone) {
		v.setInt32(o, x, true);
		v.setInt32(o + 4, y, true);
		o += 8;
	}
	return out;
}

/** a line stroke from (x, 0) to (x, 10) */
function stroke(id: string, x: number): LineStroke {
	const pts = new Float32Array(3 * STRIDE);
	for (let i = 0; i < 3; i++) pts.set([x, i * 5, 0.5, 0, 0, 0.01], i * STRIDE);
	return { id, pts, brush: 'pen', color: '#000000', size: 1, opacity: 1, radius: 2, seed: 7 };
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

	it('checks a layer style in a look', () => {
		const lid = 'aaaaaaaaaaaa:1';
		const look = { visible: true, opacity: 1, blend: 'normal', alphaLock: false, clip: false };
		const outline = {
			...NO_STYLE,
			stroke: { on: true, size: 500, colour: '#FF0000', position: 'sideways', opacity: 1 }
		};
		const e = parseEdit({ k: 'look', lid, look: { ...look, style: outline } });
		expect(e).toMatchObject({
			k: 'look',
			look: { style: { stroke: { on: true, size: 100, colour: '#ff0000', position: 'outside' } } }
		});
		// junk is no style; an app without styles leaves it out (and the style alone)
		expect(parseEdit({ k: 'look', lid, look: { ...look, style: 'glow' } })).toMatchObject({
			look: { style: null }
		});
		const old = parseEdit({ k: 'look', lid, look });
		expect(old && old.k === 'look' && 'style' in old.look).toBe(false);
	});

	it('lets only writers change a layer, and only owners give access', () => {
		const s = new Shared();
		const lid = 'aaaaaaaaaaaa:1';
		expect(s.apply(B, { k: 'layer', lid, name: 'x' })).toBe(false); // not B's id
		expect(s.apply(A, { k: 'layer', lid, name: 'A' })).toBe(true);
		const t = { k: 'tiles' as const, lid, did: lid, set: [], clear: [] };
		expect(s.apply(B, t)).toBe(false);
		expect(s.apply(B, { k: 'ask', lid, why: 'draw', own: false })).toBe(true);
		expect(s.asks).toEqual([{ lid, from: B, why: 'draw' }]);
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

describe('drawing together: groups and asking first', () => {
	it('lets groups change only with their owner’s leave, and layers join only if drawable', () => {
		const s = new Shared();
		const la = 'aaaaaaaaaaaa:1';
		const lb = 'bbbbbbbbbbbb:1';
		const g = 'aaaaaaaaaaaa:2';
		s.apply(A, { k: 'layer', lid: la, name: 'A' });
		s.apply(B, { k: 'layer', lid: lb, name: 'B' });
		// A can't put B's layer in a group, nor use B's id for the group
		expect(s.apply(A, { k: 'group', lid: la, gid: g, name: 'G', lids: [la, lb] })).toBe(false);
		expect(s.apply(A, { k: 'group', lid: la, gid: 'bbbbbbbbbbbb:9', name: 'G', lids: [la] })).toBe(
			false
		);
		expect(s.apply(A, { k: 'group', lid: la, gid: g, name: 'G', lids: [la] })).toBe(true);
		expect(s.members(g)).toEqual([la]);
		// B can't rename A's group, or move his layer into it, until A says yes
		const gset = { k: 'gset' as const, lid: g, name: 'Mine', visible: true, opacity: 0.5 };
		expect(s.apply(B, gset)).toBe(false);
		expect(s.apply(B, { k: 'place', lid: lb, to: 0, gid: g })).toBe(false);
		expect(s.apply(B, { k: 'ask', lid: g, why: 'group', own: false })).toBe(true);
		expect(s.asks).toEqual([{ lid: g, from: B, why: 'group' }]);
		expect(s.apply(A, { k: 'grant', lid: g, to: B, handover: false })).toBe(true);
		expect(s.apply(B, { k: 'place', lid: lb, to: 0, gid: g })).toBe(true);
		expect(s.order).toEqual([lb, la]);
		expect(s.members(g)).toEqual([lb, la]);
		expect(s.apply(B, gset)).toBe(true);
		// only the group's owner takes it apart; a group left empty goes
		expect(s.apply(B, { k: 'ungroup', lid: g })).toBe(false);
		expect(s.apply(A, { k: 'block', lid: g, to: 0 })).toBe(true);
		expect(s.apply(B, { k: 'place', lid: lb, to: 1, gid: '' })).toBe(true);
		expect(s.apply(A, { k: 'remove', lid: la })).toBe(true);
		expect(s.groups.has(g)).toBe(false);
		// asking for a layer to be handed over (to merge it) says so; an owner can't ask
		expect(s.apply(A, { k: 'ask', lid: lb, why: 'merge', own: true, with: la })).toBe(true);
		expect(s.apply(B, { k: 'ask', lid: lb, why: 'merge', own: true })).toBe(false);
		expect(s.asks).toEqual([{ lid: lb, from: A, why: 'merge', own: true, with: la }]);
		const snap = s.snapshot();
		const back = new Shared();
		back.load(structuredClone(snap));
		expect(back.snapshot()).toEqual(snap);
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

	it('merges into someone else’s layer once they allow it', async () => {
		const session = new FakeSession();
		const hc = new FakeCanvas(['Sky']);
		const host = new Together(A, 'Ana', hc, session.net(A));
		session.apps.set(A, host);
		hc.paint(hc.byName('Sky').id, 0, 0, 10);
		await host.startHost();
		await session.settle();
		const bc = new FakeCanvas();
		const bo = new Together(B, 'Bo', bc, session.net(B));
		session.apps.set(B, bo);
		bo.startJoin(A);
		await session.settle();
		const mine = bc.byName('Bo’s layer');
		bc.paint(mine.id, 1, 1, 77);
		await bo.localChanged();
		await session.settle();

		// Bo merges his layer down into Ana's Sky: Ana is asked first
		const notes: string[] = [];
		bo.onnote = (t) => notes.push(t);
		expect(bo.mergeDown(mine.id)).toEqual([A]);
		await session.settle();
		const skyLid = host.lidOf(hc.byName('Sky').id)!;
		expect(host.shared.asks).toMatchObject([{ lid: skyLid, from: B, why: 'merge' }]);
		expect(bc.layerList.some((l) => l.name === 'Bo’s layer')).toBe(true);

		// she allows it: merged, for both
		host.grant(skyLid, B);
		await session.settle();
		for (const c of [hc, bc]) {
			expect(c.layerList.some((l) => l.name === 'Bo’s layer')).toBe(false);
			expect(c.pixel(c.byName('Sky').id, 1, 1)).toBe(77);
			expect(c.pixel(c.byName('Sky').id, 0, 0)).toBe(10);
		}
		expect(notes).toEqual([]);

		// asked again and refused: nothing changes, and Bo is told
		bc.addLayer('Extra');
		await bo.localChanged();
		await session.settle();
		host.revoke(skyLid, B);
		await session.settle();
		const extra = bc.byName('Extra');
		const sky = bc.byName('Sky');
		expect(bc.layerList.indexOf(extra)).toBe(bc.layerList.indexOf(sky) + 1);
		expect(bo.mergeDown(extra.id)).toEqual([A]);
		await session.settle();
		host.deny(skyLid, B);
		await session.settle();
		expect(notes).toEqual(['They said no: nothing was changed.']);
		expect(hc.layerList.some((l) => l.name === 'Extra')).toBe(true);
	});

	it('shares groups, and groups and merges others’ layers with their leave', async () => {
		const session = new FakeSession();
		const hc = new FakeCanvas(['Sky', 'Hills']);
		// Ana's Hills are in a group already when she starts
		hc.addGroup(hc.byName('Hills').id);
		hc.setGroup(1, { name: 'Land', visible: true, opacity: 0.8 });
		const host = new Together(A, 'Ana', hc, session.net(A));
		session.apps.set(A, host);
		hc.paint(hc.byName('Sky').id, 0, 0, 10);
		await host.startHost();
		await session.settle();
		const bc = new FakeCanvas();
		const bo = new Together(B, 'Bo', bc, session.net(B));
		session.apps.set(B, bo);
		bo.startJoin(A);
		await session.settle();
		// Bo has the group, as Ana does
		const land = bc.groupList.find((g) => g.name === 'Land')!;
		expect(land).toMatchObject({ opacity: 0.8 });
		expect(bc.byName('Hills').group).toBe(land.id);

		// Bo puts his layer in Ana's group: asks; once she allows it, it's in, for both
		const mine = bc.byName('Bo’s layer');
		bc.paint(mine.id, 2, 2, 55);
		bo.timelineChanged();
		await bo.localChanged();
		await session.settle();
		const at = bo.shared.order.indexOf(bo.lidOf(bc.byName('Hills').id)!) + 1;
		expect(bo.moveLayerTo(mine.id, at, land.id)).toEqual([A]);
		await session.settle();
		const gid = bo.gidOf(land.id)!;
		host.grant(gid, B);
		await session.settle();
		expect(bc.byName('Bo’s layer').group).toBe(land.id);
		expect(hc.byName('Bo’s layer').group).toBe(1);
		expect(hc.layers().map((l) => l.name)).toEqual(['Sky', 'Hills', 'Bo’s layer']);

		// Bo changes the group's opacity (he may now): Ana sees it
		bc.setGroup(land.id, { name: 'Land', visible: true, opacity: 0.3 });
		await bo.localChanged();
		await session.settle();
		expect(hc.groupList[0].opacity).toBeCloseTo(0.3);

		// Ana merges the group into one layer: Bo's layer must be handed over first
		expect(host.mergeGroup(1)).toEqual([B]);
		await session.settle();
		const boLid = host.lidOf(hc.byName('Bo’s layer').id)!;
		expect(bo.shared.asks).toMatchObject([{ lid: boLid, from: A, why: 'merge', own: true }]);
		bo.grant(boLid, A, true);
		await session.settle();
		for (const c of [hc, bc]) {
			expect(c.layers().map((l) => l.name)).toContain('Land');
			expect(c.layerList.some((l) => l.name === 'Bo’s layer' || l.name === 'Hills')).toBe(false);
			expect(c.pixel(c.byName('Land').id, 2, 2)).toBe(55);
			expect(c.byName('Land').group).toBe(0);
			expect(c.groupList).toEqual([]);
		}
		expect(host.shared.groups.size).toBe(0);
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
		expect(host.shared.asks).toEqual([{ lid: skyLid, from: B, why: 'draw' }]);
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

		// the host puts the ball in clean-up: Bo's ball layer is in it too
		setStage(hc.tl, ball, 'clean');
		host.timelineChanged();
		await session.settle();
		expect(bc.tl.stages).toEqual({ [bBall]: 'clean' });

		// Ana draws on frame 2 (showing it): Bo's frame 2 changes, not frame 1
		hc.showFrame(3);
		hc.paint(ball, 2, 2, 33);
		await host.localChanged();
		await session.settle();
		expect(bc.drawings.get(track[1].drawing)!.get('2,2')?.[0]).toBe(33);
		expect(bc.drawings.get(track[0].drawing)!.has('2,2')).toBe(false);
	});

	it('shares a layer made from another: above it, timed as it, in its stage', async () => {
		const session = new FakeSession();
		const hc = new FakeCanvas(['Rough', 'Sky']);
		const rough = hc.byName('Rough').id;
		const [d1, d2] = [hc.newDrawing(), hc.newDrawing()];
		hc.drawings.get(d1)!.set('0,0', tile(11));
		hc.tl = newTimeline([{ id: rough, drawing: d1 }]);
		hc.tl.scenes[0].cuts[0].tracks[rough] = [
			{ drawing: d1, length: 2 },
			{ drawing: 0, length: 1 },
			{ drawing: d2, length: 1 }
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

		// Ana makes a clean-up layer from the rough (on her canvas, as the studio does)
		const clean = hc.addLayer('Clean-up (Rough)');
		expect(await host.shareAbove(clean, rough)).toBeTruthy();
		followTiming(hc.tl, rough, clean, () => hc.newDrawing());
		setStage(hc.tl, clean, 'clean');
		host.timelineChanged();
		await session.settle();
		const names = (c: FakeCanvas) => c.layerList.filter((l) => l.visible).map((l) => l.name);
		const order = ['Rough', 'Clean-up (Rough)', 'Sky', 'Bo’s layer'];
		expect(names(hc)).toEqual(order);
		expect(names(bc)).toEqual(order);
		const bClean = bc.byName('Clean-up (Rough)').id;
		const t = bc.tl.scenes[0].cuts[0].tracks[bClean];
		expect(t.map((e) => e.length)).toEqual([2, 1, 1]);
		expect(t[0].drawing && t[2].drawing && t[0].drawing !== t[2].drawing).toBeTruthy();
		expect(t[1].drawing).toBe(0);
		expect(bc.tl.stages?.[bClean]).toBe('clean');
	});

	it('shares layer stages by shared layer, keeping unshared layers’ own', () => {
		const local = newTimeline([
			{ id: 1, drawing: 0 },
			{ id: 2, drawing: 0 }
		]);
		setStage(local, 1, 'rough');
		setStage(local, 2, 'comp');
		const lid = (l: number) => (l === 1 ? 'm:1' : null);
		const out = toShared(local, lid, () => '');
		expect(out.stages).toEqual({ 'm:1': 'rough' });
		const back = (stages: Record<string, string> | undefined) =>
			fromShared(
				{ ...out, stages: stages as never },
				(l) => (l === 'm:1' ? 1 : undefined),
				() => 0,
				local,
				(l) => l === 1
			).stages;
		// theirs for the shared layer (unknown stages and layers left out), mine for my own
		expect(back({ 'm:1': 'clean', 'm:9': 'rough' })).toEqual({ 1: 'clean', 2: 'comp' });
		expect(back({ 'm:1': 'wobbly' })).toEqual({ 2: 'comp' });
		// an app that sends no stages leaves mine as they were
		expect(back(undefined)).toEqual({ 1: 'rough', 2: 'comp' });
	});

	it('shares character palettes; an app that sends none leaves mine', () => {
		const local = newTimeline([{ id: 1, drawing: 0 }]);
		local.palettes = [{ name: 'Mina', colours: [{ name: 'Skin', hex: '#f2c9a8' }] }];
		const out = toShared(
			local,
			() => 'm:1',
			() => ''
		);
		expect(out.palettes).toEqual(local.palettes);
		const back = (palettes: unknown) =>
			fromShared(
				{ ...out, palettes: palettes as never },
				() => 1,
				() => 0,
				local,
				() => true
			).palettes;
		// theirs (cleaned: a colour that isn't one goes)
		expect(
			back([
				{
					name: 'Rin',
					colours: [
						{ name: 'Hair', hex: '#223344' },
						{ name: 'x', hex: 'red' }
					]
				}
			])
		).toEqual([{ name: 'Rin', colours: [{ name: 'Hair', hex: '#223344' }] }]);
		expect(back([])).toBeUndefined();
		expect(back(undefined)).toEqual(local.palettes);
	});

	it('comes back after a break: own work kept, the others’ caught up', async () => {
		const session = new FakeSession();
		const hc = new FakeCanvas(['Sky']);
		const host = new Together(A, 'Ana', hc, session.net(A));
		session.apps.set(A, host);
		await host.startHost();
		await session.settle();
		const bc = new FakeCanvas();
		let bo = new Together(B, 'Bo', bc, session.net(B));
		session.apps.set(B, bo);
		bo.startJoin(A);
		await session.settle();
		const boLayer = bc.byName('Bo’s layer').id;
		bc.paint(boLayer, 0, 0, 1);
		bo.timelineChanged();
		await bo.localChanged();
		await session.settle();

		// Bo saves (the file keeps the session) and leaves
		const saved = readSavedShare(
			JSON.parse(JSON.stringify(bo.save('pp1-code', [{ id: A, name: 'Ana', addr: {} }])))
		)!;
		expect(saved.ids.length).toBe(2);
		session.apps.delete(B);

		// apart: Ana draws on her sky, Bo on his layer
		const sky = hc.byName('Sky').id;
		hc.paint(sky, 5, 5, 55);
		await host.localChanged();
		await session.settle();
		bc.paint(boLayer, 6, 6, 66);

		// Bo comes back
		bo = new Together(B, 'Bo', bc, session.net(B));
		session.apps.set(B, bo);
		bo.restore(saved);
		bo.rejoin(A);
		await session.settle();
		expect(bo.synced).toBe(true);
		expect(bc.pixel(bc.byName('Sky').id, 5, 5)).toBe(55);
		// his work from the break is still his, and Ana got it
		expect(bc.pixel(boLayer, 6, 6)).toBe(66);
		expect(hc.pixel(hc.byName('Bo’s layer').id, 6, 6)).toBe(66);
		// no extra layer for him
		expect(bc.layerList.filter((l) => l.name === 'Bo’s layer').length).toBe(1);
	});

	it('the first one back hosts the project from their copy', async () => {
		const session = new FakeSession();
		const hc = new FakeCanvas(['Sky']);
		hc.paint(1, 1, 1, 9);
		const ana = new Together(A, 'Ana', hc, session.net(A));
		session.apps.set(A, ana);
		await ana.startHost();
		await session.settle();
		const saved = readSavedShare(JSON.parse(JSON.stringify(ana.save('pp1-code', []))))!;
		session.apps.delete(A);

		// next day: Ana alone hosts it again, Cy joins and gets it
		const again = new Together(A, 'Ana', hc, session.net(A));
		session.apps.set(A, again);
		again.restore(saved);
		await again.resumeHost();
		const cc = new FakeCanvas();
		const cy = new Together(C, 'Cy', cc, session.net(C));
		session.apps.set(C, cy);
		cy.startJoin(A);
		await session.settle();
		expect(cc.pixel(cc.byName('Sky').id, 1, 1)).toBe(9);
		expect(readSavedShare({ code: 'nope' })).toBeNull();
	});

	it('shares layer settings (opacity, blend, visibility) from whoever may draw there', async () => {
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

		hc.setLayer(hc.byName('Sky').id, { opacity: 0.4, blend: 'multiply' });
		await host.localChanged();
		await session.settle();
		expect(bc.byName('Sky')).toMatchObject({ opacity: 0.4, blend: 'multiply' });

		// Bo changing Ana's layer here doesn't go anywhere (he may not draw there)
		bc.setLayer(bc.byName('Sky').id, { opacity: 1 });
		await bo.localChanged();
		await session.settle();
		expect(hc.byName('Sky').opacity).toBe(0.4);
		// a layer style (an outline) goes too
		const outline: LayerStyle = {
			...NO_STYLE,
			stroke: { ...NO_STYLE.stroke, on: true, colour: '#ff0000' }
		};
		hc.setLayer(hc.byName('Sky').id, { style: outline });
		await host.localChanged();
		await session.settle();
		expect(bc.byName('Sky').style).toEqual(outline);
		// and nothing echoes back and forth
		const before = session.seq;
		await host.localChanged();
		await bo.localChanged();
		await session.settle();
		expect(session.seq).toBe(before);
	});

	it('shares masks and adjustment layers', async () => {
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

		// Ana masks part of the sky
		const sky = hc.byName('Sky');
		sky.mask = new Map([['0,0', tile(200)]]);
		await host.localChanged();
		await session.settle();
		expect(bc.byName('Sky').mask?.get('0,0')?.[0]).toBe(200);

		// and adds a warm adjustment layer (made here above the sky: it goes on top)
		hc.addLayer('Warm', '{"hue":12}');
		await host.localChanged();
		await session.settle();
		expect(bc.byName('Warm').adjust).toBe('{"hue":12}');
		expect(bc.layerList.at(-1)?.name).toBe('Warm');
		// changes it
		hc.setAdjust(hc.byName('Warm').id, '{"hue":30}');
		await host.localChanged();
		await session.settle();
		expect(bc.byName('Warm').adjust).toBe('{"hue":30}');

		// removes the mask
		sky.mask = null;
		await host.localChanged();
		await session.settle();
		expect(bc.byName('Sky').mask).toBeNull();

		// a joiner later gets both
		const cc = new FakeCanvas();
		const cy = new Together(C, 'Cy', cc, session.net(C));
		session.apps.set(C, cy);
		sky.mask = new Map([['1,1', tile(99)]]);
		await host.localChanged();
		cy.startJoin(A);
		await session.settle();
		expect(cc.byName('Sky').mask?.get('1,1')?.[0]).toBe(99);
		expect(cc.byName('Warm').adjust).toBe('{"hue":30}');
	});

	it('shares sound clips and their files (big ones in pieces)', async () => {
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

		// Ana adds a 600 KB song to the scene (three pieces on the way)
		const song = new Uint8Array(600 * 1024).map((_, i) => i % 251);
		hc.sounds.set('s1-abc.mp3', { bytes: song, name: 'song.mp3' });
		hc.tl.scenes[0].audio = [
			{ id: 1, name: 'song.mp3', file: 's1-abc.mp3', start: 4, gain: 0.8, muted: false }
		];
		host.timelineChanged();
		await session.settle();
		expect(bc.tl.scenes[0].audio?.[0]).toMatchObject({ file: 's1-abc.mp3', start: 4, gain: 0.8 });
		expect(bc.sounds.get('s1-abc.mp3')?.bytes).toEqual(song);

		// sent once: moving the clip doesn't send the file again
		const before = session.seq;
		hc.tl.scenes[0].audio![0].start = 8;
		host.timelineChanged();
		await session.settle();
		expect(session.seq - before).toBe(1);
		expect(bc.tl.scenes[0].audio?.[0].start).toBe(8);

		// a joiner later gets the song with the project
		const cc = new FakeCanvas();
		const cy = new Together(C, 'Cy', cc, session.net(C));
		session.apps.set(C, cy);
		cy.startJoin(A);
		await session.settle();
		expect(cc.sounds.get('s1-abc.mp3')?.bytes).toEqual(song);
		expect(cc.tl.scenes[0].audio?.[0].start).toBe(8);
	});

	it('shares line layers: their strokes stay editable everywhere, from their drawer only', async () => {
		const session = new FakeSession();
		const hc = new FakeCanvas(['Ink']);
		const ink = hc.byName('Ink').id;
		hc.setLineLayer(ink, true);
		hc.paint(ink, 0, 0, 10);
		const d = hc.drawingOf(ink);
		hc.setLineStrokes(d, [stroke('a', 1), stroke('b', 2)]);
		const host = new Together(A, 'Ana', hc, session.net(A));
		session.apps.set(A, host);
		await host.startHost();
		await session.settle();

		// Bo joins: a line layer, with the strokes (and the pixels, as tiles)
		const bc = new FakeCanvas([]);
		const bo = new Together(B, 'Bo', bc, session.net(B));
		session.apps.set(B, bo);
		bo.startJoin(A);
		await session.settle();
		const bInk = bc.byName('Ink');
		expect(bc.isLineLayer(bInk.id)).toBe(true);
		expect(bc.pixel(bInk.id, 0, 0)).toBe(10);
		const bd = bc.layerList.find((l) => l.id === bInk.id)!.drawing;
		expect(bc.lineStrokes(bd).map((s) => s.id)).toEqual(['a', 'b']);
		expect([...bc.lineStrokes(bd)[1].pts]).toEqual([...stroke('b', 2).pts]);

		// Ana moves a stroke and adds one: only those travel, in place
		const [a, b] = hc.lineStrokes(d);
		hc.setLineStrokes(d, [a, transformStroke(b, [1, 0, 0, 1, 5, 0]), stroke('c', 3)]);
		hc.paint(ink, 1, 0, 20);
		const sent: unknown[] = [];
		const net = session.net(A);
		const spy = { edit: (e: unknown) => (sent.push(e), net.edit(e)), direct: net.direct };
		(host as unknown as { net: typeof spy }).net = spy;
		await host.localChanged();
		await session.settle();
		const lineSent = sent.filter((e) => (e as { k: string }).k === 'lines') as {
			del: string[];
			put: [number, string][];
		}[];
		expect(lineSent).toHaveLength(1);
		expect(lineSent[0].del).toEqual(['b']);
		expect(lineSent[0].put.map(([i]) => i)).toEqual([1, 2]);
		expect(bc.lineStrokes(bd).map((s) => s.id)).toEqual(['a', 'b', 'c']);
		expect(bc.lineStrokes(bd)[1].pts[0]).toBe(7);
		expect(bc.pixel(bInk.id, 1, 0)).toBe(20);
		// nothing new: nothing sent
		sent.length = 0;
		await host.localChanged();
		expect(sent.filter((e) => (e as { k: string }).k === 'lines')).toEqual([]);

		// Bo may not change Ana's strokes
		const lid = bo.lidOf(bInk.id)!;
		session.net(B).edit({ k: 'lines', lid, did: host.didOf(d), del: ['a'], put: [] });
		await session.settle();
		expect(hc.lineStrokes(d).map((s) => s.id)).toEqual(['a', 'b', 'c']);
		expect(bc.lineStrokes(bd).map((s) => s.id)).toEqual(['a', 'b', 'c']);

		// Ana converts it to pixels: a plain layer for Bo too
		hc.setLineLayer(ink, false);
		await host.localChanged();
		await session.settle();
		expect(bc.isLineLayer(bInk.id)).toBe(false);
		expect(host.shared.layers.get(lid)?.lines).toBeUndefined();
	});

	it('shares a plain layer’s editable strokes with the pixels under them', async () => {
		const session = new FakeSession();
		const hc = new FakeCanvas(['Paint']);
		const paint = hc.byName('Paint').id;
		hc.paint(paint, 0, 0, 10);
		const d = hc.drawingOf(paint);
		hc.setLineStrokes(d, [stroke('a', 1)], true);
		hc.baseFull.set(d, basePack([[0, 0]]));
		hc.baseOut.set(d, basePack([[0, 0]]));
		const host = new Together(A, 'Ana', hc, session.net(A));
		session.apps.set(A, host);
		await host.startHost();
		await session.settle();

		// Bo joins: the strokes, and the base whole
		const bc = new FakeCanvas([]);
		const bo = new Together(B, 'Bo', bc, session.net(B));
		session.apps.set(B, bo);
		bo.startJoin(A);
		await session.settle();
		const bPaint = bc.byName('Paint');
		expect(bc.isLineLayer(bPaint.id)).toBe(false);
		const bd = bc.layerList.find((l) => l.id === bPaint.id)!.drawing;
		expect(bc.lineStrokes(bd).map((s) => s.id)).toEqual(['a']);
		expect(bc.plainStrokes(bd)).toBe(true);
		expect(bc.baseIn.map(([x]) => x)).toContain(bd);
		expect([...bc.baseIn.at(-1)![1]]).toEqual([...basePack([[0, 0]])]);

		// Ana draws another: the stroke and the base's change travel in one edit
		bc.baseIn.length = 0;
		hc.setLineStrokes(d, [...hc.lineStrokes(d), stroke('b', 2)], true);
		hc.baseOut.set(d, basePack([[1, 0]]));
		hc.paint(paint, 1, 0, 20);
		const sent: unknown[] = [];
		const net = session.net(A);
		const spy = { edit: (e: unknown) => (sent.push(e), net.edit(e)), direct: net.direct };
		(host as unknown as { net: typeof spy }).net = spy;
		await host.localChanged();
		await session.settle();
		const lineSent = sent.filter((e) => (e as { k: string }).k === 'lines') as {
			put: unknown[];
			base?: string;
		}[];
		expect(lineSent).toHaveLength(1);
		expect(lineSent[0].put).toHaveLength(1);
		expect(lineSent[0].base).toBeTruthy();
		expect(bc.lineStrokes(bd).map((s) => s.id)).toEqual(['a', 'b']);
		expect([...bc.baseIn[0][1]]).toEqual([...basePack([[1, 0]])]);
		// a base change alone (strokes baked by a fill) still travels
		hc.baseOut.set(d, basePack([], [[1, 0]]));
		await host.localChanged();
		await session.settle();
		expect([...bc.baseIn.at(-1)![1]]).toEqual([...basePack([], [[1, 0]])]);

		// Bo may not change them
		const lid = bo.lidOf(bPaint.id)!;
		session.net(B).edit({ k: 'lines', lid, did: host.didOf(d), del: ['a'], put: [] });
		await session.settle();
		expect(hc.lineStrokes(d).map((s) => s.id)).toEqual(['a', 'b']);
	});
});

describe('drawing together: line edits', () => {
	it('checks line edits and new line layers', () => {
		const lid = 'aaaaaaaaaaaa:1';
		const did = 'aaaaaaaaaaaa:5';
		const enc = strokeToShare(stroke('s1', 4));
		expect(parseEdit({ k: 'layer', lid, name: 'Ink', lines: true })).toEqual({
			k: 'layer',
			lid,
			name: 'Ink',
			lines: true
		});
		// (an adjustment layer is never a line layer)
		expect(parseEdit({ k: 'layer', lid, name: 'A', adjust: '{}', lines: true })).toEqual({
			k: 'layer',
			lid,
			name: 'A',
			adjust: '{}'
		});
		expect(parseEdit({ k: 'lines', lid, did, del: ['x', 7, '../'], put: [[0, enc]] })).toEqual({
			k: 'lines',
			lid,
			did,
			del: ['x'],
			put: [[0, enc]]
		});
		expect(parseEdit({ k: 'lines', lid, did: 'nope', del: [], put: [] })).toBeNull();
		expect(parseEdit({ k: 'lines', lid, did, del: [], put: [[-1, enc]] })).toBeNull();
		expect(parseEdit({ k: 'lines', lid, did, del: [], put: [[0, '<script>']] })).toBeNull();
		expect(
			readLineEdit({ del: [], put: Array.from({ length: 401 }, () => [0, 'AA==']) })
		).toBeNull();
		expect(parseEdit({ k: 'pixels', lid })).toEqual({ k: 'pixels', lid });
		// a stroke that isn't one is dropped as it's put in
		expect(strokeFromShare('AAAA')).toBeNull();
		expect(
			applyLineEdit([], {
				del: [],
				put: [
					[0, 'AAAA'],
					[1, enc]
				]
			}).map((s) => s.id)
		).toEqual(['s1']);
	});

	it('lets only writers send strokes, on any layer but adjustment layers', () => {
		const s = new Shared();
		const lid = 'aaaaaaaaaaaa:1';
		const did = 'aaaaaaaaaaaa:5';
		s.apply(A, { k: 'layer', lid, name: 'Ink', lines: true });
		s.apply(A, { k: 'layer', lid: 'aaaaaaaaaaaa:2', name: 'Paint' });
		const e = { k: 'lines' as const, lid, did, del: [], put: [] };
		expect(s.apply(A, e)).toBe(true);
		expect(s.apply(B, e)).toBe(false);
		// a plain layer's editable strokes too, from its writers only
		expect(s.apply(A, { ...e, lid: 'aaaaaaaaaaaa:2' })).toBe(true);
		expect(s.apply(B, { ...e, lid: 'aaaaaaaaaaaa:2' })).toBe(false);
		s.apply(A, { k: 'layer', lid: 'aaaaaaaaaaaa:3', name: 'Tone', adjust: '{}' });
		expect(s.apply(A, { ...e, lid: 'aaaaaaaaaaaa:3' })).toBe(false);
		expect(s.apply(B, { k: 'pixels', lid })).toBe(false);
		expect(s.apply(A, { k: 'pixels', lid })).toBe(true);
		expect(s.apply(A, e)).toBe(true);
		// kept across a save
		const t = new Shared();
		t.load({ layers: [{ lid, owner: A, name: 'Ink', writers: [A], lines: true }], asks: [] });
		expect(t.layers.get(lid)?.lines).toBe(true);
	});

	it('reads a base with the strokes, and splits big bases into parts', () => {
		const lid = 'aaaaaaaaaaaa:1';
		const did = 'aaaaaaaaaaaa:5';
		expect(parseEdit({ k: 'lines', lid, did, del: [], put: [], base: 'AAAA' })).toEqual({
			k: 'lines',
			lid,
			did,
			del: [],
			put: [],
			base: 'AAAA'
		});
		expect(parseEdit({ k: 'lines', lid, did, del: [], put: [], base: '<x>' })).toBeNull();
		expect(parseEdit({ k: 'lines', lid, did, del: [], put: [], base: 5 })).toBeNull();
		// 40 tiles with paint, 3 clear, 2 gone: parts under the size, the same entries in all
		const keys: [number, number][] = Array.from({ length: 43 }, (_, i) => [i, 1]);
		const tiles = 40;
		const big = new Uint8Array(4 + 43 * 8 + 4 + tiles * (8 + 16384) + 4 + 2 * 8);
		const v = new DataView(big.buffer);
		let o = 0;
		v.setUint32(o, 43, true);
		o += 4;
		for (const [x, y] of keys) {
			v.setInt32(o, x, true);
			v.setInt32(o + 4, y, true);
			o += 8;
		}
		v.setUint32(o, tiles, true);
		o += 4;
		for (let i = 0; i < tiles; i++) {
			v.setInt32(o, i, true);
			v.setInt32(o + 4, 1, true);
			big[o + 8] = i + 1;
			o += 8 + 16384;
		}
		v.setUint32(o, 2, true);
		v.setInt32(o + 4, 99, true);
		v.setInt32(o + 12, 98, true);
		const parts = splitBase(big, 200_000)!;
		expect(parts.length).toBeGreaterThan(3);
		expect(parts.every((p) => p.length <= 200_000)).toBe(true);
		const count = (p: Uint8Array) => new DataView(p.buffer).getUint32(0, true);
		expect(parts.reduce((n, p) => n + count(p), 0)).toBe(43);
		expect(splitBase(big.subarray(0, big.length - 1))).toBeNull();
		expect(splitBase(big)).toHaveLength(1);
		// edits: the first part rides with the strokes, the rest on their own
		const edits = withBase(lineEdits(lid, did, [], [stroke('a', 1)]), lid, did, basePack([[0, 0]]));
		expect(edits).toHaveLength(1);
		expect(edits[0].put).toHaveLength(1);
		expect(edits[0].base).toBeTruthy();
		expect(withBase([], lid, did, basePack([[0, 0]]))).toHaveLength(1);
		expect(withBase([], lid, did, null)).toEqual([]);
	});

	it('splits big changes into several edits that put back the same strokes', () => {
		const many = Array.from({ length: 900 }, (_, i) => stroke(`s${i}`, i));
		const edits = lineEdits('aaaaaaaaaaaa:1', 'aaaaaaaaaaaa:5', [], many);
		expect(edits.length).toBe(3);
		let list: readonly LineStroke[] = [];
		for (const e of edits) list = applyLineEdit(list, e);
		expect(list.map((s) => s.id)).toEqual(many.map((s) => s.id));
		expect(lineEdits('aaaaaaaaaaaa:1', 'aaaaaaaaaaaa:5', many, many)).toEqual([]);
		// a deletion alone is one edit
		const gone = lineEdits('aaaaaaaaaaaa:1', 'aaaaaaaaaaaa:5', many, many.slice(1));
		expect(gone).toEqual([
			{ k: 'lines', lid: 'aaaaaaaaaaaa:1', did: 'aaaaaaaaaaaa:5', del: ['s0'], put: [] }
		]);
	});
});
