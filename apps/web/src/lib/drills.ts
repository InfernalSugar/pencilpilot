/**
 * Fundamentals drills (M6-T02): short, scored practice exercises.
 *
 * Every drill has a seeded, procedural target generator, so there is endless,
 * licence-free practice and tests are deterministic. Nothing here uses AI:
 * targets are plain geometry and scoring is the maths grader plus the stroke
 * metrics in `drill-metrics.ts` (see docs/planning/CONTENT_GUIDE.md → Drills).
 */
import type { GradeWeights } from './grader';

export type DrillKind =
	'lines' | 'ellipses' | 'boxes' | 'shapes' | 'values' | 'figure' | 'reference';

/** Same skill ids as `content/feedback/rules.toml`. */
export type DrillSkill =
	'line_confidence' | 'proportions' | 'placement' | 'outline' | 'silhouette' | 'tone';

export interface Pt {
	x: number;
	y: number;
}

/** One round's target. */
export interface DrillTarget {
	/** What the student sees in the target tile. */
	prompt: string;
	/** What their drawing is graded against (often the same picture as `prompt`). */
	answer: string;
	/** Optional marks shown faintly *over* the canvas (never part of the graded ink). */
	guide?: string;
	/** ghosted-lines: the two dots to join, canvas pixels. */
	segment?: { a: Pt; b: Pt };
	/** angle-matching: the line's angle, degrees in [0, 180). */
	angle?: number;
}

export type Rng = () => number;

export interface Drill {
	id: string;
	name: string;
	skill: DrillSkill;
	blurb: string;
	howTo: string[];
	rounds: number;
	/** Countdown per round; without it the timer counts up. */
	secondsPerRound?: number;
	kind: DrillKind;
	/** Brush the canvas starts with. */
	brush: DrillBrush;
	/** Grader weight overrides (missing fields use the fundamentals defaults). */
	weights?: Partial<GradeWeights>;
	/** Draw "blind": canvas ink is hidden until the round ends; scored on line confidence only. */
	blind?: boolean;
	/** Seeded target. Reference drills use it as the fallback when no picture loads. */
	target: (rng: Rng, w?: number, h?: number) => DrillTarget;
	/** The prompt picture as an SVG string: `target(rng, w, h).prompt`. */
	generate: (rng: Rng, w?: number, h?: number) => string;
}

export type DrillBrush = 'pen' | 'pencil' | 'marker';

/** The three brushes the drill toolbar offers. */
export const DRILL_BRUSHES: { id: DrillBrush; name: string; brushId: string }[] = [
	{ id: 'pen', name: 'Fine pen', brushId: 'ramon-thin-pen' },
	{ id: 'pencil', name: 'Pencil', brushId: 'deevad-2b-pencil' },
	{ id: 'marker', name: 'Marker', brushId: 'classic-marker-fat' }
];

export const SKILL_NAMES: Record<DrillSkill, string> = {
	line_confidence: 'Line confidence',
	proportions: 'Proportions & angles',
	placement: 'Placement & measuring',
	outline: 'Outlines & seeing',
	silhouette: 'Big shapes',
	tone: 'Light & dark'
};

// ---------------------------------------------------------------- randomness

/** Small, fast seeded PRNG (mulberry32). Returns floats in [0, 1). */
export function seeded(seed: number): Rng {
	let a = seed >>> 0;
	return () => {
		a = (a + 0x6d2b79f5) >>> 0;
		let t = a;
		t = Math.imul(t ^ (t >>> 15), t | 1);
		t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
}

const between = (rng: Rng, a: number, b: number) => a + rng() * (b - a);
const pick = <T>(rng: Rng, xs: readonly T[]) => xs[Math.floor(rng() * xs.length)];
const n = (v: number) => +v.toFixed(1);

// ---------------------------------------------------------------- svg helpers

const INK = '#16161a';
const PALE = '#d9d5cc';
const GUIDE = '#8a7fd0';
const DOT = '#e8492f';

export function svgDoc(w: number, h: number, body: string, background = true) {
	const bg = background ? `<rect width="${w}" height="${h}" fill="#fff"/>` : '';
	return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}">${bg}${body}</svg>`;
}

const line = (a: Pt, b: Pt, attrs = `stroke="${INK}" stroke-width="4"`) =>
	`<line x1="${n(a.x)}" y1="${n(a.y)}" x2="${n(b.x)}" y2="${n(b.y)}" ${attrs} stroke-linecap="round"/>`;

