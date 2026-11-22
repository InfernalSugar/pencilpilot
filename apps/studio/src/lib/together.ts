/**
 * Drawing together, the drawing side (ADR-0013, S12-T03): shared layers, who may draw on
 * each, and their pixels kept the same in every app.
 *
 * - **One drawer per layer at a time.** A layer has an owner (who made it) and the people
 *   allowed to draw on it ("writers"). Only their changes to it count; everyone else sees
 *   it locked. Asking for access: `ask` → the owner's `grant` (let them draw, or hand it
 *   over), `deny`, or later `revoke`.
 * - **Pixels travel as tiles** (64×64, as in `.ppstudio`): after each change, the drawer's
 *   app sends the tiles of their layers that changed (compressed); the others put them
 *   in. Fills, transforms and imports travel the same way as strokes.
 * - **Layers stack in the order they were made**, which every app sees the same (edits are
 *   numbered by the session), so no app ever reorders anything.
 * - **Joining:** the new member asks the host for the project (a snapshot of the layers,
 *   their access and tiles), then applies the edits numbered after it.
 * - **The edits are a closed list** (below): anything else, AI-made content included, is
 *   dropped on arrival. AI is solo only.
 */

import {
	DRAWING_KINDS,
	MOUTH_SHAPES,
	type Camera,
	type DrawingKind,
	type Marker,
	type MouthChart,
	type MouthShape,
	type Scene,
	type Timeline,
	type Track
} from './timeline';

export const TILE = 64;
export const TILE_BYTES = TILE * TILE * 4;
/** tiles per edit (an edit stays around a megabyte at most) */
const TILES_PER_EDIT = 48;

export interface SharedLayer {
	lid: string;
	owner: string;
	name: string;
	writers: string[];
}

export type TogetherEdit =
	| { k: 'layer'; lid: string; name: string }
	| { k: 'rename'; lid: string; name: string }
	| { k: 'remove'; lid: string }
	| { k: 'move'; lid: string; to: number }
	| {
			k: 'tiles';
			lid: string;
			did: string;
			set: [number, number, string][];
			clear: [number, number][];
	  }
	| { k: 'timeline'; lid: string; tl: SharedTimeline }
	| { k: 'ask'; lid: string }
	| { k: 'grant'; lid: string; to: string; handover: boolean }
	| { k: 'deny'; lid: string; to: string }
	| { k: 'revoke'; lid: string; to: string };

const LID = /^[0-9a-f]{12}:[0-9]{1,12}$/;
const MEMBER = /^[0-9a-f]{64}$/;
const int = (v: unknown) => Number.isInteger(v) && Math.abs(v as number) < 1e6;

/** An edit from the session, checked; null for anything not on the list. */
export function parseEdit(body: unknown): TogetherEdit | null {
	if (!body || typeof body !== 'object') return null;
	const b = body as Record<string, unknown>;
	// the timeline is everyone's (the later one wins); checked as it's put in (fromShared)
	if (b.k === 'timeline')
		return b.tl && typeof b.tl === 'object' && Array.isArray((b.tl as SharedTimeline).scenes)
			? { k: 'timeline', lid: '', tl: b.tl as SharedTimeline }
			: null;
	const lid = typeof b.lid === 'string' && LID.test(b.lid) ? b.lid : null;
	if (!lid) return null;
	const name =
		typeof b.name === 'string'
			? b.name
					.replace(/\p{Cc}/gu, '')
					.trim()
					.slice(0, 60)
			: '';
	const to = typeof b.to === 'string' && MEMBER.test(b.to) ? b.to : null;
	switch (b.k) {
		case 'layer':
			return { k: 'layer', lid, name: name || 'Layer' };
		case 'rename':
			return name ? { k: 'rename', lid, name } : null;
		case 'remove':
			return { k: 'remove', lid };
		case 'move':
			return int(b.to) && (b.to as number) >= 0 ? { k: 'move', lid, to: b.to as number } : null;
		case 'tiles': {
			const did = typeof b.did === 'string' && DID.test(b.did) ? b.did : null;
			if (!did || !Array.isArray(b.set) || !Array.isArray(b.clear)) return null;
			if (b.set.length + b.clear.length > 4 * TILES_PER_EDIT) return null;
			const set = b.set.filter(
				(t): t is [number, number, string] =>
					Array.isArray(t) && int(t[0]) && int(t[1]) && typeof t[2] === 'string'
			);
			const clear = b.clear.filter(
				(t): t is [number, number] => Array.isArray(t) && int(t[0]) && int(t[1])
			);
			return { k: 'tiles', lid, did, set, clear };
		}
		case 'ask':
			return { k: 'ask', lid };
		case 'grant':
			return to ? { k: 'grant', lid, to, handover: b.handover === true } : null;
		case 'deny':
			return to ? { k: 'deny', lid, to } : null;
		case 'revoke':
			return to ? { k: 'revoke', lid, to } : null;
		default:
			return null;
	}
}

