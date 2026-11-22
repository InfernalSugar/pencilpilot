/**
 * Drawing together, the drawing side (ADR-0013, S12-T03): shared layers, who may draw on
 * each, and their pixels kept the same in every app.
 *
 * - **One drawer per layer at a time.** A layer has an owner (who made it) and the people
 *   allowed to draw on it ("writers"). Only their changes to it count; everyone else sees
 *   it locked. Asking for access: `ask` → the owner's `grant` (let them draw, or hand it
 *   over), `deny`, or later `revoke`.
 * - **Groups** are shared the same way: a group has an owner and people who may change it
 *   (its name, eye and opacity, and which layers are in it).
 * - **Merging and grouping others' layers** asks first: what's missing is asked for (say
 *   why: to merge, to group, to delete), and the change happens once it's granted. Merging
 *   a layer away needs it handed over; merging into a layer, or grouping it, needs leave to
 *   draw on it.
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

import { readStyle, sameStyle, type LayerStyle } from '@pencilpilot/paint/layer-style';
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
	/** how it shows (set by whoever may draw on it) */
	look?: Look;
	/** an adjustment layer: its settings (the app's JSON) */
	adjust?: string;
	/** the group it's in */
	group?: string;
}

/** A shared group of layers (its id is made like a layer's, so ids never clash). */
export interface SharedGroup {
	gid: string;
	owner: string;
	name: string;
	/** who may change it: its settings, and which of their layers are in it */
	writers: string[];
	visible: boolean;
	/** 0–1 */
	opacity: number;
}

/** why someone asks: to draw, or for a change that needs the owner's leave */
export type Why = 'draw' | 'merge' | 'group' | 'delete';
const WHYS: Why[] = ['draw', 'merge', 'group', 'delete'];

/** An ask waiting for a layer's (or group's) owner. */
export interface Ask {
	/** the layer or group asked for */
	lid: string;
	from: string;
	why?: Why;
	/** asks for it to be handed over (merging or deleting it), not just to draw on it */
	own?: boolean;
	/** the other layer of a merge (to show what's merged with what) */
	with?: string;
}

/** A layer's settings that everyone sees the same (its lock stays each app's own: it's how
 * access is shown). */
export interface Look {
	visible: boolean;
	/** 0–1 */
	opacity: number;
	blend: string;
	alphaLock: boolean;
	clip: boolean;
	/** stroke, drop shadow and outer glow (null: none; missing: from an app without them,
	 * leaving the style as it is) */
	style?: LayerStyle | null;
}

function readLook(v: unknown): Look | null {
	const l = v as Look;
	if (!l || typeof l !== 'object') return null;
	if (typeof l.blend !== 'string' || !/^[a-z-]{1,20}$/.test(l.blend)) return null;
	if (!Number.isFinite(l.opacity)) return null;
	return {
		visible: l.visible !== false,
		opacity: Math.min(1, Math.max(0, l.opacity)),
		blend: l.blend,
		alphaLock: l.alphaLock === true,
		clip: l.clip === true,
		...(l.style !== undefined ? { style: readStyle(l.style) } : {})
	};
}

const sameLook = (a: Look | undefined, b: Look | undefined) =>
	!!a &&
	!!b &&
	a.visible === b.visible &&
	Math.abs(a.opacity - b.opacity) < 1e-3 &&
	a.blend === b.blend &&
	a.alphaLock === b.alphaLock &&
	a.clip === b.clip &&
	sameStyle(a.style, b.style);

export type TogetherEdit =
	| { k: 'layer'; lid: string; name: string; adjust?: string }
	| { k: 'adjust'; lid: string; spec: string }
	| {
			k: 'mask';
			lid: string;
			/** false: the layer has no mask (any more) */
			on: boolean;
			set: [number, number, string][];
			clear: [number, number][];
	  }
	| { k: 'rename'; lid: string; name: string }
	| { k: 'look'; lid: string; look: Look }
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
	| { k: 'sound'; lid: string; file: string; name: string; i: number; n: number; data: string }
	| { k: 'ask'; lid: string; why: Why; own: boolean; with?: string }
	/** a new group (id `gid`) holding these layers, in their places */
	| { k: 'group'; lid: string; gid: string; name: string; lids: string[] }
	/** a group's settings (`lid`: the group) */
	| { k: 'gset'; lid: string; name: string; visible: boolean; opacity: number }
	/** a layer to `to` in the shared order and into a group ('' = none) */
	| { k: 'place'; lid: string; to: number; gid: string }
	/** take a group's layers out of it and remove it (`lid`: the group) */
	| { k: 'ungroup'; lid: string }
	/** move a group's layers together, so the lowest is at `to` among the others (`lid`: the group) */
	| { k: 'block'; lid: string; to: number }
	| { k: 'grant'; lid: string; to: string; handover: boolean }
	| { k: 'deny'; lid: string; to: string }
	| { k: 'revoke'; lid: string; to: string };

const LID = /^[0-9a-f]{12}:[0-9]{1,12}$/;
const MEMBER = /^[0-9a-f]{64}$/;
const int = (v: unknown) => Number.isInteger(v) && Math.abs(v as number) < 1e6;

/** An adjustment layer's settings: JSON, a few kilobytes at most. */
function readSpec(v: unknown): string | null {
	if (typeof v !== 'string' || v.length > 8000) return null;
	try {
		const o = JSON.parse(v);
		return o && typeof o === 'object' && !Array.isArray(o) ? v : null;
	} catch {
		return null;
	}
}

/** Tiles to set and clear, checked. */
function readTiles(b: Record<string, unknown>) {
	if (!Array.isArray(b.set) || !Array.isArray(b.clear)) return null;
	if (b.set.length + b.clear.length > 4 * TILES_PER_EDIT) return null;
	const set = b.set.filter(
		(t): t is [number, number, string] =>
			Array.isArray(t) && int(t[0]) && int(t[1]) && typeof t[2] === 'string'
	);
	const clear = b.clear.filter(
		(t): t is [number, number] => Array.isArray(t) && int(t[0]) && int(t[1])
	);
	return { set, clear };
}

