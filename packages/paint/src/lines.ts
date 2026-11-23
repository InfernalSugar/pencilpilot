/**
 * Line layers (vector, as Clip Studio's): every stroke on them stays editable. The app
 * keeps each drawing's strokes here; the paint engine draws them, through the same
 * brush engine as any stroke, into the layer's pixels (so they look the same), and
 * redraws only the tiles an edit touches.
 *
 * Plain logic (no engine, no DOM): bounds, hit tests, the eraser's cuts, transforms,
 * point dragging, symmetry copies, boil, and the binary form strokes are saved and
 * shared in. Strokes are never changed in place: an edit makes a new stroke (with the
 * same id), so a drawing's list can be kept for undo as it is.
 */
import type { BrushTexture } from './brush-textures';
import type { EditShape, Pt } from './shape-edit';

/** numbers per point: x, y (canvas px), pressure (0–1), x and y tilt (−1–1), dtime (s) */
export const STRIDE = 6;

export interface LineStroke {
	/** unique (shared drawings mix strokes from several apps) */
	id: string;
	/** points, `STRIDE` numbers each, exactly as they were painted */
	pts: Float32Array;
	/** the app's brush id */
	brush: string;
	/** #rrggbb */
	color: string;
	/** the brush's size multiplier as painted */
	size: number;
	/** 0–1 */
	opacity: number;
	/** the brush's radius at that size (canvas px), for bounds and hit tests */
	radius: number;
	/** the brush engine's random seed: redrawing paints the same pixels */
	seed: number;
	/** grain, picture tip and dual brush (none: plain) */
	texture?: BrushTexture | null;
	/** a QuickShape: its corners / axes come back to edit when it's selected */
	shape?: EditShape;
	/** made with symmetry on: its mirrored / turned twins share `g` (see `Link`) */
	link?: Link;
}

/**
 * Strokes one stroke made with symmetry on are linked: `g` names the group, `k` says
 * which copy this is (0: the one drawn), and the symmetry they were made with says how
 * to carry an edit from one to the others.
 */
export interface Link {
	g: string;
	k: number;
	mode: SymmetryMode;
	n: number;
	cx: number;
	cy: number;
}

export interface Box {
	x0: number;
	y0: number;
	x1: number;
	y1: number;
}

/** an affine transform as canvas 2D takes it: x' = a·x + c·y + e, y' = b·x + d·y + f */
export type Affine = [number, number, number, number, number, number];

/** Most strokes a drawing may hold, and points a stroke may have (files and sharing). */
export const MAX_STROKES = 20000;
export const MAX_POINTS = 100000;

const rand32 = () => (Math.random() * 2 ** 32) >>> 0;

/** A new stroke id. */
export function newId(): string {
	return rand32().toString(36).padStart(7, '0') + rand32().toString(36).padStart(7, '0');
}

/** A new random seed for the brush engine. */
export const newSeed = rand32;

export const pointCount = (s: LineStroke) => Math.floor(s.pts.length / STRIDE);

// ---- bounds ----

const boundsCache = new WeakMap<LineStroke, Box>();

/** How far paint may reach past the path: brushes vary their dabs with pressure. */
export const reach = (s: LineStroke) => s.radius * 2.5 + 3;

/** The area a stroke's paint can cover (canvas px). */
export function bounds(s: LineStroke): Box {
	const known = boundsCache.get(s);
	if (known) return known;
	let [x0, y0, x1, y1] = [Infinity, Infinity, -Infinity, -Infinity];
	const p = s.pts;
	for (let i = 0; i + 1 < p.length; i += STRIDE) {
		if (p[i] < x0) x0 = p[i];
		if (p[i] > x1) x1 = p[i];
		if (p[i + 1] < y0) y0 = p[i + 1];
		if (p[i + 1] > y1) y1 = p[i + 1];
	}
	const m = reach(s);
	const b = x0 <= x1 ? { x0: x0 - m, y0: y0 - m, x1: x1 + m, y1: y1 + m } : { x0, y0, x1, y1 };
	boundsCache.set(s, b);
	return b;
}

export const touches = (a: Box, b: Box) =>
	a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1;

export function union(boxes: Box[]): Box | null {
	const ok = boxes.filter((b) => b.x0 <= b.x1);
	if (!ok.length) return null;
	return {
		x0: Math.min(...ok.map((b) => b.x0)),
		y0: Math.min(...ok.map((b) => b.y0)),
		x1: Math.max(...ok.map((b) => b.x1)),
		y1: Math.max(...ok.map((b) => b.y1))
	};
}

/** The path's own box (no brush width). */
export function pathBox(strokes: readonly LineStroke[]): Box | null {
	let [x0, y0, x1, y1] = [Infinity, Infinity, -Infinity, -Infinity];
	for (const s of strokes)
		for (let i = 0; i + 1 < s.pts.length; i += STRIDE) {
			x0 = Math.min(x0, s.pts[i]);
			x1 = Math.max(x1, s.pts[i]);
			y0 = Math.min(y0, s.pts[i + 1]);
			y1 = Math.max(y1, s.pts[i + 1]);
		}
	return x0 <= x1 ? { x0, y0, x1, y1 } : null;
}

/**
 * The tiles (`tile` px squares of a `w`×`h` canvas, and the ring just outside it) the boxes cover, as flat
 * (tx, ty) pairs for the engine.
 */
