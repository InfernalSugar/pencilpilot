/**
 * The pen tool (Photoshop's Pen, Illustrator-style paths): click to add a
 * corner, drag to pull out smooth handles, click the first point to close.
 * The path stays editable until it's stroked with the brush or filled.
 */
export type Pt = [number, number];

export interface Anchor {
	p: Pt;
	/** handle towards the previous anchor (absolute canvas px; = p for a corner) */
	hin: Pt;
	/** handle towards the next anchor */
	hout: Pt;
}

export interface PenPath {
	anchors: Anchor[];
	closed: boolean;
}

export const EMPTY_PATH: PenPath = { anchors: [], closed: false };

export const corner = (p: Pt): Anchor => ({ p, hin: p, hout: p });

/** Point at t (0–1) on the cubic from a to b. */
function cubic(a: Anchor, b: Anchor, t: number): Pt {
	const u = 1 - t;
	const k = [u * u * u, 3 * u * u * t, 3 * u * t * t, t * t * t];
	return [
		k[0] * a.p[0] + k[1] * a.hout[0] + k[2] * b.hin[0] + k[3] * b.p[0],
		k[0] * a.p[1] + k[1] * a.hout[1] + k[2] * b.hin[1] + k[3] * b.p[1]
	];
}

/** The path's segments as anchor pairs (with the closing one when closed). */
function segments(path: PenPath): [Anchor, Anchor][] {
	const a = path.anchors;
	const out: [Anchor, Anchor][] = [];
	for (let i = 0; i + 1 < a.length; i++) out.push([a[i], a[i + 1]]);
	if (path.closed && a.length > 2) out.push([a[a.length - 1], a[0]]);
	return out;
}

/** Points along the path about `step` px apart (for stroking and filling). */
export function samplePath(path: PenPath, step = 2): Pt[] {
	const segs = segments(path);
	if (!segs.length) return path.anchors.length ? [path.anchors[0].p] : [];
	const out: Pt[] = [segs[0][0].p];
	for (const [a, b] of segs) {
		// a fine polyline first, then even spacing along it
		const fine: Pt[] = Array.from({ length: 65 }, (_, i) => cubic(a, b, i / 64));
		let carry = 0;
		for (let i = 1; i < fine.length; i++) {
			const [x0, y0] = fine[i - 1];
			const [x1, y1] = fine[i];
			const len = Math.hypot(x1 - x0, y1 - y0);
			let d = step - carry;
			while (d <= len) {
				const t = d / len;
				out.push([x0 + (x1 - x0) * t, y0 + (y1 - y0) * t]);
				d += step;
			}
			carry = len - (d - step);
		}
		out.push(b.p);
	}
	return out;
}

/** An SVG path for drawing it. */
export function pathD(path: PenPath): string {
	const a = path.anchors;
	if (!a.length) return '';
	const f = (p: Pt) => `${p[0].toFixed(1)} ${p[1].toFixed(1)}`;
	let d = `M${f(a[0].p)}`;
	for (const [s, e] of segments(path)) d += ` C${f(s.hout)} ${f(e.hin)} ${f(e.p)}`;
	return path.closed ? d + ' Z' : d;
}

/** What's under the pointer: an anchor, one of its handles, or nothing. */
export function hitPath(
	path: PenPath,
	p: Pt,
	r: number
): { index: number; part: 'p' | 'hin' | 'hout' } | null {
	const near = (q: Pt) => Math.hypot(q[0] - p[0], q[1] - p[1]) <= r;
	for (let i = path.anchors.length - 1; i >= 0; i--) {
		const a = path.anchors[i];
		if (near(a.p)) return { index: i, part: 'p' };
		if (a.hout !== a.p && near(a.hout) && (a.hout[0] !== a.p[0] || a.hout[1] !== a.p[1]))
			return { index: i, part: 'hout' };
		if (a.hin !== a.p && near(a.hin) && (a.hin[0] !== a.p[0] || a.hin[1] !== a.p[1]))
			return { index: i, part: 'hin' };
	}
	return null;
}

/**
 * Drag part of an anchor to `to`. Moving the point carries its handles along;
 * moving a handle mirrors the other one (a smooth point) unless `broken`.
 */
export function dragPart(
	path: PenPath,
	index: number,
	part: 'p' | 'hin' | 'hout',
	to: Pt,
	broken = false
): PenPath {
	const anchors = path.anchors.map((a, i) => {
		if (i !== index) return a;
		if (part === 'p') {
			const [dx, dy] = [to[0] - a.p[0], to[1] - a.p[1]];
			const mv = (q: Pt): Pt => [q[0] + dx, q[1] + dy];
			return { p: to, hin: mv(a.hin), hout: mv(a.hout) };
		}
		const mirror: Pt = [2 * a.p[0] - to[0], 2 * a.p[1] - to[1]];
		if (part === 'hout') return { ...a, hout: to, hin: broken ? a.hin : mirror };
		return { ...a, hin: to, hout: broken ? a.hout : mirror };
	});
	return { ...path, anchors };
}
