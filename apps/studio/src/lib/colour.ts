/**
 * Colouring an animation: a fill made on a colour layer (with its lines from a line-art
 * layer) can carry on through the following frames. Each next line drawing is cut into
 * the shapes its lines close off; the shape that overlaps most with the one filled
 * before is the same part of the character, and gets the same colour.
 *
 * Colour drawings follow the line drawings' timing: a colour drawing is held exactly as
 * long as the line drawing it colours.
 */
import { spell, unspell } from './frametools';
import { exposureAt, startOf, type Track } from './timeline';

/** Line pixels (alpha over `min`), grown by `grow` px so small gaps count as closed. */
export function walls(
	rgba: Uint8Array | Uint8ClampedArray,
	w: number,
	h: number,
	min = 96,
	grow = 1
) {
	const wall = new Uint8Array(w * h);
	for (let i = 0; i < wall.length; i++) wall[i] = rgba[i * 4 + 3] > min ? 1 : 0;
	if (grow <= 0) return wall;
	const out = new Uint8Array(wall);
	for (let y = 0; y < h; y++)
		for (let x = 0; x < w; x++) {
			if (!wall[y * w + x]) continue;
			for (let dy = -grow; dy <= grow; dy++)
				for (let dx = -grow; dx <= grow; dx++) {
					const [nx, ny] = [x + dx, y + dy];
					if (nx >= 0 && ny >= 0 && nx < w && ny < h) out[ny * w + nx] = 1;
				}
		}
	return out;
}

/** The shapes between the walls: each pixel's shape (0: a wall), and each shape's size. */
export function regions(wall: Uint8Array, w: number, h: number) {
	const labels = new Int32Array(w * h);
	const sizes = [0];
	const stack = new Int32Array(w * h);
	let next = 1;
	for (let start = 0; start < labels.length; start++) {
		if (wall[start] || labels[start]) continue;
		let top = 0;
		stack[top++] = start;
		labels[start] = next;
		let n = 0;
		while (top) {
			const i = stack[--top];
			n++;
			const x = i % w;
			const visit = (j: number) => {
				if (!wall[j] && !labels[j]) {
					labels[j] = next;
					stack[top++] = j;
				}
			};
			if (x > 0) visit(i - 1);
			if (x < w - 1) visit(i + 1);
			if (i >= w) visit(i - w);
			if (i + w < labels.length) visit(i + w);
		}
		sizes.push(n);
		next++;
	}
	return { labels, sizes };
}

/** Every pixel of the shape `label` (as indices). */
export function pixelsOf(labels: Int32Array, label: number): Uint32Array {
	let n = 0;
	for (let i = 0; i < labels.length; i++) if (labels[i] === label) n++;
	const out = new Uint32Array(n);
	let k = 0;
	for (let i = 0; i < labels.length; i++) if (labels[i] === label) out[k++] = i;
	return out;
}

/**
 * The shape that matches `prev` best (overlap over union), and how well (0–1). Shapes
 * much bigger than `prev` (the outside, when lines didn't close) score low by themselves.
 */
export function bestMatch(labels: Int32Array, sizes: number[], prev: Uint32Array) {
	const overlap = new Map<number, number>();
	for (const i of prev) {
		const l = labels[i];
		if (l) overlap.set(l, (overlap.get(l) ?? 0) + 1);
	}
	let best = { label: 0, score: 0 };
	for (const [l, o] of overlap) {
		const score = o / (prev.length + sizes[l] - o);
		if (score > best.score) best = { label: l, score };
	}
	return best;
}

/** A point well inside the shape: the pixel of it nearest the middle of where it overlaps `prev`. */
export function seedIn(labels: Int32Array, label: number, prev: Uint32Array, w: number) {
	let [sx, sy, n] = [0, 0, 0];
	for (const i of prev)
		if (labels[i] === label) {
			sx += i % w;
			sy += Math.floor(i / w);
			n++;
		}
	const [cx, cy] = n ? [sx / n, sy / n] : [0, 0];
	let best = -1;
	let bd = Infinity;
	for (const i of prev) {
		if (labels[i] !== label) continue;
		const d = ((i % w) - cx) ** 2 + (Math.floor(i / w) - cy) ** 2;
		if (d < bd) [bd, best] = [d, i];
	}
	return best < 0 ? null : { x: best % w, y: Math.floor(best / w) };
}

/**
 * The colour drawing for the line drawing exposed on `frame`: held exactly as long as
 * the line drawing. One already there for just that span is kept (and stretched over
 * the span); one shared with other spans is copied (`make(drawing)`); none: a new one
 * (`make(0)`). Returns it, or 0 when there's no line drawing there.
 */
export function colourFor(
	colour: Track,
	line: Track,
	frame: number,
	make: (from: number) => number
) {
	const at = exposureAt(line, frame);
	if (!at || !line[at.index].drawing) return 0;
	const s = startOf(line, at.index);
	const e = s + line[at.index].length;
	const f = spell(colour, e);
	const inSpan = new Set(f.slice(s, e).filter(Boolean));
	const outside = new Set([...f.slice(0, s), ...f.slice(e)].filter(Boolean));
	const own = f[s] && !outside.has(f[s]) ? f[s] : [...inSpan].find((d) => !outside.has(d));
	const d = own ?? make([...inSpan][0] ?? 0);
	for (let i = s; i < e; i++) f[i] = d;
	colour.splice(0, colour.length, ...unspell(f));
	return d;
}

/** Where the line drawings after `frame` start, up to frame `last` (in order). */
export function lineStartsAfter(line: Track, frame: number, last: number): number[] {
	const out: number[] = [];
	line.forEach((e, i) => {
		const s = startOf(line, i);
		if (e.drawing && s > frame && s <= last) out.push(s);
	});
	return out;
}