export function tilesOf(boxes: Box[], w: number, h: number, tile = 64): Int32Array {
	// (one tile past each edge too: paint spills off the canvas, and must move with its line)
	const keys = new Set<number>();
	const cols = Math.ceil(w / tile) + 2;
	const rows = Math.ceil(h / tile) + 2;
	for (const b of boxes) {
		if (!(b.x0 <= b.x1)) continue;
		const tx0 = Math.max(-1, Math.floor(b.x0 / tile));
		const ty0 = Math.max(-1, Math.floor(b.y0 / tile));
		const tx1 = Math.min(cols - 2, Math.floor(b.x1 / tile));
		const ty1 = Math.min(rows - 2, Math.floor(b.y1 / tile));
		for (let ty = ty0; ty <= ty1; ty++)
			for (let tx = tx0; tx <= tx1; tx++) keys.add((ty + 1) * cols + tx + 1);
	}
	const out = new Int32Array(keys.size * 2);
	let i = 0;
	for (const k of keys) {
		out[i++] = (k % cols) - 1;
		out[i++] = Math.floor(k / cols) - 1;
	}
	return out;
}

/** The strokes (bottom → top) whose paint reaches any of the tiles. */
export function strokesOnTiles(
	strokes: readonly LineStroke[],
	tiles: Int32Array,
	tile = 64
): LineStroke[] {
	const set = new Set<string>();
	for (let i = 0; i + 1 < tiles.length; i += 2) set.add(`${tiles[i]},${tiles[i + 1]}`);
	return strokes.filter((s) => {
		const b = bounds(s);
		if (!(b.x0 <= b.x1)) return false;
		const tx0 = Math.floor(b.x0 / tile);
		const ty0 = Math.floor(b.y0 / tile);
		const tx1 = Math.floor(b.x1 / tile);
		const ty1 = Math.floor(b.y1 / tile);
		for (let ty = ty0; ty <= ty1; ty++)
			for (let tx = tx0; tx <= tx1; tx++) if (set.has(`${tx},${ty}`)) return true;
		return false;
	});
}

// ---- hit tests ----

function segDist(px: number, py: number, ax: number, ay: number, bx: number, by: number) {
	const dx = bx - ax;
	const dy = by - ay;
	const l2 = dx * dx + dy * dy;
	const t = l2 ? Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / l2)) : 0;
	return Math.hypot(px - ax - t * dx, py - ay - t * dy);
}

/** Distance from (x, y) to a stroke's path. */
export function distTo(s: LineStroke, x: number, y: number): number {
	const p = s.pts;
	const n = pointCount(s);
	if (!n) return Infinity;
	if (n === 1) return Math.hypot(x - p[0], y - p[1]);
	let best = Infinity;
	for (let i = STRIDE; i + 1 < p.length; i += STRIDE) {
		const d = segDist(x, y, p[i - STRIDE], p[i - STRIDE + 1], p[i], p[i + 1]);
		if (d < best) best = d;
	}
	return best;
}

/** Distance between segment ab and a stroke's path (0 when they cross). */
function distToSeg(s: LineStroke, ax: number, ay: number, bx: number, by: number): number {
	const p = s.pts;
	const n = pointCount(s);
	if (!n) return Infinity;
	let best = Infinity;
	for (let i = 0; i + 1 < p.length; i += STRIDE) {
		best = Math.min(best, segDist(p[i], p[i + 1], ax, ay, bx, by));
		if (i >= STRIDE) {
			const [cx, cy, dx, dy] = [p[i - STRIDE], p[i - STRIDE + 1], p[i], p[i + 1]];
			if (segmentsCross(ax, ay, bx, by, cx, cy, dx, dy)) return 0;
			best = Math.min(best, segDist(ax, ay, cx, cy, dx, dy), segDist(bx, by, cx, cy, dx, dy));
		}
	}
	return best;
}

function segmentsCross(
	ax: number,
	ay: number,
	bx: number,
	by: number,
	cx: number,
	cy: number,
	dx: number,
	dy: number
) {
	const o = (px: number, py: number, qx: number, qy: number, rx: number, ry: number) =>
		Math.sign((qx - px) * (ry - py) - (qy - py) * (rx - px));
	return (
		o(ax, ay, bx, by, cx, cy) * o(ax, ay, bx, by, dx, dy) < 0 &&
		o(cx, cy, dx, dy, ax, ay) * o(cx, cy, dx, dy, bx, by) < 0
	);
}

/** How close a tap must come to a stroke's path to pick it: its half width, and `tol`. */
const pickReach = (s: LineStroke, tol: number) => Math.max(1.5, s.radius) + tol;

/**
 * The stroke under (x, y): the topmost whose path passes within its half width plus
 * `tol` (canvas px); its index, or −1.
 */
export function hitTest(strokes: readonly LineStroke[], x: number, y: number, tol: number): number {
	// a stroke whose paint is right under the point wins; else the nearest within reach
	let near = -1;
	let nearD = Infinity;
	for (let i = strokes.length - 1; i >= 0; i--) {
		const s = strokes[i];
		const r = pickReach(s, tol);
		const b = bounds(s);
		if (x < b.x0 - r || x > b.x1 + r || y < b.y0 - r || y > b.y1 + r) continue;
		const d = distTo(s, x, y);
		if (d <= Math.max(1.5, s.radius)) return i;
		if (d <= r && d - r < nearD) [near, nearD] = [i, d - r];
	}
	return near;
}

/** Indices of the strokes with part of their path inside the box. */
export function inBox(strokes: readonly LineStroke[], box: Box): number[] {
	const out: number[] = [];
	strokes.forEach((s, i) => {
		const p = s.pts;
		for (let k = 0; k + 1 < p.length; k += STRIDE)
			if (p[k] >= box.x0 && p[k] <= box.x1 && p[k + 1] >= box.y0 && p[k + 1] <= box.y1) {
				out.push(i);
				return;
			}
	});
	return out;
}