const polyPath = (pts: Pt[]) => 'M' + pts.map((p) => `${n(p.x)} ${n(p.y)}`).join(' L') + ' Z';

/** Closed Catmull-Rom spline through the points, as cubic Béziers. */
function smoothPath(pts: Pt[]) {
	const k = pts.length;
	let d = `M${n(pts[0].x)} ${n(pts[0].y)}`;
	for (let i = 0; i < k; i++) {
		const p0 = pts[(i - 1 + k) % k];
		const p1 = pts[i];
		const p2 = pts[(i + 1) % k];
		const p3 = pts[(i + 2) % k];
		const c1 = { x: p1.x + (p2.x - p0.x) / 6, y: p1.y + (p2.y - p0.y) / 6 };
		const c2 = { x: p2.x - (p3.x - p1.x) / 6, y: p2.y - (p3.y - p1.y) / 6 };
		d += ` C${n(c1.x)} ${n(c1.y)} ${n(c2.x)} ${n(c2.y)} ${n(p2.x)} ${n(p2.y)}`;
	}
	return d + ' Z';
}

const ellipse = (c: Pt, rx: number, ry: number, deg: number, attrs: string) =>
	`<ellipse cx="${n(c.x)}" cy="${n(c.y)}" rx="${n(rx)}" ry="${n(ry)}" transform="rotate(${n(deg)} ${n(c.x)} ${n(c.y)})" ${attrs}/>`;

// ---------------------------------------------------------------- shapes

/** A smooth organic blob: polar radius with a few low-frequency bumps. */
export function blobPoints(rng: Rng, c: Pt, rx: number, ry: number, count = 40): Pt[] {
	const waves = [2, 3, 5].map((f) => ({ f, amp: between(rng, 0.04, 0.16), ph: rng() * 6.283 }));
	const turn = rng() * 6.283;
	const pts: Pt[] = [];
	for (let i = 0; i < count; i++) {
		const t = (i / count) * Math.PI * 2;
		const r = 1 + waves.reduce((s, w) => s + w.amp * Math.sin(w.f * t + w.ph), 0);
		const x = Math.cos(t) * rx * r;
		const y = Math.sin(t) * ry * r;
		pts.push({
			x: c.x + x * Math.cos(turn * 0.15) - y * Math.sin(turn * 0.15),
			y: c.y + x * Math.sin(turn * 0.15) + y * Math.cos(turn * 0.15)
		});
	}
	return pts;
}

/** Random non-convex polygon (a "star-ish" shape), clockwise. */
function randomPolygon(rng: Rng, c: Pt, r: number, sides: number): Pt[] {
	const pts: Pt[] = [];
	const ph = rng() * 6.283;
	for (let i = 0; i < sides; i++) {
		const t = ph + (i / sides) * Math.PI * 2 + between(rng, -0.2, 0.2);
		const rr = r * between(rng, 0.45, 1);
		pts.push({ x: c.x + Math.cos(t) * rr, y: c.y + Math.sin(t) * rr * 0.85 });
	}
	return pts;
}

const cross = (o: Pt, a: Pt, b: Pt) => (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);

/** Convex hull (monotone chain). */
export function convexHull(points: Pt[]): Pt[] {
	const p = [...points].sort((a, b) => a.x - b.x || a.y - b.y);
	if (p.length < 3) return p;
	const lower: Pt[] = [];
	for (const q of p) {
		while (lower.length >= 2 && cross(lower[lower.length - 2], lower[lower.length - 1], q) <= 0)
			lower.pop();
		lower.push(q);
	}
	const upper: Pt[] = [];
	for (const q of [...p].reverse()) {
		while (upper.length >= 2 && cross(upper[upper.length - 2], upper[upper.length - 1], q) <= 0)
			upper.pop();
		upper.push(q);
	}
	return [...lower.slice(0, -1), ...upper.slice(0, -1)];
}

/** Hull reduced to `max` corners by dropping the corner that adds the least area. */
function envelope(points: Pt[], max: number): Pt[] {
	const h = convexHull(points);
	while (h.length > max) {
		let best = 0;
		let bestArea = Infinity;
		for (let i = 0; i < h.length; i++) {
			const a = Math.abs(cross(h[(i - 1 + h.length) % h.length], h[i], h[(i + 1) % h.length]));
			if (a < bestArea) [best, bestArea] = [i, a];
		}
		h.splice(best, 1);
	}
	return h;
}

