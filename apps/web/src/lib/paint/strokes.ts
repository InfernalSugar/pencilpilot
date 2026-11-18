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
	points: StrokePoint[];
}

type Action = { kind: 'stroke'; stroke: Stroke } | { kind: 'clear'; removed: Stroke[] };

/**
 * Keeps the list of visible strokes in step with the paint engine's undo/redo,
 * so what we send to the grader always matches what is on screen.
 */
export class StrokeLog {
	#visible: Stroke[] = [];
	#undo: Action[] = [];
	#redo: Action[] = [];
	readonly limit: number;

	constructor(limit = 60) {
		this.limit = limit;
	}

	get strokes(): readonly Stroke[] {
		return this.#visible;
	}

	add(stroke: Stroke) {
		if (stroke.points.length === 0) return;
		this.#visible.push(stroke);
		this.#push({ kind: 'stroke', stroke });
	}

	clear() {
		if (this.#visible.length === 0) return;
		const removed = this.#visible;
		this.#visible = [];
		this.#push({ kind: 'clear', removed });
	}

	undo(): boolean {
		const a = this.#undo.pop();
		if (!a) return false;
		if (a.kind === 'stroke') this.#visible.pop();
		else this.#visible = a.removed;
		this.#redo.push(a);
		return true;
	}

	redo(): boolean {
		const a = this.#redo.pop();
		if (!a) return false;
		if (a.kind === 'stroke') this.#visible.push(a.stroke);
		else {
			a.removed = this.#visible;
			this.#visible = [];
		}
		this.#undo.push(a);
		return true;
	}

	#push(a: Action) {
		this.#undo.push(a);
		if (this.#undo.length > this.limit) this.#undo.shift();
		this.#redo = [];
	}
}