// ---- the eraser ----

/**
 * Whole-line eraser: the strokes the eraser touches moving from a to b with radius `r`
 * (their paint, not just their path). Indices.
 */
export function eraseWhole(strokes: readonly LineStroke[], a: Pt, b: Pt, r: number): number[] {
	const out: number[] = [];
	const box: Box = {
		x0: Math.min(a[0], b[0]) - r,
		y0: Math.min(a[1], b[1]) - r,
		x1: Math.max(a[0], b[0]) + r,
		y1: Math.max(a[1], b[1]) + r
	};
	strokes.forEach((s, i) => {
		if (!touches(bounds(s), box)) return;
		if (distToSeg(s, a[0], a[1], b[0], b[1]) <= r + s.radius * 0.5) out.push(i);
	});
	return out;
}

/**
 * Normal eraser on one stroke: the points the eraser covers (moving from a to b with
 * radius `r`) go, and what's left on either side becomes strokes of its own. Null when
 * it doesn't touch the stroke; [] when nothing is left.
 */
export function eraseCut(s: LineStroke, a: Pt, b: Pt, r: number): LineStroke[] | null {
	const p = s.pts;
	const n = pointCount(s);
	const reachR = r + s.radius * 0.35;
	const gone = new Uint8Array(n);
	let any = false;
	for (let i = 0; i < n; i++) {
		if (segDist(p[i * STRIDE], p[i * STRIDE + 1], a[0], a[1], b[0], b[1]) <= reachR) {
			gone[i] = 1;
			any = true;
		}
	}
	if (!any) return null;
	const pieces: LineStroke[] = [];
	let start = -1;
	for (let i = 0; i <= n; i++) {
		if (i < n && !gone[i]) {
			if (start < 0) start = i;
			continue;
		}
		if (start >= 0) {
			// a lone point left over is a dot nobody meant to keep (unless it was a dot)
			if (i - start >= 2) {
				const pts = p.slice(start * STRIDE, i * STRIDE);
				pts[5] = 0;
				pieces.push({
					...s,
					id: newId(),
					seed: (s.seed + pieces.length * 7919 + 1) >>> 0,
					pts,
					shape: undefined
				});
			}
			start = -1;
		}
	}
	return pieces.map(clean);
}

/** No `shape: undefined` left lying around (it would travel as a key). */
function clean(s: LineStroke): LineStroke {
	if (s.shape === undefined && 'shape' in s) {
		const { shape: _, ...rest } = s;
		void _;
		return rest;
	}
	return s;
}

// ---- edits ----

/** A stroke moved, scaled, turned (its brush scales along). */
export function transformStroke(s: LineStroke, m: Affine): LineStroke {
	const [a, b, c, d, e, f] = m;
	const pts = new Float32Array(s.pts.length);
	for (let i = 0; i + STRIDE - 1 < s.pts.length; i += STRIDE) {
		const [x, y] = [s.pts[i], s.pts[i + 1]];
		pts[i] = a * x + c * y + e;
		pts[i + 1] = b * x + d * y + f;
		pts[i + 2] = s.pts[i + 2];
		pts[i + 3] = s.pts[i + 3];
		pts[i + 4] = s.pts[i + 4];
		pts[i + 5] = s.pts[i + 5];
	}
	const k = Math.sqrt(Math.abs(a * d - b * c)) || 1;
	const out: LineStroke = {
		...s,
		pts,
		size: s.size * k,
		radius: s.radius * k
	};
	if (s.shape) out.shape = transformShape(s.shape, m);
	return out;
}

function mapPt(m: Affine, p: Pt): Pt {
	return [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]];
}

export function transformShape(sh: EditShape, m: Affine): EditShape {
	if (sh.kind === 'poly') return { ...sh, pts: sh.pts.map((p) => mapPt(m, p)) };
	const [cs, sn] = [Math.cos(sh.theta), Math.sin(sh.theta)];
	const u: Pt = [m[0] * sh.a * cs + m[2] * sh.a * sn, m[1] * sh.a * cs + m[3] * sh.a * sn];
	const v: Pt = [-m[0] * sh.b * sn + m[2] * sh.b * cs, -m[1] * sh.b * sn + m[3] * sh.b * cs];
	const a = Math.hypot(u[0], u[1]);
	const b = Math.hypot(v[0], v[1]);
	return {
		...sh,
		c: mapPt(m, sh.c),
		a,
		b,
		theta: Math.atan2(u[1], u[0]),
		circle: sh.circle && Math.abs(a - b) < 0.01 * Math.max(a, b, 1)
	};
}

/** Every stroke of a drawing moved and scaled (a canvas resize: offset, then scale). */
export function offsetScale(
	strokes: readonly LineStroke[],
	dx: number,
	dy: number,
	scale = 1
): LineStroke[] {
	const m: Affine = [scale, 0, 0, scale, dx * scale, dy * scale];
	return strokes.map((s) => transformStroke(s, m));
}

/** The transform that maps the box `from` onto `to` (both as [x0, y0, x1, y1]). */
export function boxToBox(from: Box, to: Box): Affine {
	const sx = (to.x1 - to.x0) / (from.x1 - from.x0 || 1);
	const sy = (to.y1 - to.y0) / (from.y1 - from.y0 || 1);
	return [sx, 0, 0, sy, to.x0 - from.x0 * sx, to.y0 - from.y0 * sy];
}

