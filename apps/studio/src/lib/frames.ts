/**
 * The window's side of the frame store (crates/anim, in the app's Rust side): the paint
 * engine keeps the drawings showing, the ones undo needs, and a few recent ones; the
 * rest live in the store, in memory up to a budget and on disk past it.
 *
 * - Before frames show, their drawings are fetched if the engine doesn't have them.
 * - Drawings changed in the engine are "dirty" until they're sent to the store (when
 *   they stop showing, and before saving).
 * - Clean drawings that aren't showing are let go, oldest first, past `keep`. Undo and
 *   redo bring back the drawing they change before changing it.
 *
 * Outside the app (a plain browser), there's no store: the engine keeps everything.
 *
 * Packets (crates/anim): u32 drawings, then per drawing u32 id, u32 tiles, then per
 * tile i32 x, i32 y and 64×64 RGBA8. The engine packs one drawing as `u32 tiles, …`.
 */
import type PaintCanvas from '@pencilpilot/paint/PaintCanvas.svelte';

type Canvas = ReturnType<typeof PaintCanvas>;

const TILE_BYTES = 64 * 64 * 4;

/** Drawings kept in the engine besides the ones showing, as a developer set it (0: not set). */
function keepByDeveloper(): number {
	try {
		const n = Number(globalThis.localStorage?.getItem('pp.dev.keepDrawings'));
		return Number.isFinite(n) && n > 0 ? n : 0;
	} catch {
		return 0;
	}
}

/** The most drawings the engine keeps besides the ones showing. */
export const KEEP_MOST = 24;

/**
 * How the memory limit for frames (`limit` bytes) is shared: the app's frame store takes
 * three quarters (its Rust side works that out from the same setting), and the engine
 * keeps as many recent drawings as fit in the last quarter, counting each as a whole
 * canvas (at most 24; none when not even one fits: they come from the store when needed).
 * The drawings showing are the work itself, so they're always in the engine.
 */
export function memoryPlan(limit: number, width: number, height: number) {
	const drawing = Math.max(1, width * height * 4);
	const store = Math.floor(limit / 4) * 3;
	const keep = Math.min(KEEP_MOST, Math.max(0, Math.floor((limit - store) / drawing)));
	return { store, keep, engine: keep * drawing };
}

/** Split a store packet into each drawing's engine packing. */
export function splitPacket(bytes: Uint8Array): { id: number; packed: Uint8Array }[] {
	const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
	const n = v.getUint32(0, true);
	const out: { id: number; packed: Uint8Array }[] = [];
	let at = 4;
	for (let i = 0; i < n; i++) {
		const id = v.getUint32(at, true);
		const tiles = v.getUint32(at + 4, true);
		const end = at + 8 + tiles * (8 + TILE_BYTES);
		if (end > bytes.length) throw new Error('the drawings packet is cut short');
		out.push({ id, packed: bytes.subarray(at + 4, end) });
		at = end;
	}
	return out;
}

/** Join drawings (each as the engine packs it) into a store packet. */
export function joinPacket(drawings: { id: number; packed: Uint8Array }[]): Uint8Array {
	const size = drawings.reduce((n, d) => n + 4 + d.packed.length, 4);
	const out = new Uint8Array(size);
	const v = new DataView(out.buffer);
	v.setUint32(0, drawings.length, true);
	let at = 4;
	for (const d of drawings) {
		v.setUint32(at, d.id, true);
		out.set(d.packed, at + 4);
		at += 4 + d.packed.length;
	}
	return out;
}

export class FrameCache {
	/** changed in the engine since the store last had them */
	#dirty = new Set<number>();
	/** the store has these (sent to it, or opened from a file) */
	#stored = new Set<number>();
	/** how many times each drawing was changed (playback re-renders the frames it's on) */
	#versions = new Map<number, number>();
	/** when each drawing was last shown or changed */
	#used = new Map<number, number>();
	#clock = 0;
	/** drawings kept in the engine besides the ones showing */
	keep = keepByDeveloper() || KEEP_MOST;