function bbox(pts: Pt[]) {
	const xs = pts.map((p) => p.x);
	const ys = pts.map((p) => p.y);
	return {
		x0: Math.min(...xs),
		y0: Math.min(...ys),
		x1: Math.max(...xs),
		y1: Math.max(...ys)
	};
}

/** A blob of random size and proportions, somewhere near the middle of the canvas. */
function randomBlob(rng: Rng, w: number, h: number, scale = 1) {
	const rx = between(rng, 0.18, 0.34) * w * scale;
	const ry = between(rng, 0.2, 0.36) * h * scale;
	const c = { x: w / 2 + between(rng, -0.06, 0.06) * w, y: h / 2 + between(rng, -0.05, 0.05) * h };
	return blobPoints(rng, c, rx, ry);
}

// ---------------------------------------------------------------- generators

function ghostedLines(rng: Rng, w = 800, h = 600): DrillTarget {
	const m = 70;
	let a: Pt, b: Pt;
	do {
		a = { x: between(rng, m, w - m), y: between(rng, m, h - m) };
		b = { x: between(rng, m, w - m), y: between(rng, m, h - m) };
	} while (Math.hypot(b.x - a.x, b.y - a.y) < Math.min(w, h) * 0.45);
	const dots = [a, b]
		.map((p) => `<circle cx="${n(p.x)}" cy="${n(p.y)}" r="8" fill="${DOT}"/>`)
		.join('');
	return {
		prompt: svgDoc(
			w,
			h,
			line(a, b, `stroke="${PALE}" stroke-width="3" stroke-dasharray="2 14"`) + dots
		),
		answer: svgDoc(w, h, line(a, b)),
		guide: svgDoc(w, h, dots, false),
		segment: { a, b }
	};
}

function angleMatching(rng: Rng, w = 800, h = 600): DrillTarget {
	// Avoid near-horizontal/vertical lines: those are too easy to "match".
	const angle = pick(rng, [-1, 1]) * between(rng, 12, 78);
	const len = Math.min(w / 2, h) * between(rng, 0.55, 0.75);
	const rad = (angle * Math.PI) / 180;
	const seg = (cx: number) => {
		const dx = (Math.cos(rad) * len) / 2;
		const dy = (Math.sin(rad) * len) / 2;
		return [
			{ x: cx - dx, y: h / 2 - dy },
			{ x: cx + dx, y: h / 2 + dy }
		] as const;
	};
	const [a, b] = seg(w * 0.25);
	const [c, d] = seg(w * 0.75);
	const slot = `<rect x="${n(w * 0.53)}" y="${n(h * 0.08)}" width="${n(w * 0.44)}" height="${n(h * 0.84)}" rx="24" fill="none" stroke="${GUIDE}" stroke-width="2" stroke-dasharray="10 10"/>`;
	const divider = line(
		{ x: w / 2, y: h * 0.1 },
		{ x: w / 2, y: h * 0.9 },
		`stroke="${PALE}" stroke-width="2"`
	);
	return {
		prompt: svgDoc(w, h, line(a, b) + slot),
		answer: svgDoc(w, h, line(c, d)),
		guide: svgDoc(w, h, line(a, b, `stroke="#9b978e" stroke-width="4"`) + divider + slot, false),
		angle: ((angle % 180) + 180) % 180 // screen convention (y down), in [0, 180)
	};
}

function ellipsePages(rng: Rng, w = 800, h = 600): DrillTarget {
	const rx = between(rng, 0.12, 0.38) * w;
	const ry = rx * between(rng, 0.18, 1);
	const deg = between(rng, -90, 90);
	const c = { x: w / 2 + between(rng, -0.05, 0.05) * w, y: h / 2 + between(rng, -0.05, 0.05) * h };
	// Keep it on the page.
	const fit = Math.min(
		1,
		(h * 0.44) / Math.max(rx * Math.abs(Math.sin((deg * Math.PI) / 180)), ry)
	);
	const shape = ellipse(c, rx * fit, ry * fit, deg, `fill="none" stroke="${INK}" stroke-width="4"`);
	const r = (deg * Math.PI) / 180;
	const ax = (len: number, t: number) =>
		line(
			{ x: c.x - Math.cos(t) * len, y: c.y - Math.sin(t) * len },
			{ x: c.x + Math.cos(t) * len, y: c.y + Math.sin(t) * len },
			`stroke="${GUIDE}" stroke-width="2" stroke-dasharray="6 8"`
		);
	const axes = ax(rx * fit + 24, r) + ax(ry * fit + 24, r + Math.PI / 2);
	return { prompt: svgDoc(w, h, axes + shape), answer: svgDoc(w, h, shape) };
}