/** Rotation by `rad` about (cx, cy). */
export function rotateAbout(rad: number, cx: number, cy: number): Affine {
	const [c, s] = [Math.cos(rad), Math.sin(rad)];
	return [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy];
}

/** The points of a stroke as [x, y] pairs. */
export function pathOf(s: LineStroke): Pt[] {
	const out: Pt[] = [];
	for (let i = 0; i + 1 < s.pts.length; i += STRIDE) out.push([s.pts[i], s.pts[i + 1]]);
	return out;
}

function rdp(p: Float32Array, i0: number, i1: number, eps: number, keep: Uint8Array) {
	let far = 0;
	let idx = -1;
	for (let i = i0 + 1; i < i1; i++) {
		const d = segDist(
			p[i * STRIDE],
			p[i * STRIDE + 1],
			p[i0 * STRIDE],
			p[i0 * STRIDE + 1],
			p[i1 * STRIDE],
			p[i1 * STRIDE + 1]
		);
		if (d > far) [far, idx] = [d, i];
	}
	if (idx >= 0 && far > eps) {
		keep[idx] = 1;
		rdp(p, i0, idx, eps, keep);
		rdp(p, idx, i1, eps, keep);
	}
}

/**
 * The points to drag (point indices): the corners of the path, simplified, at most
 * `max` of them, always both ends.
 */
export function handles(s: LineStroke, max = 24): number[] {
	const n = pointCount(s);
	if (n <= 2) return n === 2 ? [0, 1] : n ? [0] : [];
	let eps = Math.max(1.5, s.radius * 0.5);
	for (let tries = 0; tries < 12; tries++) {
		const keep = new Uint8Array(n);
		keep[0] = keep[n - 1] = 1;
		rdp(s.pts, 0, n - 1, eps, keep);
		const out: number[] = [];
		keep.forEach((k, i) => k && out.push(i));
		if (out.length <= max) return out;
		eps *= 1.6;
	}
	return [0, n - 1];
}

/**
 * The stroke with handle `h` (of `hs`, from `handles`) dragged by (dx, dy): the point
 * moves all the way, its neighbours less the farther along the line they are, nothing
 * past the next handles. Its QuickShape goes (it's freehand now).
 */
export function dragHandle(
	s: LineStroke,
	hs: number[],
	h: number,
	dx: number,
	dy: number
): LineStroke {
	const n = pointCount(s);
	const at = hs[h];
	const from = h > 0 ? hs[h - 1] : at;
	const to = h < hs.length - 1 ? hs[h + 1] : at;
	const pts = s.pts.slice();
	// arc length along the path, for even falloff however the points are spaced
	const len = new Float64Array(n);
	for (let i = 1; i < n; i++)
		len[i] =
			len[i - 1] +
			Math.hypot(
				s.pts[i * STRIDE] - s.pts[(i - 1) * STRIDE],
				s.pts[i * STRIDE + 1] - s.pts[(i - 1) * STRIDE + 1]
			);
	const ease = (t: number) => 0.5 - 0.5 * Math.cos(Math.PI * Math.min(1, Math.max(0, t)));
	for (let i = from; i <= to; i++) {
		let w = 1;
		if (i < at) w = ease((len[i] - len[from]) / (len[at] - len[from] || 1));
		else if (i > at) w = ease((len[to] - len[i]) / (len[to] - len[at] || 1));
		pts[i * STRIDE] += dx * w;
		pts[i * STRIDE + 1] += dy * w;
	}
	return clean({ ...s, pts, shape: undefined });
}

/**
 * Points along a path (`path` of [x, y]) as stroke points at one pressure, `dt` apart:
 * a QuickShape's outline, drawn again.
 */
export function pointsAlong(path: Pt[], pressure: number, dt = 0.008): Float32Array {
	const pts = new Float32Array(path.length * STRIDE);
	path.forEach(([x, y], i) => {
		pts.set([x, y, pressure, 0, 0, dt], i * STRIDE);
	});
	return pts;
}

// ---- symmetry and boil ----

export type SymmetryMode = 'vertical' | 'horizontal' | 'quadrant' | 'radial';

/**
 * Where symmetry's copies of a point go (not the point itself), in the engine's order
 * (`brush::Symmetry::copies`): copy k of a line stroke is painted with seed + k + 1.
 */
export function symmetryCopies(
	mode: SymmetryMode,
	n: number,
	cx: number,
	cy: number,
	x: number,
	y: number
): Pt[] {
	switch (mode) {
		case 'vertical':
			return [[2 * cx - x, y]];
		case 'horizontal':
			return [[x, 2 * cy - y]];
		case 'quadrant':
			return [
				[2 * cx - x, y],
				[x, 2 * cy - y],
				[2 * cx - x, 2 * cy - y]
			];
		case 'radial': {
			const k = Math.min(24, Math.max(2, Math.round(n)));
			return Array.from({ length: k - 1 }, (_, i) => {
				const a = (Math.fround(i + 1) * Math.fround(Math.PI * 2)) / k;
				const [dx, dy] = [x - cx, y - cy];
				return [cx + dx * Math.cos(a) - dy * Math.sin(a), cy + dx * Math.sin(a) + dy * Math.cos(a)];
			});
		}
	}
}

/** The affine that takes the drawn stroke onto copy `k` of a link. */
export function linkMap(l: Link): Affine {
	const { mode, k, cx, cy } = l;
	if (k <= 0) return [1, 0, 0, 1, 0, 0];
	const flipX: Affine = [-1, 0, 0, 1, 2 * cx, 0];
	const flipY: Affine = [1, 0, 0, -1, 0, 2 * cy];
	switch (mode) {
		case 'vertical':
			return flipX;
		case 'horizontal':
			return flipY;
		case 'quadrant':
			return k === 1 ? flipX : k === 2 ? flipY : [-1, 0, 0, -1, 2 * cx, 2 * cy];
		case 'radial': {
			const n = Math.min(24, Math.max(2, Math.round(l.n)));
			return rotateAbout((k * Math.PI * 2) / n, cx, cy);
		}
	}
}