/** The shared layers and who may draw on them, the same in every app. */
export class Shared {
	layers = new Map<string, SharedLayer>();
	/** bottom → top: the order they were made */
	order: string[] = [];
	/** asks waiting for the owner */
	asks: { lid: string; from: string }[] = [];

	canDraw(lid: string, who: string): boolean {
		return this.layers.get(lid)?.writers.includes(who) ?? false;
	}

	/** Take an edit in; false when it isn't allowed (and changes nothing). */
	apply(from: string, e: TogetherEdit): boolean {
		const l = this.layers.get(e.lid);
		switch (e.k) {
			case 'timeline':
				return true;
			case 'layer':
				// only the member whose id starts the layer's id can make it, once
				if (l || !e.lid.startsWith(from.slice(0, 12) + ':')) return false;
				this.layers.set(e.lid, { lid: e.lid, owner: from, name: e.name, writers: [from] });
				this.order.push(e.lid);
				return true;
			case 'remove':
				if (!l || l.owner !== from) return false;
				this.layers.delete(e.lid);
				this.order = this.order.filter((x) => x !== e.lid);
				this.asks = this.asks.filter((a) => a.lid !== e.lid);
				return true;
			case 'move': {
				if (!l || l.owner !== from) return false;
				const rest = this.order.filter((x) => x !== e.lid);
				rest.splice(Math.min(e.to, rest.length), 0, e.lid);
				this.order = rest;
				return true;
			}
			case 'rename':
			case 'tiles':
				if (!l || !l.writers.includes(from)) return false;
				if (e.k === 'rename') l.name = e.name;
				return true;
			case 'ask':
				if (!l || l.writers.includes(from)) return false;
				if (!this.asks.some((a) => a.lid === e.lid && a.from === from))
					this.asks.push({ lid: e.lid, from });
				return true;
			case 'grant':
				if (!l || l.owner !== from) return false;
				this.asks = this.asks.filter((a) => !(a.lid === e.lid && a.from === e.to));
				if (e.handover) {
					l.owner = e.to;
					l.writers = [e.to, ...l.writers.filter((w) => w !== e.to && w !== from)];
				} else if (!l.writers.includes(e.to)) l.writers.push(e.to);
				return true;
			case 'deny':
				if (!l || l.owner !== from) return false;
				this.asks = this.asks.filter((a) => !(a.lid === e.lid && a.from === e.to));
				return true;
			case 'revoke':
				if (!l || l.owner !== from || e.to === l.owner) return false;
				l.writers = l.writers.filter((w) => w !== e.to);
				return true;
		}
	}

	snapshot() {
		return {
			layers: this.order.map((lid) => structuredClone(this.layers.get(lid)!)),
			asks: structuredClone(this.asks)
		};
	}

	load(s: { layers: SharedLayer[]; asks: { lid: string; from: string }[] }) {
		this.layers.clear();
		this.order = [];
		for (const l of s.layers) {
			if (!LID.test(l.lid) || this.layers.has(l.lid)) continue;
			this.layers.set(l.lid, {
				lid: l.lid,
				owner: String(l.owner),
				name: String(l.name).slice(0, 60),
				writers: l.writers.map(String)
			});
			this.order.push(l.lid);
		}
		this.asks = s.asks.filter((a) => this.layers.has(a.lid));
	}
}

// ---- tiles ----

/** A drawing's tiles: "x,y" (tile coordinates) → 64×64 straight RGBA8. */
export type TileMap = Map<string, Uint8Array>;

/** From the engine's packed form: u32 count; per tile i32 x, i32 y, 64×64 RGBA8 (LE). */
export function unpack(packed: Uint8Array): TileMap {
	const m: TileMap = new Map();
	if (packed.length < 4) return m;
	const dv = new DataView(packed.buffer, packed.byteOffset, packed.byteLength);
	const n = dv.getUint32(0, true);
	for (let i = 0; i < n; i++) {
		const at = 4 + i * (8 + TILE_BYTES);
		if (at + 8 + TILE_BYTES > packed.length) break;
		const x = dv.getInt32(at, true);
		const y = dv.getInt32(at + 4, true);
		m.set(`${x},${y}`, packed.slice(at + 8, at + 8 + TILE_BYTES));
	}
	return m;
}