function blockInBoxes(rng: Rng, w = 800, h = 600): DrillTarget {
	const count = 2 + Math.floor(rng() * 2);
	const total = h * between(rng, 0.6, 0.8);
	const parts = Array.from({ length: count }, () => between(rng, 0.6, 1.4));
	const sum = parts.reduce((s, p) => s + p, 0);
	const cx = w / 2 + between(rng, -0.05, 0.05) * w;
	let y = (h + total) / 2;
	const boxes: { x: number; y: number; w: number; h: number }[] = [];
	for (const p of parts) {
		const bh = (p / sum) * total;
		const bw = between(rng, 0.14, 0.55) * w;
		const off = between(rng, -0.04, 0.04) * w;
		y -= bh;
		boxes.push({ x: cx - bw / 2 + off, y, w: bw, h: bh });
	}
	const rect = (b: (typeof boxes)[number], attrs: string) =>
		`<rect x="${n(b.x)}" y="${n(b.y)}" width="${n(b.w)}" height="${n(b.h)}" ${attrs}/>`;
	return {
		prompt: svgDoc(w, h, boxes.map((b) => rect(b, `fill="${PALE}"`)).join('')),
		answer: svgDoc(
			w,
			h,
			boxes.map((b) => rect(b, `fill="none" stroke="${INK}" stroke-width="4"`)).join('')
		)
	};
}

function fourCornerMarks(rng: Rng, w = 800, h = 600): DrillTarget {
	const pts = randomBlob(rng, w, h);
	const { x0, y0, x1, y1 } = bbox(pts);
	const arm = Math.min(x1 - x0, y1 - y0) * 0.18;
	const tick = (x: number, y: number, sx: number, sy: number) =>
		`<path d="M${n(x + sx * arm)} ${n(y)} L${n(x)} ${n(y)} L${n(x)} ${n(y + sy * arm)}" fill="none" stroke="${INK}" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>`;
	return {
		prompt: svgDoc(w, h, `<path d="${smoothPath(pts)}" fill="${PALE}"/>`),
		answer: svgDoc(
			w,
			h,
			tick(x0, y0, 1, 1) + tick(x1, y0, -1, 1) + tick(x0, y1, 1, -1) + tick(x1, y1, -1, -1)
		)
	};
}

function envelopeShapes(rng: Rng, w = 800, h = 600): DrillTarget {
	const pts = randomBlob(rng, w, h);
	const env = envelope(pts, 5 + Math.floor(rng() * 3));
	return {
		prompt: svgDoc(w, h, `<path d="${smoothPath(pts)}" fill="${PALE}"/>`),
		answer: svgDoc(
			w,
			h,
			`<path d="${polyPath(env)}" fill="none" stroke="${INK}" stroke-width="4" stroke-linejoin="round"/>`
		)
	};
}

function silhouetteFill(rng: Rng, w = 800, h = 600): DrillTarget {
	const pts = randomBlob(rng, w, h, 0.9);
	const shape = svgDoc(w, h, `<path d="${smoothPath(pts)}" fill="${INK}"/>`);
	return { prompt: shape, answer: shape };
}

function gridLines(w: number, h: number, attrs: string) {
	let s = `<rect x="1" y="1" width="${w - 2}" height="${h - 2}" fill="none" ${attrs}/>`;
	for (let i = 1; i < 3; i++) {
		s += line({ x: (w * i) / 3, y: 0 }, { x: (w * i) / 3, y: h }, attrs);
		s += line({ x: 0, y: (h * i) / 3 }, { x: w, y: (h * i) / 3 }, attrs);
	}
	return s;
}

function gridCopy(rng: Rng, w = 800, h = 600): DrillTarget {
	const c = { x: w / 2 + between(rng, -0.08, 0.08) * w, y: h / 2 + between(rng, -0.06, 0.06) * h };
	const pts = randomPolygon(rng, c, Math.min(w, h) * 0.42, 6 + Math.floor(rng() * 4));
	const shape = `<path d="${polyPath(pts)}" fill="none" stroke="${INK}" stroke-width="4" stroke-linejoin="round"/>`;
	const grid = gridLines(w, h, `stroke="${GUIDE}" stroke-width="2"`);
	return {
		prompt: svgDoc(w, h, grid + shape),
		answer: svgDoc(w, h, shape),
		guide: svgDoc(w, h, grid, false)
	};
}

