/**
 * Canvas size for a whole animation (Image → Canvas size…): the new size, where the
 * drawing sits on it (an anchor: crop, or more room) or whether it's scaled to fit, and
 * what that does to everything kept in canvas pixels besides the drawings themselves:
 * the cuts' cameras, text layers' boxes and the guides. The paint engine moves the
 * pixels (every drawing of every frame, and the masks) in one undo step.
 */
import { checkSize, type CanvasSize } from '@pencilpilot/paint/canvas-size';
import type { GuideSettings } from '@pencilpilot/paint/guides';
import type { Camera, Timeline } from './timeline';

/** 0–1 across and down: which part of the drawing stays put (0.5, 0.5: the middle). */
export type Anchor = [number, number];

export interface ResizePlan {
	from: CanvasSize;
	to: CanvasSize;
	/** the drawing is resized with the canvas (else it keeps its size and moves by dx, dy) */
	scale: boolean;
	/** how far the drawing moves, canvas px (0 when scaling) */
	dx: number;
	dy: number;
}

/** The anchors of the 3×3 grid, row by row. */
export const ANCHORS: Anchor[] = [0, 0.5, 1].flatMap((ay) =>
	[0, 0.5, 1].map((ax) => [ax, ay] as Anchor)
);

/** The plan for going from one size to another. */
export function planResize(
	from: CanvasSize,
	to: CanvasSize,
	how: { scale: boolean; anchor: Anchor }
): ResizePlan {
	if (how.scale) return { from, to, scale: true, dx: 0, dy: 0 };
	return {
		from,
		to,
		scale: false,
		dx: Math.round((to.width - from.width) * how.anchor[0]),
		dy: Math.round((to.height - from.height) * how.anchor[1])
	};
}

/** The plan that crops to a rectangle (canvas px), keeping the drawing's size. */
export function planCrop(
	from: CanvasSize,
	r: { x: number; y: number; width: number; height: number }
): ResizePlan {
	return {
		from,
		to: { width: Math.round(r.width), height: Math.round(r.height) },
		scale: false,
		dx: -Math.round(r.x),
		dy: -Math.round(r.y)
	};
}

/** How much the plan stretches across and down (1, 1 when not scaling). */
export function stretch(p: ResizePlan): [number, number] {
	return p.scale ? [p.to.width / p.from.width, p.to.height / p.from.height] : [1, 1];
}

/** A point on the old canvas, on the new one. */
export function movePoint(p: ResizePlan, [x, y]: [number, number]): [number, number] {
	const [kx, ky] = stretch(p);
	return [x * kx + p.dx, y * ky + p.dy];
}

/** A rectangle on the old canvas, on the new one (not clamped). */
export function moveRect(p: ResizePlan, r: Camera): Camera {
	const [kx, ky] = stretch(p);
	const [x, y] = movePoint(p, [r.x, r.y]);
	return { x, y, width: r.width * kx, height: r.height * ky };
}

/** Whether the plan changes anything at all. */
export const changes = (p: ResizePlan) =>
	p.from.width !== p.to.width ||
	p.from.height !== p.to.height ||
	(!p.scale && (p.dx !== 0 || p.dy !== 0));

/**
 * The cuts' cameras, framing the same part of the drawing on the new canvas (a camera
 * that ends up partly off it is pulled back on, and shrunk if it's now too big).
 */
export function moveCameras(tl: Timeline, p: ResizePlan): Timeline {
	const { width: W, height: H } = p.to;
	return {
		...tl,
		scenes: tl.scenes.map((s) => ({
			...s,
			cuts: s.cuts.map((c) => {
				if (!c.camera) return c;
				const m = moveRect(p, c.camera);
				const width = Math.max(1, Math.min(W, Math.round(m.width)));
				const height = Math.max(1, Math.min(H, Math.round(m.height)));
				const camera = {
					x: Math.max(0, Math.min(W - width, Math.round(m.x))),
					y: Math.max(0, Math.min(H - height, Math.round(m.y))),
					width,
					height
				};
				return { ...c, camera };
			})
		}))
	};
}

/**
 * A text layer's description with its box moved (and, scaling, its size, wrap width and
 * spacing resized with it). Other fields are kept as they are. Null: not a text.
 */