export function pack(m: TileMap): Uint8Array {
	const out = new Uint8Array(4 + m.size * (8 + TILE_BYTES));
	const dv = new DataView(out.buffer);
	dv.setUint32(0, m.size, true);
	let at = 4;
	for (const [k, px] of m) {
		const [x, y] = k.split(',').map(Number);
		dv.setInt32(at, x, true);
		dv.setInt32(at + 4, y, true);
		out.set(px, at + 8);
		at += 8 + TILE_BYTES;
	}
	return out;
}

function same(a: Uint8Array, b: Uint8Array): boolean {
	if (a.length !== b.length) return false;
	for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
	return true;
}

/** What changed from `prev` to `next`: tiles to set, tiles to clear. */
export function diffTiles(prev: TileMap, next: TileMap) {
	const set: [number, number, Uint8Array][] = [];
	const clear: [number, number][] = [];
	for (const [k, px] of next) {
		const old = prev.get(k);
		if (!old || !same(old, px)) {
			const [x, y] = k.split(',').map(Number);
			set.push([x, y, px]);
		}
	}
	for (const k of prev.keys())
		if (!next.has(k)) {
			const [x, y] = k.split(',').map(Number);
			clear.push([x, y]);
		}
	return { set, clear };
}

function toBase64(b: Uint8Array): string {
	let s = '';
	for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000));
	return btoa(s);
}

function fromBase64(s: string): Uint8Array {
	const bin = atob(s);
	const out = new Uint8Array(bin.length);
	for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
	return out;
}

async function through(b: Uint8Array, t: CompressionStream | DecompressionStream) {
	const stream = new Blob([b as BlobPart]).stream().pipeThrough(t);
	return new Uint8Array(await new Response(stream).arrayBuffer());
}

/** A tile for the wire: deflated where the browser can ("z:"), else as is ("r:"). */
export async function encodeTile(px: Uint8Array): Promise<string> {
	if (typeof CompressionStream !== 'undefined')
		return 'z:' + toBase64(await through(px, new CompressionStream('deflate-raw')));
	return 'r:' + toBase64(px);
}

export async function decodeTile(s: string): Promise<Uint8Array | null> {
	try {
		const raw =
			s.startsWith('z:') && typeof DecompressionStream !== 'undefined'
				? await through(fromBase64(s.slice(2)), new DecompressionStream('deflate-raw'))
				: s.startsWith('r:')
					? fromBase64(s.slice(2))
					: null;
		return raw && raw.length === TILE_BYTES ? raw : null;
	} catch {
		return null;
	}
}

// ---- the app's side ----

/** What the sync needs from the app (silent: none of it is an undo step). */
export interface CanvasPort {
	/** a new layer on top; its engine id (0: couldn't) */
	addLayer(name: string): number;
	setLayer(id: number, patch: { name?: string; locked?: boolean; visible?: boolean }): void;
	/** the drawing the layer shows now (made if it has none) */
	drawingOf(id: number): number;
	/** a new, empty drawing's id */
	newDrawing(): number;
	/** the layer a drawing belongs to (showing, or on the timeline); null if none */
	layerOfDrawing(drawing: number): number | null;
	/** a drawing's tiles (fetched back from the frame store if need be) */
	packed(drawing: number): Promise<Uint8Array>;
	/** false when the canvas is busy (mid-stroke): try again later */
	put(drawing: number, packed: Uint8Array): boolean;
	select(id: number): void;
	/** remove a layer (false: busy, try later); clears undo */
	removeLayer(id: number): boolean;
	/** move a layer right above / below another (false: busy, try later); clears undo */
	moveNextTo(id: number, ref: number, above: boolean): boolean;
	/** paint layers, bottom → top */
	layers(): { id: number; name: string }[];
	/** the app's timeline now */
	timeline(): Timeline;
	/** put the session's timeline in (not an undo step; shows the frame again) */
	setTimeline(tl: Timeline): void;
	/** every drawing on a layer, on the timeline (and the one showing) */
	drawingsOfLayer(id: number): number[];
}

export interface NetPort {
	edit(body: unknown): void;
	direct(to: string, body: unknown): void;
}