/** Five-step value scale greys, light to dark (0 = black, 255 = white). */
export const VALUE_STEPS = [235, 185, 135, 85, 35];

function fiveStepValueScale(rng: Rng, w = 800, h = 600): DrillTarget {
	const gap = 14;
	const bw = Math.min((w * between(rng, 0.78, 0.9) - gap * 4) / 5, 150);
	const bh = bw * between(rng, 1, 1.5);
	const x0 = (w - (bw * 5 + gap * 4)) / 2;
	const y0 = (h - bh) / 2 + between(rng, -0.05, 0.05) * h;
	const box = (i: number, attrs: string) =>
		`<rect x="${n(x0 + i * (bw + gap))}" y="${n(y0)}" width="${n(bw)}" height="${n(bh)}" ${attrs}/>`;
	const grey = (v: number) => `rgb(${v},${v},${v})`;
	const fills = VALUE_STEPS.map((v, i) => box(i, `fill="${grey(v)}"`)).join('');
	const outlines = (attrs: string) => VALUE_STEPS.map((_, i) => box(i, attrs)).join('');
	return {
		prompt: svgDoc(w, h, fills + outlines(`fill="none" stroke="${INK}" stroke-width="2"`)),
		answer: svgDoc(w, h, fills),
		guide: svgDoc(
			w,
			h,
			outlines(`fill="none" stroke="${GUIDE}" stroke-width="2" stroke-dasharray="8 6"`),
			false
		)
	};
}

function threeValueThumbnail(rng: Rng, w = 800, h = 600): DrillTarget {
	const fw = w * 0.8;
	const fh = h * 0.8;
	const fx = (w - fw) / 2;
	const fy = (h - fh) / 2;
	const id = `pp-frame-${Math.floor(rng() * 1e9)}`;
	const clip = `<clipPath id="${id}"><rect x="${n(fx)}" y="${n(fy)}" width="${n(fw)}" height="${n(fh)}"/></clipPath>`;
	// A big mid-grey mass (a "hill" or mass touching the frame) and a smaller dark accent.
	const mid = blobPoints(
		rng,
		{ x: fx + fw * between(rng, 0.3, 0.7), y: fy + fh * between(rng, 0.65, 0.9) },
		fw * between(rng, 0.45, 0.65),
		fh * between(rng, 0.35, 0.5)
	);
	const dark = blobPoints(
		rng,
		{ x: fx + fw * between(rng, 0.25, 0.75), y: fy + fh * between(rng, 0.35, 0.6) },
		fw * between(rng, 0.1, 0.18),
		fh * between(rng, 0.14, 0.24)
	);
	const body =
		`<defs>${clip}</defs><g clip-path="url(#${id})">` +
		`<path d="${smoothPath(mid)}" fill="rgb(150,150,150)"/>` +
		`<path d="${smoothPath(dark)}" fill="rgb(40,40,40)"/></g>`;
	const frame = (attrs: string) =>
		`<rect x="${n(fx)}" y="${n(fy)}" width="${n(fw)}" height="${n(fh)}" fill="none" ${attrs}/>`;
	return {
		prompt: svgDoc(w, h, body + frame(`stroke="${INK}" stroke-width="2"`)),
		answer: svgDoc(w, h, body),
		guide: svgDoc(w, h, frame(`stroke="${GUIDE}" stroke-width="2" stroke-dasharray="8 6"`), false)
	};
}

