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
	/** brush colour (#rrggbb), size multiplier and opacity, for replays */
	color?: string;
	size?: number;
	opacity?: number;
	points: StrokePoint[];
}

type Action = (
	| { kind: 'stroke'; stroke: Stroke }
	| { kind: 'clear'; layer: number; removed: Stroke[] }
	| { kind: 'merge'; from: number; to: number; moved: Stroke[] }
	/** add / delete / move / settings: no strokes change, but it's an undo step */
	| { kind: 'layer' }
	/** several of the above as one engine undo step */
	| { kind: 'batch'; actions: Action[] }
) & {
	/** what the history panel calls this step */
	label?: string;
};

/** The history panel's name for an action without its own label. */
function labelOf(a: Action): string {
	if (a.label) return a.label;
	switch (a.kind) {
		case 'stroke':
			return a.stroke.eraser ? 'Eraser' : 'Brush stroke';
		case 'clear':
			return 'Clear layer';
		case 'merge':
			return 'Merge down';
		case 'batch':
			return a.actions[0] ? labelOf(a.actions[0]) : 'Change';
		default:
			return 'Layer change';
	}
}

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

	/** Record what `run` logs as one undo step (the engine did it in one). */
	batch(run: () => void) {
		const outer = this.#collect;
		const actions: Action[] = [];
		this.#collect = actions;
		try {
			run();
		} finally {
			this.#collect = outer;
		}
		this.#push({ kind: 'batch', actions });
	}

	#collect: Action[] | null = null;

	/** Any other undoable layer change. */
	layerChange() {
		this.#push({ kind: 'layer' });
	}

	/** Start over with these strokes (a drawing brought back from the history); no undo. */
	reset(strokes: Stroke[]) {
		this.#all = [...strokes];
		this.#undo = [];
		this.#redo = [];
	}

	/** Forget undo/redo (the engine's history was cleared too); strokes stay. */
	forgetHistory() {
		this.#undo = [];
		this.#redo = [];
	}

	/** Name the step just recorded (e.g. "Fill", "Transform") for the history panel. */
	labelLast(label: string) {
		const a = this.#collect ? this.#collect.at(-1) : this.#undo.at(-1);
		if (a) a.label = label;
	}

	/** Step names: `undo` oldest → newest (the last one is the current state), `redo` next → furthest. */
	history(): { undo: string[]; redo: string[] } {
		return {
			undo: this.#undo.map(labelOf),
			redo: [...this.#redo].reverse().map(labelOf)
		};
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
		else if (a.kind === 'batch') for (const x of [...a.actions].reverse()) this.#revert(x);
	}

	#reapply(a: Action) {
		if (a.kind === 'stroke') {
			if (a.stroke.points.length) this.#all.push(a.stroke);
		} else if (a.kind === 'clear') {
			const gone = new Set(a.removed);
			this.#all = this.#all.filter((s) => !gone.has(s));
		} else if (a.kind === 'merge') for (const s of a.moved) s.layer = a.to;
		else if (a.kind === 'batch') for (const x of a.actions) this.#reapply(x);
	}

	#push(a: Action) {
		if (this.#collect) return void this.#collect.push(a);
		this.#undo.push(a);
		if (this.#undo.length > this.limit) this.#undo.shift();
		this.#redo = [];
	}
}