/** `a` after `b` (apply b, then a). */
export function compose(a: Affine, b: Affine): Affine {
	return [
		a[0] * b[0] + a[2] * b[1],
		a[1] * b[0] + a[3] * b[1],
		a[0] * b[2] + a[2] * b[3],
		a[1] * b[2] + a[3] * b[3],
		a[0] * b[4] + a[2] * b[5] + a[4],
		a[1] * b[4] + a[3] * b[5] + a[5]
	];
}

export function invert(m: Affine): Affine {
	const det = m[0] * m[3] - m[1] * m[2] || 1;
	const [a, b, c, d] = [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det];
	return [a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])];
}

/** What takes stroke `from` onto its twin `to` (both of one link). */
export const twinMap = (from: Link, to: Link): Affine =>
	compose(linkMap(to), invert(linkMap(from)));

/** An edit `m` made to `from`, as its twin `to` gets it (mirrored, turned). */
export function mirrored(m: Affine, from: Link, to: Link): Affine {
	const t = twinMap(from, to);
	return compose(t, compose(m, invert(t)));
}

/** A drag by (dx, dy) on `from`, as its twin `to` gets it. */
export function mirroredDelta(dx: number, dy: number, from: Link, to: Link): Pt {
	const t = twinMap(from, to);
	return [t[0] * dx + t[2] * dy, t[1] * dx + t[3] * dy];
}

/** The strokes linked with `s` (itself too), in `list`. */
export function twins(list: readonly LineStroke[], s: LineStroke): LineStroke[] {
	const g = s.link?.g;
	return g ? list.filter((x) => x.link?.g === g) : [s];
}

/** A stroke and its symmetry copies, linked (see `Link`). */
export function linkedCopies(
	s: LineStroke,
	mode: SymmetryMode,
	n: number,
	cx: number,
	cy: number
): LineStroke[] {
	const g = newId();
	const link = (k: number): Link => ({ g, k, mode, n, cx, cy });
	return [
		{ ...s, link: link(0) },
		...symmetryStrokes(s, mode, n, cx, cy).map((c, i) => ({ ...c, link: link(i + 1) }))
	];
}

/** A stroke's mirrored / turned copies (no tilt, as the engine paints them). */
export function symmetryStrokes(
	s: LineStroke,
	mode: SymmetryMode,
	n: number,
	cx: number,
	cy: number
): LineStroke[] {
	const count = symmetryCopies(mode, n, cx, cy, 0, 0).length;
	const copies = Array.from({ length: count }, () => new Float32Array(s.pts.length));
	for (let i = 0; i + STRIDE - 1 < s.pts.length; i += STRIDE) {
		const c = symmetryCopies(mode, n, cx, cy, s.pts[i], s.pts[i + 1]);
		c.forEach(([x, y], k) => {
			copies[k].set([x, y, s.pts[i + 2], 0, 0, s.pts[i + 5]], i);
		});
	}
	return copies.map((pts, k) =>
		clean({ ...s, id: newId(), seed: (s.seed + k + 1) >>> 0, pts, shape: undefined })
	);
}

/** A small seeded random generator (mulberry32). */
function mulberry(seed: number) {
	let t = seed >>> 0;
	return () => {
		t = (t + 0x6d2b79f5) >>> 0;
		let r = Math.imul(t ^ (t >>> 15), 1 | t);
		r ^= r + Math.imul(r ^ (r >>> 7), 61 | r);
		return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
	};
}

/**
 * Boiled strokes (a drawing traced again): each path wobbles smoothly, up to `amount`
 * px, differently for each seed. New ids.
 */
export function boilStrokes(strokes: readonly LineStroke[], amount: number, seed: number) {
	const rnd = mulberry(seed);
	const wave = 40; // px between wobble knots along a line
	return strokes.map((s) => {
		const n = pointCount(s);
		const pts = s.pts.slice();
		let length = 0;
		for (let i = 1; i < n; i++)
			length += Math.hypot(
				s.pts[i * STRIDE] - s.pts[(i - 1) * STRIDE],
				s.pts[i * STRIDE + 1] - s.pts[(i - 1) * STRIDE + 1]
			);
		const knots = Math.max(2, Math.ceil(length / wave) + 1);
		const kx = Array.from({ length: knots }, () => (rnd() * 2 - 1) * amount);
		const ky = Array.from({ length: knots }, () => (rnd() * 2 - 1) * amount);
		let at = 0;
		for (let i = 0; i < n; i++) {
			if (i)
				at += Math.hypot(
					s.pts[i * STRIDE] - s.pts[(i - 1) * STRIDE],
					s.pts[i * STRIDE + 1] - s.pts[(i - 1) * STRIDE + 1]
				);
			const u = length ? (at / length) * (knots - 1) : 0;
			const k = Math.min(knots - 2, Math.floor(u));
			const t = 0.5 - 0.5 * Math.cos(Math.PI * (u - k));
			pts[i * STRIDE] += kx[k] + (kx[k + 1] - kx[k]) * t;
			pts[i * STRIDE + 1] += ky[k] + (ky[k + 1] - ky[k]) * t;
		}
		return clean({ ...s, id: newId(), pts, shape: undefined });
	});
}