/** A mannequin built from ovals, `units` heads tall, with head-unit marks beside it. */
function headUnits(rng: Rng, w = 800, h = 600): DrillTarget {
	const units = pick(rng, [6, 7, 7.5, 8]);
	const height = h * 0.86;
	const u = height / units;
	const top = (h - height) / 2;
	const cx = w / 2 + between(rng, -0.04, 0.04) * w;
	const at = (k: number) => top + k * u; // y of head-unit line k
	// Proportions as fractions of the figure height, so they hold for any `units`.
	const H = (f: number) => top + f * height;
	const shoulderW = u * between(rng, 0.9, 1.1);
	const hipW = u * between(rng, 0.6, 0.8);
	const p = {
		neck: { x: cx, y: at(1) + u * 0.15 },
		shL: { x: cx - shoulderW, y: H(0.19) },
		shR: { x: cx + shoulderW, y: H(0.19) },
		hipL: { x: cx - hipW * 0.6, y: H(0.52) },
		hipR: { x: cx + hipW * 0.6, y: H(0.52) }
	};
	const limb = (a: Pt, len: number, deg: number) => ({
		x: a.x + Math.sin((deg * Math.PI) / 180) * len,
		y: a.y + Math.cos((deg * Math.PI) / 180) * len
	});
	const armL = between(rng, 8, 40);
	const armR = between(rng, 8, 40);
	const legL = between(rng, 2, 14);
	const legR = between(rng, 2, 14);
	const elL = limb(p.shL, height * 0.19, armL);
	const elR = limb(p.shR, height * 0.19, -armR);
	const wrL = limb(elL, height * 0.16, armL * 0.4);
	const wrR = limb(elR, height * 0.16, -armR * 0.4);
	const knL = limb(p.hipL, height * 0.24, legL);
	const knR = limb(p.hipR, height * 0.24, -legR);
	const anL = { x: knL.x + (knL.x - p.hipL.x) * 0.3, y: top + height - u * 0.08 };
	const anR = { x: knR.x + (knR.x - p.hipR.x) * 0.3, y: top + height - u * 0.08 };
	const stroke = `fill="none" stroke="${INK}" stroke-width="4"`;
	const bone = (a: Pt, b: Pt, t: number) => {
		const c = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
		const len = Math.hypot(b.x - a.x, b.y - a.y);
		return ellipse(c, len / 2, t, (Math.atan2(b.y - a.y, b.x - a.x) * 180) / Math.PI, stroke);
	};
	const figure =
		ellipse({ x: cx, y: at(0.5) }, u * 0.36, u * 0.5, 0, stroke) +
		ellipse({ x: cx, y: H(0.29) }, shoulderW * 0.95, (H(0.42) - H(0.17)) / 2, 0, stroke) +
		ellipse({ x: cx, y: H(0.5) }, hipW, u * 0.42, 0, stroke) +
		bone(p.shL, elL, u * 0.16) +
		bone(elL, wrL, u * 0.13) +
		bone(p.shR, elR, u * 0.16) +
		bone(elR, wrR, u * 0.13) +
		bone(p.hipL, knL, u * 0.22) +
		bone(knL, anL, u * 0.17) +
		bone(p.hipR, knR, u * 0.22) +
		bone(knR, anR, u * 0.17);
	let ruler = '';
	const rx = Math.max(40, cx - u * 2.6);
	for (let k = 0; k <= Math.ceil(units); k++) {
		const y = Math.min(at(k), top + height);
		ruler += line({ x: rx - 10, y }, { x: rx + 10, y }, `stroke="${GUIDE}" stroke-width="2"`);
		if (k < units)
			ruler += `<text x="${n(rx - 18)}" y="${n(Math.min(at(k + 0.5), top + height) + 6)}" font-family="sans-serif" font-size="18" font-weight="700" fill="${GUIDE}" text-anchor="end">${k + 1}</text>`;
	}
	ruler += line(
		{ x: rx, y: top },
		{ x: rx, y: top + height },
		`stroke="${GUIDE}" stroke-width="2"`
	);
	return { prompt: svgDoc(w, h, ruler + figure), answer: svgDoc(w, h, figure) };
}

/** Fallback subject for the contour drills when no reference picture is available. */
function contourSubject(rng: Rng, w = 800, h = 600): DrillTarget {
	const outer = randomBlob(rng, w, h);
	const { x0, y0, x1, y1 } = bbox(outer);
	const inner = blobPoints(
		rng,
		{ x: x0 + (x1 - x0) * between(rng, 0.4, 0.6), y: y0 + (y1 - y0) * between(rng, 0.35, 0.6) },
		(x1 - x0) * 0.16,
		(y1 - y0) * 0.14
	);
	const attrs = `fill="none" stroke="${INK}" stroke-width="4" stroke-linejoin="round"`;
	const pic = svgDoc(
		w,
		h,
		`<path d="${smoothPath(outer)}" ${attrs}/><path d="${smoothPath(inner)}" ${attrs}/>`
	);
	return { prompt: pic, answer: pic };
}

