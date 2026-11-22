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
	| { k: 'tiles'; lid: string; set: [number, number, string][]; clear: [number, number][] }
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
			if (!Array.isArray(b.set) || !Array.isArray(b.clear)) return null;
			if (b.set.length + b.clear.length > 4 * TILES_PER_EDIT) return null;
			const set = b.set.filter(
				(t): t is [number, number, string] =>
					Array.isArray(t) && int(t[0]) && int(t[1]) && typeof t[2] === 'string'
			);
			const clear = b.clear.filter(
				(t): t is [number, number] => Array.isArray(t) && int(t[0]) && int(t[1])
			);
			return { k: 'tiles', lid, set, clear };
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

/** What the sync needs from the canvas (silent: none of it is an undo step). */
export interface CanvasPort {
	/** a new layer on top; its engine id (0: couldn't) */
	addLayer(name: string): number;
	setLayer(id: number, patch: { name?: string; locked?: boolean; visible?: boolean }): void;
	/** the drawing the layer shows (made if it has none) */
	drawingOf(id: number): number;
	packed(drawing: number): Uint8Array;
	/** false when the canvas is busy (mid-stroke): try again later */
	put(drawing: number, packed: Uint8Array): boolean;
	select(id: number): void;
	/** remove a layer (false: busy, try later); clears undo */
	removeLayer(id: number): boolean;
	/** move a layer right above / below another (false: busy, try later); clears undo */
	moveNextTo(id: number, ref: number, above: boolean): boolean;
	/** paint layers, bottom → top */
	layers(): { id: number; name: string }[];
}

export interface NetPort {
	edit(body: unknown): void;
	direct(to: string, body: unknown): void;
}

type Snap =
	| { k: 'snap.req' }
	| { k: 'snap.begin'; seq: number; layers: SharedLayer[]; asks: { lid: string; from: string }[] }
	| { k: 'snap.tiles'; lid: string; set: [number, number, string][] }
	| { k: 'snap.end' };

/**
 * One app's side of drawing together: turns the session's edits into canvas changes and
 * the canvas's changes into edits.
 */
export class Together {
	shared = new Shared();
	/** shared layer → the engine's layer here */
	ids = new Map<string, number>();
	/** each shared layer's tiles as last synced */
	cache = new Map<string, TileMap>();
	/** layers whose tiles couldn't go in yet (the canvas was busy) */
	#pending = new Set<string>();
	/** layer removals and moves that couldn't happen yet (the canvas was busy) */
	#structure: { f: () => boolean; tries: number }[] = [];
	/** this app's layers from before joining: hidden, not shared */
	#local = new Set<number>();
	#synced = false;
	#buffer: { seq: number; from: string; body: unknown }[] = [];
	#snapSeq = 0;
	#lastSeq = 0;
	#n = Math.floor(Math.random() * 1e8);
	#sending = false;
	#again = false;
	/** a layer made here, to select when the session numbers it */
	#mine = new Set<string>();

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

	/** Hosting: share what's on the canvas (every layer, owned by this app). */
	startHost() {
		for (const l of this.canvas.layers()) {
			const lid = this.#newLid();
			this.ids.set(lid, l.id);
			this.net.edit({ k: 'layer', lid, name: l.name });
		}
		this.#synced = true;
		void this.localChanged();
	}

