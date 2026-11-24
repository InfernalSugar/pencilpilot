/* eslint-disable svelte/prefer-svelte-reactivity -- sets here are lookups, never rendered */
/**
 * Line layers on the canvas (see lines.ts for the strokes themselves): drawing them into
 * the layer's pixels through the paint engine, the undo steps for every edit, and the
 * Select line tool and the line eraser. `PaintCanvas.svelte` owns one and passes it the
 * pointer; this keeps the strokes of every drawing and what's selected.
 *
 * Every edit is one engine undo step (the tiles it redrew) with the strokes' own undo
 * attached to the matching history entry, so undo / redo bring back pixels and strokes
 * together. Only the tiles an edit touches are redrawn, from the strokes that reach them.
 *
 * Plain layers keep their strokes editable too, up to a limit per drawing: the engine
 * keeps the pixels under them (a "base", see crates/brush/src/lines.rs) and redraws from
 * it. Strokes become plain pixels ("baked") when other paint lands on them (`tainted`)
 * or when they're the oldest past the limit (`added`); see `bakePlan` in lines.ts.
 */
import type { PaintBrush, PaintCanvas as Engine } from './brushes';
import type { BrushTexture } from './brush-textures';
import { resample } from './quickshape';
import { shapePath, type EditShape, type Pt } from './shape-edit';
import {
	LineStore,
	bakePlan,
	bounds,
	cutStroke,
	detach,
	dragHandle,
	eraseCut,
	eraseWhole,
	handles,
	hitTest,
	inBox,
	markReaches,
	pasteCopies,
	pasteOffset,
	pickable,
	replayGroups,
	smudgeArea,
	strokeTiles,
	mirrored,
	mirroredDelta,
	overLimit,
	pathBox,
	pointsAlong,
	rotateAbout,
	strokesBytes,
	strokesOnKeys,
	strokesOnTiles,
	tilesOf,
	transformShape,
	transformStroke,
	twinMap,
	twins,
	type Affine,
	type Box,
	type LineStroke
} from './lines';

/** What the line layers need from the canvas. */
export interface LineHost {
	engine(): Engine | undefined;
	width(): number;
	height(): number;
	/** whether a layer is a line layer */
	isLineLayer(id: number): boolean;
	/** symmetry is on: strokes made together (mirrored) are edited together */
	symmetryOn(): boolean;
	/** the layer showing a drawing (0: none) */
	layerShowing(drawing: number): number;
	/** a loaded brush to redraw strokes with (null: not loaded yet; see `loadBrushes`) */
	brush(id: string): PaintBrush | null;
	loadBrushes(ids: string[]): Promise<void>;
	/** the dab style (grain, tip…) for the strokes redrawn next; `restoreDabStyle` puts the
	 * current brush's back */
	dabStyle(t: BrushTexture | null | undefined): void;
	restoreDabStyle(): void;
	/** the engine just made an undo step: log it (named), with the strokes' undo / redo */
	step(label: string, undo: () => void, redo: () => void): void;
	/** more to undo / redo with the last step logged */
	attach(effect: Effect): void;
	schedule(): void;
	notify(): void;
}

export type LineEraser = 'whole' | 'normal';

/** What the Select line tool's press is doing. */
type Drag =
	| { kind: 'move'; start: Pt; before: readonly LineStroke[]; ids: Set<string> }
	| { kind: 'scale'; corner: number; box: Box; before: readonly LineStroke[]; ids: Set<string> }
	| { kind: 'rotate'; start: Pt; c: Pt; before: readonly LineStroke[]; ids: Set<string> }
	| { kind: 'point'; id: string; hs: number[]; h: number; start: Pt; before: readonly LineStroke[] }
	| { kind: 'box'; start: Pt; add: boolean };

/** An undo / redo of the app's own, to go with an engine step. */
export type Effect = { undo(): void; redo(): void };

/** Paint brushes reach this far past their radius, to keep tiles from cutting dabs. */
const SEL_TOL = 6;

export class LineLayers {
	store = new LineStore();
	/** bumps when any drawing's strokes change (for the selection overlay) */
	version = $state(0);
	/**
	 * the selected strokes: on which layer and drawing, and their ids; `primary` is the
	 * one picked when its mirrored twins came along (edits go to them mirrored)
	 */
	sel = $state<{ layer: number; drawing: number; ids: string[]; primary?: string } | null>(null);
	/** the strokes as a live change shows them (null: as kept) */
	shown = $state.raw<readonly LineStroke[] | null>(null);
	/** a box being dragged to select (canvas px) */
	marquee = $state<[Pt, Pt] | null>(null);
	/** someone else hears about changes (drawing together) */
	onlines: (drawing: number) => void = () => {};
	/** drawings whose pixels must be drawn again from their strokes when they show */
	#stale = new Set<number>();
	/** a drag or an erase showing live, not an undo step yet */
	#live: { layer: number; before: readonly LineStroke[]; shown: readonly LineStroke[] } | null =
		null;
	#drag: Drag | null = null;
	#erase: { layer: number; mode: LineEraser; r: number; at: Pt } | null = null;

	constructor(private host: LineHost) {
		this.store.onchange = (d) => {
			this.version++;
			this.onlines(d);
		};
	}

	// ---- drawing strokes into the layer ----

	drawingOf(layer: number) {
		return this.host.engine()?.layerDrawing(layer) ?? 0;
	}

	/**
	 * A layer's editable strokes (the drawing showing). A plain layer has none but its own
	 * (a line layer turned into pixels leaves strokes the pixels no longer follow).
	 */
	strokesOf(layer: number): readonly LineStroke[] {
		const d = this.drawingOf(layer);
		if (!this.host.isLineLayer(layer) && !this.store.isPlain(d)) return [];
		return this.store.get(d);
	}

