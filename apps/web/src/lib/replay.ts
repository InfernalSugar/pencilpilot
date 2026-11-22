/**
 * Replays: redraw a submitted drawing stroke by stroke from its recorded
 * strokes, with the same brushes, colours and layers.
 */
import {
	DEFAULT_BRUSH,
	loadBrush,
	loadEngine,
	PaintCanvas,
	type PaintBrush
} from '@pencilpilot/paint/brushes';
import type { Stroke } from '@pencilpilot/paint/strokes';

export interface ReplayLayer {
	id: number;
	opacity: number;
	blend: string;
	clip?: boolean;
}

/** Pauses longer than this (ms) are shortened to it, so replays don't drag. */
export const MAX_GAP = 600;

export interface Timeline {
	/** strokes in drawing order (empty strokes dropped) */
	strokes: Stroke[];
	/** replay time (ms) of every point, per stroke */
	times: number[][];
	/** replay time when each stroke ends */
	ends: number[];
	duration: number;
}

/** Order strokes by when they were drawn and give every point a replay time. */
export function timeline(strokes: readonly Stroke[], maxGap = MAX_GAP): Timeline {
	const list = strokes
		.filter((s) => s.points.length > 0)
		.slice()
		.sort((a, b) => a.points[0].t - b.points[0].t);
	const times: number[][] = [];
	const ends: number[] = [];
	let clock = 0;
	let last: number | null = null;
	for (const s of list) {
		const ts: number[] = [];
		for (const p of s.points) {
			if (last !== null) clock += Math.max(0, Math.min(maxGap, p.t - last));
			last = p.t;
			ts.push(clock);
		}
		times.push(ts);
		ends.push(clock);
	}
	return { strokes: list, times, ends, duration: clock };
}

const hex = (c: string): [number, number, number] => {
	const n = parseInt((c || '#16161a').slice(1), 16) || 0;
	return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};

/** A paint engine that redraws strokes (no input, no history). */
export class Replayer {
	readonly width: number;
	readonly height: number;
	memory!: WebAssembly.Memory;
	paint!: PaintCanvas;
	#layers: ReplayLayer[];
	#brushes = new Map<string, PaintBrush>();
	/** submission layer id → engine layer id */
	#ids = new Map<number, number>();

	private constructor(width: number, height: number, layers: ReplayLayer[]) {
		this.width = width;
		this.height = height;
		this.#layers = layers.length ? layers : [{ id: 1, opacity: 1, blend: 'normal' }];
	}

	static async create(
		width: number,
		height: number,
		layers: ReplayLayer[] | undefined,
		strokes: readonly Stroke[]
	): Promise<Replayer> {
		const r = new Replayer(width, height, layers ?? []);
		r.memory = await loadEngine();
		for (const id of new Set([...strokes.map((s) => s.brushId), DEFAULT_BRUSH])) {
			try {
				r.#brushes.set(id, await loadBrush(id));
			} catch {
				/* a brush that no longer exists: falls back to the default pencil */
			}
		}
		r.reset();
		return r;
	}

	/** Start again from a blank canvas with the drawing's layers. */
	reset() {
		this.paint?.free();
		this.paint = new PaintCanvas(this.width, this.height);
		this.#ids.clear();
		this.#layers.forEach((l, i) => {
			const id = i === 0 ? this.paint.activeLayer : this.paint.addLayer('');
			this.#ids.set(l.id, id);
			this.paint.setLayer(
				id,
				'',
				true,
				l.opacity,
				l.blend,
				false,
				false,
				!!l.clip,
				false,
				new Float32Array()
			);
		});
	}

	#brush(s: Stroke): PaintBrush | undefined {
		const b = this.#brushes.get(s.brushId) ?? this.#brushes.get(DEFAULT_BRUSH);
		if (!b) return undefined;
		b.setSizeScale(s.size ?? 1);
		b.setOpacity(s.opacity ?? 1);
		b.setColor(...hex(s.color ?? '#16161a'));
		b.setEraser(s.eraser);
		return b;
	}

	/** Start a stroke; false if it can't be drawn. */
	begin(s: Stroke): boolean {
		const layer = this.#ids.get(s.layer ?? -1) ?? this.paint.activeLayer;
		this.paint.setActiveLayer(layer);
		return !!this.#brush(s) && this.paint.beginStroke();
	}

	/** Feed point `i` of stroke `s`. */
	point(s: Stroke, i: number) {
		const b = this.#brush(s);
		if (!b) return;
		const p = s.points[i];
		const dt = i > 0 ? Math.max(0, (p.t - s.points[i - 1].t) / 1000) : 0;
		this.paint.strokeTo(b, p.x, p.y, p.pressure || 0.5, 0, 0, dt);
	}

	end(s: Stroke) {
		const b = this.#brush(s);
		if (b) this.paint.endStroke(b);
	}

	/** Draw whole strokes at once. */
	drawStrokes(strokes: readonly Stroke[]) {
		for (const s of strokes) {
			if (!this.begin(s)) continue;
			for (let i = 0; i < s.points.length; i++) this.point(s, i);
			this.end(s);
		}
	}

	/** Composite changed tiles; the changed rectangle (or null). */
	flush(): [number, number, number, number] | null {
		const r = this.paint.flush();
		return r.length === 4 ? [r[0], r[1], r[2], r[3]] : null;
	}

	/** The composited RGBA pixels (a view into engine memory: copy before the next stroke). */
	rgba(): Uint8ClampedArray {
		this.flush();
		return new Uint8ClampedArray(
			this.memory.buffer,
			this.paint.rgbaPtr(),
			this.width * this.height * 4
		);
	}

	free() {
		this.paint?.free();
		for (const b of this.#brushes.values()) b.free();
		this.#brushes.clear();
	}
}

/**
 * When did each tip's mistake appear? The drawing is graded at checkpoints
 * (30%, 40% … 100% of its strokes); a tip's moment is the first checkpoint
 * from which it keeps firing until the end. Returns replay times (ms) by tip id.
 */
export async function findMoments(
	tl: Timeline,
	replayer: Replayer,
	tipIds: string[],
	firedAt: (gray: Uint8Array, strokes: Stroke[]) => Promise<string[]>,
	toGray: (rgba: Uint8ClampedArray) => Uint8Array
): Promise<Record<string, number>> {
	const n = tl.strokes.length;
	if (!n || !tipIds.length) return {};
	const counts = [
		...new Set([3, 4, 5, 6, 7, 8, 9, 10].map((k) => Math.max(1, Math.ceil((n * k) / 10))))
	];
	replayer.reset();
	let drawn = 0;
	const fired: Set<string>[] = [];
	for (const c of counts) {
		replayer.drawStrokes(tl.strokes.slice(drawn, c));
		drawn = c;
		const gray = toGray(replayer.rgba());
		fired.push(new Set(await firedAt(gray, tl.strokes.slice(0, c))));
	}
	const out: Record<string, number> = {};
	for (const id of tipIds) {
		let k = counts.length;
		while (k > 0 && fired[k - 1].has(id)) k--;
		// k = first checkpoint of the final unbroken run (counts.length if it never fired)
		const at = Math.min(k, counts.length - 1);
		out[id] = tl.ends[counts[at] - 1];
	}
	return out;
}