// ---------------------------------------------------------------- the drills

type DrillDef = Omit<Drill, 'generate'>;

const VALUE_WEIGHTS: Partial<GradeWeights> = {
	placement: 0.05,
	outline: 0.05,
	silhouette: 0.2,
	grid: 0.1,
	tone: 0.45,
	ssim: 0.15,
	lineConfidence: 0,
	keypoints: 0
};

const defs: DrillDef[] = [
	{
		id: 'ghosted-lines',
		name: 'Ghosted lines',
		skill: 'line_confidence',
		kind: 'lines',
		blurb: 'Join two dots with one straight, confident stroke.',
		howTo: [
			'Hover over the first dot and ghost the motion a few times without touching.',
			'Move from the shoulder, not the wrist.',
			'Draw once, quickly, and aim to land on the second dot.'
		],
		rounds: 6,
		brush: 'pen',
		target: ghostedLines
	},
	{
		id: 'angle-matching',
		name: 'Angle matching',
		skill: 'proportions',
		kind: 'lines',
		blurb: 'Copy a slanted line at exactly the same angle.',
		howTo: [
			'Compare the line to a true vertical or horizontal: how far does it lean?',
			'Draw your copy in the dashed box, in one stroke.',
			'Length doesn’t matter much; the angle does.'
		],
		rounds: 6,
		brush: 'pen',
		target: angleMatching
	},
	{
		id: 'ellipse-pages',
		name: 'Ellipse pages',
		skill: 'line_confidence',
		kind: 'ellipses',
		blurb: 'Copy ellipses of every size, tilt and fullness.',
		howTo: [
			'Find the long axis first; the ellipse is symmetrical around it.',
			'Ghost the loop a couple of times, then draw it in one or two passes.',
			'Keep the ends round, not pointed.'
		],
		rounds: 6,
		brush: 'pen',
		target: ellipsePages
	},
	{
		id: 'block-in-boxes',
		name: 'Block-in boxes',
		skill: 'placement',
		kind: 'boxes',
		blurb: 'Block in a simple object with the boxes it’s made of.',
		howTo: [
			'Look at the whole shape first: how tall is it compared to how wide?',
			'Draw the biggest box, then stack the others on it.',
			'Check each box’s width against the one below.'
		],
		rounds: 5,
		brush: 'pencil',
		target: blockInBoxes
	},
	{
		id: 'four-corner-marks',
		name: 'Four corner marks',
		skill: 'placement',
		kind: 'boxes',
		blurb: 'Mark the top, bottom, left and right limits before you draw.',
		howTo: [
			'Find the highest, lowest, leftmost and rightmost points of the shape.',
			'Put a small corner tick at each of the four corners of that box.',
			'Get the width-to-height ratio right. Size on the page is up to you.'
		],
		rounds: 5,
		brush: 'pencil',
		target: fourCornerMarks
	},
	{
		id: 'envelope-shapes',
		name: 'Envelope shapes',
		skill: 'placement',
		kind: 'shapes',
		blurb: 'Wrap a curvy shape in a few straight lines.',
		howTo: [
			'Imagine stretching a rubber band around the shape.',
			'Use 5 to 7 straight lines that touch its outermost points.',
			'Ignore the small bumps; get the big angles.'
		],
		rounds: 5,
		brush: 'pencil',
		target: envelopeShapes
	},
	{
		id: 'silhouette-fill',
		name: 'Silhouette fill',
		skill: 'silhouette',
		kind: 'shapes',
		blurb: 'Paint the shape as one solid, dark mass.',
		howTo: [
			'No outlines first: block in the shape straight away with the marker.',
			'Squint at the target and match the big shape, not the edges.',
			'Fill it solid.'
		],
		rounds: 4,
		secondsPerRound: 90,
		brush: 'marker',
		weights: {
			placement: 0.1,
			outline: 0.15,
			silhouette: 0.5,
			grid: 0.15,
			tone: 0.1,
			ssim: 0,
			lineConfidence: 0,
			keypoints: 0
		},
		target: silhouetteFill
	},
	{
		id: 'grid-copy',
		name: 'Grid copy',
		skill: 'silhouette',
		kind: 'shapes',
		blurb: 'Copy a shape one grid square at a time.',
		howTo: [
			'Use the 3×3 grid on your canvas.',
			'For each corner, note which square it sits in and where inside that square.',
			'Join the corners with straight lines.'
		],
		rounds: 4,
		brush: 'pencil',
		target: gridCopy
	},
	{
		id: 'five-step-value-scale',
		name: 'Five-step value scale',
		skill: 'tone',
		kind: 'values',
		blurb: 'Shade five boxes from the lightest light to the darkest dark.',
		howTo: [
			'Do the two ends first: barely-there for box 1, as dark as you can for box 5.',
			'Then the middle box, halfway between them.',
			'Fill boxes 2 and 4 last. Squint to check the steps are even.'
		],
		rounds: 3,
		brush: 'pencil',
		weights: VALUE_WEIGHTS,
		target: fiveStepValueScale
	},
	{
		id: 'three-value-thumbnail',
		name: 'Three-value thumbnail',
		skill: 'tone',
		kind: 'values',
		blurb: 'Simplify a picture into paper white, one mid grey and one dark.',
		howTo: [
			'Leave the light shapes as bare paper.',
			'Lay in the mid grey as one flat tone.',
			'Add the darkest shape last, and keep it to three values only.'
		],
		rounds: 3,
		secondsPerRound: 120,
		brush: 'marker',
		weights: VALUE_WEIGHTS,
		target: threeValueThumbnail
	},
	{
		id: 'head-units',
		name: 'Head units',
		skill: 'proportions',
		kind: 'figure',
		blurb: 'Measure a mannequin figure in head-heights.',
		howTo: [
			'Count how many heads tall the figure is (the ruler shows it).',
			'Draw the head first, then mark where each head unit falls.',
			'Place the shoulders, hips and knees on those marks, then add the ovals.'
		],
		rounds: 3,
		brush: 'pencil',
		target: headUnits
	},
	{
		id: 'contour-in-one-minute',
		name: 'Contour in one minute',
		skill: 'outline',
		kind: 'reference',
		blurb: 'Draw the outline of a real picture in 60 seconds.',
		howTo: [
			'Start anywhere on the edge and follow it around.',
			'Go for the whole outline first; skip the details.',
			'The round ends on its own after one minute.'
		],
		rounds: 3,
		secondsPerRound: 60,
		brush: 'pen',
		target: contourSubject
	},
	{
		id: 'blind-contour',
		name: 'Blind contour',
		skill: 'outline',
		kind: 'reference',
		blurb: 'A “looking” exercise: draw the edges without seeing your lines.',
		howTo: [
			'Your ink stays hidden until the round ends. That’s the point.',
			'Keep your eyes on the picture and move slowly along every edge.',
			'It won’t look like the picture. It is scored only on how steady your lines are.'
		],
		rounds: 3,
		secondsPerRound: 60,
		brush: 'pen',
		blind: true,
		target: contourSubject
	}
];