/** An edit from the session, checked; null for anything not on the list. */
export function parseEdit(body: unknown): TogetherEdit | null {
	if (!body || typeof body !== 'object') return null;
	const b = body as Record<string, unknown>;
	// the timeline is everyone's (the later one wins); checked as it's put in (fromShared)
	if (b.k === 'timeline')
		return b.tl && typeof b.tl === 'object' && Array.isArray((b.tl as SharedTimeline).scenes)
			? { k: 'timeline', lid: '', tl: b.tl as SharedTimeline }
			: null;
	// a piece of a sound file (anyone's; the timeline says where it plays)
	if (b.k === 'sound')
		return typeof b.file === 'string' &&
			SOUND_KEY.test(b.file) &&
			Number.isInteger(b.i) &&
			Number.isInteger(b.n) &&
			(b.n as number) > 0 &&
			(b.n as number) <= Math.ceil(SOUND_MAX / SOUND_CHUNK) &&
			(b.i as number) >= 0 &&
			(b.i as number) < (b.n as number) &&
			typeof b.data === 'string'
			? {
					k: 'sound',
					lid: '',
					file: b.file,
					name: String(b.name ?? b.file).slice(0, 100),
					i: b.i as number,
					n: b.n as number,
					data: b.data
				}
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
		case 'layer': {
			const adjust = readSpec(b.adjust);
			return { k: 'layer', lid, name: name || 'Layer', ...(adjust ? { adjust } : {}) };
		}
		case 'adjust': {
			const spec = readSpec(b.spec);
			return spec ? { k: 'adjust', lid, spec } : null;
		}
		case 'mask': {
			const t = readTiles(b);
			return t ? { k: 'mask', lid, on: b.on !== false, ...t } : null;
		}
		case 'rename':
			return name ? { k: 'rename', lid, name } : null;
		case 'look': {
			const look = readLook(b.look);
			return look ? { k: 'look', lid, look } : null;
		}
		case 'remove':
			return { k: 'remove', lid };
		case 'move':
			return int(b.to) && (b.to as number) >= 0 ? { k: 'move', lid, to: b.to as number } : null;
		case 'tiles': {
			const did = typeof b.did === 'string' && DID.test(b.did) ? b.did : null;
			const t = readTiles(b);
			return did && t ? { k: 'tiles', lid, did, ...t } : null;
		}
		case 'ask': {
			const why = WHYS.includes(b.why as Why) ? (b.why as Why) : 'draw';
			const other = typeof b.with === 'string' && LID.test(b.with) ? b.with : undefined;
			return { k: 'ask', lid, why, own: b.own === true, ...(other ? { with: other } : {}) };
		}
		case 'group': {
			const gid = typeof b.gid === 'string' && LID.test(b.gid) ? b.gid : null;
			const lids = Array.isArray(b.lids)
				? b.lids.filter((x): x is string => typeof x === 'string' && LID.test(x)).slice(0, 500)
				: [];
			return gid && lids.length ? { k: 'group', lid, gid, name: name || 'Group', lids } : null;
		}
		case 'gset':
			return Number.isFinite(b.opacity)
				? {
						k: 'gset',
						lid,
						name: name || 'Group',
						visible: b.visible !== false,
						opacity: Math.min(1, Math.max(0, b.opacity as number))
					}
				: null;
		case 'place': {
			const gid = b.gid === '' ? '' : typeof b.gid === 'string' && LID.test(b.gid) ? b.gid : null;
			return gid !== null && int(b.to) && (b.to as number) >= 0
				? { k: 'place', lid, to: b.to as number, gid }
				: null;
		}
		case 'ungroup':
			return { k: 'ungroup', lid };
		case 'block':
			return int(b.to) && (b.to as number) >= 0 ? { k: 'block', lid, to: b.to as number } : null;
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
	groups = new Map<string, SharedGroup>();
	/** asks waiting for the owner */
	asks: Ask[] = [];

	/** whether `who` may draw on a layer (or change a group) */
	canDraw(lid: string, who: string): boolean {
		return (this.layers.get(lid) ?? this.groups.get(lid))?.writers.includes(who) ?? false;
	}

	/** whether `who` owns a layer (or group) */
	owns(lid: string, who: string): boolean {
		return (this.layers.get(lid) ?? this.groups.get(lid))?.owner === who;
	}

	/** a group's layers, bottom → top */
	members(gid: string): string[] {
		return this.order.filter((x) => this.layers.get(x)?.group === gid);
	}

	/** groups left with no layers go */
	#dropEmpty() {
		for (const gid of [...this.groups.keys()])
			if (!this.members(gid).length) {
				this.groups.delete(gid);
				this.asks = this.asks.filter((a) => a.lid !== gid);
			}
	}

	/** Take an edit in; false when it isn't allowed (and changes nothing). */
	apply(from: string, e: TogetherEdit): boolean {
		const l = this.layers.get(e.lid);
		/** the layer or group an ask (or its answer) is about */
		const thing = l ?? this.groups.get(e.lid);
		switch (e.k) {
			case 'timeline':
			case 'sound':
				return true;
			case 'layer':
				// only the member whose id starts the layer's id can make it, once
				if (l || this.groups.has(e.lid) || !e.lid.startsWith(from.slice(0, 12) + ':')) return false;
				this.layers.set(e.lid, {
					lid: e.lid,
					owner: from,
					name: e.name,
					writers: [from],
					...(e.adjust ? { adjust: e.adjust } : {})
				});
				this.order.push(e.lid);
				return true;
			case 'remove':
				if (!l || l.owner !== from) return false;
				this.layers.delete(e.lid);
				this.order = this.order.filter((x) => x !== e.lid);
				this.asks = this.asks.filter((a) => a.lid !== e.lid && a.with !== e.lid);
				this.#dropEmpty();
				return true;
			case 'move': {
				if (!l || l.owner !== from) return false;
				const rest = this.order.filter((x) => x !== e.lid);
				rest.splice(Math.min(e.to, rest.length), 0, e.lid);
				this.order = rest;
				return true;
			}
			case 'rename':
			case 'look':
			case 'tiles':
			case 'mask':
				if (!l || !l.writers.includes(from)) return false;
				if (e.k === 'rename') l.name = e.name;
				if (e.k === 'look') l.look = e.look;
				return true;
			case 'adjust':
				if (!l || !l.adjust || !l.writers.includes(from)) return false;
				l.adjust = e.spec;
				return true;
			case 'ask': {
				if (!thing || (e.own ? thing.owner === from : thing.writers.includes(from))) return false;
				const ask: Ask = {
					lid: e.lid,
					from,
					why: e.why,
					...(e.own ? { own: true } : {}),
					...(e.with ? { with: e.with } : {})
				};
				// (asked again: the newer reason counts)
				this.asks = [...this.asks.filter((a) => !(a.lid === e.lid && a.from === from)), ask];
				return true;
			}
			case 'grant':
				if (!thing || thing.owner !== from) return false;
				this.asks = this.asks.filter((a) => !(a.lid === e.lid && a.from === e.to));
				if (e.handover) {
					thing.owner = e.to;
					thing.writers = [e.to, ...thing.writers.filter((w) => w !== e.to && w !== from)];
				} else if (!thing.writers.includes(e.to)) thing.writers.push(e.to);
				return true;
			case 'deny':
				if (!thing || thing.owner !== from) return false;
				this.asks = this.asks.filter((a) => !(a.lid === e.lid && a.from === e.to));
				return true;
			case 'revoke':
				if (!thing || thing.owner !== from || e.to === thing.owner) return false;
				thing.writers = thing.writers.filter((w) => w !== e.to);
				return true;
			case 'group': {
				// a new id, the maker's; every layer one the maker may draw on, in one group or none
				if (this.layers.has(e.gid) || this.groups.has(e.gid)) return false;
				if (!e.gid.startsWith(from.slice(0, 12) + ':')) return false;
				const lids = [...new Set(e.lids)];
				for (const x of lids) {
					const m = this.layers.get(x);
					if (!m || !m.writers.includes(from)) return false;
					if (m.group && !this.canDraw(m.group, from)) return false;
				}
				this.groups.set(e.gid, {
					gid: e.gid,
					owner: from,
					name: e.name,
					writers: [from],
					visible: true,
					opacity: 1
				});
				for (const x of lids) this.layers.get(x)!.group = e.gid;
				this.#dropEmpty();
				return true;
			}
			case 'gset': {
				const g = this.groups.get(e.lid);
				if (!g || !g.writers.includes(from)) return false;
				g.name = e.name;
				g.visible = e.visible;
				g.opacity = e.opacity;
				return true;
			}
			case 'place': {
				if (!l || !l.writers.includes(from)) return false;
				if (e.gid && !this.canDraw(e.gid, from)) return false;
				if (l.group && l.group !== e.gid && !this.canDraw(l.group, from)) return false;
				const rest = this.order.filter((x) => x !== e.lid);
				rest.splice(Math.min(e.to, rest.length), 0, e.lid);
				this.order = rest;
				if (e.gid) l.group = e.gid;
				else delete l.group;
				this.#dropEmpty();
				return true;
			}
			case 'ungroup': {
				const g = this.groups.get(e.lid);
				if (!g || g.owner !== from) return false;
				for (const x of this.members(e.lid)) delete this.layers.get(x)!.group;
				this.groups.delete(e.lid);
				this.asks = this.asks.filter((a) => a.lid !== e.lid);
				return true;
			}
			case 'block': {
				const g = this.groups.get(e.lid);
				if (!g || !g.writers.includes(from)) return false;
				const m = this.members(e.lid);
				const rest = this.order.filter((x) => !m.includes(x));
				rest.splice(Math.min(e.to, rest.length), 0, ...m);
				this.order = rest;
				return true;
			}
		}
	}

	snapshot() {
		return {
			layers: this.order.map((lid) => structuredClone(this.layers.get(lid)!)),
			groups: [...this.groups.values()].map((g) => structuredClone(g)),
			asks: structuredClone(this.asks)
		};
	}

	load(s: { layers: SharedLayer[]; groups?: SharedGroup[]; asks: Ask[] }) {
		this.layers.clear();
		this.groups.clear();
		this.order = [];
		for (const g of Array.isArray(s.groups) ? s.groups : []) {
			if (!g || !LID.test(g.gid) || !Array.isArray(g.writers)) continue;
			this.groups.set(g.gid, {
				gid: g.gid,
				owner: String(g.owner),
				name: String(g.name).slice(0, 60),
				writers: g.writers.map(String),
				visible: g.visible !== false,
				opacity: Number.isFinite(g.opacity) ? Math.min(1, Math.max(0, g.opacity)) : 1
			});
		}
		for (const l of s.layers) {
			if (!LID.test(l.lid) || this.layers.has(l.lid) || this.groups.has(l.lid)) continue;
			this.layers.set(l.lid, {
				lid: l.lid,
				owner: String(l.owner),
				name: String(l.name).slice(0, 60),
				writers: l.writers.map(String),
				...(readLook(l.look) ? { look: readLook(l.look)! } : {}),
				...(readSpec(l.adjust) ? { adjust: l.adjust } : {}),
				...(typeof l.group === 'string' && this.groups.has(l.group) ? { group: l.group } : {})
			});
			this.order.push(l.lid);
		}
		this.#dropEmpty();
		this.asks = (Array.isArray(s.asks) ? s.asks : [])
			.filter((a) => a && (this.layers.has(a.lid) || this.groups.has(a.lid)))
			.map((a) => ({
				lid: a.lid,
				from: String(a.from),
				why: WHYS.includes(a.why as Why) ? a.why : 'draw',
				...(a.own ? { own: true } : {}),
				...(typeof a.with === 'string' && LID.test(a.with) ? { with: a.with } : {})
			}));
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
	addLayer(name: string, adjust?: string): number;
	/** change an adjustment layer's settings (false: busy, try later) */
	setAdjust(id: number, spec: string): boolean;
	/** a layer's mask as packed tiles; empty when it has none */
	maskPacked(id: number): Uint8Array;
	/** put a layer's mask (empty bytes: none); false when busy */
	putMask(id: number, packed: Uint8Array): boolean;
	setLayer(id: number, patch: { name?: string; locked?: boolean } & Partial<Look>): void;
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
	/** move a layer right above / below `ref` (0: where it is) and into a group (0: none);
	 * false: busy, try later; clears undo */
	place(id: number, ref: number, above: boolean, group: number): boolean;
	/** a new group holding a layer: its id (0: busy, try later); clears undo */
	addGroup(layer: number): number;
	/** a group's settings (not an undo step) */
	setGroup(id: number, s: { name: string; visible: boolean; opacity: number }): void;
	/** take a group's layers out and remove it (false: busy); clears undo */
	ungroup(id: number): boolean;
	/** merge a layer into the one below (false: couldn't); clears undo */
	mergeDown(id: number): boolean;
	/** flatten a group into its lowest layer: that layer (0: couldn't); clears undo */
	mergeGroup(id: number): number;
	/** the groups now */
	groups(): { id: number; name: string; visible: boolean; opacity: number }[];
	/** every layer, bottom → top (`adjust`: an adjustment layer's settings; `group`: its
	 * group, 0 = none) */
	layers(): { id: number; name: string; look?: Look; adjust?: string | null; group?: number }[];
	/** a sound file the studio has (by key), or null */
	sound(file: string): { bytes: Uint8Array; name: string } | null;
	/** add a sound file under its key (decoded for playing) */
	putSound(file: string, bytes: Uint8Array, name: string): Promise<void>;
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
			groups?: SharedGroup[];
			asks: Ask[];
			tl: SharedTimeline | null;
	  }
	| { k: 'snap.tiles'; lid: string; did: string; set: [number, number, string][] }
	| { k: 'snap.end' };