// ---- saving and sharing: "PPL1", u32 JSON length, JSON (the strokes' settings),
// padding to 4 bytes, then every stroke's points as little-endian f32 ----

const MAGIC = [0x50, 0x50, 0x4c, 0x31]; // "PPL1"

interface Meta {
	id: string;
	n: number;
	brush: string;
	color: string;
	size: number;
	opacity: number;
	radius: number;
	seed: number;
	texture?: BrushTexture;
	shape?: EditShape;
	link?: Link;
}

/** Strokes as bytes (a drawing in a project file, or an edit shared with others). */
export function encodeLines(strokes: readonly LineStroke[]): Uint8Array {
	const meta: Meta[] = strokes.map((s) => ({
		id: s.id,
		n: pointCount(s),
		brush: s.brush,
		color: s.color,
		size: s.size,
		opacity: s.opacity,
		radius: s.radius,
		seed: s.seed,
		...(s.texture ? { texture: s.texture } : {}),
		...(s.shape ? { shape: s.shape } : {}),
		...(s.link ? { link: s.link } : {})
	}));
	const json = new TextEncoder().encode(JSON.stringify(meta));
	const head = 8 + json.length;
	const pad = (4 - (head % 4)) % 4;
	const floats = meta.reduce((t, m) => t + m.n * STRIDE, 0);
	const out = new Uint8Array(head + pad + floats * 4);
	out.set(MAGIC, 0);
	const dv = new DataView(out.buffer);
	dv.setUint32(4, json.length, true);
	out.set(json, 8);
	let o = head + pad;
	for (const s of strokes) {
		const n = pointCount(s) * STRIDE;
		for (let i = 0; i < n; i++, o += 4) dv.setFloat32(o, s.pts[i], true);
	}
	return out;
}

const ID = /^[0-9a-z~._-]{1,40}$/;
const COLOR = /^#[0-9a-f]{6}$/i;
const BRUSH = /^[\w:./-]{1,120}$/;
const num = (v: unknown, lo: number, hi: number) =>
	typeof v === 'number' && Number.isFinite(v) && v >= lo && v <= hi;

function readTexture(v: unknown): BrushTexture | undefined {
	const t = v as BrushTexture;
	if (!t || typeof t !== 'object') return undefined;
	const id = (x: unknown) => (typeof x === 'string' && x.length <= 120 ? x : null);
	const n = (x: unknown, d: number) => (typeof x === 'number' && Number.isFinite(x) ? x : d);
	return {
		grain: id(t.grain),
		grainScale: n(t.grainScale, 1),
		grainStrength: n(t.grainStrength, 0.7),
		tip: id(t.tip),
		dual: id(t.dual),
		dualSize: n(t.dualSize, 0.35),
		dualScatter: n(t.dualScatter, 0),
		dualCount: n(t.dualCount, 1)
	};
}

function readShape(v: unknown): EditShape | undefined {
	const s = v as EditShape;
	if (!s || typeof s !== 'object') return undefined;
	const pt = (p: unknown): p is Pt =>
		Array.isArray(p) && p.length === 2 && num(p[0], -1e6, 1e6) && num(p[1], -1e6, 1e6);
	if (!num(s.pressure, 0, 1)) return undefined;
	if (s.kind === 'poly' && Array.isArray(s.pts) && s.pts.length >= 2 && s.pts.length <= 64) {
		if (!s.pts.every(pt)) return undefined;
		return {
			kind: 'poly',
			pts: s.pts.map((p) => [p[0], p[1]] as Pt),
			closed: s.closed === true,
			pressure: s.pressure,
			...(s.smooth ? { smooth: true } : {})
		};
	}
	if (
		s.kind === 'ellipse' &&
		pt(s.c) &&
		num(s.a, 0, 1e6) &&
		num(s.b, 0, 1e6) &&
		num(s.theta, -10, 10)
	)
		return {
			kind: 'ellipse',
			c: [s.c[0], s.c[1]],
			a: s.a,
			b: s.b,
			theta: s.theta,
			circle: s.circle === true,
			pressure: s.pressure
		};
	return undefined;
}

/** Bytes back into strokes, checked: throws when they aren't line data. */
export function decodeLines(bytes: Uint8Array): LineStroke[] {
	if (bytes.length < 8 || MAGIC.some((m, i) => bytes[i] !== m)) throw new Error('not line data');
	const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
	const len = dv.getUint32(4, true);
	if (8 + len > bytes.length) throw new Error('line data ends inside its header');
	const raw = JSON.parse(new TextDecoder().decode(bytes.subarray(8, 8 + len))) as unknown;
	if (!Array.isArray(raw) || raw.length > MAX_STROKES) throw new Error('bad line data');
	const head = 8 + len;
	let o = head + ((4 - (head % 4)) % 4);
	const out: LineStroke[] = [];
	for (const m of raw as Meta[]) {
		if (
			!m ||
			typeof m.id !== 'string' ||
			!ID.test(m.id) ||
			!Number.isInteger(m.n) ||
			m.n < 1 ||
			m.n > MAX_POINTS ||
			typeof m.brush !== 'string' ||
			!BRUSH.test(m.brush) ||
			typeof m.color !== 'string' ||
			!COLOR.test(m.color) ||
			!num(m.size, 0.01, 1000) ||
			!num(m.opacity, 0, 1) ||
			!num(m.radius, 0, 5000) ||
			!num(m.seed, 0, 2 ** 32 - 1)
		)
			throw new Error('bad stroke in line data');
		const count = m.n * STRIDE;
		if (o + count * 4 > bytes.length) throw new Error('line data is cut short');
		const pts = new Float32Array(count);
		for (let i = 0; i < count; i++, o += 4) {
			const v = dv.getFloat32(o, true);
			if (!Number.isFinite(v)) throw new Error('bad point in line data');
			pts[i] = v;
		}
		const texture = readTexture(m.texture);
		const shape = readShape(m.shape);
		const link = readLink(m.link);
		out.push({
			id: m.id,
			pts,
			brush: m.brush,
			color: m.color.toLowerCase(),
			size: m.size,
			opacity: m.opacity,
			radius: m.radius,
			seed: Math.floor(m.seed) >>> 0,
			...(texture ? { texture } : {}),
			...(shape ? { shape } : {}),
			...(link ? { link } : {})
		});
	}
	return out;
}