export const drills: Drill[] = defs.map((d) => ({
	...d,
	generate: (rng, w = 800, h = 600) => d.target(rng, w, h).prompt
}));

export const drillById = (id: string | null | undefined) => drills.find((d) => d.id === id);

/** Drills grouped by skill, in the order skills first appear. */
export function drillsBySkill() {
	const groups = new Map<DrillSkill, Drill[]>();
	for (const d of drills) groups.set(d.skill, [...(groups.get(d.skill) ?? []), d]);
	return [...groups].map(([skill, list]) => ({ skill, name: SKILL_NAMES[skill], drills: list }));
}

// ---------------------------------------------------------------- results

/** One finished drill session, stored in localStorage under `pp.drills` (last 200). */
export interface DrillResult {
	id: string;
	/** ms since epoch */
	at: number;
	/** mean round score, 1–10 */
	avg: number;
	/** each round's score, 1–10 */
	rounds: number[];
}

export const DRILLS_KEY = 'pp.drills';

export function loadDrillResults(storage: Pick<Storage, 'getItem'> = localStorage): DrillResult[] {
	try {
		const v = JSON.parse(storage.getItem(DRILLS_KEY) ?? '[]');
		return Array.isArray(v) ? v : [];
	} catch {
		return [];
	}
}

export function saveDrillResult(
	r: DrillResult,
	storage: Pick<Storage, 'getItem' | 'setItem'> = localStorage
) {
	try {
		const list = [...loadDrillResults(storage), r].slice(-200);
		storage.setItem(DRILLS_KEY, JSON.stringify(list));
	} catch {
		/* storage unavailable: the summary still shows, nothing is kept */
	}
}