const key = (lid: string, did: string) => `${lid}|${did}`;

/**
 * A shared project's session, kept in the project file (`extra.share`), so it can be
 * rejoined after a break: the invite, who was in it, the shared layers and drawings.
 */
export interface SavedShare {
	/** the invite code (it holds the project's secret: anyone with the file can rejoin) */
	code: string;
	members: { id: string; name: string; addr: unknown }[];
	layers: SharedLayer[];
	groups?: SharedGroup[];
	asks: Ask[];
	/** shared layer → its id in this document */
	ids: [string, number][];
	/** shared group → its id in this document */
	gids?: [string, number][];
	/** shared drawing → its id in this document */
	dids: [string, number][];
	n: number;
}

/** A saved session from a file, checked (null if it isn't one). */
export function readSavedShare(raw: unknown): SavedShare | null {
	const s = raw as SavedShare;
	if (!s || typeof s !== 'object' || typeof s.code !== 'string' || !s.code.startsWith('pp1-'))
		return null;
	const pairs = (v: unknown, ok: RegExp) =>
		Array.isArray(v)
			? v.filter(
					(p): p is [string, number] =>
						Array.isArray(p) && typeof p[0] === 'string' && ok.test(p[0]) && Number.isInteger(p[1])
				)
			: [];
	return {
		code: s.code,
		members: Array.isArray(s.members)
			? s.members
					.filter((m) => m && typeof m.id === 'string')
					.map((m) => ({ id: m.id, name: String(m.name ?? ''), addr: m.addr }))
			: [],
		layers: Array.isArray(s.layers)
			? s.layers.filter((l) => l && LID.test(l.lid) && Array.isArray(l.writers))
			: [],
		groups: Array.isArray(s.groups) ? s.groups : [],
		asks: Array.isArray(s.asks) ? s.asks : [],
		ids: pairs(s.ids, LID),
		gids: pairs(s.gids, LID),
		dids: pairs(s.dids, DID),
		n: Number.isInteger(s.n) ? s.n : 0
	};
}

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
	/** sound files the session has (sent from here, or arrived) */
	#soundsShared = new Set<string>();
	/** sound files arriving in pieces: key → pieces so far */
	#soundParts = new Map<string, { name: string; parts: (string | undefined)[] }>();
	/** whether each shared layer has a mask, as last synced */
	#maskOn = new Map<string, boolean>();
	/** adjustment settings last sent per layer */
	#adjustSent = new Map<string, string>();
	/** the settings last sent for each layer (so a change goes once) */
	#lookSent = new Map<string, Look>();
	/** coming back after a break: this app's own layers keep what was drawn meanwhile */
	#rejoining = false;
	/** someone's tiles arrived (their live stroke previews can go) */
	ontiles: (from: string) => void = () => {};
	/** something to tell this person (a change asked for went ahead, or was refused) */
	onnote: (text: string) => void = () => {};
	/** shared group → the engine's group here */
	gids = new Map<string, number>();
	/** groups the session has had */
	#groupsSeen = new Set<string>();
	/** each group's settings as last synced */
	#groupSent = new Map<string, { name: string; visible: boolean; opacity: number }>();
	/** changes waiting for owners to say yes: what they need, and what to do then */
	#waiting: { needs: { lid: string; own: boolean }[]; run: () => Promise<void> | void }[] = [];
	/** edits to send once this app's changed tiles have gone (a merged layer's removal) */
	#afterSend: (() => void)[] = [];

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

	/** Send the sound files the timeline plays that the session doesn't have yet. */
	#shareSounds(tl: SharedTimeline, to?: string) {
		for (const sc of tl.scenes)
			for (const a of sc.audio ?? []) {
				if (!to && this.#soundsShared.has(a.file)) continue;
				const snd = this.canvas.sound(a.file);
				if (!snd || snd.bytes.length > SOUND_MAX) continue;
				if (!to) this.#soundsShared.add(a.file);
				const n = Math.max(1, Math.ceil(snd.bytes.length / SOUND_CHUNK));
				for (let i = 0; i < n; i++) {
					const data = toBase64(snd.bytes.subarray(i * SOUND_CHUNK, (i + 1) * SOUND_CHUNK));
					const piece = { k: 'sound', file: a.file, name: snd.name, i, n, data };
					if (to) this.net.direct(to, { ...piece, k: 'snap.sound' });
					else this.net.edit(piece);
				}
			}
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
		const adjusts = new Set<number>();
		for (const l of this.canvas.layers()) {
			const lid = this.#newLid();
			this.ids.set(lid, l.id);
			if (l.adjust) adjusts.add(l.id);
			this.net.edit({ k: 'layer', lid, name: l.name, ...(l.adjust ? { adjust: l.adjust } : {}) });
		}
		this.#shareGroups();
		for (const [lid, id] of this.ids)
			if (!adjusts.has(id))
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
		const tl = this.#sharedTimeline();
		this.#shareSounds(tl);
		this.net.edit({ k: 'timeline', tl });
		this.#synced = true;
	}

	/** This app's groups, shared (hosting): each with its layers, then its settings. */
	#shareGroups() {
		const layers = this.canvas.layers();
		for (const g of this.canvas.groups()) {
			const lids = layers
				.filter((l) => l.group === g.id)
				.map((l) => this.lidOf(l.id))
				.filter((x): x is string => !!x);
			if (!lids.length) continue;
			const gid = this.#newLid();
			this.gids.set(gid, g.id);
			this.net.edit({ k: 'group', lid: lids[0], gid, name: g.name, lids });
			const s = { name: g.name, visible: g.visible, opacity: g.opacity };
			this.#groupSent.set(gid, s);
			this.net.edit({ k: 'gset', lid: gid, ...s });
		}
	}

	/** The session, to keep in the project file. */
	save(code: string, members: { id: string; name: string; addr?: unknown }[]): SavedShare {
		const s = this.shared.snapshot();
		return {
			code,
			members: members.map((m) => ({ id: m.id, name: m.name, addr: m.addr })),
			layers: s.layers,
			groups: s.groups,
			asks: s.asks,
			ids: [...this.ids],
			gids: [...this.gids],
			dids: [...this.#dids],
			n: this.#n
		};
	}

	/** Back to a saved session (its layer ids already this document's). */
	restore(saved: SavedShare) {
		this.shared.load({ layers: saved.layers, groups: saved.groups, asks: saved.asks });
		const here = new Set(this.canvas.layers().map((l) => l.id));
		this.ids = new Map(saved.ids.filter(([, id]) => here.has(id)));
		const groupsHere = new Set(this.canvas.groups().map((g) => g.id));
		this.gids = new Map((saved.gids ?? []).filter(([, id]) => groupsHere.has(id)));
		for (const [gid] of this.gids) this.#groupsSeen.add(gid);
		for (const [gid] of this.gids) {
			const g = this.shared.groups.get(gid);
			if (g) this.#groupSent.set(gid, { name: g.name, visible: g.visible, opacity: g.opacity });
		}
		for (const [did, d] of saved.dids) {
			this.#dids.set(did, d);
			this.#didOf.set(d, did);
		}
		this.#n = Math.max(this.#n, saved.n + 1);
		// layers here that aren't shared (from before joining) stay out of it
		const shared = new Set(this.ids.values());
		for (const l of this.canvas.layers()) if (!shared.has(l.id)) this.#local.add(l.id);
	}

	/** After `restore`, alone: the copy here is the project (others coming back get it). */
	async resumeHost() {
		for (const [lid, id] of this.ids)
			for (const d of new Set([this.canvas.drawingOf(id), ...this.canvas.drawingsOfLayer(id)]))
				this.cache.set(key(lid, this.didOf(d)), unpack(await this.canvas.packed(d)));
		for (const lid of this.ids.keys()) this.#refreshLock(lid);
		this.#synced = true;
		this.onchange();
	}

	/** After `restore`, with others online: get the project from `host`; this app's own
	 * layers keep what was drawn here meanwhile (it's sent once the project is in). */
	rejoin(host: string) {
		this.#rejoining = true;
		this.net.direct(host, { k: 'snap.req' });
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
		this.net.edit({ k: 'ask', lid, why: 'draw', own: false });
	}

	/** The group an engine group is, if any. */
	gidOf(id: number): string | null {
		for (const [gid, i] of this.gids) if (i === id) return gid;
		return null;
	}

	#ok(n: { lid: string; own: boolean }) {
		return n.own ? this.shared.owns(n.lid, this.me) : this.shared.canDraw(n.lid, this.me);
	}

	/**
	 * Do `run` once this app may: it needs `needs` (a layer or group handed over, `own`, or
	 * leave to draw on / change it). What's missing is asked for, saying `why`. Returns the
	 * owners asked (none: it went ahead now).
	 */
	request(
		why: Why,
		needs: { lid: string; own: boolean; with?: string }[],
		run: () => Promise<void> | void
	): string[] {
		const missing = needs.filter((n) => !this.#ok(n));
		if (!missing.length) {
			void run();
			return [];
		}
		for (const n of missing)
			this.net.edit({ k: 'ask', lid: n.lid, why, own: n.own, ...(n.with ? { with: n.with } : {}) });
		this.#waiting.push({ needs, run });
		return [
			...new Set(
				missing.map((n) => (this.shared.layers.get(n.lid) ?? this.shared.groups.get(n.lid))!.owner)
			)
		];
	}

	/** Changes waiting for a yes: those that may go ahead now do. */
	async #tryWaiting() {
		const ready = this.#waiting.filter((w) => w.needs.every((n) => this.#ok(n)));
		this.#waiting = this.#waiting.filter((w) => !ready.includes(w));
		for (const w of ready) await w.run();
	}

	/** The layer below a layer here, if shared. */
	#below(id: number): string | null {
		const all = this.canvas.layers();
		const i = all.findIndex((l) => l.id === id);
		return i > 0 ? this.lidOf(all[i - 1].id) : null;
	}

	/** Merge a shared layer into the one below it: it needs the layer handed over (it goes)
	 * and leave to draw on the one below. Owners asked (none: merged now); null: can't. */
	mergeDown(id: number): string[] | null {
		const a = this.lidOf(id);
		const b = this.#below(id);
		if (!a || !b) return null;
		return this.request(
			'merge',
			[
				{ lid: a, own: true, with: b },
				{ lid: b, own: false, with: a }
			],
			async () => {
				const idA = this.ids.get(a);
				// meanwhile moved apart, or gone: nothing merged
				if (!idA || this.#below(idA) !== b) {
					this.onnote('The layers moved meanwhile: merge again.');
					return;
				}
				if (!this.canvas.mergeDown(idA)) {
					this.onnote('Couldn’t merge those layers.');
					return;
				}
				this.#forget(a);
				this.#afterSend.push(() => this.net.edit({ k: 'remove', lid: a }));
				await this.localChanged();
			}
		);
	}

	/** Forget a layer merged away here (its removal is sent after the merged pixels). */
	#forget(lid: string) {
		this.ids.delete(lid);
		for (const k of [...this.cache.keys()])
			if (k.startsWith(lid + '|')) {
				this.cache.delete(k);
				this.#pending.delete(k);
			}
	}

	/** Delete a shared layer: needs it handed over. */
	deleteLayer(id: number): string[] | null {
		const lid = this.lidOf(id);
		if (!lid) return null;
		return this.request('delete', [{ lid, own: true }], () => this.net.edit({ k: 'remove', lid }));
	}

	/** Put a shared layer in a new group: needs leave to draw on it (and on the group it's
	 * in now). Owners asked; null: not a shared layer. */
	addGroup(id: number): string[] | null {
		const lid = this.lidOf(id);
		if (!lid) return null;
		const now = this.shared.layers.get(lid)?.group;
		return this.request(
			'group',
			[{ lid, own: false }, ...(now ? [{ lid: now, own: false }] : [])],
			() => {
				const gid = this.#newLid();
				this.net.edit({ k: 'group', lid, gid, name: 'Group', lids: [lid] });
			}
		);
	}

	/** Move a shared layer to `to` (the shared order) and into a group (engine id, 0: none):
	 * needs leave to draw on it, on that group, and on the group it leaves. */
	moveLayerTo(id: number, to: number, group: number): string[] | null {
		const lid = this.lidOf(id);
		const gid = group ? this.gidOf(group) : '';
		if (!lid || gid === null) return null;
		const now = this.shared.layers.get(lid)?.group;
		const needs = [{ lid, own: false }];
		if (gid) needs.push({ lid: gid, own: false });
		if (now && now !== gid) needs.push({ lid: now, own: false });
		return this.request('group', needs, () => this.net.edit({ k: 'place', lid, to, gid }));
	}

	/** Change a shared group's settings (`run` changes them here, once allowed). */
	changeGroup(group: number, run: () => void): string[] | null {
		const gid = this.gidOf(group);
		if (!gid) return null;
		return this.request('group', [{ lid: gid, own: false }], async () => {
			run();
			await this.localChanged();
		});
	}

	/** Take a group's layers out of it: needs the group handed over. */
	ungroup(group: number): string[] | null {
		const gid = this.gidOf(group);
		if (!gid) return null;
		return this.request('group', [{ lid: gid, own: true }], () =>
			this.net.edit({ k: 'ungroup', lid: gid })
		);
	}

	/** Delete a group and its layers: needs each layer handed over. */
	deleteGroup(group: number): string[] | null {
		const gid = this.gidOf(group);
		if (!gid) return null;
		const m = this.shared.members(gid);
		return this.request(
			'delete',
			m.map((lid) => ({ lid, own: true })),
			() => {
				for (const lid of this.shared.members(gid)) this.net.edit({ k: 'remove', lid });
			}
		);
	}

	/** Flatten a group into its lowest layer: needs the others handed over, and leave to draw
	 * on the lowest and to change the group. */
	mergeGroup(group: number): string[] | null {
		const gid = this.gidOf(group);
		if (!gid) return null;
		const m = this.shared.members(gid);
		if (!m.length) return null;
		return this.request(
			'merge',
			[
				{ lid: gid, own: false },
				{ lid: m[0], own: false },
				...m.slice(1).map((lid) => ({ lid, own: true }))
			],
			async () => {
				const now = this.shared.members(gid);
				const local = this.gids.get(gid);
				if (!local || now.join() !== m.join()) {
					this.onnote('The group changed meanwhile: merge again.');
					return;
				}
				const g = this.shared.groups.get(gid)!;
				if (!this.canvas.mergeGroup(local)) {
					this.onnote('Couldn’t merge that group.');
					return;
				}
				for (const lid of m.slice(1)) this.#forget(lid);
				this.gids.delete(gid);
				this.#groupSent.delete(gid);
				const bottom = m[0];
				this.#afterSend.push(() => {
					this.net.edit({ k: 'rename', lid: bottom, name: g.name });
					this.net.edit({
						k: 'place',
						lid: bottom,
						to: this.shared.order.indexOf(bottom),
						gid: ''
					});
					for (const lid of m.slice(1)) this.net.edit({ k: 'remove', lid });
				});
				await this.localChanged();
			}
		);
	}

	/** Move a group's layers together, up or down past the next layer (or group). */
	moveGroup(group: number, up: boolean): string[] | null {
		const gid = this.gidOf(group);
		if (!gid) return null;
		return this.request('group', [{ lid: gid, own: false }], () => {
			const m = this.shared.members(gid);
			if (!m.length) return;
			const rest = this.shared.order.filter((x) => !m.includes(x));
			let p = this.shared.order.indexOf(m[0]);
			const groupOf = (x: string | undefined) => (x ? this.shared.layers.get(x)?.group : undefined);
			if (up) {
				if (p >= rest.length) return;
				const h = groupOf(rest[p]);
				p++;
				while (h && p < rest.length && groupOf(rest[p]) === h) p++;
			} else {
				if (p === 0) return;
				const h = groupOf(rest[p - 1]);
				p--;
				while (h && p > 0 && groupOf(rest[p - 1]) === h) p--;
			}
			this.net.edit({ k: 'block', lid: gid, to: p });
		});
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
		if (!this.#synced) return;
		const tl = this.#sharedTimeline();
		this.#shareSounds(tl);
		this.net.edit({ k: 'timeline', tl });
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
			case 'look': {
				const id = this.ids.get(e.lid);
				// (as if sent from here too, so it isn't sent back)
				this.#lookSent.set(e.lid, e.look);
				if (id && from !== this.me) this.canvas.setLayer(id, e.look);
				break;
			}
			case 'adjust': {
				const id = this.ids.get(e.lid);
				this.#adjustSent.set(e.lid, e.spec);
				if (id && from !== this.me) this.#later(() => this.canvas.setAdjust(id, e.spec));
				break;
			}
			case 'mask':
				if (from !== this.me) {
					this.#maskOn.set(e.lid, e.on);
					if (!e.on) this.cache.delete(key(e.lid, 'mask'));
					else {
						const t = this.cache.get(key(e.lid, 'mask')) ?? new Map();
						this.cache.set(key(e.lid, 'mask'), t);
						for (const [x, y] of e.clear) t.delete(`${x},${y}`);
						for (const [x, y, s] of e.set) {
							const px = await decodeTile(s);
							if (px) t.set(`${x},${y}`, px);
						}
					}
					this.#put(key(e.lid, 'mask'));
				}
				break;
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
			case 'place':
				this.#place(e.lid);
				break;
			case 'block':
				for (const lid of this.shared.members(e.lid)) this.#place(lid);
				break;
			case 'group':
				this.#makeGroup(e.gid);
				break;
			case 'gset': {
				const s = { name: e.name, visible: e.visible, opacity: e.opacity };
				this.#groupSent.set(e.lid, s);
				const g = this.gids.get(e.lid);
				if (g && from !== this.me) this.canvas.setGroup(g, s);
				break;
			}
			case 'tiles':
				// this app's own changes are on its canvas already
				if (from !== this.me) {
					await this.#takeTiles(e.lid, e.did, e.set, e.clear);
					this.ontiles(from);
				}
				break;
			case 'sound':
				if (from !== this.me) await this.#takeSound(e.file, e.name, e.i, e.n, e.data);
				break;
			case 'timeline':
				// everyone's, this app's too, in the session's order: all end the same
				this.canvas.setTimeline(this.#fromShared(e.tl));
				break;
			case 'grant':
			case 'revoke':
				this.#refreshLock(e.lid);
				if (e.k === 'grant' && e.to === this.me) await this.#tryWaiting();
				break;
			case 'deny':
				if (e.to === this.me) {
					const was = this.#waiting.length;
					this.#waiting = this.#waiting.filter((w) => !w.needs.some((n) => n.lid === e.lid));
					if (this.#waiting.length < was) this.onnote('They said no: nothing was changed.');
				}
				break;
		}
		this.#dropGroups();
		this.onchange();
	}

	/** Groups the session no longer has: taken apart here too. */
	#dropGroups() {
		// (a group made here counts once the session has it: until then it's on its way)
		for (const gid of this.shared.groups.keys()) this.#groupsSeen.add(gid);
		for (const [gid, g] of [...this.gids])
			if (this.#groupsSeen.has(gid) && !this.shared.groups.has(gid)) {
				this.gids.delete(gid);
				this.#groupSent.delete(gid);
				this.#later(() => this.canvas.ungroup(g));
			}
	}

	/** Make a shared group here (its layers first), and put its layers in it. */
	#makeGroup(gid: string) {
		if (this.gids.has(gid)) return;
		const g = this.shared.groups.get(gid);
		const m = this.shared.members(gid).filter((lid) => this.ids.has(lid));
		if (!g || !m.length) return;
		this.#later(() => {
			if (this.gids.has(gid) || !this.shared.groups.has(gid)) return true;
			const local = this.canvas.addGroup(this.ids.get(m[0])!);
			if (!local) return false;
			this.gids.set(gid, local);
			const s = { name: g.name, visible: g.visible, opacity: g.opacity };
			this.canvas.setGroup(local, s);
			this.#groupSent.set(gid, s);
			for (const lid of this.shared.members(gid)) this.#place(lid);
			return true;
		});
	}

	/** A piece of a sound file; once all are here, the studio gets the file. */
	async #takeSound(file: string, name: string, i: number, n: number, data: string) {
		this.#soundsShared.add(file);
		if (this.canvas.sound(file)) return;
		const got = this.#soundParts.get(file) ?? {
			name,
			parts: Array.from({ length: n }, (): string | undefined => undefined)
		};
		if (got.parts.length !== n) return;
		got.parts[i] = data;
		this.#soundParts.set(file, got);
		if (got.parts.some((p) => p === undefined)) return;
		this.#soundParts.delete(file);
		const chunks = got.parts.map((p) => fromBase64(p!));
		const bytes = new Uint8Array(chunks.reduce((t, c) => t + c.length, 0));
		let at = 0;
		for (const c of chunks) {
			bytes.set(c, at);
			at += c.length;
		}
		await this.canvas.putSound(file, bytes, got.name).catch(() => {});
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
			if (
				!this.shared.layers.has(m.lid) ||
				typeof m.did !== 'string' ||
				!(DID.test(m.did) || m.did === 'mask')
			)
				return;
			if (m.did === 'mask') this.#maskOn.set(m.lid, true);
			const t = this.cache.get(key(m.lid, m.did)) ?? new Map();
			this.cache.set(key(m.lid, m.did), t);
			for (const [x, y, s] of m.set) {
				const px = await decodeTile(s);
				if (px) t.set(`${x},${y}`, px);
			}
		} else if ((m as { k: string }).k === 'snap.sound' && !this.#synced) {
			const p = parseEdit({ ...(m as object), k: 'sound' });
			if (p?.k === 'sound') await this.#takeSound(p.file, p.name, p.i, p.n, p.data);
		} else if (m.k === 'snap.end' && !this.#synced) {
			// back from a break: layers deleted meanwhile go
			for (const [lid, id] of [...this.ids])
				if (!this.shared.layers.has(lid)) {
					this.ids.delete(lid);
					this.#later(() => this.canvas.removeLayer(id));
				}
			for (const lid of this.shared.order) {
				this.#makeLayer(lid);
				this.#refreshLock(lid);
			}
			// groups: those this app has (back from a break) put right, the others made
			this.#dropGroups();
			for (const gid of this.shared.groups.keys()) {
				const g = this.gids.get(gid);
				const sg = this.shared.groups.get(gid)!;
				if (g) {
					this.canvas.setGroup(g, { name: sg.name, visible: sg.visible, opacity: sg.opacity });
					for (const lid of this.shared.members(gid)) this.#place(lid);
				} else this.#makeGroup(gid);
			}
			for (const [lid] of this.ids) if (!this.shared.layers.get(lid)?.group) this.#place(lid);
			const own = (k: string) =>
				this.#rejoining && this.shared.layers.get(k.split('|')[0])?.owner === this.me;
			for (const k of this.cache.keys()) if (!own(k)) this.#put(k);
			if (this.#snapTl) this.canvas.setTimeline(this.#fromShared(this.#snapTl));
			this.#synced = true;
			const later = this.#buffer.filter((b) => b.seq > this.#snapSeq);
			this.#buffer = [];
			for (const b of later) await this.onEdit(b.seq, b.from, b.body);
			if (this.#rejoining) {
				// this app's own drawings, as drawn here (compared with the session's): sent
				for (const [lid, id] of this.ids)
					if (this.shared.layers.get(lid)?.owner === this.me)
						this.touch([this.canvas.drawingOf(id), ...this.canvas.drawingsOfLayer(id)]);
				this.#rejoining = false;
				await this.localChanged();
			} else this.addOwnLayer();
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
				const all = this.canvas.layers();
				for (const l of all)
					if (!known.has(l.id) && !this.#local.has(l.id)) {
						// it goes on top, here as for everyone (the same stack everywhere)
						const top = [...this.shared.order].reverse().find((x) => this.ids.has(x));
						const topId = top && this.ids.get(top);
						if (topId && (all.at(-1)?.id !== l.id || l.group))
							this.canvas.place(l.id, topId, true, 0);
						else if (l.group) this.canvas.place(l.id, 0, true, 0);
						const lid = this.#newLid();
						this.ids.set(lid, l.id);
						this.net.edit({
							k: 'layer',
							lid,
							name: l.name,
							...(l.adjust ? { adjust: l.adjust } : {})
						});
						if (l.adjust) this.#adjustSent.set(lid, l.adjust);
					}
				// adjustment settings and masks changed here, on layers this app may change
				for (const l of all) {
					const lid = this.lidOf(l.id);
					if (!lid || !this.shared.canDraw(lid, this.me)) continue;
					if (
						l.adjust &&
						l.adjust !== (this.#adjustSent.get(lid) ?? this.shared.layers.get(lid)?.adjust)
					) {
						this.#adjustSent.set(lid, l.adjust);
						this.net.edit({ k: 'adjust', lid, spec: l.adjust });
					}
					const packed = this.canvas.maskPacked(l.id);
					const on = packed.length > 0;
					const next = on ? unpack(packed) : new Map<string, Uint8Array>();
					const prev = this.cache.get(key(lid, 'mask')) ?? new Map();
					const d = diffTiles(prev, next);
					if (on === (this.#maskOn.get(lid) ?? false) && !d.set.length && !d.clear.length) continue;
					this.#maskOn.set(lid, on);
					if (on) this.cache.set(key(lid, 'mask'), next);
					else this.cache.delete(key(lid, 'mask'));
					await this.#sendMask(lid, on, d.set, d.clear);
				}
				// layer settings changed here (opacity, blend…) on layers this app may change
				const now = new Map(this.canvas.layers().map((l) => [l.id, l.look]));
				for (const [lid, id] of this.ids) {
					const look = now.get(id);
					if (!look || !this.shared.canDraw(lid, this.me)) continue;
					if (sameLook(look, this.#lookSent.get(lid) ?? this.shared.layers.get(lid)?.look))
						continue;
					this.#lookSent.set(lid, look);
					this.net.edit({ k: 'look', lid, look });
				}
				// group settings changed here, on groups this app may change
				for (const g of this.canvas.groups()) {
					const gid = this.gidOf(g.id);
					if (!gid || !this.shared.canDraw(gid, this.me)) continue;
					const s = { name: g.name, visible: g.visible, opacity: g.opacity };
					const was = this.#groupSent.get(gid);
					if (
						was &&
						was.name === s.name &&
						was.visible === s.visible &&
						Math.abs(was.opacity - s.opacity) < 1e-3
					)
						continue;
					this.#groupSent.set(gid, s);
					this.net.edit({ k: 'gset', lid: gid, ...s });
				}
				// what may have changed: each drawable layer's drawing showing, and drawings
				// touched elsewhere (an undo on another frame)
				const look = new Map<number, number>();
				const adjusts = new Set(all.filter((l) => l.adjust).map((l) => l.id));
				for (const [, id] of this.ids)
					if (!adjusts.has(id)) look.set(this.canvas.drawingOf(id), id);
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
				// (a merged layer's removal: after the merged pixels)
				for (const f of this.#afterSend.splice(0)) f();
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

	async #sendMask(
		lid: string,
		on: boolean,
		set: [number, number, Uint8Array][],
		clear: [number, number][]
	) {
		for (let i = 0; i < Math.max(set.length, 1); i += TILES_PER_EDIT) {
			const enc: [number, number, string][] = [];
			for (const [x, y, px] of set.slice(i, i + TILES_PER_EDIT))
				enc.push([x, y, await encodeTile(px)]);
			this.net.edit({ k: 'mask', lid, on, set: enc, clear: i === 0 ? clear : [] });
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
		this.#shareSounds(this.#sharedTimeline(), to);
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
		const id = this.canvas.addLayer(l.name, l.adjust);
		if (!id) return;
		this.ids.set(lid, id);
		if (l.adjust) this.#adjustSent.set(lid, l.adjust);
		if (l.look) this.canvas.setLayer(id, l.look);
		this.#refreshLock(lid);
	}

	/** Put a shared layer next to its neighbour in the shared order. */
	#place(lid: string) {
		const id = this.ids.get(lid);
		const i = this.shared.order.indexOf(lid);
		if (!id || i < 0) return;
		const below = this.ids.get(this.shared.order[i - 1]);
		const above = this.ids.get(this.shared.order[i + 1]);
		const gid = this.shared.layers.get(lid)?.group;
		const group = () => (gid ? (this.gids.get(gid) ?? 0) : 0);
		if (below) this.#later(() => this.canvas.place(id, below, true, group()));
		else if (above) this.#later(() => this.canvas.place(id, above, false, group()));
		else this.#later(() => this.canvas.place(id, 0, true, group()));
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
		const [lid, did] = k.split('|');
		if (did === 'mask') {
			const id = this.ids.get(lid);
			if (!id) return;
			const t = this.cache.get(k);
			const on = this.#maskOn.get(lid) ?? !!t;
			if (this.canvas.putMask(id, on ? pack(t ?? new Map()) : new Uint8Array()))
				this.#pending.delete(k);
			else this.#pending.add(k);
			return;
		}
		const t = this.cache.get(k);
		if (!t) return;
		const d = this.localOf(did);
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
		/** sound clips; their files travel as 'sound' edits */
		audio?: { name: string; file: string; start: number; gain: number; muted: boolean }[];
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
/** a sound file's key, as the studio makes them (`s3-lq2x9f.mp3`) */
const SOUND_KEY = /^[A-Za-z0-9._-]{1,80}$/;
/** sound files travel in pieces of this many bytes */
const SOUND_CHUNK = 256 * 1024;
/** the largest sound file shared */
const SOUND_MAX = 20 * 1024 * 1024;

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
			...(s.audio?.length
				? {
						audio: s.audio.map((a) => ({
							name: a.name,
							file: a.file,
							start: a.start,
							gain: a.gain,
							muted: a.muted
						}))
					}
				: {}),
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
			...(Array.isArray(sc?.audio) && sc.audio.length
				? {
						audio: sc.audio
							.filter((a) => typeof a?.file === 'string' && SOUND_KEY.test(a.file))
							.slice(0, 200)
							.map((a, i) => ({
								id: i + 1,
								name: String(a.name ?? 'Sound').slice(0, 100),
								file: a.file,
								start: Number.isFinite(a.start) ? Math.round(a.start) : 0,
								gain: Number.isFinite(a.gain) ? Math.min(16, Math.max(0, a.gain)) : 1,
								muted: !!a.muted
							}))
					}
				: {}),
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