const BASE_MAGIC = [0x50, 0x50, 0x42, 0x31]; // "PPB1"

/**
 * A drawing's strokes as saved: `encodeLines`'s bytes, and for a plain layer's drawing
 * the pixels kept under them (the engine's packed base) after: "PPB1", u32 length, the
 * bytes. (Readers of plain "PPL1" data stop before it.)
 */
export function encodeLineData(strokes: readonly LineStroke[], base?: Uint8Array | null) {
	const lines = encodeLines(strokes);
	if (!base) return lines;
	const out = new Uint8Array(lines.length + 8 + base.length);
	out.set(lines, 0);
	out.set(BASE_MAGIC, lines.length);
	new DataView(out.buffer).setUint32(lines.length + 4, base.length, true);
	out.set(base, lines.length + 8);
	return out;
}

/** `encodeLineData`'s bytes back (throws when they aren't line data). */
export function decodeLineData(bytes: Uint8Array): {
	strokes: LineStroke[];
	base: Uint8Array | null;
} {
	const strokes = decodeLines(bytes);
	const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
	const head = 8 + dv.getUint32(4, true);
	const end =
		head + ((4 - (head % 4)) % 4) + strokes.reduce((n, s) => n + pointCount(s) * STRIDE * 4, 0);
	if (end + 8 > bytes.length || BASE_MAGIC.some((m, i) => bytes[end + i] !== m))
		return { strokes, base: null };
	const len = dv.getUint32(end + 4, true);
	if (end + 8 + len > bytes.length) throw new Error('line data is cut short');
	return { strokes, base: bytes.subarray(end + 8, end + 8 + len) };
}

const MODES: readonly string[] = ['vertical', 'horizontal', 'quadrant', 'radial'];

function readLink(v: unknown): Link | undefined {
	const l = v as Link;
	if (!l || typeof l !== 'object' || typeof l.g !== 'string' || !ID.test(l.g)) return undefined;
	if (!MODES.includes(l.mode) || !Number.isInteger(l.k) || l.k < 0 || l.k > 24) return undefined;
	if (!num(l.n, 2, 24) || !num(l.cx, -1e6, 1e6) || !num(l.cy, -1e6, 1e6)) return undefined;
	return { g: l.g, k: l.k, mode: l.mode, n: l.n, cx: l.cx, cy: l.cy };
}

// ---- editable strokes on plain layers: what becomes plain pixels ----
//
// On a plain layer the engine keeps, per tile, the pixels under the editable strokes
// (its "base"). When other paint changes tiles, the strokes there become plain pixels
// ("baked"); so do the oldest past the limit. A tile's base sits under all its editable
// strokes, so baking a stroke bakes every older one sharing a tile with it too; then on
// each tile those reached, either no editable stroke is left (the engine forgets the
// tile) or the baked ones are replayed into its base, under the ones left.

/** The tiles (`tile` px) a stroke's paint can reach, as "x,y" keys. */
export function strokeTiles(s: LineStroke, tile = 64): string[] {
	const b = bounds(s);
	if (!(b.x0 <= b.x1)) return [];
	const out: string[] = [];
	for (let ty = Math.floor(b.y0 / tile); ty <= Math.floor(b.y1 / tile); ty++)
		for (let tx = Math.floor(b.x0 / tile); tx <= Math.floor(b.x1 / tile); tx++)
			out.push(`${tx},${ty}`);
	return out;
}

/**
 * The strokes (indices) that must become plain pixels when `start` do: those, and
 * every older stroke sharing a tile with one of them, again and again.
 */
export function bakeClosure(strokes: readonly LineStroke[], start: Iterable<number>): Set<number> {
	const tiles = strokes.map((s) => strokeTiles(s));
	const onTile = new Map<string, number[]>();
	tiles.forEach((ts, i) =>
		ts.forEach((t) => {
			const at = onTile.get(t);
			if (at) at.push(i);
			else onTile.set(t, [i]);
		})
	);
	const out = new Set<number>();
	const todo = [...start].filter((i) => i >= 0 && i < strokes.length);
	while (todo.length) {
		const i = todo.pop()!;
		if (out.has(i)) continue;
		out.add(i);
		for (const t of tiles[i])
			for (const j of onTile.get(t)!) if (j < i && !out.has(j)) todo.push(j);
	}
	return out;
}

/** The strokes whose paint reaches any of `keys` ("x,y" tiles), as indices. */
export function strokesOnKeys(strokes: readonly LineStroke[], keys: Set<string>): number[] {
	const out: number[] = [];
	strokes.forEach((s, i) => {
		if (strokeTiles(s).some((t) => keys.has(t))) out.push(i);
	});
	return out;
}

