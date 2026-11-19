/** Raw input recorded for every stroke, used later by the grader's stroke-quality metrics. */
export interface StrokePoint {
	/** canvas pixels */
	x: number;
	y: number;
	/** 0–1 */
	pressure: number;
	/** ms since the drawing session started */
	t: number;
}

export interface Stroke {
	brushId: string;
	eraser: boolean;
	/** the layer it was painted on (paint engine layer id) */
	layer?: number;
	points: StrokePoint[];
}

type Action =
	| { kind: 'stroke'; stroke: Stroke }
	| { kind: 'clear'; layer: number; removed: Stroke[] }
	| { kind: 'merge'; from: number; to: number; moved: Stroke[] }
	/** add / delete / move / settings: no strokes change, but it's an undo step */
	| { kind: 'layer' };

/**
 * Mirrors the paint engine's undo history one-to-one (every engine step has a
 * matching action), so the strokes we send to the grader always match what is
 * on screen. Strokes on deleted or hidden layers are filtered out at export.
 */
export class StrokeLog {
	#all: Stroke[] = [];
	#undo: Action[] = [];
	#redo: Action[] = [];
	readonly limit: number;

	constructor(limit = 60) {
		this.limit = limit;
	}

	/** every stroke that is currently applied, on any layer */
	get strokes(): readonly Stroke[] {
		return this.#all;
	}

	/** strokes on the given layers (strokes without a layer always count) */
	on(layers: Set<number>): Stroke[] {
		return this.#all.filter((s) => s.layer === undefined || layers.has(s.layer));
	}

	/** One finished stroke. Empty strokes still take an undo step (the engine records one). */
	add(stroke: Stroke) {
		if (stroke.points.length) this.#all.push(stroke);
		this.#push({ kind: 'stroke', stroke });
	}

	/** The engine erased `layer`. */
	clear(layer: number) {
		const removed = this.#all.filter((s) => s.layer === layer);
		this.#all = this.#all.filter((s) => s.layer !== layer);
		this.#push({ kind: 'clear', layer, removed });
	}

	/** The engine merged layer `from` into `to`. */
	merge(from: number, to: number) {
		const moved = this.#all.filter((s) => s.layer === from);
		for (const s of moved) s.layer = to;
		this.#push({ kind: 'merge', from, to, moved });
	}

	/** Any other undoable layer change. */
	layerChange() {
		this.#push({ kind: 'layer' });
	}

	/** Forget undo/redo (the engine's history was cleared too); strokes stay. */
	forgetHistory() {
		this.#undo = [];
		this.#redo = [];
	}

	undo(): boolean {
		const a = this.#undo.pop();
		if (!a) return false;
		this.#revert(a);
		this.#redo.push(a);
		return true;
	}

	redo(): boolean {
		const a = this.#redo.pop();
		if (!a) return false;
		this.#reapply(a);
		this.#undo.push(a);
		return true;
	}

	#revert(a: Action) {
		if (a.kind === 'stroke') {
			const i = this.#all.lastIndexOf(a.stroke);
			if (i >= 0) this.#all.splice(i, 1);
		} else if (a.kind === 'clear') this.#all.push(...a.removed);
		else if (a.kind === 'merge') for (const s of a.moved) s.layer = a.from;
	}

	#reapply(a: Action) {
		if (a.kind === 'stroke') {
			if (a.stroke.points.length) this.#all.push(a.stroke);
		} else if (a.kind === 'clear') {
			const gone = new Set(a.removed);
			this.#all = this.#all.filter((s) => !gone.has(s));
		} else if (a.kind === 'merge') for (const s of a.moved) s.layer = a.to;
	}

	#push(a: Action) {
		this.#undo.push(a);
		if (this.#undo.length > this.limit) this.#undo.shift();
		this.#redo = [];
	}
}