export function moveText(json: string, p: ResizePlan): string | null {
	let v: Record<string, unknown>;
	try {
		v = JSON.parse(json);
	} catch {
		return null;
	}
	if (!v || typeof v !== 'object' || Array.isArray(v)) return null;
	const [kx, ky] = stretch(p);
	const num = (k: string) => (typeof v[k] === 'number' && Number.isFinite(v[k]) ? v[k] : 0);
	const [x, y] = movePoint(p, [num('x'), num('y')]);
	const out: Record<string, unknown> = { ...v, x: round2(x), y: round2(y) };
	if (p.scale) {
		// (text can't stretch: its size follows the height, its wrap width the width)
		if (typeof v.size === 'number') out.size = round2(v.size * ky);
		if (typeof v.width === 'number') out.width = round2(v.width * kx);
		if (typeof v.letterSpacing === 'number') out.letterSpacing = round2(v.letterSpacing * kx);
	}
	return JSON.stringify(out);
}

const round2 = (n: number) => Math.round(n * 100) / 100;

/** The guides, on the new canvas: they stay where they were on the drawing. */
export function moveGuides(g: GuideSettings, p: ResizePlan): GuideSettings {
	const [kx, ky] = stretch(p);
	const pt = (q: [number, number]) => movePoint(p, q);
	return {
		// (the composition overlay fits whatever the canvas is: it stays as it is)
		...g,
		grid: { ...g.grid, size: Math.max(2, Math.round(g.grid.size * Math.min(kx, ky))) },
		perspective: { ...g.perspective, vps: g.perspective.vps.map(pt) },
		ruler: { ...g.ruler, a: pt(g.ruler.a), b: pt(g.ruler.b) },
		symmetry: { ...g.symmetry, c: pt(g.symmetry.c) }
	};
}

// ---- units ----

export type Unit = 'px' | 'cm' | 'in';
export const UNITS: { id: Unit; label: string }[] = [
	{ id: 'px', label: 'pixels' },
	{ id: 'cm', label: 'cm' },
	{ id: 'in', label: 'inches' }
];
/** Pixels per inch for cm and inches when the project doesn't say (print quality). */
export const DEFAULT_PPI = 300;

/** A length in `unit` as whole pixels at `ppi`. */
export function toPx(v: number, unit: Unit, ppi: number): number {
	if (unit === 'px') return Math.round(v);
	return Math.round(unit === 'in' ? v * ppi : (v / 2.54) * ppi);
}

/** Pixels as a length in `unit` at `ppi` (to two decimals). */
export function fromPx(px: number, unit: Unit, ppi: number): number {
	if (unit === 'px') return px;
	const inches = px / ppi;
	return Math.round((unit === 'in' ? inches : inches * 2.54) * 100) / 100;
}

/** The other side for a locked aspect ratio. */
export function keepAspect(from: CanvasSize, side: 'width' | 'height', value: number): CanvasSize {
	if (side === 'width')
		return { width: value, height: Math.max(1, Math.round((value * from.height) / from.width)) };
	return { width: Math.max(1, Math.round((value * from.width) / from.height)), height: value };
}

// ---- memory ----

/** Bytes in a 64×64 RGBA tile. */
export const TILE_BYTES = 64 * 64 * 4;
/** Past this the paint engine (a 32-bit WebAssembly memory) can't hold old and new at once. */
export const ENGINE_MOST = 1536 * 2 ** 20;

/**
 * About how much memory a canvas size change takes while it works and for its undo:
 * the paint as it is (`tiles` 64×64 tiles, kept for the undo) plus the paint resized.
 * In the app, `tiles` is only what the engine holds: the frames in the frame store are
 * resized a batch at a time and their undo waits in the store (see `FrameCache.resizeEach`).
 */
export function resizeBytes(tiles: number, p: ResizePlan): number {
	const [kx, ky] = stretch(p);
	const before = tiles * TILE_BYTES;
	return Math.round(before + before * Math.max(1, kx * ky));
}

/** Whether a canvas size can change: the size is allowed and it fits in memory. */
export function checkResize(
	p: ResizePlan,
	tiles: number,
	limitBytes: number
): { ok: true; note: string | null } | { ok: false; why: string } {
	const size = checkSize(p.to.width, p.to.height);
	if (!size.ok) return size;
	if (!changes(p)) return { ok: false, why: 'That’s the size it is.' };
	const bytes = resizeBytes(tiles, p);
	if (bytes > ENGINE_MOST)
		return {
			ok: false,
			why: `This project has too much paint to resize in one go (it would need about ${mb(bytes)}).`
		};
	if (bytes > limitBytes)
		return {
			ok: true,
			note: `This takes about ${mb(bytes)} of memory while it works and for its undo, more than the frame memory limit in Settings.`
		};
	return { ok: true, note: null };
}

const mb = (b: number) =>
	b >= 2 ** 30 ? `${(b / 2 ** 30).toFixed(1)} GB` : `${Math.max(1, Math.round(b / 2 ** 20))} MB`;