type Snap =
	| { k: 'snap.req' }
	| {
			k: 'snap.begin';
			seq: number;
			layers: SharedLayer[];
			asks: { lid: string; from: string }[];
			tl: SharedTimeline | null;
	  }
	| { k: 'snap.tiles'; lid: string; did: string; set: [number, number, string][] }
	| { k: 'snap.end' };

const key = (lid: string, did: string) => `${lid}|${did}`;

/**
 * One app's side of drawing together: turns the session's edits into canvas and
 * timeline changes, and this app's changes into edits.
 */
export class Together {
	shared = new Shared();
	/** shared layer → the engine's layer here */
	ids = new Map<string, number>();
	/** each shared drawing's tiles as last synced ("lid|did" → tiles) */
	cache = new Map<string, TileMap>();
	/** shared drawing ids ↔ this app's */
	#dids = new Map<string, number>();
	#didOf = new Map<number, string>();
	/** drawings whose tiles couldn't go in yet (the canvas was busy): "lid|did" */
	#pending = new Set<string>();
	/** layer removals and moves that couldn't happen yet (the canvas was busy) */
	#structure: { f: () => boolean; tries: number }[] = [];
	/** this app's layers that aren't shared (from before joining, live previews) */
	#local = new Set<number>();
	/** drawings changed here that may not be showing (an undo on another frame) */
	#touched = new Set<number>();
	#synced = false;
	#buffer: { seq: number; from: string; body: unknown }[] = [];
	#snapSeq = 0;
	#snapTl: SharedTimeline | null = null;
	#lastSeq = 0;
	#n = Math.floor(Math.random() * 1e8);
	#sending = false;
	#again = false;
	/** a layer made here, to select when the session numbers it */
	#mine = new Set<string>();
	/** someone's tiles arrived (their live stroke previews can go) */
	ontiles: (from: string) => void = () => {};

	constructor(
		readonly me: string,
		readonly myName: string,
		readonly canvas: CanvasPort,
		readonly net: NetPort,
		readonly onchange: () => void = () => {}
	) {}

	get synced() {
		return this.#synced;
	}