	constructor(
		private canvas: () => Canvas | undefined,
		/** there's a store (the app; not a plain browser) */
		readonly native: boolean
	) {}

	async #invoke<T>(cmd: string, args?: unknown): Promise<T> {
		const { invoke } = await import('@tauri-apps/api/core');
		return invoke<T>(cmd, args as never);
	}

	/** These drawings were (or may have been) changed in the engine. */
	touch(ids: number[]) {
		for (const id of ids) {
			if (!id) continue;
			this.#dirty.add(id);
			this.#versions.set(id, (this.#versions.get(id) ?? 0) + 1);
			this.#used.set(id, ++this.#clock);
		}
	}

	/** A drawing's change count (it goes up with every change). */
	version(id: number): number {
		return this.#versions.get(id) ?? 0;
	}

	/** The store has these (a project was opened into it). */
	stored(ids: number[]) {
		for (const id of ids) if (id) this.#stored.add(id);
	}

	/** Whether showing these needs the store (so the frame change has to wait for it). */
	needs(ids: number[]): boolean {
		const c = this.canvas();
		return this.native && !!c && ids.some((id) => id && this.#stored.has(id) && !c.hasDrawing(id));
	}

	/** These drawings are showing. */
	shown(ids: number[]) {
		for (const id of ids) if (id) this.#used.set(id, ++this.#clock);
	}

	/** Bring drawings the engine doesn't have from the store. */
	async ensure(ids: number[]) {
		const c = this.canvas();
		if (!this.native || !c) return;
		// (a drawing the store never got is empty: nothing to fetch)
		const missing = [...new Set(ids)].filter(
			(id) => id && this.#stored.has(id) && !c.hasDrawing(id)
		);
		if (!missing.length) return;
		const got = await this.#invoke<ArrayBuffer>('frames_get', { ids: missing });
		const now = this.canvas();
		if (!now) return;
		for (const d of splitPacket(new Uint8Array(got)))
			if (!now.hasDrawing(d.id)) now.putDrawingTiles(d.id, d.packed);
	}

	/** Send changed drawings (these, or all) to the store. */
	async push(ids?: number[]) {
		const c = this.canvas();
		if (!this.native || !c) return;
		const send = [...(ids ?? this.#dirty)].filter((id) => this.#dirty.has(id) && c.hasDrawing(id));
		if (!send.length) return;
		const packet = joinPacket(send.map((id) => ({ id, packed: c.drawingTilesPacked(id) })));
		for (const id of send) this.#dirty.delete(id);
		try {
			await this.#invoke('frames_put', packet);
			for (const id of send) this.#stored.add(id);
		} catch (e) {
			for (const id of send) this.#dirty.add(id);
			throw e;
		}
	}

	/** Let go of clean drawings that aren't showing, oldest first, past `keep`. */
	trim() {
		const c = this.canvas();
		if (!this.native || !c) return;
		// (undo brings back a drawing it needs first: see the studio's undo)
		const kept = c.keptDrawings();
		const free = kept
			.filter((id) => !this.#dirty.has(id))
			.sort((a, b) => (this.#used.get(a) ?? 0) - (this.#used.get(b) ?? 0));
		let over = kept.length - this.keep;
		for (const id of free) {
			if (over <= 0) break;
			if (c.forgetDrawing(id)) over--;
		}
	}

	/** Keep as many drawings in the engine as the memory limit allows (a developer's choice wins). */
	limit(bytes: number, width: number, height: number) {
		this.keep = keepByDeveloper() || memoryPlan(bytes, width, height).keep;
		this.trim();
	}

	/** A new document: the store starts empty. */
	async clear() {
		this.#dirty.clear();
		this.#used.clear();
		this.#stored.clear();
		if (this.native) await this.#invoke('frames_retain', { keep: [] });
	}
}