	/** the brushes these strokes need, loaded */
	async ready(strokes: readonly LineStroke[]) {
		const need = [...new Set(strokes.map((s) => s.brush))].filter((b) => !this.host.brush(b));
		if (need.length) await this.host.loadBrushes(need);
	}

	/**
	 * Draw the tiles under `boxes` again from `list` (the drawing's strokes as they should
	 * be). Not an undo step on its own: the engine keeps what it replaced until `push`.
	 */
	#redraw(layer: number, list: readonly LineStroke[], boxes: Box[]): boolean {
		const e = this.host.engine();
		if (!e) return false;
		const [w, h] = [this.host.width(), this.host.height()];
		// (strokes that smudge read the paint around them: all of it is drawn again too)
		let tiles = smudgeArea(list, tilesOf(boxes, w, h), w, h);
		// (nothing on the canvas changes: one tile, so the step still exists)
		if (!tiles.length) tiles = new Int32Array([0, 0]);
		if (!e.linesBegin(layer, tiles)) return false;
		this.#replay(list, tiles);
		e.linesCommit();
		this.host.schedule();
		return true;
	}

	/** The strokes of `list` that reach `tiles` (all of them: none), replayed into the redraw begun. */
	#replay(list: readonly LineStroke[], tiles?: Int32Array) {
		const e = this.host.engine();
		if (!e) return;
		// (the dab style is set only when it changes; the current brush's comes back after)
		let style: string | undefined;
		const on = tiles ? strokesOnTiles(list, tiles) : list;
		// (strokes made together are replayed together, as symmetry painted them)
		const groups = replayGroups(on);
		/** the brush set for `s` (an erase mark erases), its dab style too */
		const use = (s: LineStroke) => {
			const b = this.host.brush(s.brush);
			if (!b) return null;
			b.setSizeScale(s.size);
			b.setOpacity(s.opacity);
			const n = parseInt(s.color.slice(1), 16) || 0;
			b.setColor((n >> 16) & 255, (n >> 8) & 255, n & 255);
			b.setEraser(!!s.erase);
			const key = s.texture ? JSON.stringify(s.texture) : '';
			if (key !== style) {
				this.host.dabStyle(s.texture);
				style = key;
			}
			return b;
		};
		for (const [i, s] of on.entries()) {
			const b = use(s);
			if (!b) continue;
			e.linesStroke(b, s.seed, s.pts, groups[i]);
			// a cut piece draws its part of the whole stroke's dabs
			if (s.dabs) e.linesSpan(s.dabs[0], s.dabs[1]);
			// and erasers of its own erase only it
			for (const c of s.clip ?? []) {
				const cb = use(c);
				if (cb) e.linesClip(cb, c.seed, c.pts);
			}
		}
		if (style !== undefined) this.host.restoreDabStyle();
	}

	// ---- plain layers: strokes becoming plain pixels ----

	/**
	 * Strokes `start` of drawing `d` (indices in `list`; older ones sharing their tiles
	 * come along) become plain pixels: the engine's base takes them where strokes stay
	 * editable over them, and forgets tiles none reach. Joins the last undo step. The
	 * strokes left editable (null: none baked).
	 */
	#bake(
		layer: number,
		d: number,
		list: readonly LineStroke[],
		start: number[],
		tainted?: Set<string>
	): readonly LineStroke[] | null {
		return this.#bakePlan(layer, d, list, start, tainted)?.keep ?? null;
	}

	/** `#bake`, with what it did: the strokes kept and baked (null: none baked). */
	#bakePlan(
		layer: number,
		d: number,
		list: readonly LineStroke[],
		start: number[],
		tainted?: Set<string>
	): { keep: LineStroke[]; baked: LineStroke[] } | null {
		const e = this.host.engine();
		if (!e || !start.length) return null;
		// (a stroke that smudged paint now gone can't be replayed as it was: the strokes
		// over it become plain pixels too)
		const plan = bakePlan(
			list,
			start,
			(s) =>
				!!this.host.brush(s.brush) &&
				!(s.smudge && tainted && strokeTiles(s).some((t) => tainted.has(t)))
		);
		if (!plan.baked.length) return null;
		if (plan.replay.length) {
			// (drawn over the tiles about to go too: what smudges reads them)
			const all = new Int32Array([...plan.replay, ...plan.drop]);
			if (e.baseBegin(d, layer, all)) {
				this.#replay(plan.baked, all);
				e.linesCommit();
			}
		}
		if (plan.drop.length) e.baseDrop(d, plan.drop);
		return plan;
	}

	/** Draw `tiles` of `layer` again from `list`, into the last undo step. */
	#redrawJoined(layer: number, list: readonly LineStroke[], tiles: Int32Array) {
		const e = this.host.engine();
		if (!e || !tiles.length || !e.linesBegin(layer, tiles)) return;
		this.#replay(list, tiles);
		e.linesCommit();
		if (e.linesPush()) e.joinLastEdits();
		this.host.schedule();
	}

	/**
	 * A transform by `m` (affine) was just kept on `layer` (the engine kept what it did,
	 * `xf*`): the strokes `follow` (ids: those it moved whole) move along and stay
	 * editable, drawn again where they land over the pixels that were under them, moved
	 * the same way; the others it changed become plain pixels (`runs`: the engine's
	 * `takeTainted`). On a line layer every stroke follows. All in the transform's undo
	 * step; what to undo and redo with it (null: nothing).
	 */
	followTransform(
		layer: number,
		m: Affine,
		follow: Set<string>,
		runs: Int32Array | number[]
	): Effect | null {
		const e = this.host.engine();
		const d = this.drawingOf(layer);
		if (!e || !d) return null;
		const before = this.strokesOf(layer);
		const [w, h] = [this.host.width(), this.host.height()];
		let after: readonly LineStroke[];
		if (this.host.isLineLayer(layer)) {
			after = before.map((s) => (follow.has(s.id) ? transformStroke(s, m) : s));
			const tiles = tilesOf([...before, ...after].map(bounds), w, h);
			this.#redrawJoined(layer, after, smudgeArea(after, tiles, w, h));
		} else {
			const keys = new Set<string>();
			for (let i = 0; i + 1 < runs.length;) {
				const n = runs[i + 1];
				if (runs[i] === d)
					for (let k = 0; k < n; k++) keys.add(`${runs[i + 2 + k * 2]},${runs[i + 3 + k * 2]}`);
				i += 2 + n * 2;
			}
			// strokes the transform changed but didn't move whole: plain pixels
			const start = strokesOnKeys(before, keys).filter((i) => !follow.has(before[i].id));
			const plan = this.#bakePlan(layer, d, before, start, keys) ?? {
				keep: [...before],
				baked: []
			};
			const moving = plan.keep.filter((s) => follow.has(s.id));
			if (moving.length) {
				// the layer as it was without its editable strokes (but those just baked)…
				if (e.xfWithoutBegin()) {
					this.#replay(plan.baked);
					e.linesCommit();
				}
				// …moved as the transform moved it: under the strokes where they land
				const moved = new Map(moving.map((s) => [s.id, transformStroke(s, m)]));
				after = plan.keep.map((s) => moved.get(s.id) ?? s);
				const tiles = smudgeArea(after, tilesOf([...moved.values()].map(bounds), w, h), w, h);
				e.xfBase(tiles);
				this.#redrawJoined(layer, after, tiles);
			} else after = plan.keep;
		}
		e.xfForget();
		if (after === before) return null;
		this.store.set(d, after);
		this.#keepSelection();
		return {
			undo: () => this.store.set(d, before),
			redo: () => this.store.set(d, after)
		};
	}

	/**
	 * Other paint landed on base tiles (the engine's `takeTainted` runs: drawing, count,
	 * x, y…): the strokes there become plain pixels. What to undo and redo with the step
	 * that did it (null: nothing changed).
	 */
	tainted(runs: Int32Array | number[]): Effect | null {
		const effects: Effect[] = [];
		for (let i = 0; i + 1 < runs.length;) {
			const d = runs[i];
			const n = runs[i + 1];
			const keys = new Set<string>();
			for (let k = 0; k < n; k++) keys.add(`${runs[i + 2 + k * 2]},${runs[i + 3 + k * 2]}`);
			i += 2 + n * 2;
			const before = this.store.get(d);
			const start = strokesOnKeys(before, keys);
			if (!start.length) continue;
			const layer = this.host.layerShowing(d) || (this.host.engine()?.activeLayer ?? 0);
			const after = this.#bake(layer, d, before, start, keys) ?? before;
			if (after === before) continue;
			this.store.set(d, after);
			effects.push({
				undo: () => this.store.set(d, before),
				redo: () => this.store.set(d, after)
			});
		}
		if (!effects.length) return null;
		this.#keepSelection();
		return {
			undo: () => [...effects].reverse().forEach((f) => f.undo()),
			redo: () => effects.forEach((f) => f.redo())
		};
	}

	/** Memory editable strokes take: their points, and the pixels kept under them. */
	memory(): { strokes: number; base: number; count: number } {
		let strokes = 0;
		let count = 0;
		for (const d of this.store.drawings()) {
			if (!this.store.isPlain(d)) continue;
			const list = this.store.get(d);
			strokes += strokesBytes(list);
			count += pickable(list).length;
		}
		return { strokes, base: this.host.engine()?.baseBytes(0) ?? 0, count };
	}

	/** The boxes where `a` and `b` differ (strokes compared as objects). */
	#changed(a: readonly LineStroke[], b: readonly LineStroke[]): Box[] {
		const inA = new Set(a);
		const inB = new Set(b);
		return [...a.filter((s) => !inB.has(s)), ...b.filter((s) => !inA.has(s))].map(bounds);
	}

	/** Record a change the engine has pending as one undo step, with the strokes'. */
	#record(
		layer: number,
		before: readonly LineStroke[],
		after: readonly LineStroke[],
		label: string
	) {
		const e = this.host.engine();
		if (!e) return false;
		if (!e.linesPush()) return false;
		// (the first change to an empty cel gives the layer its drawing)
		const d = this.drawingOf(layer);
		if (!this.host.isLineLayer(layer)) this.store.setPlain(d);
		this.store.set(d, after);
		this.host.step(
			label,
			() => this.store.set(d, before),
			() => this.store.set(d, after)
		);
		this.#keepSelection();
		this.host.notify();
		return true;
	}

	/**
	 * Strokes an edit moved (their points changed) go up past the erase marks reaching
	 * them where they are now: a mark erases where it erased, not a stroke moved under it.
	 */
	#settle(before: readonly LineStroke[], next: readonly LineStroke[]) {
		const was = new Map(before.map((s) => [s.id, s.pts]));
		const moved = new Set(
			next.filter((s) => !s.erase && was.has(s.id) && was.get(s.id) !== s.pts).map((s) => s.id)
		);
		return moved.size ? detach(next, moved, true) : next;
	}

	/** One edit, as one undo step: the strokes become `after`. */
	async edit(layer: number, next: readonly LineStroke[], label: string): Promise<boolean> {
		if (this.#live) return false;
		const before = this.strokesOf(layer);
		if (next === before) return false;
		const after = this.#settle(before, next);
		await this.ready(after);
		if (!this.#redraw(layer, after, this.#changed(before, after))) return false;
		return this.#record(layer, before, after, label);
	}

	/** Show `next` live (a drag), not yet an undo step. */
	#preview(layer: number, next: readonly LineStroke[]) {
		const live = (this.#live ??= {
			layer,
			before: this.strokesOf(layer),
			shown: this.strokesOf(layer)
		});
		next = this.#settle(live.before, next);
		const boxes = this.#changed(live.shown, next);
		if (!boxes.length) return;
		if (this.#redraw(layer, next, boxes)) live.shown = this.shown = next;
	}

	/** The live change becomes one undo step (none if nothing changed). */
	#finish(label: string) {
		const live = this.#live;
		this.#live = null;
		this.shown = null;
		const e = this.host.engine();
		if (!live || !e) return;
		if (live.shown === live.before) {
			e.linesCancel();
			return;
		}
		this.#record(live.layer, live.before, live.shown, label);
	}

	/** Drop a live change (a pinch, a cancelled drag): the layer as it was. */
	cancel() {
		this.#drag = null;
		this.#erase = null;
		this.marquee = null;
		if (!this.#live) return;
		this.#live = null;
		this.shown = null;
		this.host.engine()?.linesCancel();
		this.host.schedule();
	}

	get busy() {
		return !!this.#live || !!this.#drag || !!this.#erase;
	}

	/**
	 * A drawing's pixels drawn again from its strokes, silently (it was copied or boiled
	 * as strokes; its pixels follow the first time it shows). `layer` shows it now.
	 */
	renderSilently(layer: number) {
		const e = this.host.engine();
		const d = this.drawingOf(layer);
		if (!e || !d) return false;
		const all: Box = { x0: 0, y0: 0, x1: this.host.width() - 1, y1: this.host.height() - 1 };
		if (!this.#redraw(layer, this.store.get(d), [all])) return false;
		e.linesForget();
		this.#stale.delete(d);
		return true;
	}

	/** Pixels for `drawing` come from its strokes the next time it shows. */
	markStale(drawing: number) {
		this.#stale.add(drawing);
	}

	/** Layers just shown: the ones showing stale drawings are drawn from their strokes. */
	async showing(layers: number[]) {
		const todo = layers.filter(
			(l) => this.#stale.has(this.drawingOf(l)) && this.host.isLineLayer(l)
		);
		if (!todo.length) return;
		await this.ready(todo.flatMap((l) => [...this.strokesOf(l)]));
		for (const l of todo) this.renderSilently(l);
		this.host.notify();
	}

	// ---- a new stroke (drawn live by the brush) ----

	/**
	 * A finished stroke (and its symmetry copies), already painted: one undo step's
	 * strokes. On a plain layer the oldest past `limit` become plain pixels.
	 */
	added(layer: number, strokes: LineStroke[], limit = Infinity) {
		const d = this.drawingOf(layer);
		if (!d || !strokes.length) return;
		const before = this.store.get(d);
		let after: readonly LineStroke[] = [...this.strokesOf(layer), ...strokes];
		if (!this.host.isLineLayer(layer)) {
			this.store.setPlain(d);
			// what smudging strokes read without painting joins the base too
			const e = this.host.engine();
			const smudged = strokes.filter((s) => s.smudge).map(bounds);
			if (e && smudged.length)
				e.baseCover(d, tilesOf(smudged, this.host.width(), this.host.height()));
			const over = overLimit(after, limit);
			if (over.length) after = this.#bake(layer, d, after, over) ?? after;
		}
		this.store.set(d, after);
		this.#keepSelection();
		return {
			undo: () => this.store.set(d, before),
			redo: () => this.store.set(d, after)
		};
	}

	// ---- the eraser over editable strokes, kept as erase marks ----

	/** Where a stroke's dabs fall (see `Dabs`; null: its brush isn't loaded). */
	#dabs(s: LineStroke): Float32Array | null {
		const e = this.host.engine();
		const b = this.host.brush(s.brush);
		if (!e || !b) return null;
		b.setSizeScale(s.size);
		b.setOpacity(s.opacity);
		b.setEraser(!!s.erase);
		return e.linesDabs(b, s.seed, s.pts);
	}

	/** Whether redrawing `tiles` of `layer` from `list` gives exactly the pixels there now. */
	#same(layer: number, list: readonly LineStroke[], tiles: Int32Array): boolean {
		const e = this.host.engine();
		if (!e || !tiles.length || !e.linesBegin(layer, tiles)) return false;
		this.#replay(list, tiles);
		return e.linesDiff() === 0;
	}

	/**
	 * The eraser just ended on `layer`, drawn as a line stroke (`marks`: it and its
	 * symmetry copies, `erase` set; the engine's step has it). The strokes it cut through
	 * become pieces, and it stays as an erase mark over them, so redrawn they give
	 * exactly the pixels the eraser left (checked: if a cut can't be exact, the strokes
	 * stay whole under the mark, which is). On a plain layer strokes that smudge become
	 * plain pixels instead. An eraser that reached no editable stroke isn't kept. What to
	 * undo and redo with the step (undefined: nothing changed).
	 */
	erased(layer: number, marks: LineStroke[], limit = Infinity): Effect | undefined {
		const e = this.host.engine();
		const d = this.drawingOf(layer);
		if (!e || !d || !marks.length) return;
		const line = this.host.isLineLayer(layer);
		const before = this.store.get(d);
		const list = this.strokesOf(layer);
		const [w, h] = [this.host.width(), this.host.height()];
		const reached = list.filter((s) => !s.erase && marks.some((m) => markReaches(m, s)));
		if (!reached.length) {
			// (the pixels under the strokes there, erased as the eraser erased them)
			if (!line) this.#sink(layer, d, list, marks);
			return;
		}
		// pieces, each drawing its share of the whole stroke's dabs; checked against what
		// the eraser left (with a soft edge, gone points can't be told: every dab is kept)
		const tiles = smudgeArea(
			[...list, ...marks],
			tilesOf([...marks, ...reached].map(bounds), w, h),
			w,
			h
		);
		const ready = [...list, ...marks].every((s) => this.host.brush(s.brush));
		let after: readonly LineStroke[] = [...list, ...marks];
		// where their dabs fall (the brush trails behind the pen: cuts go by the dabs)
		const markDabs = ready ? marks.map((m) => this.#dabs(m)) : [];
		const dabsOf = new Map(
			ready ? reached.filter((s) => !s.smudge).map((s) => [s, this.#dabs(s)] as const) : []
		);
		for (const [core, whole] of [
			[1, false],
			[0.6, false],
			[1, true]
		] as const) {
			if (!ready) break;
			const cut = list.flatMap((s) =>
				s.erase || s.smudge || !reached.includes(s)
					? [s]
					: (cutStroke(s, marks, core, whole, {
							stroke: dabsOf.get(s) ?? null,
							marks: markDabs
						}) ?? [s])
			);
			if (cut.length === list.length && cut.every((s, i) => s === list[i])) break;
			const next = [...cut, ...marks];
			if (this.#same(layer, next, tiles)) {
				after = next;
				break;
			}
		}
		if (!line) {
			this.store.setPlain(d);
			// strokes that smudge can't be cut exactly: they become plain pixels
			const smudged = after.flatMap((s, i) => (s.smudge && reached.includes(s) ? [i] : []));
			if (smudged.length) after = this.#bake(layer, d, after, smudged) ?? after;
			const over = overLimit(after, limit);
			if (over.length) after = this.#bake(layer, d, after, over) ?? after;
		}
		this.store.set(d, after);
		this.#keepSelection();
		return {
			undo: () => this.store.set(d, before),
			redo: () => this.store.set(d, after)
		};
	}

	/**
	 * Erase marks that reached no editable stroke on a plain layer: the pixels under the
	 * strokes (the base) take what they erased, and tiles no stroke reaches leave it (in
	 * the last undo step). Exact: no stroke has paint where they erased.
	 */
	#sink(layer: number, d: number, list: readonly LineStroke[], marks: readonly LineStroke[]) {
		const e = this.host.engine();
		if (!e) return;
		const keep = new Set(list.flatMap((s) => strokeTiles(s)));
		const all = [...new Set(marks.flatMap((m) => strokeTiles(m)))];
		const pairs = (keys: string[]) => new Int32Array(keys.flatMap((k) => k.split(',').map(Number)));
		const under = all.filter((k) => keep.has(k));
		if (
			under.length &&
			marks.every((m) => this.host.brush(m.brush)) &&
			e.baseBegin(d, layer, pairs(under))
		) {
			this.#replay(marks, pairs(under));
			e.linesCommit();
		}
		const drop = all.filter((k) => !keep.has(k));
		if (drop.length) e.baseDrop(d, pairs(drop));
	}

	// ---- copy and paste ----

	/**
	 * The selected strokes, to copy: the erase marks over them come along as their own
	 * (so cut pieces keep their cut ends wherever they're pasted).
	 */
	copySelected(): LineStroke[] {
		const s = this.sel;
		if (!s) return [];
		const ids = new Set(s.ids);
		return detach(this.strokesOf(s.layer), ids).filter((x) => ids.has(x.id) && !x.erase);
	}

	/**
	 * Strokes pasted onto `layer` (a line layer, or a plain one: they stay editable up to
	 * `limit`), where they were copied from (a little further each time copies are
	 * already there), as one undo step; then they're selected. False if they can't be.
	 */
	paste(
		layer: number,
		strokes: readonly LineStroke[],
		limit = Infinity
	): boolean | Promise<boolean> {
		if (this.#live || !strokes.length) return false;
		// (brushes loaded already: done now; else once they are)
		const need = [...strokes, ...strokes.flatMap((s) => s.clip ?? [])];
		if (need.every((s) => this.host.brush(s.brush))) return this.#paste(layer, strokes, limit);
		return this.ready(need).then(() => this.#paste(layer, strokes, limit));
	}

	#paste(layer: number, strokes: readonly LineStroke[], limit: number): boolean {
		if (this.#live) return false;
		const line = this.host.isLineLayer(layer);
		const list = this.strokesOf(layer);
		const step = pasteOffset(list, strokes);
		const copies = pasteCopies(strokes, step, step);
		const d0 = this.drawingOf(layer);
		const was = this.store.get(d0);
		const plain = this.store.isPlain(d0);
		const after = [...list, ...copies];
		if (!this.#redraw(layer, after, copies.map(bounds))) return false;
		if (!this.#record(layer, was, after, 'Paste lines')) return false;
		const d = this.drawingOf(layer);
		let kept: readonly LineStroke[] = after;
		if (!line) {
			// past the limit, the oldest become plain pixels (with the paste)
			const over = overLimit(after, limit);
			if (over.length) kept = this.#bake(layer, d, after, over) ?? after;
			if (kept !== after) this.store.set(d, kept);
			this.host.attach({
				undo: () => {
					this.store.setPlain(d, plain && d === d0);
					this.store.set(d, was);
				},
				redo: () => {
					this.store.setPlain(d);
					this.store.set(d, kept);
				}
			});
		}
		const ids = new Set(kept.map((s) => s.id));
		const picked = copies.filter((s) => ids.has(s.id));
		const p = picked.find((s) => s.link && picked.some((t) => t !== s && t.link?.g === s.link?.g));
		this.select(
			layer,
			picked.map((s) => s.id),
			p?.id
		);
		this.host.notify();
		return true;
	}

	// ---- the selection ----

	/** the selected strokes as they are now */
	selected(): LineStroke[] {
		void this.version;
		const s = this.sel;
		if (!s) return [];
		const ids = new Set(s.ids);
		return (this.shown ?? this.store.get(s.drawing)).filter((x) => ids.has(x.id));
	}

	/**
	 * The stroke picked when the selection is it and its linked twins (edits then go to
	 * the twins mirrored), else null.
	 */
	primary(): LineStroke | null {
		const s = this.sel;
		if (!s?.primary) return null;
		const list = this.selected();
		const p = list.find((x) => x.id === s.primary);
		if (!p?.link || list.length < 2 || list.some((x) => x.link?.g !== p.link!.g)) return null;
		return p;
	}

	/** the box round the selected paths (canvas px); the picked one's, with its twins */
	selBox(): Box | null {
		const p = this.primary();
		return pathBox(p ? [p] : this.selected());
	}

	/** Whether the stroke under (x, y) on `layer` can be picked (its index, or −1). */
	hit(layer: number, x: number, y: number, scale: number): number {
		return hitTest(this.strokesOf(layer), x, y, SEL_TOL / scale);
	}

	/** Whether a press at (x, y) is on the selection: its handles, its box or its strokes. */
	onSelection(x: number, y: number, scale: number): boolean {
		const s = this.sel;
		if (!s) return false;
		const near = (p: Pt) => Math.hypot(p[0] - x, p[1] - y) <= 12 / scale;
		if (this.transformHandles(scale).some(near)) return true;
		if (this.pointHandles()?.pts.some(near)) return true;
		const b = this.selBox();
		if (b && x >= b.x0 && x <= b.x1 && y >= b.y0 && y <= b.y1) return true;
		return hitTest(this.selected(), x, y, SEL_TOL / scale) >= 0;
	}

	/** Pick the stroke under (x, y) (with its twins while symmetry is on). */
	pick(layer: number, x: number, y: number, scale: number): boolean {
		const list = this.strokesOf(layer);
		const i = hitTest(list, x, y, SEL_TOL / scale);
		if (i < 0) return false;
		this.#choose(layer, list, list[i]);
		return true;
	}

	/** Select `s` (and its twins while symmetry is on). */
	#choose(layer: number, list: readonly LineStroke[], s: LineStroke) {
		const group = this.host.symmetryOn() ? twins(list, s) : [s];
		this.select(
			layer,
			group.map((x) => x.id),
			group.length > 1 ? s.id : undefined
		);
	}

	deselect() {
		this.sel = null;
	}

	/** Select strokes by id on a layer (its drawing showing now). */
	select(layer: number, ids: string[], primary?: string) {
		this.sel = ids.length
			? { layer, drawing: this.drawingOf(layer), ids, ...(primary ? { primary } : {}) }
			: null;
	}

	/**
	 * After undo, a frame change…: strokes that went drop out of the selection. Another
	 * drawing showing (a frame change) keeps the strokes it has too: the same drawing held
	 * over frames, or a copy of it (a duplicated frame has the same strokes).
	 */
	#keepSelection() {
		const s = this.sel;
		if (!s) return;
		const d = this.drawingOf(s.layer);
		if (!d) return void (this.sel = null);
		const here = new Set(pickable(this.strokesOf(s.layer)).map((x) => x.id));
		const ids = s.ids.filter((id) => here.has(id));
		if (ids.length !== s.ids.length || d !== s.drawing) {
			const primary = s.primary && ids.includes(s.primary) ? s.primary : undefined;
			this.sel = ids.length
				? { layer: s.layer, drawing: d, ids, ...(primary ? { primary } : {}) }
				: null;
		}
	}
	check() {
		this.#keepSelection();
	}

	/** Where the selection's handles are: 4 corners (scale), then the turn handle. */
	transformHandles(scale: number): Pt[] {
		const b = this.selBox();
		if (!b) return [];
		const pad = 8 / scale;
		const [x0, y0, x1, y1] = [b.x0 - pad, b.y0 - pad, b.x1 + pad, b.y1 + pad];
		return [
			[x0, y0],
			[x1, y0],
			[x1, y1],
			[x0, y1],
			[(x0 + x1) / 2, y0 - 24 / scale]
		];
	}

	/** the single selected stroke's draggable points (none for a QuickShape: its own) */
	pointHandles(): { id: string; hs: number[]; pts: Pt[] } | null {
		const p = this.primary();
		const [s, more] = p ? [p] : this.selected();
		if (!s || more || s.shape) return null;
		const hs = handles(s);
		return { id: s.id, hs, pts: hs.map((i) => [s.pts[i * 6], s.pts[i * 6 + 1]] as Pt) };
	}

	/** the single selected stroke, when it's a QuickShape */
	selectedShape(): { id: string; shape: EditShape } | null {
		const p = this.primary();
		const [s, more] = p ? [p] : this.selected();
		return s && !more && s.shape ? { id: s.id, shape: s.shape } : null;
	}

	// ---- the Select line tool ----

	/**
	 * A press with the Select line tool at canvas (x, y); `scale` is screen px per canvas
	 * px (handles keep their size on screen). Shift adds to the selection.
	 */
	down(layer: number, x: number, y: number, shift: boolean, scale: number) {
		if (this.#live) this.cancel();
		const list = this.strokesOf(layer);
		const tol = SEL_TOL / scale;
		const ids = new Set(this.sel?.layer === layer ? this.sel.ids : []);
		const near = (p: Pt) => Math.hypot(p[0] - x, p[1] - y) <= 12 / scale;
		if (ids.size && !shift) {
			const th = this.transformHandles(scale);
			const hit = th.findIndex(near);
			const box = this.selBox();
			if (hit === 4 && box) {
				const c: Pt = [(box.x0 + box.x1) / 2, (box.y0 + box.y1) / 2];
				this.#drag = { kind: 'rotate', start: [x, y], c, before: detach(list, ids), ids };
				return;
			}
			if (hit >= 0 && box) {
				this.#drag = { kind: 'scale', corner: hit, box, before: detach(list, ids), ids };
				return;
			}
			const ph = this.pointHandles();
			const pi = ph ? ph.pts.findIndex(near) : -1;
			if (ph && pi >= 0) {
				this.#drag = {
					kind: 'point',
					id: ph.id,
					hs: ph.hs,
					h: pi,
					start: [x, y],
					before: detach(list, new Set(this.sel?.ids ?? [ph.id]))
				};
				return;
			}
		}
		const i = hitTest(list, x, y, tol);
		if (i >= 0) {
			const id = list[i].id;
			const group = (this.host.symmetryOn() ? twins(list, list[i]) : [list[i]]).map((s) => s.id);
			if (shift) {
				const on = !ids.has(id);
				for (const g of group) {
					if (on) ids.add(g);
					else ids.delete(g);
				}
				this.select(layer, [...ids]);
				return;
			}
			if (!ids.has(id)) {
				this.#choose(layer, list, list[i]);
				ids.clear();
				for (const g of group) ids.add(g);
			}
			this.#drag = { kind: 'move', start: [x, y], before: detach(list, ids), ids };
			return;
		}
		// inside the selection's box: move it
		const b = this.selBox();
		if (!shift && b && x >= b.x0 && x <= b.x1 && y >= b.y0 && y <= b.y1 && ids.size) {
			this.#drag = { kind: 'move', start: [x, y], before: detach(list, ids), ids };
			return;
		}
		if (!shift) this.sel = null;
		this.#drag = { kind: 'box', start: [x, y], add: shift };
		this.marquee = [
			[x, y],
			[x, y]
		];
	}

	/** The Select line tool's press moved to (x, y). Shift keeps scaling in proportion. */
	move(layer: number, x: number, y: number, shift = false) {
		const d = this.#drag;
		if (!d) return;
		if (d.kind === 'box') {
			this.marquee = [d.start, [x, y]];
			return;
		}
		const ids = 'ids' in d ? d.ids : new Set<string>();
		// linked twins (mirrored with the one picked) get each edit mirrored
		const p = this.primary();
		const map = (m: Affine) =>
			d.before.map((s) => {
				if (!ids.has(s.id)) return s;
				if (!p?.link || !s.link || s.id === p.id) return transformStroke(s, m);
				return transformStroke(s, mirrored(m, p.link, s.link));
			});
		if (d.kind === 'move') {
			const [dx, dy] = [x - d.start[0], y - d.start[1]];
			this.#preview(layer, map([1, 0, 0, 1, dx, dy]));
		} else if (d.kind === 'scale') {
			const b = d.box;
			// the corner opposite stays put
			const fx = d.corner === 1 || d.corner === 2 ? b.x0 : b.x1;
			const fy = d.corner >= 2 ? b.y0 : b.y1;
			const gx = d.corner === 1 || d.corner === 2 ? b.x1 : b.x0;
			const gy = d.corner >= 2 ? b.y1 : b.y0;
			let sx = (x - fx) / (gx - fx || 1);
			let sy = (y - fy) / (gy - fy || 1);
			if (shift || !Number.isFinite(sx) || !Number.isFinite(sy) || gx === fx || gy === fy) {
				const k = Math.max(Math.abs(sx), Math.abs(sy));
				sx = Math.sign(sx || 1) * k;
				sy = Math.sign(sy || 1) * k;
			}
			const tiny = (v: number) => (Math.abs(v) < 0.02 ? Math.sign(v || 1) * 0.02 : v);
			[sx, sy] = [tiny(sx), tiny(sy)];
			this.#preview(layer, map([sx, 0, 0, sy, fx - fx * sx, fy - fy * sy]));
		} else if (d.kind === 'rotate') {
			const a0 = Math.atan2(d.start[1] - d.c[1], d.start[0] - d.c[0]);
			let a = Math.atan2(y - d.c[1], x - d.c[0]) - a0;
			if (shift) a = Math.round(a / (Math.PI / 12)) * (Math.PI / 12);
			this.#preview(layer, map(rotateAbout(a, d.c[0], d.c[1])));
		} else if (d.kind === 'point') {
			const [dx, dy] = [x - d.start[0], y - d.start[1]];
			const sel = new Set(this.sel?.ids ?? []);
			const from = d.before.find((s) => s.id === d.id)?.link;
			this.#preview(
				layer,
				d.before.map((s) => {
					if (s.id === d.id) return dragHandle(s, d.hs, d.h, dx, dy);
					if (!p || !from || !s.link || !sel.has(s.id) || s.link.g !== from.g) return s;
					const [mx, my] = mirroredDelta(dx, dy, from, s.link);
					return dragHandle(s, d.hs, d.h, mx, my);
				})
			);
		}
	}

	/** The Select line tool's press let go. */
	up(layer: number) {
		const d = this.#drag;
		this.#drag = null;
		if (!d) return;
		if (d.kind === 'box') {
			const m = this.marquee;
			this.marquee = null;
			if (!m) return;
			const box: Box = {
				x0: Math.min(m[0][0], m[1][0]),
				y0: Math.min(m[0][1], m[1][1]),
				x1: Math.max(m[0][0], m[1][0]),
				y1: Math.max(m[0][1], m[1][1])
			};
			if (box.x1 - box.x0 < 1 && box.y1 - box.y0 < 1) return;
			const list = this.strokesOf(layer);
			const ids = inBox(list, box).map((i) => list[i].id);
			const keep = d.add && this.sel?.layer === layer ? this.sel.ids : [];
			this.select(layer, [...new Set([...keep, ...ids])]);
			return;
		}
		const label = {
			move: 'Move lines',
			scale: 'Scale lines',
			rotate: 'Turn lines',
			point: 'Reshape line'
		}[d.kind];
		this.#finish(label);
	}

	/** Move the selection by (dx, dy) (arrow keys): one step. */
	nudge(dx: number, dy: number) {
		const s = this.sel;
		if (!s || this.#live) return;
		const ids = new Set(s.ids);
		// (erase marks over them go along, erasing only them)
		const list = detach(this.strokesOf(s.layer), ids);
		const p = this.primary();
		const m: Affine = [1, 0, 0, 1, dx, dy];
		void this.edit(
			s.layer,
			list.map((x) => {
				if (!ids.has(x.id)) return x;
				if (!p?.link || !x.link || x.id === p.id) return transformStroke(x, m);
				return transformStroke(x, mirrored(m, p.link, x.link));
			}),
			'Move lines'
		);
	}

	/** Delete the selected strokes (one step). */
	deleteSelected(label = 'Delete lines') {
		const s = this.sel;
		if (!s || this.#live) return false;
		const ids = new Set(s.ids);
		const list = this.strokesOf(s.layer);
		const after = list.filter((x) => !ids.has(x.id));
		if (after.length === list.length) return false;
		this.sel = null;
		void this.edit(s.layer, after, label);
		return true;
	}

	/**
	 * The selected strokes get another colour, size, opacity or brush (one step). The
	 * radius goes with the size and brush: `radiusOf` says what it is for a brush and size.
	 */
	async restyle(
		patch: Partial<Pick<LineStroke, 'color' | 'size' | 'opacity' | 'brush' | 'texture'>>,
		radiusOf: (brush: string, size: number) => number | null,
		label = 'Change lines'
	) {
		const s = this.sel;
		if (!s || this.#live) return false;
		if (patch.brush) await this.ready([{ brush: patch.brush } as LineStroke]);
		const ids = new Set(s.ids);
		const list = this.strokesOf(s.layer);
		let any = false;
		const after = list.map((x) => {
			if (!ids.has(x.id)) return x;
			const next = { ...x, ...patch };
			if (patch.texture === null) delete next.texture;
			if (patch.size !== undefined || patch.brush !== undefined) {
				const r = radiusOf(next.brush, next.size);
				if (r !== null) next.radius = r;
			}
			if (patch.brush !== undefined) {
				// (another brush reaches as far as it does)
				const b = this.host.brush(next.brush);
				delete next.reach;
				delete next.smudge;
				if (b) {
					const reach = Math.round(b.paintReach() * 100) / 100;
					if (reach > 2.5) next.reach = reach;
					if (b.smudgeReach() > 0) next.smudge = true;
				}
			}
			const same =
				next.color === x.color &&
				next.size === x.size &&
				next.opacity === x.opacity &&
				next.brush === x.brush &&
				JSON.stringify(next.texture ?? null) === JSON.stringify(x.texture ?? null);
			if (same) return x;
			any = true;
			return next;
		});
		if (!any) return false;
		return this.edit(s.layer, after, label);
	}

	/** A QuickShape stroke changed shape: drawn along its new outline (live, or one step). */
	reshape(layer: number, id: string, sh: EditShape, live: boolean) {
		const list = this.#live?.shown ?? this.strokesOf(layer);
		const along = (s: LineStroke, shape: EditShape) => ({
			...s,
			shape,
			pts: pointsAlong(resample(shapePath(shape), 3), shape.pressure)
		});
		// its linked twins take the shape mirrored: those picked with it, or (a shape just
		// snapped, with symmetry on) all of them
		const from = list.find((s) => s.id === id)?.link;
		const picked = this.primary() ? new Set(this.sel?.ids ?? []) : null;
		const linked = !!picked || this.host.symmetryOn();
		const next = list.map((s) => {
			if (s.id === id) return along(s, sh);
			if (!linked || !from || !s.link || s.link.g !== from.g) return s;
			if (picked && !picked.has(s.id)) return s;
			return along(s, transformShape(sh, twinMap(from, s.link)));
		});
		if (live) this.#preview(layer, next);
		else void this.edit(layer, next, 'Edit shape');
	}

	/** The end of a live reshape (handles dragged): one step. */
	reshaped() {
		this.#finish('Edit shape');
	}

	// ---- the eraser ----

	/** An eraser press on a line layer: whole strokes, or cut them (`r`: its radius). */
	eraseBegin(layer: number, x: number, y: number, r: number, mode: LineEraser) {
		if (this.#live) this.cancel();
		this.#erase = { layer, mode, r: Math.max(1, r), at: [x, y] };
		this.eraseTo(x, y);
	}

	eraseTo(x: number, y: number) {
		const er = this.#erase;
		if (!er) return;
		const list = this.#live?.shown ?? this.strokesOf(er.layer);
		const a = er.at;
		const b: Pt = [x, y];
		er.at = b;
		let next: LineStroke[];
		if (er.mode === 'whole') {
			const gone = new Set(eraseWhole(list, a, b, er.r));
			if (!gone.size) return;
			next = list.filter((_, i) => !gone.has(i));
		} else {
			let changed = false;
			next = list.flatMap((s) => {
				const cut = eraseCut(s, a, b, er.r);
				if (!cut) return [s];
				changed = true;
				return cut;
			});
			if (!changed) return;
		}
		this.#preview(er.layer, next);
	}

	eraseEnd() {
		if (!this.#erase) return;
		this.#erase = null;
		this.#finish('Erase lines');
		this.#keepSelection();
	}

	get erasing() {
		return !!this.#erase;
	}
	get dragging() {
		return !!this.#drag;
	}
}