	#newLid() {
		return `${this.me.slice(0, 12)}:${++this.#n}`;
	}

	/** A drawing's shared id (given one the first time it's shared). */
	didOf(drawing: number): string {
		let did = this.#didOf.get(drawing);
		if (!did) {
			did = `${this.me.slice(0, 12)}:${drawing}`;
			this.#didOf.set(drawing, did);
			this.#dids.set(did, drawing);
		}
		return did;
	}

	/** A shared drawing's id here (a new, empty one the first time). */
	localOf(did: string): number {
		let d = this.#dids.get(did);
		if (!d) {
			d = this.canvas.newDrawing();
			this.#dids.set(did, d);
			this.#didOf.set(d, did);
		}
		return d;
	}

	#sharedTimeline(): SharedTimeline {
		return toShared(
			this.canvas.timeline(),
			(l) => this.lidOf(l),
			(d) => this.didOf(d)
		);
	}

	/** Hosting: share what's on the canvas (every layer, every frame, the timeline). */
	async startHost() {
		for (const l of this.canvas.layers()) {
			const lid = this.#newLid();
			this.ids.set(lid, l.id);
			this.net.edit({ k: 'layer', lid, name: l.name });
		}
		for (const [lid, id] of this.ids)
			for (const d of new Set([this.canvas.drawingOf(id), ...this.canvas.drawingsOfLayer(id)])) {
				const tiles = unpack(await this.canvas.packed(d));
				const did = this.didOf(d);
				this.cache.set(key(lid, did), tiles);
				const set: [number, number, Uint8Array][] = [...tiles].map(([k, px]) => {
					const [x, y] = k.split(',').map(Number);
					return [x, y, px];
				});
				if (set.length) await this.#sendTiles(lid, did, set, []);
			}
		this.net.edit({ k: 'timeline', tl: this.#sharedTimeline() });
		this.#synced = true;
	}

	/** Joining: ask the host for the project; this app's own layers are put aside. */
	startJoin(host: string) {
		for (const l of this.canvas.layers()) {
			this.#local.add(l.id);
			this.canvas.setLayer(l.id, { visible: false, locked: true });
		}
		this.net.direct(host, { k: 'snap.req' });
	}

	/** A layer of this app's own that isn't shared (e.g. live stroke previews). */
	exclude(id: number) {
		this.#local.add(id);
	}

	/** A new layer of this app's own (on top, for everyone). */
	addOwnLayer(name = '') {
		const lid = this.#newLid();
		this.#mine.add(lid);
		this.net.edit({ k: 'layer', lid, name: name || `${this.myName}’s layer` });
	}

	/** Delete a shared layer of this app's own, for everyone. */
	removeOwn(lid: string) {
		if (this.shared.layers.get(lid)?.owner === this.me) this.net.edit({ k: 'remove', lid });
	}
	/** Move a shared layer of this app's own to `to` in the shared order (0 = bottom). */
	moveOwn(lid: string, to: number) {
		if (this.shared.layers.get(lid)?.owner === this.me) this.net.edit({ k: 'move', lid, to });
	}

	ask(lid: string) {
		this.net.edit({ k: 'ask', lid });
	}
	grant(lid: string, to: string, handover = false) {
		this.net.edit({ k: 'grant', lid, to, handover });
	}
	deny(lid: string, to: string) {
		this.net.edit({ k: 'deny', lid, to });
	}
	revoke(lid: string, to: string) {
		this.net.edit({ k: 'revoke', lid, to });
	}

	/** The timeline changed here (a step of this app's): everyone gets it. */
	timelineChanged() {
		if (this.#synced) this.net.edit({ k: 'timeline', tl: this.#sharedTimeline() });
	}

	/** These drawings may have changed here (e.g. an undo on another frame). */
	touch(drawings: number[]) {
		for (const d of drawings) if (d) this.#touched.add(d);
	}

	/** The shared layer an engine layer is, if any. */
	lidOf(id: number): string | null {
		for (const [lid, i] of this.ids) if (i === id) return lid;
		return null;
	}

	/** A numbered edit from the session. */
	async onEdit(seq: number, from: string, body: unknown) {
		if (!this.#synced) {
			this.#buffer.push({ seq, from, body });
			return;
		}
		this.#lastSeq = Math.max(this.#lastSeq, seq);
		const e = parseEdit(body);
		if (!e || !this.shared.apply(from, e)) return;
		switch (e.k) {
			case 'layer':
				if (!this.ids.has(e.lid)) this.#makeLayer(e.lid);
				else this.#refreshLock(e.lid);
				if (this.#mine.delete(e.lid)) this.canvas.select(this.ids.get(e.lid)!);
				break;
			case 'rename': {
				const id = this.ids.get(e.lid);
				if (id) this.canvas.setLayer(id, { name: e.name });
				break;
			}
			case 'remove': {
				const id = this.ids.get(e.lid);
				this.ids.delete(e.lid);
				for (const k of [...this.cache.keys()])
					if (k.startsWith(e.lid + '|')) {
						this.cache.delete(k);
						this.#pending.delete(k);
					}
				if (id) this.#later(() => this.canvas.removeLayer(id));
				break;
			}
			case 'move':
				this.#place(e.lid);
				break;
			case 'tiles':
				// this app's own changes are on its canvas already
				if (from !== this.me) {
					await this.#takeTiles(e.lid, e.did, e.set, e.clear);
					this.ontiles(from);
				}
				break;
			case 'timeline':
				// everyone's, this app's too, in the session's order: all end the same
				this.canvas.setTimeline(this.#fromShared(e.tl));
				break;
			case 'grant':
			case 'revoke':
				this.#refreshLock(e.lid);
				break;
		}
		this.onchange();
	}

	#fromShared(tl: SharedTimeline): Timeline {
		const known = new Set(this.ids.values());
		return fromShared(
			tl,
			(lid) => this.ids.get(lid),
			(did) => this.localOf(did),
			this.canvas.timeline(),
			(l) => known.has(l)
		);
	}

	/** A message for this app alone (the project, when joining). */
	async onDirect(from: string, body: unknown) {
		const m = body as Snap;
		if (!m || typeof m !== 'object') return;
		if (m.k === 'snap.req' && this.#synced) {
			await this.#sendSnapshot(from);
		} else if (m.k === 'snap.begin' && !this.#synced) {
			this.shared.load(m);
			this.#snapSeq = m.seq;
			this.#snapTl = m.tl && typeof m.tl === 'object' ? m.tl : null;
		} else if (m.k === 'snap.tiles' && !this.#synced) {
			if (!this.shared.layers.has(m.lid) || typeof m.did !== 'string' || !DID.test(m.did)) return;
			const t = this.cache.get(key(m.lid, m.did)) ?? new Map();
			this.cache.set(key(m.lid, m.did), t);
			for (const [x, y, s] of m.set) {
				const px = await decodeTile(s);
				if (px) t.set(`${x},${y}`, px);
			}
		} else if (m.k === 'snap.end' && !this.#synced) {
			for (const lid of this.shared.order) this.#makeLayer(lid);
			for (const k of this.cache.keys()) this.#put(k);
			if (this.#snapTl) this.canvas.setTimeline(this.#fromShared(this.#snapTl));
			this.#synced = true;
			const later = this.#buffer.filter((b) => b.seq > this.#snapSeq);
			this.#buffer = [];
			for (const b of later) await this.onEdit(b.seq, b.from, b.body);
			this.addOwnLayer();
			this.onchange();
		}
	}

	/** The canvas changed here: send this app's changed tiles (and share new layers). */
	async localChanged() {
		if (!this.#synced) return;
		if (this.#sending) {
			this.#again = true;
			return;
		}
		this.#sending = true;
		try {
			do {
				this.#again = false;
				// (one that keeps failing, e.g. the last layer can't go, is given up after a while)
				while (this.#structure.length) {
					const op = this.#structure[0];
					if (!op.f() && ++op.tries < 50) break;
					this.#structure.shift();
				}
				for (const k of [...this.#pending]) this.#put(k);
				// a layer made here outside the session's way (duplicate, paste, import)
				const known = new Set(this.ids.values());
				for (const l of this.canvas.layers())
					if (!known.has(l.id) && !this.#local.has(l.id)) {
						const lid = this.#newLid();
						this.ids.set(lid, l.id);
						this.net.edit({ k: 'layer', lid, name: l.name });
					}
				// what may have changed: each drawable layer's drawing showing, and drawings
				// touched elsewhere (an undo on another frame)
				const look = new Map<number, number>();
				for (const [, id] of this.ids) look.set(this.canvas.drawingOf(id), id);
				for (const d of this.#touched) {
					const id = this.canvas.layerOfDrawing(d);
					if (id !== null && this.lidOf(id)) look.set(d, id);
				}
				this.#touched.clear();
				for (const [d, id] of look) {
					const lid = this.lidOf(id)!;
					// own layers, before the session has numbered them, count as drawable
					const mine =
						this.shared.canDraw(lid, this.me) ||
						(!this.shared.layers.has(lid) && lid.startsWith(this.me.slice(0, 12) + ':'));
					if (!mine || !d) continue;
					const did = this.didOf(d);
					const next = unpack(await this.canvas.packed(d));
					const prev = this.cache.get(key(lid, did)) ?? new Map();
					const diff = diffTiles(prev, next);
					if (!diff.set.length && !diff.clear.length) continue;
					this.cache.set(key(lid, did), next);
					await this.#sendTiles(lid, did, diff.set, diff.clear);
				}
			} while (this.#again);
		} finally {
			this.#sending = false;
		}
	}

	async #sendTiles(
		lid: string,
		did: string,
		set: [number, number, Uint8Array][],
		clear: [number, number][]
	) {
		for (let i = 0; i < Math.max(set.length, 1); i += TILES_PER_EDIT) {
			const part = set.slice(i, i + TILES_PER_EDIT);
			const enc: [number, number, string][] = [];
			for (const [x, y, px] of part) enc.push([x, y, await encodeTile(px)]);
			this.net.edit({ k: 'tiles', lid, did, set: enc, clear: i === 0 ? clear : [] });
		}
	}

	async #sendSnapshot(to: string) {
		const s = this.shared.snapshot();
		this.net.direct(to, {
			k: 'snap.begin',
			seq: this.#lastSeq,
			...s,
			tl: this.#sharedTimeline()
		});
		for (const [k, t] of this.cache) {
			const [lid, did] = k.split('|');
			if (!this.shared.layers.has(lid)) continue;
			const tiles = [...t];
			for (let i = 0; i < Math.max(tiles.length, 1); i += TILES_PER_EDIT) {
				const set: [number, number, string][] = [];
				for (const [tk, px] of tiles.slice(i, i + TILES_PER_EDIT)) {
					const [x, y] = tk.split(',').map(Number);
					set.push([x, y, await encodeTile(px)]);
				}
				this.net.direct(to, { k: 'snap.tiles', lid, did, set });
			}
		}
		this.net.direct(to, { k: 'snap.end' });
	}

	async #takeTiles(
		lid: string,
		did: string,
		set: [number, number, string][],
		clear: [number, number][]
	) {
		const d = this.localOf(did);
		// drawing on it too: send this app's own changes first, so putting theirs in keeps them
		const id = this.ids.get(lid);
		if (id && this.shared.canDraw(lid, this.me)) {
			const next = unpack(await this.canvas.packed(d));
			const diff = diffTiles(this.cache.get(key(lid, did)) ?? new Map(), next);
			if (diff.set.length || diff.clear.length) {
				this.cache.set(key(lid, did), next);
				await this.#sendTiles(lid, did, diff.set, diff.clear);
			}
		}
		const t = this.cache.get(key(lid, did)) ?? new Map();
		this.cache.set(key(lid, did), t);
		for (const [x, y] of clear) t.delete(`${x},${y}`);
		for (const [x, y, s] of set) {
			const px = await decodeTile(s);
			if (px) t.set(`${x},${y}`, px);
		}
		this.#put(key(lid, did));
	}

	#makeLayer(lid: string) {
		if (this.ids.has(lid)) return;
		const l = this.shared.layers.get(lid);
		if (!l) return;
		const id = this.canvas.addLayer(l.name);
		if (!id) return;
		this.ids.set(lid, id);
		this.#refreshLock(lid);
	}

	/** Put a shared layer next to its neighbour in the shared order. */
	#place(lid: string) {
		const id = this.ids.get(lid);
		const i = this.shared.order.indexOf(lid);
		if (!id || i < 0) return;
		const below = this.ids.get(this.shared.order[i - 1]);
		const above = this.ids.get(this.shared.order[i + 1]);
		if (below) this.#later(() => this.canvas.moveNextTo(id, below, true));
		else if (above) this.#later(() => this.canvas.moveNextTo(id, above, false));
	}

	/** Do it now, or (the canvas busy mid-stroke) after the next change. */
	#later(f: () => boolean) {
		if (this.#structure.length || !f()) this.#structure.push({ f, tries: 0 });
	}

	#refreshLock(lid: string) {
		const id = this.ids.get(lid);
		if (id) this.canvas.setLayer(id, { locked: !this.shared.canDraw(lid, this.me) });
	}

	/** Put a shared drawing's synced tiles into the engine ("lid|did"). */
	#put(k: string) {
		const t = this.cache.get(k);
		if (!t) return;
		const d = this.localOf(k.split('|')[1]);
		if (this.canvas.put(d, pack(t))) this.#pending.delete(k);
		else this.#pending.add(k);
	}
}

// ---- the timeline, shared ----

/**
 * The timeline as it travels: layers by their shared id (lid), drawings by theirs
 * ("did", "<member>:<n>"; "" = an empty cel). Sound clips stay in each app (the sound
 * files don't travel yet).
 */
export interface SharedTimeline {
	scenes: {
		name: string;
		markers?: Marker[];
		cuts: {
			name: string;
			camera?: Camera;
			action?: Record<number, string>;
			dialogue?: Record<number, string>;
			transition?: 'flash' | 'dissolve';
			tracks: Record<string, { d: string; n: number }[]>;
		}[];
	}[];
	statics: string[];
	mouths: { lid: string; shapes: Record<string, string> }[];
	kinds: Record<string, DrawingKind>;
}

const DID = /^[0-9a-f]{12}:[0-9]{1,12}$/;

/** This app's timeline, for the session (only shared layers). */
export function toShared(
	tl: Timeline,
	lidOf: (layer: number) => string | null,
	didOf: (drawing: number) => string
): SharedTimeline {
	const did = (d: number) => (d ? didOf(d) : '');
	return {
		scenes: tl.scenes.map((s) => ({
			name: s.name,
			...(s.markers?.length ? { markers: s.markers.map((m) => ({ ...m })) } : {}),
			cuts: s.cuts.map((c) => ({
				name: c.name,
				...(c.camera ? { camera: { ...c.camera } } : {}),
				...(c.action ? { action: { ...c.action } } : {}),
				...(c.dialogue ? { dialogue: { ...c.dialogue } } : {}),
				...(c.transition ? { transition: c.transition } : {}),
				tracks: Object.fromEntries(
					Object.entries(c.tracks).flatMap(([l, t]) => {
						const lid = lidOf(+l);
						return lid ? [[lid, t.map((e) => ({ d: did(e.drawing), n: e.length }))]] : [];
					})
				)
			}))
		})),
		statics: tl.statics.flatMap((l) => {
			const lid = lidOf(l);
			return lid ? [lid] : [];
		}),
		mouths: (tl.mouths ?? []).flatMap((m) => {
			const lid = lidOf(m.layer);
			if (!lid) return [];
			const shapes: Record<string, string> = {};
			for (const [k, d] of Object.entries(m.shapes)) if (d) shapes[k] = did(d);
			return [{ lid, shapes }];
		}),
		kinds: Object.fromEntries(
			Object.entries(tl.kinds ?? {}).flatMap(([d, k]) => (+d ? [[did(+d), k]] : []))
		)
	};
}

/**
 * A timeline from the session, as this app's: `idOf` gives a shared layer's id here,
 * `localOf` a drawing's (making one when it's new). `local` is this app's timeline now:
 * its sound clips and its unshared layers' tracks stay.
 */
export function fromShared(
	s: SharedTimeline,
	idOf: (lid: string) => number | undefined,
	localOf: (did: string) => number,
	local: Timeline,
	isShared: (layer: number) => boolean
): Timeline {
	const drawing = (d: unknown) => (typeof d === 'string' && DID.test(d) ? localOf(d) : 0);
	const notes = (n: unknown) => {
		const out: Record<number, string> = {};
		if (n && typeof n === 'object')
			for (const [f, text] of Object.entries(n))
				if (Number.isInteger(+f) && +f >= 0 && typeof text === 'string' && text)
					out[+f] = text.slice(0, 500);
		return Object.keys(out).length ? out : undefined;
	};
	const scenes: Scene[] = (Array.isArray(s.scenes) ? s.scenes : []).slice(0, 200).map((sc, si) => {
		const mine = local.scenes[si];
		return {
			id: si + 1,
			name: String(sc?.name ?? `Scene ${si + 1}`).slice(0, 100),
			...(mine?.audio?.length ? { audio: mine.audio } : {}),
			...(Array.isArray(sc?.markers) && sc.markers.length
				? {
						markers: sc.markers
							.filter((m) => Number.isInteger(m?.frame) && m.frame >= 0)
							.map((m) => ({ frame: m.frame, label: String(m.label ?? '').slice(0, 100) }))
					}
				: {}),
			cuts: (Array.isArray(sc?.cuts) ? sc.cuts : []).slice(0, 500).map((c, ci) => {
				const tracks: Record<number, Track> = {};
				for (const [lid, t] of Object.entries(c?.tracks ?? {})) {
					const id = idOf(lid);
					if (id === undefined || !Array.isArray(t)) continue;
					tracks[id] = t.slice(0, 5000).map((e) => ({
						drawing: drawing(e?.d),
						length: Number.isInteger(e?.n) ? Math.min(999, Math.max(1, e.n)) : 1
					}));
				}
				// this app's own, unshared layers keep their tracks
				for (const [l, t] of Object.entries(mine?.cuts[ci]?.tracks ?? {}))
					if (!isShared(+l)) tracks[+l] = t;
				const cam = c?.camera;
				const action = notes(c?.action);
				const dialogue = notes(c?.dialogue);
				return {
					id: ci + 1,
					name: String(c?.name ?? `Cut ${ci + 1}`).slice(0, 100),
					...(cam &&
					[cam.x, cam.y, cam.width, cam.height].every(Number.isFinite) &&
					cam.width > 0 &&
					cam.height > 0
						? { camera: { x: cam.x, y: cam.y, width: cam.width, height: cam.height } }
						: {}),
					...(action ? { action } : {}),
					...(dialogue ? { dialogue } : {}),
					...(c?.transition === 'flash' || c?.transition === 'dissolve'
						? { transition: c.transition }
						: {}),
					tracks
				};
			})
		};
	});
	const statics = (Array.isArray(s.statics) ? s.statics : []).flatMap((lid) => {
		const id = idOf(lid);
		return id === undefined ? [] : [id];
	});
	for (const l of local.statics) if (!isShared(l) && !statics.includes(l)) statics.push(l);
	const mouths: MouthChart[] = (Array.isArray(s.mouths) ? s.mouths : []).flatMap((m) => {
		const layer = idOf(m?.lid);
		if (layer === undefined) return [];
		const shapes: MouthChart['shapes'] = {};
		for (const [k, d] of Object.entries(m.shapes ?? {}))
			if ((MOUTH_SHAPES as readonly string[]).includes(k) && drawing(d))
				shapes[k as MouthShape] = drawing(d);
		return [{ layer, shapes }];
	});
	const kinds: Record<number, DrawingKind> = {};
	for (const [d, k] of Object.entries(s.kinds ?? {}))
		if ((DRAWING_KINDS as string[]).includes(k) && drawing(d)) kinds[drawing(d)] = k;
	return {
		scenes: scenes.length ? scenes : local.scenes,
		statics,
		...(mouths.length ? { mouths } : {}),
		...(Object.keys(kinds).length ? { kinds } : {})
	};
}