export interface BakePlan {
	/** the strokes still editable, in order */
	keep: LineStroke[];
	/** the strokes becoming plain pixels, in order */
	baked: LineStroke[];
	/** tiles where the baked go into the base, under strokes still editable */
	replay: Int32Array;
	/** tiles no editable stroke reaches any more: the base forgets them */
	drop: Int32Array;
}

const pairsOf = (keys: Iterable<string>) =>
	new Int32Array([...keys].flatMap((k) => k.split(',').map(Number)));

/**
 * What baking strokes `start` (indices; and what `bakeClosure` adds) does. `canReplay`
 * says whether a stroke can be replayed (its brush is loaded): where one can't, the
 * strokes over it on those tiles become plain pixels too, so nothing needs replaying.
 */
export function bakePlan(
	strokes: readonly LineStroke[],
	start: Iterable<number>,
	canReplay: (s: LineStroke) => boolean = () => true
): BakePlan {
	let out = bakeClosure(strokes, start);
	for (;;) {
		const keepTiles = new Set<string>();
		strokes.forEach((s, i) => {
			if (!out.has(i)) for (const t of strokeTiles(s)) keepTiles.add(t);
		});
		const bakedTiles = new Set<string>();
		for (const i of out) for (const t of strokeTiles(strokes[i])) bakedTiles.add(t);
		const replay = [...bakedTiles].filter((t) => keepTiles.has(t));
		const stuck = replay.filter((t) =>
			[...out].some((i) => !canReplay(strokes[i]) && strokeTiles(strokes[i]).includes(t))
		);
		if (stuck.length) {
			out = bakeClosure(strokes, [...out, ...strokesOnKeys(strokes, new Set(stuck))]);
			continue;
		}
		return {
			keep: strokes.filter((_, i) => !out.has(i)),
			baked: strokes.filter((_, i) => out.has(i)),
			replay: pairsOf(replay),
			drop: pairsOf([...bakedTiles].filter((t) => !keepTiles.has(t)))
		};
	}
}

/** The oldest strokes past `limit` (indices): they become plain pixels. */
export function overLimit(strokes: readonly LineStroke[], limit: number): number[] {
	const n = Math.max(0, strokes.length - Math.max(0, limit));
	return Array.from({ length: n }, (_, i) => i);
}

/** Roughly the bytes a drawing's strokes take (their points, and the rest). */
export function strokesBytes(strokes: readonly LineStroke[]): number {
	return strokes.reduce((n, s) => n + s.pts.byteLength + 240, 0);
}

// ---- drawing together: what changed in a drawing, as a small edit ----

/**
 * What turns `before` into `after`: the strokes gone (or changed) by id, and the new or
 * changed ones with their place in `after`. Strokes are compared as objects (edits make
 * new ones), so unchanged strokes never travel.
 */
export function diffLines(
	before: readonly LineStroke[],
	after: readonly LineStroke[]
): { del: string[]; put: [number, LineStroke][] } {
	const was = new Map(before.map((s) => [s.id, s]));
	const now = new Map(after.map((s) => [s.id, s]));
	const del = before.filter((s) => now.get(s.id) !== s).map((s) => s.id);
	const put: [number, LineStroke][] = [];
	after.forEach((s, i) => {
		if (was.get(s.id) !== s) put.push([i, s]);
	});
	return { del, put };
}

/** A diff (from `diffLines`) applied to a drawing's strokes. */
export function patchLines(
	list: readonly LineStroke[],
	del: string[],
	put: [number, LineStroke][]
): LineStroke[] {
	const gone = new Set([...del, ...put.map(([, s]) => s.id)]);
	const out = list.filter((s) => !gone.has(s.id));
	for (const [i, s] of [...put].sort((a, b) => a[0] - b[0]))
		out.splice(Math.min(Math.max(0, i), out.length), 0, s);
	return out;
}

// ---- every drawing's strokes ----

/**
 * The strokes of every line-layer drawing, by drawing id. Lists are replaced, never
 * changed in place (so undo can keep the old list). `onchange` hears every change.
 */
export class LineStore {
	#docs = new Map<number, readonly LineStroke[]>();
	/** drawings of plain layers: their strokes lie over pixels (see "editable strokes") */
	#plain = new Set<number>();
	onchange: (drawing: number) => void = () => {};

	/** A drawing's strokes lie over a plain layer's pixels (`false`: a line layer's). */
	setPlain(drawing: number, on = true) {
		if (!drawing) return;
		if (on) this.#plain.add(drawing);
		else this.#plain.delete(drawing);
	}

	isPlain(drawing: number) {
		return this.#plain.has(drawing);
	}

	get(drawing: number): readonly LineStroke[] {
		return this.#docs.get(drawing) ?? [];
	}

	has(drawing: number) {
		return this.#docs.has(drawing);
	}

	set(drawing: number, strokes: readonly LineStroke[]) {
		if (!drawing) return;
		if (strokes.length) this.#docs.set(drawing, strokes);
		else this.#docs.delete(drawing);
		this.onchange(drawing);
	}

	/** A drawing copied (a duplicated frame or layer): the copy gets the same strokes. */
	copy(from: number, to: number) {
		const s = this.#docs.get(from);
		if (s && to) {
			this.setPlain(to, this.#plain.has(from));
			this.set(to, s);
		}
	}

	drawings(): number[] {
		return [...this.#docs.keys()];
	}

	/** Forget the drawings not in `keep`. */
	keepOnly(keep: Set<number>) {
		for (const d of [...this.#docs.keys()]) if (!keep.has(d)) this.#docs.delete(d);
		for (const d of [...this.#plain]) if (!keep.has(d)) this.#plain.delete(d);
	}

	clear() {
		this.#docs.clear();
		this.#plain.clear();
	}
}
