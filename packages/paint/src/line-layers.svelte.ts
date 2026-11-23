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
	dragHandle,
	eraseCut,
	eraseWhole,
	handles,
	hitTest,
	inBox,
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
		let tiles = tilesOf(boxes, this.host.width(), this.host.height());
		// (nothing on the canvas changes: one tile, so the step still exists)
		if (!tiles.length) tiles = new Int32Array([0, 0]);
		if (!e.linesBegin(layer, tiles)) return false;
		this.#replay(list, tiles);
		e.linesCommit();
		this.host.schedule();
		return true;
	}

	/** The strokes of `list` that reach `tiles`, replayed into the redraw begun. */
	#replay(list: readonly LineStroke[], tiles: Int32Array) {
		const e = this.host.engine();
		if (!e) return;
		// (the dab style is set only when it changes; the current brush's comes back after)
		let style: string | undefined;
		for (const s of strokesOnTiles(list, tiles)) {
			const b = this.host.brush(s.brush);
			if (!b) continue;
			b.setSizeScale(s.size);
			b.setOpacity(s.opacity);
			const n = parseInt(s.color.slice(1), 16) || 0;
			b.setColor((n >> 16) & 255, (n >> 8) & 255, n & 255);
			b.setEraser(false);
			const key = s.texture ? JSON.stringify(s.texture) : '';
			if (key !== style) {
				this.host.dabStyle(s.texture);
				style = key;
			}
			e.linesStroke(b, s.seed, s.pts);
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
		start: number[]
	): readonly LineStroke[] | null {
		const e = this.host.engine();
		if (!e || !start.length) return null;
		const plan = bakePlan(list, start, (s) => !!this.host.brush(s.brush));
		if (!plan.baked.length) return null;
		if (plan.replay.length && e.baseBegin(d, layer, plan.replay)) {
			this.#replay(plan.baked, plan.replay);
			e.linesCommit();
		}
		if (plan.drop.length) e.baseDrop(d, plan.drop);
		return plan.keep;
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
			const after = this.#bake(layer, d, before, start) ?? before;
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
			count += list.length;
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

	/** One edit, as one undo step: the strokes become `after`. */
	async edit(layer: number, after: readonly LineStroke[], label: string): Promise<boolean> {
		if (this.#live) return false;
		const before = this.strokesOf(layer);
		if (after === before) return false;
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

	/** After undo, a frame change…: strokes that went drop out of the selection. */
	#keepSelection() {
		const s = this.sel;
		if (!s) return;
		if (this.drawingOf(s.layer) !== s.drawing) return void (this.sel = null);
		const here = new Set(this.store.get(s.drawing).map((x) => x.id));
		const ids = s.ids.filter((id) => here.has(id));
		if (ids.length !== s.ids.length) {
			const primary = s.primary && ids.includes(s.primary) ? s.primary : undefined;
			this.sel = ids.length ? { layer: s.layer, drawing: s.drawing, ids, primary } : null;
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
				this.#drag = { kind: 'rotate', start: [x, y], c, before: list, ids };
				return;
			}
			if (hit >= 0 && box) {
				this.#drag = { kind: 'scale', corner: hit, box, before: list, ids };
				return;
			}
			const ph = this.pointHandles();
			const pi = ph ? ph.pts.findIndex(near) : -1;
			if (ph && pi >= 0) {
				this.#drag = { kind: 'point', id: ph.id, hs: ph.hs, h: pi, start: [x, y], before: list };
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
			this.#drag = { kind: 'move', start: [x, y], before: list, ids };
			return;
		}
		// inside the selection's box: move it
		const b = this.selBox();
		if (!shift && b && x >= b.x0 && x <= b.x1 && y >= b.y0 && y <= b.y1 && ids.size) {
			this.#drag = { kind: 'move', start: [x, y], before: list, ids };
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
		const list = this.strokesOf(s.layer);
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
	deleteSelected() {
		const s = this.sel;
		if (!s || this.#live) return false;
		const ids = new Set(s.ids);
		const list = this.strokesOf(s.layer);
		const after = list.filter((x) => !ids.has(x.id));
		if (after.length === list.length) return false;
		this.sel = null;
		void this.edit(s.layer, after, 'Delete lines');
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
		// its linked twins take the shape mirrored
		const from = list.find((s) => s.id === id)?.link;
		const sel = new Set(this.sel?.ids ?? []);
		const linked = !!this.primary();
		const next = list.map((s) => {
			if (s.id === id) return along(s, sh);
			if (!linked || !from || !s.link || s.link.g !== from.g || !sel.has(s.id)) return s;
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