	/** Joining: ask the host for the project; this app's own layers are put aside. */
	startJoin(host: string) {
		for (const l of this.canvas.layers()) {
			this.#local.add(l.id);
			this.canvas.setLayer(l.id, { visible: false, locked: true });
		}
		this.net.direct(host, { k: 'snap.req' });
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
				this.cache.delete(e.lid);
				this.#pending.delete(e.lid);
				if (id) this.#later(() => this.canvas.removeLayer(id));
				break;
			}
			case 'move':
				this.#place(e.lid);
				break;
			case 'tiles':
				// this app's own changes are on its canvas already
				if (from !== this.me) await this.#takeTiles(e.lid, e.set, e.clear);
				break;
			case 'grant':
			case 'revoke':
				this.#refreshLock(e.lid);
				break;
		}
		this.onchange();
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
			for (const lid of this.shared.order) this.cache.set(lid, new Map());
		} else if (m.k === 'snap.tiles' && !this.#synced) {
			const t = this.cache.get(m.lid);
			if (!t) return;
			for (const [x, y, s] of m.set) {
				const px = await decodeTile(s);
				if (px) t.set(`${x},${y}`, px);
			}
		} else if (m.k === 'snap.end' && !this.#synced) {
			for (const lid of this.shared.order) {
				this.#makeLayer(lid);
				this.#put(lid);
			}
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
				for (const lid of [...this.#pending]) this.#put(lid);
				// a layer made here outside the session's way (duplicate, paste, import)
				const known = new Set(this.ids.values());
				for (const l of this.canvas.layers())
					if (!known.has(l.id) && !this.#local.has(l.id)) {
						const lid = this.#newLid();
						this.ids.set(lid, l.id);
						this.net.edit({ k: 'layer', lid, name: l.name });
					}
				for (const [lid, id] of this.ids) {
					// own layers, before the session has numbered them, count as drawable
					const mine =
						this.shared.canDraw(lid, this.me) ||
						(!this.shared.layers.has(lid) && lid.startsWith(this.me.slice(0, 12) + ':'));
					if (!mine) continue;
					const next = unpack(this.canvas.packed(this.canvas.drawingOf(id)));
					const prev = this.cache.get(lid) ?? new Map();
					const d = diffTiles(prev, next);
					if (!d.set.length && !d.clear.length) continue;
					this.cache.set(lid, next);
					await this.#sendTiles(lid, d.set, d.clear);
				}
			} while (this.#again);
		} finally {
			this.#sending = false;
		}
	}

	async #sendTiles(lid: string, set: [number, number, Uint8Array][], clear: [number, number][]) {
		for (let i = 0; i < Math.max(set.length, 1); i += TILES_PER_EDIT) {
			const part = set.slice(i, i + TILES_PER_EDIT);
			const enc: [number, number, string][] = [];
			for (const [x, y, px] of part) enc.push([x, y, await encodeTile(px)]);
			this.net.edit({ k: 'tiles', lid, set: enc, clear: i === 0 ? clear : [] });
		}
	}

	async #sendSnapshot(to: string) {
		const s = this.shared.snapshot();
		this.net.direct(to, { k: 'snap.begin', seq: this.#lastSeq, ...s });
		for (const lid of this.shared.order) {
			const tiles = [...(this.cache.get(lid) ?? new Map<string, Uint8Array>())];
			for (let i = 0; i < tiles.length; i += TILES_PER_EDIT) {
				const set: [number, number, string][] = [];
				for (const [k, px] of tiles.slice(i, i + TILES_PER_EDIT)) {
					const [x, y] = k.split(',').map(Number);
					set.push([x, y, await encodeTile(px)]);
				}
				this.net.direct(to, { k: 'snap.tiles', lid, set });
			}
		}
		this.net.direct(to, { k: 'snap.end' });
	}

	async #takeTiles(lid: string, set: [number, number, string][], clear: [number, number][]) {
		// drawing on it too: send this app's own changes first, so putting theirs in keeps them
		const id = this.ids.get(lid);
		if (id && this.shared.canDraw(lid, this.me)) {
			const next = unpack(this.canvas.packed(this.canvas.drawingOf(id)));
			const d = diffTiles(this.cache.get(lid) ?? new Map(), next);
			if (d.set.length || d.clear.length) {
				this.cache.set(lid, next);
				await this.#sendTiles(lid, d.set, d.clear);
			}
		}
		const t = this.cache.get(lid) ?? new Map();
		this.cache.set(lid, t);
		for (const [x, y] of clear) t.delete(`${x},${y}`);
		for (const [x, y, s] of set) {
			const px = await decodeTile(s);
			if (px) t.set(`${x},${y}`, px);
		}
		this.#put(lid);
	}

	#makeLayer(lid: string) {
		if (this.ids.has(lid)) return;
		const l = this.shared.layers.get(lid);
		if (!l) return;
		const id = this.canvas.addLayer(l.name);
		if (!id) return;
		this.ids.set(lid, id);
		if (!this.cache.has(lid)) this.cache.set(lid, new Map());
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

	#put(lid: string) {
		const id = this.ids.get(lid);
		const t = this.cache.get(lid);
		if (!id || !t) return;
		if (this.canvas.put(this.canvas.drawingOf(id), pack(t))) this.#pending.delete(lid);
		else this.#pending.add(lid);
	}
}
