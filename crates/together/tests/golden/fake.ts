/**
 * together.test.ts's fake canvas, as it is there (the Rust tests' `FakeCanvas` does the
 * same), for recording what the studio's code sends and ends with.
 */
import type { LayerStyle } from '../../../../packages/paint/src/layer-style.ts';
import type { LineStroke } from '../../../../packages/paint/src/lines.ts';
import { newTimeline, type Timeline } from '../../../../apps/studio/src/lib/timeline.ts';
import {
	pack,
	TILE_BYTES,
	unpack,
	type CanvasPort,
	type TileMap
} from '../../../../apps/studio/src/lib/together.ts';

export function tile(v: number) {
	return new Uint8Array(TILE_BYTES).fill(v);
}

export class FakeCanvas implements CanvasPort {
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
	setLayer(id: number, patch: Record<string, unknown>) {
		Object.assign(this.layerList.find((l) => l.id === id)!, patch);
	}
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
	setTimeline(tl: Timeline) {
		this.tl = tl;
		this.showFrame(0);
	}
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
	paint(id: number, x: number, y: number, v: number) {
		const l = this.layerList.find((l) => l.id === id)!;
		if (l.locked) return false;
		this.drawings.get(this.drawingOf(id))!.set(`${x},${y}`, tile(v));
		return true;
	}
	byName(name: string) {
		return this.layerList.find((l) => l.name === name)!;
	}
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
