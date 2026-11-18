/**
 * Shared, dependency-free helpers for the drill target generators: a seeded PRNG
 * and small SVG-building functions. Everything is deterministic for a given RNG.
 */

export interface Pt {
	x: number;
	y: number;
}

export type Rng = () => number;

// ---------------------------------------------------------------- randomness

/** Small, fast seeded PRNG (mulberry32). Returns floats in [0, 1). */
export function seeded(seed: number): Rng {
	// Scramble the seed first (murmur3 finaliser): without it, mulberry32's first few
	// outputs are correlated for nearby seeds, so e.g. every round made the same first pick.
	let a = seed >>> 0;
	a = Math.imul(a ^ (a >>> 16), 0x85ebca6b);
	a = Math.imul(a ^ (a >>> 13), 0xc2b2ae35);
	a = (a ^ (a >>> 16)) >>> 0;
	return () => {
		a = (a + 0x6d2b79f5) >>> 0;
		let t = a;
		t = Math.imul(t ^ (t >>> 15), t | 1);
		t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
}

export const between = (rng: Rng, a: number, b: number) => a + rng() * (b - a);
export const pick = <T>(rng: Rng, xs: readonly T[]) => xs[Math.floor(rng() * xs.length)];
export const n = (v: number) => +v.toFixed(1);

// ---------------------------------------------------------------- svg helpers

export const INK = '#16161a';
export const PALE = '#d9d5cc';
export const GUIDE = '#8a7fd0';
export const DOT = '#e8492f';

export function svgDoc(w: number, h: number, body: string, background = true) {
	const bg = background ? `<rect width="${w}" height="${h}" fill="#fff"/>` : '';
	return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}">${bg}${body}</svg>`;
}

export const line = (a: Pt, b: Pt, attrs = `stroke="${INK}" stroke-width="4"`) =>
	`<line x1="${n(a.x)}" y1="${n(a.y)}" x2="${n(b.x)}" y2="${n(b.y)}" ${attrs} stroke-linecap="round"/>`;

export const polyPath = (pts: Pt[]) =>
	'M' + pts.map((p) => `${n(p.x)} ${n(p.y)}`).join(' L') + ' Z';

/** Closed Catmull-Rom spline through the points, as cubic Béziers. */
export function smoothPath(pts: Pt[]) {
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

export const ellipse = (c: Pt, rx: number, ry: number, deg: number, attrs: string) =>
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

export const cross = (o: Pt, a: Pt, b: Pt) => (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);

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

export function bbox(pts: Pt[]) {
	const xs = pts.map((p) => p.x);
	const ys = pts.map((p) => p.y);
	return {
		x0: Math.min(...xs),
		y0: Math.min(...ys),
		x1: Math.max(...xs),
		y1: Math.max(...ys)
	};
}

// ---------------------------------------------------------------- more helpers

/** `rgb(v,v,v)` for a grey value 0 (black) – 255 (white). */
export const grey = (v: number) => {
	const k = Math.round(Math.min(255, Math.max(0, v)));
	return `rgb(${k},${k},${k})`;
};

/** A document-unique-enough id for clip paths, masks and gradients (several previews share a page). */
export const uid = (rng: Rng, prefix: string) => `pp-${prefix}-${Math.floor(rng() * 1e9)}`;

export const STROKE = `fill="none" stroke="${INK}" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"`;
export const DASHED = `fill="none" stroke="${GUIDE}" stroke-width="2" stroke-dasharray="8 6"`;

/** Open polyline path (no Z). */
export const openPath = (pts: Pt[]) => 'M' + pts.map((p) => `${n(p.x)} ${n(p.y)}`).join(' L');

export const lerp = (a: Pt, b: Pt, t: number): Pt => ({
	x: a.x + (b.x - a.x) * t,
	y: a.y + (b.y - a.y) * t
});

export const rotate = (p: Pt, deg: number, o: Pt = { x: 0, y: 0 }): Pt => {
	const r = (deg * Math.PI) / 180;
	const [c, s] = [Math.cos(r), Math.sin(r)];
	return { x: o.x + (p.x - o.x) * c - (p.y - o.y) * s, y: o.y + (p.x - o.x) * s + (p.y - o.y) * c };
};

/** Point on a cubic Bézier. */
export function cubic(p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: number): Pt {
	const u = 1 - t;
	const [a, b, c, d] = [u * u * u, 3 * u * u * t, 3 * u * t * t, t * t * t];
	return {
		x: a * p0.x + b * p1.x + c * p2.x + d * p3.x,
		y: a * p0.y + b * p1.y + c * p2.y + d * p3.y
	};
}

/**
 * Uniform scale + translate that fits the points' bounding box into `frac` of a w×h canvas,
 * centred. Returns the mapping and the scale factor (for stroke widths given in model units).
 */
export function fitter(pts: Pt[], w: number, h: number, frac = 0.84) {
	const b = bbox(pts);
	const bw = Math.max(1e-6, b.x1 - b.x0);
	const bh = Math.max(1e-6, b.y1 - b.y0);
	const k = Math.min((w * frac) / bw, (h * frac) / bh);
	const ox = (w - bw * k) / 2 - b.x0 * k;
	const oy = (h - bh * k) / 2 - b.y0 * k;
	return { map: (p: Pt): Pt => ({ x: ox + p.x * k, y: oy + p.y * k }), k };
}

/** Clip the segment a→b to an axis-aligned rectangle (Liang–Barsky); null when it misses. */
export function clipSegment(
	a: Pt,
	b: Pt,
	r: { x0: number; y0: number; x1: number; y1: number }
): [Pt, Pt] | null {
	let t0 = 0;
	let t1 = 1;
	const dx = b.x - a.x;
	const dy = b.y - a.y;
	const checks: [number, number][] = [
		[-dx, a.x - r.x0],
		[dx, r.x1 - a.x],
		[-dy, a.y - r.y0],
		[dy, r.y1 - a.y]
	];
	for (const [p, q] of checks) {
		if (p === 0) {
			if (q < 0) return null;
		} else {
			const t = q / p;
			if (p < 0) t0 = Math.max(t0, t);
			else t1 = Math.min(t1, t);
		}
	}
	if (t0 > t1) return null;
	return [lerp(a, b, t0), lerp(a, b, t1)];
}

/** Text label in the guide colour (prompt pictures only, never graded). */
export const label = (p: Pt, text: string, anchor = 'start', size = 16, fill = GUIDE) =>
	`<text x="${n(p.x)}" y="${n(p.y)}" font-family="sans-serif" font-size="${size}" font-weight="700" fill="${fill}" text-anchor="${anchor}">${text}</text>`;

/** Small sun icon marking where the light comes from. */
export function sun(c: Pt, r = 14) {
	let rays = '';
	for (let i = 0; i < 8; i++) {
		const t = (i / 8) * Math.PI * 2;
		rays += line(
			{ x: c.x + Math.cos(t) * r * 1.45, y: c.y + Math.sin(t) * r * 1.45 },
			{ x: c.x + Math.cos(t) * r * 2, y: c.y + Math.sin(t) * r * 2 },
			`stroke="#f2b705" stroke-width="3"`
		);
	}
	return `<circle cx="${n(c.x)}" cy="${n(c.y)}" r="${n(r)}" fill="#ffd23f" stroke="#f2b705" stroke-width="2"/>${rays}`;
}
