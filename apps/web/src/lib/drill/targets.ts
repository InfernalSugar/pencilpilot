/**
 * Target generators for the M8 drills (see docs/planning/M8_CONTENT_PLAN.md).
 *
 * Each one is `(rng, w, h) => DrillTarget`: plain, seeded geometry drawn as SVG, so
 * rounds are endless and deterministic. Most build their picture in "model units"
 * and fit it to the canvas with `fitter`, so every seed stays on the page.
 */
import type { DrillTarget } from '../drills';
import {
	DASHED,
	DOT,
	GUIDE,
	INK,
	PALE,
	STROKE,
	between,
	bbox,
	blobPoints,
	clipSegment,
	convexHull,
	cubic,
	ellipse,
	fitter,
	grey,
	label,
	lerp,
	line,
	n,
	openPath,
	pick,
	polyPath,
	rotate,
	smoothPath,
	sun,
	svgDoc,
	uid,
	type Pt,
	type Rng
} from './svg';

const stroke = (width: number, colour = INK) =>
	`fill="none" stroke="${colour}" stroke-width="${n(width)}" stroke-linecap="round" stroke-linejoin="round"`;
const dot = (p: Pt, r: number, fill = DOT) =>
	`<circle cx="${n(p.x)}" cy="${n(p.y)}" r="${n(r)}" fill="${fill}"/>`;
/** Model-unit coordinates (scaled up later by an SVG transform) need more than 1 decimal. */
const u = (v: number) => +v.toFixed(4);
const rectU = (x: number, y: number, w: number, h: number, attrs: string) =>
	`<rect x="${u(x)}" y="${u(y)}" width="${u(w)}" height="${u(h)}" ${attrs}/>`;
const polyU = (pts: Pt[]) => 'M' + pts.map((p) => `${u(p.x)} ${u(p.y)}`).join(' L') + ' Z';
const ellipseU = (c: Pt, rx: number, ry: number, attrs: string) =>
	`<ellipse cx="${u(c.x)}" cy="${u(c.y)}" rx="${u(rx)}" ry="${u(ry)}" ${attrs}/>`;
const rectSvg = (x: number, y: number, w: number, h: number, attrs: string) =>
	`<rect x="${n(x)}" y="${n(y)}" width="${n(w)}" height="${n(h)}" ${attrs}/>`;
const add = (a: Pt, b: Pt): Pt => ({ x: a.x + b.x, y: a.y + b.y });
const polar = (len: number, deg: number): Pt => ({
	x: Math.cos((deg * Math.PI) / 180) * len,
	y: Math.sin((deg * Math.PI) / 180) * len
});

// ================================================================ gesture

/**
 * Fallback for gesture-30-second when no reference picture loads: a figure hung on
 * one C- or S-shaped line of action (head → standing foot), with simple limbs.
 */
export function gestureFigure(rng: Rng, w = 800, h = 600): DrillTarget {
	const head: Pt = { x: 0, y: 0 };
	const foot: Pt = { x: between(rng, -0.25, 0.25), y: 1 };
	const s = pick(rng, [-1, 1]);
	const sCurve = rng() < 0.45;
	const c1: Pt = { x: s * between(rng, 0.15, 0.35), y: 0.3 };
	const c2: Pt = { x: foot.x + (sCurve ? -s : s) * between(rng, 0.1, 0.3), y: 0.7 };
	const at = (t: number) => cubic(head, c1, c2, foot, t);
	const shoulder = at(0.2);
	const hip = at(0.5);
	// Second leg: hip → knee → foot, bent and swung out.
	const swing = pick(rng, [-1, 1]);
	const knee = add(hip, polar(0.27, 90 - swing * between(rng, 15, 55)));
	const foot2 = add(knee, polar(0.27, 90 + swing * between(rng, -25, 30)));
	// Arms: shoulder → elbow → hand, one up-ish one down-ish for variety.
	const arm = (deg: number, bend: number) => {
		const elbow = add(shoulder, polar(0.2, deg));
		return [elbow, add(elbow, polar(0.19, deg + bend))] as const;
	};
	const [e1, h1] = arm(pick(rng, [-1, 1]) * between(rng, 100, 175), between(rng, -50, 50));
	const [e2, h2] = arm(pick(rng, [-1, 1]) * between(rng, 5, 75), between(rng, -50, 50));
	const pts = [head, foot, shoulder, hip, knee, foot2, e1, h1, e2, h2, c1, c2];
	pts.push({ x: -0.09, y: -0.1 }, { x: 0.09, y: -0.1 });
	const { map, k } = fitter(pts, w, h, 0.8);
	const P = (p: Pt) => `${n(map(p).x)} ${n(map(p).y)}`;
	const action = `<path d="M${P(head)} C${P(c1)} ${P(c2)} ${P(foot)}" ${stroke(0.022 * k)}/>`;
	const limbs =
		`<path d="M${P(hip)} L${P(knee)} L${P(foot2)}" ${stroke(0.014 * k)}/>` +
		`<path d="M${P(e1)} L${P(shoulder)} L${P(e2)}" ${stroke(0.014 * k)}/>` +
		`<path d="M${P(e1)} L${P(h1)}" ${stroke(0.014 * k)}/><path d="M${P(e2)} L${P(h2)}" ${stroke(0.014 * k)}/>`;
	const headShape = ellipse(map({ x: 0, y: -0.04 }), 0.065 * k, 0.08 * k, 0, stroke(0.012 * k));
	const highlight = `<path d="M${P(head)} C${P(c1)} ${P(c2)} ${P(foot)}" ${stroke(0.055 * k, '#f7b8a8')}/>`;
	return {
		prompt: svgDoc(w, h, highlight + action + limbs + headShape),
		answer: svgDoc(w, h, action + limbs + headShape)
	};
}

// ================================================================ negative space

type Paint = (fill: string) => string;

/** Silhouette subjects in model units (floor at y = 1), each a function of the paint colour. */
function negativeSubject(rng: Rng): { paint: Paint; pts: Pt[] } {
	const kind = pick(rng, ['chair', 'table', 'mug'] as const);
	if (kind === 'chair') {
		const d = between(rng, 0.38, 0.52); // seat depth
		const seat = between(rng, 0.45, 0.56);
		const top = between(rng, 0, 0.08);
		const pw = between(rng, 0.04, 0.06);
		const tilt = between(rng, 0.03, 0.1);
		const rail = between(rng, 0.74, 0.86);
		const back = (y: number) => -tilt * (1 - (y - top) / (seat - top)); // back post leans back
		const paint: Paint = (f) =>
			`<path d="M${u(back(top))} ${u(top)} L${u(back(top) + pw)} ${u(top)} L${u(pw)} ${u(seat)} L${u(pw)} 1 L0 1 L0 ${u(seat)} Z" fill="${f}"/>` +
			rectU(0, seat, d, 0.05, `fill="${f}"`) +
			rectU(d - pw, seat, pw, 1 - seat, `fill="${f}"`) +
			rectU(0, rail, d, 0.03, `fill="${f}"`);
		return {
			paint,
			pts: [
				{ x: -tilt, y: top },
				{ x: d, y: 1 }
			]
		};
	}
	if (kind === 'table') {
		const tw = between(rng, 0.8, 1.2);
		const ty = between(rng, 0.3, 0.5);
		const splay = between(rng, 0, 0.08);
		const lw = between(rng, 0.05, 0.08);
		const rail = between(rng, 0.68, 0.82);
		const inset = between(rng, 0.06, 0.14);
		const leg = (x: number, dir: number) =>
			`<path d="${polyU([
				{ x, y: ty },
				{ x: x + dir * lw, y: ty },
				{ x: x + dir * lw - dir * splay, y: 1 },
				{ x: x - dir * splay, y: 1 }
			])}"`;
		const paint: Paint = (f) =>
			rectU(0, ty, tw, 0.06, `rx="0.01" fill="${f}"`) +
			`${leg(inset, 1)} fill="${f}"/>${leg(tw - inset, -1)} fill="${f}"/>` +
			rectU(inset, rail, tw - 2 * inset, 0.03, `fill="${f}"`);
		return {
			paint,
			pts: [
				{ x: -splay, y: ty },
				{ x: tw + splay, y: 1 }
			]
		};
	}
	const bw = between(rng, 0.45, 0.65);
	const top = between(rng, 0.2, 0.4);
	const hr = between(rng, 0.13, 0.2);
	const paint: Paint = (f) =>
		ellipseU(
			{ x: bw, y: (top + 1) / 2 + 0.02 },
			hr,
			(1 - top) * 0.3,
			`fill="none" stroke="${f}" stroke-width="0.07"`
		) + rectU(0, top, bw, 1 - top, `rx="0.06" fill="${f}"`);
	return {
		paint,
		pts: [
			{ x: 0, y: top },
			{ x: bw + hr + 0.04, y: 1 }
		]
	};
}

export function negativeSpace(rng: Rng, w = 800, h = 600): DrillTarget {
	const { paint, pts } = negativeSubject(rng);
	const m = between(rng, 0.08, 0.16);
	const b = bbox(pts);
	const frame = { x0: b.x0 - m, y0: b.y0 - m, x1: b.x1 + m, y1: 1 + m * 0.4 };
	const { map, k } = fitter(
		[
			{ x: frame.x0, y: frame.y0 },
			{ x: frame.x1, y: frame.y1 }
		],
		w,
		h,
		0.86
	);
	const flip = rng() < 0.5;
	const o = map({ x: frame.x0, y: frame.y0 });
	const fw = (frame.x1 - frame.x0) * k;
	const fh = (frame.y1 - frame.y0) * k;
	// Model → canvas, mirrored half the time.
	const tf = flip
		? `translate(${n(o.x + fw)} ${n(o.y)}) scale(${n(-k)} ${n(k)}) translate(${n(-frame.x0)} ${n(-frame.y0)})`
		: `translate(${n(o.x)} ${n(o.y)}) scale(${n(k)}) translate(${n(-frame.x0)} ${n(-frame.y0)})`;
	const subject = (f: string) => `<g transform="${tf}">${paint(f)}</g>`;
	const frameRect = (attrs: string) => rectSvg(o.x, o.y, fw, fh, attrs);
	const id = uid(rng, 'neg');
	const gaps =
		`<defs><mask id="${id}">${frameRect('fill="#fff"')}${subject('#000')}</mask></defs>` +
		frameRect(`fill="${INK}" mask="url(#${id})"`);
	return {
		prompt: svgDoc(
			w,
			h,
			frameRect(`fill="#e4dcfb"`) +
				subject('#b9b4a9') +
				frameRect(`fill="none" stroke="${INK}" stroke-width="3"`)
		),
		answer: svgDoc(w, h, gaps),
		guide: svgDoc(w, h, frameRect(DASHED), false)
	};
}

// ================================================================ faces

/** Front-view face on the classic proportions: eyes halfway, hairline/brow/nose/chin thirds. */
export function faceProportions(rng: Rng, w = 800, h = 600): DrillTarget {
	const ww = between(rng, 0.64, 0.76); // head width (head height = 1)
	const jw = ww * between(rng, 0.62, 0.78);
	const hair = between(rng, 0.12, 0.18);
	const third = (1 - hair) / 3;
	const brow = hair + third;
	const nose = hair + 2 * third;
	const eye = between(rng, 0.48, 0.52);
	const mouth = nose + (1 - nose) * between(rng, 0.32, 0.42);
	const ew = ww / 5; // a face is about five eyes wide
	const tilt = between(rng, -6, 6);
	const half = ww / 2;
	const outline: Pt[] = [
		{ x: 0, y: 0 },
		{ x: half * 0.72, y: 0.07 },
		{ x: half, y: 0.36 },
		{ x: half * 0.95, y: 0.6 },
		{ x: jw / 2, y: 0.82 },
		{ x: jw * 0.18, y: 0.98 },
		{ x: 0, y: 1 },
		{ x: -jw * 0.18, y: 0.98 },
		{ x: -jw / 2, y: 0.82 },
		{ x: -half * 0.95, y: 0.6 },
		{ x: -half, y: 0.36 },
		{ x: -half * 0.72, y: 0.07 }
	];
	const { map, k } = fitter(
		[
			{ x: -half - 0.3, y: -0.05 },
			{ x: half + 0.3, y: 1.05 }
		],
		w,
		h,
		0.9
	);
	const c = map({ x: 0, y: 0.5 });
	const M = (p: Pt) => rotate(map(p), tilt, c);
	const P = (p: Pt) => `${n(M(p).x)} ${n(M(p).y)}`;
	const sw = (u: number) => stroke(u * k);
	const eyeH = ew * between(rng, 0.32, 0.42);
	const eyeShape = (cx: number) =>
		`<path d="M${P({ x: cx - ew / 2, y: eye })} Q${P({ x: cx, y: eye - eyeH })} ${P({ x: cx + ew / 2, y: eye })} Q${P({ x: cx, y: eye + eyeH * 0.8 })} ${P({ x: cx - ew / 2, y: eye })} Z" ${sw(0.008)}/>` +
		`<circle cx="${n(M({ x: cx, y: eye }).x)}" cy="${n(M({ x: cx, y: eye }).y)}" r="${n(eyeH * 0.42 * k)}" fill="${INK}"/>`;
	const browShape = (cx: number) =>
		`<path d="M${P({ x: cx - ew * 0.6, y: brow + 0.01 })} Q${P({ x: cx, y: brow - 0.035 })} ${P({ x: cx + ew * 0.6, y: brow })}" ${sw(0.009)}/>`;
	const ear = (s: number) =>
		`<path d="M${P({ x: s * half * 0.98, y: brow })} C${P({ x: s * (half + 0.09), y: brow - 0.02 })} ${P({ x: s * (half + 0.08), y: nose })} ${P({ x: s * half * 0.95, y: nose })}" ${sw(0.008)}/>`;
	const nw = ew * between(rng, 0.45, 0.55);
	const noseShape = `<path d="M${P({ x: -nw, y: nose - 0.025 })} Q${P({ x: -nw * 0.6, y: nose + 0.01 })} ${P({ x: 0, y: nose + 0.005 })} Q${P({ x: nw * 0.6, y: nose + 0.01 })} ${P({ x: nw, y: nose - 0.025 })}" ${sw(0.008)}/>`;
	const mw = ew * between(rng, 0.65, 0.85);
	const smile = between(rng, -0.01, 0.025);
	const mouthShape = `<path d="M${P({ x: -mw, y: mouth })} Q${P({ x: 0, y: mouth + smile })} ${P({ x: mw, y: mouth })}" ${sw(0.009)}/>`;
	const neck = (s: number) =>
		`<path d="M${P({ x: s * jw * 0.36, y: 0.86 })} L${P({ x: s * jw * 0.4, y: 1.04 })}" ${sw(0.008)}/>`;
	const face =
		`<path d="${smoothPath(outline.map(M))}" ${sw(0.01)}/>` +
		eyeShape(-ew) +
		eyeShape(ew) +
		browShape(-ew) +
		browShape(ew) +
		ear(-1) +
		ear(1) +
		noseShape +
		mouthShape +
		neck(-1) +
		neck(1);
	const guideLine = (y: number) =>
		line(
			M({ x: -half - 0.06, y }),
			M({ x: half + 0.06, y }),
			`stroke="${GUIDE}" stroke-width="2" stroke-dasharray="7 6"`
		);
	const names: [number, string][] = [
		[hair, 'hairline'],
		[brow, 'brow'],
		[eye, 'eyes ½'],
		[nose, 'nose'],
		[mouth, 'mouth']
	];
	const guides =
		line(
			M({ x: 0, y: -0.04 }),
			M({ x: 0, y: 1.04 }),
			`stroke="${GUIDE}" stroke-width="2" stroke-dasharray="7 6"`
		) +
		names
			.map(([y, t]) => guideLine(y) + label(add(M({ x: half + 0.08, y }), { x: 0, y: 5 }), t))
			.join('');
	const ticks =
		line(M({ x: -0.08, y: 0 }), M({ x: 0.08, y: 0 }), `stroke="${GUIDE}" stroke-width="3"`) +
		line(M({ x: -0.08, y: 1 }), M({ x: 0.08, y: 1 }), `stroke="${GUIDE}" stroke-width="3"`);
	return {
		prompt: svgDoc(w, h, guides + face),
		answer: svgDoc(w, h, face),
		guide: svgDoc(w, h, ticks, false)
	};
}

/** Anime face construction: the canvas shows the head and guide lines, the student places the eyes. */
export function animeEyes(rng: Rng, w = 800, h = 600): DrillTarget {
	const R = 0.4;
	const cheekW = R * between(rng, 0.7, 0.85);
	const eyeY = between(rng, 0.54, 0.64);
	const ew = between(rng, 0.17, 0.23);
	const eh = ew * between(rng, 1.0, 1.35);
	const gap = ew * between(rng, 0.8, 1.15);
	const slant = between(rng, -8, 4);
	const tilt = between(rng, -7, 7);
	const { map, k } = fitter(
		[
			{ x: -0.62, y: -0.04 },
			{ x: 0.62, y: 1.06 }
		],
		w,
		h,
		0.9
	);
	const c = map({ x: 0, y: 0.5 });
	const M = (p: Pt) => rotate(map(p), tilt, c);
	const P = (p: Pt) => `${n(M(p).x)} ${n(M(p).y)}`;
	const head =
		// cranium arc from the left cheek round the top to the right cheek, then jaw to a small chin
		`<path d="M${P({ x: -R * 0.97, y: 0.52 })} A${n(R * k)} ${n(R * k)} 0 1 1 ${P({ x: R * 0.97, y: 0.52 })} ` +
		`Q${P({ x: R * 0.95, y: 0.7 })} ${P({ x: cheekW, y: 0.76 })} L${P({ x: 0.03, y: 0.98 })} Q${P({ x: 0, y: 1 })} ${P({ x: -0.03, y: 0.98 })} ` +
		`L${P({ x: -cheekW, y: 0.76 })} Q${P({ x: -R * 0.95, y: 0.7 })} ${P({ x: -R * 0.97, y: 0.52 })}" ${stroke(0.01 * k)}/>`;
	const ears = [-1, 1]
		.map(
			(s) =>
				`<path d="M${P({ x: s * R * 0.97, y: eyeY - 0.02 })} q${n(s * 0.06 * k)} ${n(0.02 * k)} ${n(s * 0.02 * k)} ${n(0.13 * k)}" ${stroke(0.008 * k)}/>`
		)
		.join('');
	const extras =
		line(M({ x: 0.012, y: 0.79 }), M({ x: -0.006, y: 0.8 }), stroke(0.008 * k)) +
		`<path d="M${P({ x: -0.035, y: 0.88 })} Q${P({ x: 0, y: 0.895 })} ${P({ x: 0.035, y: 0.88 })}" ${stroke(0.008 * k)}/>`;
	const g = (attrs: string) =>
		line(M({ x: 0, y: -0.02 }), M({ x: 0, y: 1.02 }), attrs) +
		line(M({ x: -0.5, y: eyeY }), M({ x: 0.5, y: eyeY }), attrs);
	const guideLines = g(`stroke="${GUIDE}" stroke-width="2" stroke-dasharray="7 6"`);
	const oneEye = (s: number) => {
		const cx = s * (gap / 2 + ew / 2);
		const E = (dx: number, dy: number) => {
			const r = rotate({ x: dx * s, y: dy }, s * slant);
			return P({ x: cx + r.x, y: eyeY + r.y });
		};
		const lash = `<path d="M${E(-ew * 0.55, -eh * 0.18)} Q${E(-ew * 0.1, -eh * 0.68)} ${E(ew * 0.55, -eh * 0.38)} L${E(ew * 0.52, -eh * 0.28)} Q${E(-ew * 0.1, -eh * 0.55)} ${E(-ew * 0.5, -eh * 0.1)} Z" fill="${INK}" stroke="${INK}" stroke-width="${n(0.006 * k)}" stroke-linejoin="round"/>`;
		const ic = M({ x: cx + s * ew * 0.03, y: eyeY + eh * 0.02 });
		const iris = ellipse(ic, ew * 0.3 * k, eh * 0.4 * k, tilt, `fill="${INK}"`);
		const shine = `<circle cx="${n(ic.x - ew * 0.1 * k)}" cy="${n(ic.y - eh * 0.16 * k)}" r="${n(ew * 0.09 * k)}" fill="#fff"/>`;
		const lower = `<path d="M${E(-ew * 0.25, eh * 0.42)} L${E(ew * 0.2, eh * 0.44)}" ${stroke(0.008 * k)}/>`;
		return lash + iris + shine + lower;
	};
	const eyes = oneEye(-1) + oneEye(1);
	return {
		prompt: svgDoc(w, h, guideLines + head + ears + extras + eyes),
		answer: svgDoc(w, h, eyes),
		guide: svgDoc(
			w,
			h,
			`<g stroke-opacity="0.9">${head.replaceAll(INK, GUIDE)}${ears.replaceAll(INK, GUIDE)}</g>` +
				guideLines,
			false
		)
	};
}

// ================================================================ hands

/** A hand simplified to a palm box, a finger "mitten" with tube divisions, and a two-part thumb. */
export function handMitten(rng: Rng, w = 800, h = 600): DrillTarget {
	const pw = 0.4;
	const ph = between(rng, 0.38, 0.46);
	const fh = ph * between(rng, 0.85, 1.05);
	const bend = between(rng, -18, 12);
	const fan = between(rng, 0, 0.08);
	const thumbDeg = between(rng, 195, 240);
	const kn = 1 - ph; // knuckle line y (wrist at y = 1)
	const palm: Pt[] = [
		{ x: -pw * 0.42, y: 1 },
		{ x: -pw / 2, y: kn },
		{ x: pw / 2, y: kn },
		{ x: pw * 0.42, y: 1 }
	];
	const km: Pt = { x: 0, y: kn };
	const F = (p: Pt) => rotate(add(km, p), bend, km);
	const lt = F({ x: -pw / 2 - fan, y: -fh * 0.8 });
	const rt = F({ x: pw / 2 + fan, y: -fh * 0.72 });
	const peak = F({ x: -pw * 0.08, y: -fh * 1.04 });
	const mitten = [F({ x: -pw / 2, y: 0 }), lt, peak, rt, F({ x: pw / 2, y: 0 })];
	// finger tubes: base split points to tips at different heights (middle finger longest)
	const tips = [-fh * 0.86, -fh * 0.98, -fh * 0.92, -fh * 0.76];
	const splits = [-pw / 4, 0, pw / 4].map((x, i) => [
		F({ x, y: -0.01 }),
		F({ x: x * (1 + fan * 3), y: Math.max(tips[i], tips[i + 1]) * 0.88 })
	]);
	// Thumb: a fleshy wedge from the lower side of the palm to the joint, then a tube to the tip.
	const tRoot: Pt = { x: -pw * 0.45, y: kn + ph * 0.62 };
	const tJoint = add(tRoot, polar(ph * 0.55, thumbDeg));
	const tTip = add(tJoint, polar(ph * 0.4, thumbDeg + between(rng, 10, 35)));
	const pLow: Pt = { x: -pw * 0.44, y: kn + ph * 0.92 };
	const pHigh: Pt = { x: -pw * 0.5, y: kn + ph * 0.22 };
	const side = (c: Pt, from: Pt, to: Pt, half: number, near: Pt): [Pt, Pt] => {
		const len = Math.hypot(to.x - from.x, to.y - from.y) || 1;
		const nv = { x: (-(to.y - from.y) / len) * half, y: ((to.x - from.x) / len) * half };
		const a = add(c, nv);
		const b = add(c, { x: -nv.x, y: -nv.y });
		const d = (p: Pt) => Math.hypot(p.x - near.x, p.y - near.y);
		return d(a) < d(b) ? [a, b] : [b, a];
	};
	const [jLow, jHigh] = side(tJoint, tRoot, tJoint, 0.05, pLow);
	const [tLow, tHigh] = side(tTip, tJoint, tTip, 0.038, jLow);
	const tEnd = add(tTip, polar(0.07, thumbDeg + 20));
	const wrist = [
		[
			{ x: -pw * 0.38, y: 1 },
			{ x: -pw * 0.36, y: 1.14 }
		],
		[
			{ x: pw * 0.38, y: 1 },
			{ x: pw * 0.36, y: 1.14 }
		]
	];
	const all = [...palm, ...mitten, jLow, jHigh, tLow, tHigh, tEnd, ...wrist.flat()];
	const flip = rng() < 0.5 ? -1 : 1;
	const turn = between(rng, -30, 30);
	const model = all.map((p) => rotate({ x: p.x * flip, y: p.y }, turn));
	const { map, k } = fitter(model, w, h, 0.84);
	const M = (p: Pt) => map(rotate({ x: p.x * flip, y: p.y }, turn));
	const P = (p: Pt) => `${n(M(p).x)} ${n(M(p).y)}`;
	const [a, l, pk, r, b] = mitten;
	const mittenPath = `<path d="M${P(a)} L${P(l)} C${P(add(l, { x: 0, y: -fh * 0.25 }))} ${P(add(pk, { x: -pw * 0.3, y: 0 }))} ${P(pk)} C${P(add(pk, { x: pw * 0.3, y: 0 }))} ${P(add(r, { x: 0, y: -fh * 0.2 }))} ${P(r)} L${P(b)}" ${stroke(0.011 * k)}/>`;
	const palmPath = `<path d="${polyPath(palm.map(M))}" ${stroke(0.011 * k)}/>`;
	const tubes = splits.map(([s, e]) => line(M(s), M(e), stroke(0.006 * k))).join('');
	const thumb =
		`<path d="M${P(pLow)} Q${P(lerp(pLow, jLow, 0.5))} ${P(jLow)} L${P(tLow)} Q${P(tEnd)} ${P(tHigh)} L${P(jHigh)} Q${P(add(lerp(jHigh, pHigh, 0.5), { x: 0.02, y: 0 }))} ${P(pHigh)}" ${stroke(0.011 * k)}/>` +
		line(M(jLow), M(jHigh), stroke(0.006 * k));
	const wr = wrist.map(([s, e]) => line(M(s), M(e), stroke(0.009 * k))).join('');
	const pic = svgDoc(w, h, palmPath + mittenPath + tubes + thumb + wr);
	return { prompt: pic, answer: pic };
}

// ================================================================ animals

const ANIMALS = {
	dog: {
		body: 0.9,
		chest: 0.24,
		hip: 0.2,
		leg: 0.5,
		neck: 0.25,
		neckDeg: -55,
		head: 0.15,
		muzzle: 0.14,
		ears: 'tri',
		tail: 0.3
	},
	cat: {
		body: 0.85,
		chest: 0.2,
		hip: 0.2,
		leg: 0.42,
		neck: 0.15,
		neckDeg: -50,
		head: 0.14,
		muzzle: 0.05,
		ears: 'tri',
		tail: 0.5
	},
	horse: {
		body: 1.05,
		chest: 0.28,
		hip: 0.27,
		leg: 0.75,
		neck: 0.42,
		neckDeg: -60,
		head: 0.12,
		muzzle: 0.26,
		ears: 'tri',
		tail: 0.4
	},
	bear: {
		body: 1.0,
		chest: 0.32,
		hip: 0.3,
		leg: 0.4,
		neck: 0.12,
		neckDeg: -20,
		head: 0.17,
		muzzle: 0.1,
		ears: 'round',
		tail: 0.06
	}
} as const;

/** An animal side view built from a body bean, chest and hip circles, a head circle and jointed legs. */
export function animalBeans(rng: Rng, w = 800, h = 600): DrillTarget {
	const kind = pick(rng, ['dog', 'cat', 'horse', 'bear'] as const);
	const base = ANIMALS[kind];
	const j = () => between(rng, 0.88, 1.12);
	const L = base.body * j();
	const rc = base.chest * j();
	const rh = base.hip * j();
	const leg = base.leg * j();
	const chest: Pt = { x: L / 2, y: 0 };
	const hip: Pt = { x: -L / 2, y: -0.02 };
	const neckEnd = add(
		chest,
		polar(rc * 0.6 + base.neck * j(), base.neckDeg + between(rng, -15, 15))
	);
	const hr = base.head * j();
	const headC = add(neckEnd, { x: hr * 0.4, y: -hr * 0.2 });
	const muzzleDeg = between(rng, 10, 45);
	const muzzleC = add(headC, polar(hr * 0.8 + base.muzzle * 0.5, muzzleDeg));
	const phase = between(rng, -1, 1);
	const floor = rc + leg;
	// legs: shoulder → elbow → wrist → paw (front), hip → knee → hock → paw (hind)
	const frontLeg = (sw: number) => {
		const s = add(chest, { x: 0, y: rc * 0.4 });
		const e = add(s, { x: -0.02 + sw * 0.08, y: leg * 0.4 });
		const wr = add(e, { x: sw * 0.07, y: leg * 0.42 });
		return [s, e, wr, { x: wr.x + 0.05, y: floor }];
	};
	const hindLeg = (sw: number) => {
		const s = add(hip, { x: 0.02, y: rh * 0.3 });
		const kn = add(s, { x: 0.1 + sw * 0.06, y: leg * 0.38 });
		const hock = add(kn, { x: -0.12 + sw * 0.06, y: leg * 0.34 });
		return [s, kn, hock, { x: hock.x + 0.04, y: floor }];
	};
	const shift = (lg: Pt[], dx: number) => lg.map((p) => ({ x: p.x + dx, y: p.y }));
	// far legs first so the near ones draw over them
	const legs = [
		shift(frontLeg(-phase * 0.7), -0.05),
		frontLeg(phase),
		shift(hindLeg(phase * 0.6), -0.05),
		hindLeg(-phase)
	];
	const tailBase = add(hip, { x: -rh * 0.9, y: -rh * 0.5 });
	const tailDeg = between(rng, 150, 230);
	const tailEnd = add(tailBase, polar(base.tail * j(), tailDeg));
	// Body bean: back dips a little, belly tucks up.
	const bean: Pt[] = [
		add(hip, { x: -rh, y: 0 }),
		add(hip, { x: -rh * 0.4, y: -rh * 0.95 }),
		{ x: 0, y: -Math.min(rc, rh) * 0.8 },
		add(chest, { x: rc * 0.2, y: -rc * 0.98 }),
		add(chest, { x: rc, y: 0 }),
		add(chest, { x: rc * 0.1, y: rc * 0.98 }),
		{ x: 0, y: Math.min(rc, rh) * 0.65 },
		add(hip, { x: -rh * 0.1, y: rh * 0.9 })
	];
	const earPts = (s: number) =>
		base.ears === 'tri'
			? [
					add(headC, polar(hr * 0.9, -120 + s * 25)),
					add(headC, polar(hr * 1.7, -100 + s * 15)),
					add(headC, polar(hr * 0.95, -80 + s * 25))
				]
			: [];
	const flip = rng() < 0.5 ? -1 : 1;
	const allPts = [
		...bean,
		...legs.flat(),
		neckEnd,
		tailEnd,
		add(headC, { x: 0, y: -hr * 1.8 }),
		add(muzzleC, { x: base.muzzle, y: 0 })
	];
	const { map, k } = fitter(
		allPts.map((p) => ({ x: p.x * flip, y: p.y })),
		w,
		h,
		0.84
	);
	const M = (p: Pt) => map({ x: p.x * flip, y: p.y });
	const P = (p: Pt) => `${n(M(p).x)} ${n(M(p).y)}`;
	const thin = stroke(0.008 * k);
	const thick = stroke(0.012 * k);
	const circle = (c: Pt, r: number, attrs: string) =>
		`<circle cx="${n(M(c).x)}" cy="${n(M(c).y)}" r="${n(r * k)}" ${attrs}/>`;
	let s = `<path d="${smoothPath(bean.map(M))}" ${thick}/>`;
	s += circle(chest, rc, thin) + circle(hip, rh, thin) + circle(headC, hr, thick);
	s += ellipse(
		M(muzzleC),
		(base.muzzle * 0.5 + hr * 0.25) * k,
		hr * 0.42 * k,
		flip * muzzleDeg,
		thick
	);
	const neckSide = (d: number) =>
		line(
			M(add(chest, polar(rc * 0.8, base.neckDeg + d))),
			M(add(headC, polar(hr * 0.8, base.neckDeg + 180 - d * 0.6))),
			thick
		);
	s += neckSide(-55) + neckSide(55);
	// Legs as simple tubes through the joints (the far pair thinner), with joint circles and paws.
	legs.forEach((lg, i) => {
		const far = i % 2 === 0;
		s += `<path d="${openPath(lg.map(M))}" ${stroke((far ? 0.04 : 0.056) * k, far ? '#5b5852' : INK)}/>`;
		s += `<path d="${openPath(lg.map(M))}" ${stroke((far ? 0.028 : 0.04) * k, '#fff')}/>`;
		s += circle(lg[1], 0.032, thin) + circle(lg[2], 0.026, thin);
		s += ellipse(M(add(lg[3], { x: 0.03, y: -0.015 })), 0.05 * k, 0.022 * k, 0, thin);
	});
	for (const sd of [-1, 1]) {
		const e = earPts(sd);
		if (e.length) s += `<path d="${polyPath(e.map(M))}" ${thick}/>`;
		else s += circle(add(headC, polar(hr * 0.95, -90 + sd * 40)), hr * 0.32, thick);
	}
	s += `<path d="M${P(tailBase)} Q${P(add(lerp(tailBase, tailEnd, 0.5), { x: 0, y: -0.08 }))} ${P(tailEnd)}" ${thick}/>`;
	const pic = svgDoc(w, h, s);
	return { prompt: pic, answer: pic };
}

// ================================================================ perspective

type Seg = [Pt, Pt];
const segs = (list: Seg[], attrs: string) => list.map(([a, b]) => line(a, b, attrs)).join('');
const horizonMarks = (w: number, y: number, vps: Pt[]) =>
	line({ x: 0, y }, { x: w, y }, `stroke="${GUIDE}" stroke-width="2"`) +
	label({ x: 12, y: y - 10 }, 'horizon', 'start', 20) +
	vps.map((v) => dot(v, 9) + label({ x: v.x, y: v.y + 32 }, 'VP', 'middle', 20, DOT)).join('');
const inside = (pts: Pt[], w: number, h: number, m: number) =>
	pts.every((p) => p.x > m && p.x < w - m && p.y > m && p.y < h - m);

/** One-point perspective cube: horizon, vanishing point, and only the edges you'd see. */
export function cubePerspective(rng: Rng, w = 800, h = 600): DrillTarget {
	let vp: Pt = { x: 0, y: 0 };
	let F = { x0: 0, y0: 0, x1: 0, y1: 0 };
	let back = F;
	for (let tries = 0; tries < 40; tries++) {
		vp = { x: between(rng, 0.2, 0.8) * w, y: between(rng, 0.25, 0.7) * h };
		const s = between(rng, 0.2, 0.3) * h;
		const dx = pick(rng, [-1, 1]) * between(rng, 0.05, 0.32) * w;
		const above = rng() < 0.5;
		const gapY = between(rng, 0.05, 0.2) * h;
		const x0 = vp.x + dx - s / 2;
		const y0 = above ? vp.y - gapY - s : vp.y + gapY;
		F = { x0, y0, x1: x0 + s, y1: y0 + s };
		const kd = between(rng, 0.6, 0.74);
		const to = (x: number, y: number) => ({ x: vp.x + (x - vp.x) * kd, y: vp.y + (y - vp.y) * kd });
		const b0 = to(F.x0, F.y0);
		const b1 = to(F.x1, F.y1);
		back = { x0: b0.x, y0: b0.y, x1: b1.x, y1: b1.y };
		if (
			inside(
				[
					{ x: F.x0, y: F.y0 },
					{ x: F.x1, y: F.y1 }
				],
				w,
				h,
				30
			)
		)
			break;
	}
	const fp = (x: number, y: number) => ({ x, y });
	const corners: Record<'tl' | 'tr' | 'bl' | 'br', Seg> = {
		tl: [fp(F.x0, F.y0), fp(back.x0, back.y0)],
		tr: [fp(F.x1, F.y0), fp(back.x1, back.y0)],
		bl: [fp(F.x0, F.y1), fp(back.x0, back.y1)],
		br: [fp(F.x1, F.y1), fp(back.x1, back.y1)]
	};
	const faces = {
		top: vp.y < F.y0,
		bottom: vp.y > F.y1,
		left: vp.x < F.x0,
		right: vp.x > F.x1
	};
	const visible: Seg[] = [
		[corners.tl[0], corners.tr[0]],
		[corners.tr[0], corners.br[0]],
		[corners.br[0], corners.bl[0]],
		[corners.bl[0], corners.tl[0]]
	];
	const hidden: Seg[] = [];
	const edge = (seg: Seg, show: boolean) => (show ? visible : hidden).push(seg);
	edge([corners.tl[1], corners.tr[1]], faces.top);
	edge([corners.bl[1], corners.br[1]], faces.bottom);
	edge([corners.tl[1], corners.bl[1]], faces.left);
	edge([corners.tr[1], corners.br[1]], faces.right);
	edge(corners.tl, faces.top || faces.left);
	edge(corners.tr, faces.top || faces.right);
	edge(corners.bl, faces.bottom || faces.left);
	edge(corners.br, faces.bottom || faces.right);
	const rays = Object.values(corners)
		.map(([f]) => line(f, vp, `stroke="${PALE}" stroke-width="2" stroke-dasharray="4 8"`))
		.join('');
	const marks = horizonMarks(w, vp.y, [vp]);
	return {
		prompt: svgDoc(
			w,
			h,
			marks +
				rays +
				segs(hidden, `stroke="#a9a49a" stroke-width="2" stroke-dasharray="6 6"`) +
				segs(visible, STROKE)
		),
		answer: svgDoc(w, h, segs(visible, STROKE)),
		guide: svgDoc(w, h, marks, false)
	};
}

/** Where the lines p→q and r→s cross (infinite lines). */
function intersect(p: Pt, q: Pt, r: Pt, s: Pt): Pt {
	const d = (q.x - p.x) * (s.y - r.y) - (q.y - p.y) * (s.x - r.x);
	const t = ((r.x - p.x) * (s.y - r.y) - (r.y - p.y) * (s.x - r.x)) / (d || 1e-9);
	return lerp(p, q, t);
}

/** Two-point perspective box: horizon with a vanishing point near each side. */
export function twoPointBox(rng: Rng, w = 800, h = 600): DrillTarget {
	let vl: Pt = { x: 0, y: 0 };
	let vr: Pt = { x: 0, y: 0 };
	let visible: Seg[] = [];
	let hidden: Seg[] = [];
	for (let tries = 0; tries < 40; tries++) {
		const hy = between(rng, 0.3, 0.7) * h;
		vl = { x: between(rng, 0.03, 0.14) * w, y: hy };
		vr = { x: between(rng, 0.86, 0.97) * w, y: hy };
		const x0 = vl.x + (vr.x - vl.x) * between(rng, 0.32, 0.68);
		const bh = between(rng, 0.18, 0.36) * h;
		const where = pick(rng, ['above', 'below', 'below', 'across'] as const);
		const top =
			where === 'above'
				? hy - between(rng, 0.06, 0.16) * h - bh
				: where === 'below'
					? hy + between(rng, 0.06, 0.16) * h
					: hy - bh * between(rng, 0.25, 0.75);
		const ft: Pt = { x: x0, y: top };
		const fb: Pt = { x: x0, y: top + bh };
		const side = (vp: Pt, frac: number) => {
			const x = x0 + (vp.x - x0) * frac;
			return [
				intersect(ft, vp, { x, y: 0 }, { x, y: 1 }),
				intersect(fb, vp, { x, y: 0 }, { x, y: 1 })
			];
		};
		const [lt, lb] = side(vl, between(rng, 0.22, 0.45));
		const [rt, rb] = side(vr, between(rng, 0.22, 0.45));
		const bt = intersect(lt, vr, rt, vl);
		const bb = intersect(lb, vr, rb, vl);
		const topVis = hy < ft.y;
		const botVis = hy > fb.y;
		visible = [
			[ft, fb],
			[lt, lb],
			[rt, rb],
			[ft, lt],
			[ft, rt],
			[fb, lb],
			[fb, rb]
		];
		hidden = [[bt, bb]];
		(topVis ? visible : hidden).push([lt, bt], [rt, bt]);
		(botVis ? visible : hidden).push([lb, bb], [rb, bb]);
		if (inside([ft, fb, lt, lb, rt, rb, bt, bb], w, h, 24)) break;
	}
	const guideRays = visible
		.slice(3, 7)
		.map(([a, b]) =>
			line(b, a.x > b.x ? vl : vr, `stroke="${PALE}" stroke-width="2" stroke-dasharray="4 8"`)
		)
		.join('');
	const marks = horizonMarks(w, vl.y, [vl, vr]);
	return {
		prompt: svgDoc(
			w,
			h,
			marks +
				guideRays +
				segs(hidden, `stroke="#a9a49a" stroke-width="2" stroke-dasharray="6 6"`) +
				segs(visible, STROKE)
		),
		answer: svgDoc(w, h, segs(visible, STROKE)),
		guide: svgDoc(w, h, marks, false)
	};
}

/**
 * A square on the ground (or on a wall) seen through a simple pinhole camera, with the
 * circle inscribed in it projected the same way: a true perspective ellipse.
 */
export function ellipseInBox(rng: Rng, w = 800, h = 600): DrillTarget {
	const wall = rng() < 0.3;
	const Z0 = between(rng, 3, 5);
	const X0 = between(rng, -1.2, 1.2);
	const th =
		(wall ? pick(rng, [-1, 1]) * between(rng, 28, 58) : between(rng, 0, 90)) * (Math.PI / 180);
	const camH = between(rng, 1.4, 3.2);
	// (u, v) in [-1, 1]² on the plane → 3D → screen (y down).
	const to3 = (u: number, v: number) =>
		wall
			? { X: X0 + u * Math.cos(th), Y: v, Z: Z0 + u * Math.sin(th) }
			: {
					X: X0 + u * Math.cos(th) - v * Math.sin(th),
					Y: camH,
					Z: Z0 + u * Math.sin(th) + v * Math.cos(th)
				};
	const proj = (u: number, v: number): Pt => {
		const p = to3(u, v);
		return { x: p.X / p.Z, y: p.Y / p.Z };
	};
	const quad = [proj(-1, -1), proj(1, -1), proj(1, 1), proj(-1, 1)];
	const { map } = fitter(quad, w, h, 0.78);
	const Q = quad.map(map);
	const circle: Pt[] = [];
	for (let i = 0; i < 48; i++) {
		const t = (i / 48) * Math.PI * 2;
		circle.push(map(proj(Math.cos(t), Math.sin(t))));
	}
	const cross =
		line(
			map(proj(-1, 0)),
			map(proj(1, 0)),
			`stroke="${GUIDE}" stroke-width="2" stroke-dasharray="6 6"`
		) +
		line(
			map(proj(0, -1)),
			map(proj(0, 1)),
			`stroke="${GUIDE}" stroke-width="2" stroke-dasharray="6 6"`
		) +
		line(Q[0], Q[2], `stroke="${PALE}" stroke-width="2"`) +
		line(Q[1], Q[3], `stroke="${PALE}" stroke-width="2"`);
	const box = `<path d="${polyPath(Q)}" ${stroke(3, '#6b6760')}/>`;
	const shape = `<path d="${smoothPath(circle)}" ${STROKE}/>`;
	const guideBox = `<path d="${polyPath(Q)}" ${stroke(2.5, GUIDE)}/>`;
	return {
		prompt: svgDoc(w, h, cross + box + shape),
		answer: svgDoc(w, h, shape),
		guide: svgDoc(w, h, cross + guideBox, false)
	};
}

// ================================================================ light and shadow

/** Sphere on the ground in five flat values: light, halftone, core shadow, reflected light, cast shadow. */
export function shadedSphere(rng: Rng, w = 800, h = 600): DrillTarget {
	const r = between(rng, 0.2, 0.27) * h;
	const groundY = h * between(rng, 0.7, 0.8);
	const c: Pt = { x: w / 2 + between(rng, -0.08, 0.08) * w, y: groundY - r };
	const fromLeft = rng() < 0.5;
	// direction from the sphere towards the light (screen, y down)
	const lightDeg = fromLeft ? between(rng, 200, 245) : between(rng, 295, 340);
	const kt = between(rng, 0.3, 0.45); // terminator bulge
	const kc = Math.min(0.8, kt + between(rng, 0.2, 0.28)); // core shadow width
	const kh = kt - between(rng, 0.5, 0.65); // halftone start
	// Region from the curve through x = a to the curve through x = b (both ellipse arcs, ry = r).
	const band = (a: number, b: number) => {
		const arc = (x: number, down: boolean) =>
			`A${n(Math.abs(x) * r || 0.01)} ${n(r)} 0 0 ${x >= 0 === down ? 1 : 0}`;
		return `M0 ${n(-r)} ${arc(a, true)} 0 ${n(r)} ${arc(b, false)} 0 ${n(-r)} Z`;
	};
	const rot = lightDeg - 180; // local -x points at the light
	const g = (body: string) =>
		`<g transform="translate(${n(c.x)} ${n(c.y)}) rotate(${n(rot)})">${body}</g>`;
	const v = { light: 228, half: 178, reflect: 120, core: 62, cast: 40 };
	const sphere = g(
		`<circle r="${n(r)}" fill="${grey(v.light)}"/>` +
			`<path d="${band(kh, 1)}" fill="${grey(v.half)}"/>` +
			`<path d="${band(kt, 1)}" fill="${grey(v.reflect)}"/>` +
			`<path d="${band(kt, kc)}" fill="${grey(v.core)}"/>` +
			`<circle cx="${n(-r * 0.52)}" cy="0" r="${n(r * 0.11)}" fill="#fff"/>`
	);
	const away = -Math.cos((lightDeg * Math.PI) / 180);
	const lift = Math.abs(Math.sin((lightDeg * Math.PI) / 180));
	const castLen = r * (1.1 + (1 - lift) * 1.2);
	const cast = ellipse(
		{ x: c.x + away * castLen * 0.55, y: groundY - r * 0.04 },
		castLen,
		r * 0.26,
		0,
		`fill="${grey(v.cast)}"`
	);
	const ground = line(
		{ x: w * 0.06, y: groundY - r * 0.3 },
		{ x: w * 0.94, y: groundY - r * 0.3 },
		`stroke="${PALE}" stroke-width="2"`
	);
	const sunAt = add(c, polar(r * 1.75, lightDeg));
	const sunMark = sun({ x: Math.min(w - 40, Math.max(40, sunAt.x)), y: Math.max(40, sunAt.y) });
	return {
		prompt: svgDoc(w, h, ground + cast + sphere + sunMark),
		answer: svgDoc(w, h, cast + sphere),
		guide: svgDoc(w, h, sunMark, false)
	};
}

/** A block lit from one side: three face values plus the (darker) shadow it casts on the ground. */
export function castShadowBlock(rng: Rng, w = 800, h = 600): DrillTarget {
	const bw = between(rng, 0.8, 1.6);
	const bd = between(rng, 0.8, 1.4);
	const bh = between(rng, 0.7, 1.7);
	const phi = between(rng, 20, 70) * (Math.PI / 180);
	// light azimuth, relative to the direction towards the viewer (sin φ, cos φ) in the ground plane
	const toViewer = Math.atan2(Math.cos(phi), Math.sin(phi));
	const az = toViewer + pick(rng, [-1, 1]) * between(rng, 45, 110) * (Math.PI / 180);
	const el = between(rng, 35, 55) * (Math.PI / 180);
	// direction towards the light
	const L = { x: Math.cos(el) * Math.cos(az), y: Math.sin(el), z: Math.cos(el) * Math.sin(az) };
	const proj = (X: number, Y: number, Z: number): Pt => ({
		x: X * Math.cos(phi) - Z * Math.sin(phi),
		y: -Y + (X * Math.sin(phi) + Z * Math.cos(phi)) * 0.45
	});
	const P3 = (x: number, y: number, z: number) => ({ x, y, z });
	const base = [
		P3(-bw / 2, 0, -bd / 2),
		P3(bw / 2, 0, -bd / 2),
		P3(bw / 2, 0, bd / 2),
		P3(-bw / 2, 0, bd / 2)
	];
	const topC = base.map((p) => ({ ...p, y: bh }));
	const shadowPts = [
		...base.map((p) => proj(p.x, 0, p.z)),
		...topC.map((p) => proj(p.x - (L.x * p.y) / L.y, 0, p.z - (L.z * p.y) / L.y))
	];
	const all = [...shadowPts, ...topC.map((p) => proj(p.x, p.y, p.z))];
	const { map } = fitter(all, w, h, 0.8);
	const S = (p: { x: number; y: number; z: number }) => map(proj(p.x, p.y, p.z));
	// viewer looks along (sin φ, ·, cos φ) in the ground plane; a side face shows if its normal points that way
	const view = { x: Math.sin(phi), z: Math.cos(phi) };
	const shade = (nx: number, ny: number, nz: number) => {
		const d = nx * L.x + ny * L.y + nz * L.z;
		return d <= 0.05 ? 92 : d < 0.45 ? 160 : d < 0.75 ? 200 : 236;
	};
	const sides: [number, number, number[]][] = [
		[1, 0, [1, 2]],
		[-1, 0, [3, 0]],
		[0, 1, [2, 3]],
		[0, -1, [0, 1]]
	];
	let faces = '';
	for (const [nx, nz, [i, j]] of sides) {
		if (nx * view.x + nz * view.z <= 0) continue;
		faces += `<path d="${polyPath([S(base[i]), S(base[j]), S(topC[j]), S(topC[i])])}" fill="${grey(shade(nx, 0, nz))}" stroke="${grey(shade(nx, 0, nz))}" stroke-width="1" stroke-linejoin="round"/>`;
	}
	faces += `<path d="${polyPath(topC.map(S))}" fill="${grey(shade(0, 1, 0))}" stroke="${grey(shade(0, 1, 0))}" stroke-width="1" stroke-linejoin="round"/>`;
	const hull = convexHull(shadowPts.map(map));
	const shadow = `<path d="${polyPath(hull)}" fill="${grey(48)}"/>`;
	const centre = map(proj(0, bh / 2, 0));
	const ldir = proj(L.x, L.y, L.z);
	const len = Math.hypot(ldir.x, ldir.y) || 1;
	const sunAt = {
		x: centre.x + (ldir.x / len) * h * 0.46,
		y: centre.y + (ldir.y / len) * h * 0.46
	};
	const sunMark = sun({
		x: Math.min(w - 40, Math.max(40, sunAt.x)),
		y: Math.min(h - 40, Math.max(40, sunAt.y))
	});
	return {
		prompt: svgDoc(w, h, shadow + faces + sunMark),
		answer: svgDoc(w, h, shadow + faces),
		guide: svgDoc(w, h, sunMark, false)
	};
}

// ================================================================ value techniques

/** Patches of hatching, each denser than the last: single, closer, crossed, double-crossed. */
export function hatchingPatches(rng: Rng, w = 800, h = 600): DrillTarget {
	const count = pick(rng, [4, 5]);
	const gap = 18;
	const pw = (w * 0.88 - gap * (count - 1)) / count;
	const ph = Math.min(h * between(rng, 0.4, 0.6), pw * 1.8);
	const x0 = (w - (pw * count + gap * (count - 1))) / 2;
	const y0 = (h - ph) / 2;
	const a = pick(rng, [1, -1]) * between(rng, 25, 65);
	const crossA = a + pick(rng, [1, -1]) * between(rng, 70, 100);
	const levels: [number, number][][] = [
		[[a, 26]],
		[[a, 13]],
		[
			[a, 13],
			[crossA, 13]
		],
		[
			[a, 9],
			[crossA, 9]
		],
		[
			[a, 7],
			[crossA, 7],
			[(a + crossA) / 2 + 90, 9]
		]
	];
	const use = count === 5 ? [0, 1, 2, 3, 4] : [0, 1, 3, 4];
	let ink = '';
	let boxes = '';
	let guide = '';
	use.forEach((lvl, i) => {
		const r = { x0: x0 + i * (pw + gap), y0, x1: x0 + i * (pw + gap) + pw, y1: y0 + ph };
		const c = { x: (r.x0 + r.x1) / 2, y: (r.y0 + r.y1) / 2 };
		const reach = Math.hypot(pw, ph) / 2 + 4;
		for (const [deg, spacing] of levels[lvl]) {
			const d = polar(1, deg);
			const nrm = { x: -d.y, y: d.x };
			for (let o = -reach; o <= reach; o += spacing) {
				const j = (rng() - 0.5) * spacing * 0.3;
				const p = add(c, { x: nrm.x * (o + j), y: nrm.y * (o + j) });
				const seg = clipSegment(
					add(p, { x: -d.x * reach, y: -d.y * reach }),
					add(p, { x: d.x * reach, y: d.y * reach }),
					{
						x0: r.x0 + 3,
						y0: r.y0 + 3,
						x1: r.x1 - 3,
						y1: r.y1 - 3
					}
				);
				if (seg) ink += line(seg[0], seg[1], `stroke="${INK}" stroke-width="2.4"`);
			}
		}
		boxes += rectSvg(r.x0, r.y0, pw, ph, `fill="none" stroke="${INK}" stroke-width="1.5"`);
		guide += rectSvg(r.x0, r.y0, pw, ph, DASHED);
	});
	return {
		prompt: svgDoc(w, h, ink + boxes),
		answer: svgDoc(w, h, ink),
		guide: svgDoc(w, h, guide, false)
	};
}

/** One smooth gradient from dark to paper white, in a random direction and box shape. */
export function softBlend(rng: Rng, w = 800, h = 600): DrillTarget {
	const dir = pick(rng, ['right', 'left', 'down', 'up'] as const);
	const horiz = dir === 'right' || dir === 'left';
	const bw = horiz ? w * between(rng, 0.7, 0.86) : w * between(rng, 0.3, 0.45);
	const bh = horiz ? h * between(rng, 0.25, 0.4) : h * between(rng, 0.72, 0.84);
	const x = (w - bw) / 2;
	const y = (h - bh) / 2;
	const id = uid(rng, 'blend');
	const vec = { right: [0, 0, 1, 0], left: [1, 0, 0, 0], down: [0, 0, 0, 1], up: [0, 1, 0, 0] }[
		dir
	];
	const gamma = between(rng, 0.8, 1.25);
	const dark = between(rng, 25, 50);
	const stops = [0, 0.25, 0.5, 0.75, 1]
		.map((t) => `<stop offset="${t}" stop-color="${grey(dark + (250 - dark) * t ** gamma)}"/>`)
		.join('');
	const grad = `<defs><linearGradient id="${id}" x1="${vec[0]}" y1="${vec[1]}" x2="${vec[2]}" y2="${vec[3]}">${stops}</linearGradient></defs>`;
	const fill = rectSvg(x, y, bw, bh, `fill="url(#${id})"`);
	return {
		prompt: svgDoc(
			w,
			h,
			grad + fill + rectSvg(x, y, bw, bh, `fill="none" stroke="${INK}" stroke-width="2"`)
		),
		answer: svgDoc(w, h, grad + fill),
		guide: svgDoc(w, h, rectSvg(x, y, bw, bh, DASHED), false)
	};
}

/** A 3-value thumbnail composed on the rule of thirds: big shapes placed on purpose. */
export function thirdsThumbnail(rng: Rng, w = 800, h = 600): DrillTarget {
	const aspect = pick(rng, [4 / 3, 16 / 9, 1, 3 / 4]);
	const fw = Math.min(w * 0.86, h * 0.86 * aspect);
	const fh = fw / aspect;
	const fx = (w - fw) / 2;
	const fy = (h - fh) / 2;
	const X = (t: number) => fx + fw * t;
	const Y = (t: number) => fy + fh * t;
	const third = (rng() < 0.5 ? 1 : 2) / 3;
	const focusX = (rng() < 0.5 ? 1 : 2) / 3 + between(rng, -0.03, 0.03);
	const scene = pick(rng, ['tree', 'island', 'figure'] as const);
	const MID = grey(150);
	const DARK = grey(38);
	let body = '';
	if (scene === 'tree') {
		const hy = Y(third);
		const pts = [] as Pt[];
		for (let i = 0; i <= 20; i++)
			pts.push({ x: X(i / 20), y: hy + Math.sin(i / 3 + rng()) * fh * 0.015 });
		body += `<path d="${openPath([...pts, { x: X(1), y: Y(1) }, { x: X(0), y: Y(1) }])} Z" fill="${MID}"/>`;
		const th = fh * between(rng, 0.35, 0.5);
		const base = { x: X(focusX), y: Math.min(Y(0.95), hy + fh * 0.18) };
		body += rectSvg(
			base.x - fw * 0.012,
			base.y - th * 0.55,
			fw * 0.024,
			th * 0.55,
			`fill="${DARK}"`
		);
		body += `<path d="${smoothPath(blobPoints(rng, { x: base.x, y: base.y - th * 0.7 }, fw * 0.09, th * 0.3))}" fill="${DARK}"/>`;
	} else if (scene === 'island') {
		const hy = Y(third);
		body += rectSvg(X(0), hy, fw, Y(1) - hy, `fill="${MID}"`);
		const iw = fw * between(rng, 0.14, 0.24);
		const ih = fh * between(rng, 0.1, 0.2);
		const cx = X(focusX);
		body += `<path d="M${n(cx - iw)} ${n(hy + 2)} Q${n(cx - iw * 0.4)} ${n(hy - ih * 1.4)} ${n(cx + iw * 0.1)} ${n(hy - ih)} Q${n(cx + iw * 0.7)} ${n(hy - ih * 0.6)} ${n(cx + iw)} ${n(hy + 2)} Z" fill="${DARK}"/>`;
		// a flat-bottomed cloud of three overlapping ovals, on the other side of the frame
		const cc = { x: X(1 - focusX), y: Y(third * between(rng, 0.35, 0.55)) };
		const cw = fw * between(rng, 0.08, 0.12);
		for (const [dx, dy, r] of [
			[-0.9, 0.15, 0.55],
			[0, -0.15, 0.75],
			[0.9, 0.15, 0.5]
		])
			body += ellipse(
				{ x: cc.x + dx * cw, y: cc.y + dy * cw },
				r * cw * 1.3,
				r * cw * 0.8,
				0,
				`fill="${MID}"`
			);
	} else {
		const top = Y(between(rng, 0.55, 0.7));
		const peakX = X(focusX);
		body += `<path d="M${n(X(0))} ${n(Y(0.95))} Q${n(peakX)} ${n(top - fh * 0.25)} ${n(X(1))} ${n(Y(0.85))} L${n(X(1))} ${n(Y(1))} L${n(X(0))} ${n(Y(1))} Z" fill="${MID}"/>`;
		const fhgt = fh * 0.16;
		// stand the figure on the hill: find the curve's height at peakX
		const q = (t: number, a: number, b: number, c: number) =>
			(1 - t) ** 2 * a + 2 * (1 - t) * t * b + t * t * c;
		let tBest = 0;
		for (let s = 0; s <= 200; s++)
			if (
				Math.abs(q(s / 200, X(0), peakX, X(1)) - peakX) <
				Math.abs(q(tBest, X(0), peakX, X(1)) - peakX)
			)
				tBest = s / 200;
		const fyb = q(tBest, Y(0.95), top - fh * 0.25, Y(0.85)) + 2;
		body += ellipse({ x: peakX, y: fyb - fhgt }, fhgt * 0.15, fhgt * 0.15, 0, `fill="${DARK}"`);
		body += `<path d="M${n(peakX - fhgt * 0.2)} ${n(fyb + 4)} L${n(peakX - fhgt * 0.12)} ${n(fyb - fhgt * 0.82)} L${n(peakX + fhgt * 0.12)} ${n(fyb - fhgt * 0.82)} L${n(peakX + fhgt * 0.2)} ${n(fyb + 4)} Z" fill="${DARK}"/>`;
	}
	const id = uid(rng, 'thirds');
	const clipped = `<defs><clipPath id="${id}">${rectSvg(fx, fy, fw, fh, '')}</clipPath></defs><g clip-path="url(#${id})">${body}</g>`;
	const thirds = (attrs: string) =>
		[1 / 3, 2 / 3]
			.map(
				(t) =>
					line({ x: X(t), y: fy }, { x: X(t), y: fy + fh }, attrs) +
					line({ x: fx, y: Y(t) }, { x: fx + fw, y: Y(t) }, attrs)
			)
			.join('');
	return {
		prompt: svgDoc(
			w,
			h,
			clipped +
				thirds(`stroke="${GUIDE}" stroke-width="1.5" stroke-dasharray="5 7"`) +
				rectSvg(fx, fy, fw, fh, `fill="none" stroke="${INK}" stroke-width="2"`)
		),
		answer: svgDoc(w, h, clipped),
		guide: svgDoc(
			w,
			h,
			rectSvg(fx, fy, fw, fh, DASHED) +
				thirds(`stroke="${GUIDE}" stroke-width="1.5" stroke-dasharray="5 7"`),
			false
		)
	};
}

/** Four hill layers: near = dark, high contrast and bumpy; far = light, soft and flat. */
export function atmosphericLayers(rng: Rng, w = 800, h = 600): DrillTarget {
	const fw = w * 0.88;
	const fh = h * 0.84;
	const fx = (w - fw) / 2;
	const fy = (h - fh) / 2;
	const values = [212, 168, 116, 46].map((v) => v + between(rng, -8, 8));
	let body = '';
	for (let i = 0; i < 4; i++) {
		const baseY = fy + fh * (0.3 + i * 0.16 + between(rng, -0.03, 0.03));
		const amp = fh * (0.035 + i * 0.03);
		const waves = [1, 2, 4].map((f) => ({
			f: f * between(rng, 0.8, 1.3),
			a: between(rng, 0.4, 1) / f,
			p: rng() * 6.28
		}));
		const pts: Pt[] = [];
		for (let s = 0; s <= 40; s++) {
			const t = s / 40;
			const y =
				baseY -
				amp * waves.reduce((acc, wv) => acc + wv.a * Math.sin(t * wv.f * Math.PI * 2 + wv.p), 0);
			pts.push({ x: fx + fw * t, y: Math.max(fy + 6, y) });
		}
		body += `<path d="${openPath([...pts, { x: fx + fw, y: fy + fh }, { x: fx, y: fy + fh }])} Z" fill="${grey(values[i])}"/>`;
	}
	const frame = (attrs: string) => rectSvg(fx, fy, fw, fh, attrs);
	return {
		prompt: svgDoc(w, h, body + frame(`fill="none" stroke="${INK}" stroke-width="2"`)),
		answer: svgDoc(w, h, body),
		guide: svgDoc(w, h, frame(DASHED), false)
	};
}

// ================================================================ line weight

/** One straight stroke that tapers from thick (big dot) to thin (small dot). */
export function taperedLine(rng: Rng, w = 800, h = 600): DrillTarget {
	const m = 80;
	let a: Pt, b: Pt;
	do {
		a = { x: between(rng, m, w - m), y: between(rng, m, h - m) };
		b = { x: between(rng, m, w - m), y: between(rng, m, h - m) };
	} while (Math.hypot(b.x - a.x, b.y - a.y) < Math.min(w, h) * 0.6);
	const w0 = between(rng, 16, 26);
	const p = between(rng, 0.8, 1.6);
	const len = Math.hypot(b.x - a.x, b.y - a.y);
	const d = { x: (b.x - a.x) / len, y: (b.y - a.y) / len };
	const nrm = { x: -d.y, y: d.x };
	const left: Pt[] = [];
	const right: Pt[] = [];
	for (let i = 0; i <= 24; i++) {
		const t = i / 24;
		const half = (w0 * (1 - t) ** p + 1.5) / 2;
		const c = lerp(a, b, t);
		left.push({ x: c.x + nrm.x * half, y: c.y + nrm.y * half });
		right.push({ x: c.x - nrm.x * half, y: c.y - nrm.y * half });
	}
	const shape =
		`<path d="${polyPath([...left, ...right.reverse()])}" fill="${INK}"/>` + dot(a, w0 / 2, INK);
	const marks =
		dot(a, 11) +
		`<circle cx="${n(b.x)}" cy="${n(b.y)}" r="5" fill="none" stroke="${DOT}" stroke-width="2.5"/>`;
	return {
		prompt: svgDoc(w, h, shape + marks),
		answer: svgDoc(w, h, shape),
		guide: svgDoc(w, h, marks, false),
		taper: { a, b }
	};
}

// ================================================================ exaggeration

type Part = { w: number; h: number };
const PUSHES = [
	{ id: 'taller', text: 'Push it: much taller and thinner', top: [0.7, 1.7], bottom: [0.7, 1.7] },
	{ id: 'wider', text: 'Squash it: short and wide', top: [1.6, 0.6], bottom: [1.6, 0.6] },
	{ id: 'top', text: 'Push it: huge top, tiny bottom', top: [1.7, 1.6], bottom: [0.55, 0.6] },
	{ id: 'bottom', text: 'Push it: tiny top, huge bottom', top: [0.5, 0.55], bottom: [1.7, 1.6] }
] as const;

/** A simple two-part object, its outline as a path in model units (base at y = 0, up is negative). */
function twoPartShape(kind: 'mushroom' | 'bottle' | 'critter' | 'house', top: Part, bot: Part) {
	const r = (x: number, y: number, w: number, h: number, rr: number) =>
		`<rect x="${u(x)}" y="${u(y)}" width="${u(w)}" height="${u(h)}" rx="${u(rr)}"/>`;
	switch (kind) {
		case 'mushroom':
			return (
				r(-bot.w / 2, -bot.h, bot.w, bot.h, bot.w * 0.25) +
				`<path d="M${u(-top.w / 2)} ${u(-bot.h)} Q${u(-top.w / 2)} ${u(-bot.h - top.h * 1.3)} 0 ${u(-bot.h - top.h)} Q${u(top.w / 2)} ${u(-bot.h - top.h * 1.3)} ${u(top.w / 2)} ${u(-bot.h)} Z"/>`
			);
		case 'bottle':
			return (
				r(-bot.w / 2, -bot.h, bot.w, bot.h, bot.w * 0.2) +
				r(-top.w / 2, -bot.h - top.h, top.w, top.h + 0.02, top.w * 0.2)
			);
		case 'critter':
			return (
				ellipseU({ x: 0, y: -bot.h / 2 }, bot.w / 2, bot.h / 2, '') +
				ellipseU({ x: 0, y: -bot.h - top.h / 2 + bot.h * 0.08 }, top.w / 2, top.h / 2, '')
			);
		case 'house':
			return (
				r(-bot.w / 2, -bot.h, bot.w, bot.h, 0) +
				`<path d="M${u(-top.w / 2)} ${u(-bot.h)} L0 ${u(-bot.h - top.h)} L${u(top.w / 2)} ${u(-bot.h)} Z"/>`
			);
	}
}

/** A simple object plus an instruction for how to push its proportions (graded loosely). */
export function exaggerateShapes(rng: Rng, w = 800, h = 600): DrillTarget {
	const kind = pick(rng, ['mushroom', 'bottle', 'critter', 'house'] as const);
	const push = pick(rng, PUSHES);
	const j = () => between(rng, 0.85, 1.15);
	const sizes: Record<typeof kind, [Part, Part]> = {
		mushroom: [
			{ w: 1.0 * j(), h: 0.45 * j() },
			{ w: 0.32 * j(), h: 0.55 * j() }
		],
		bottle: [
			{ w: 0.22 * j(), h: 0.4 * j() },
			{ w: 0.55 * j(), h: 0.75 * j() }
		],
		critter: [
			{ w: 0.5 * j(), h: 0.45 * j() },
			{ w: 0.62 * j(), h: 0.6 * j() }
		],
		house: [
			{ w: 0.95 * j(), h: 0.45 * j() },
			{ w: 0.75 * j(), h: 0.6 * j() }
		]
	};
	const [top, bot] = sizes[kind];
	const scale = (p: Part, f: readonly [number, number]): Part => ({ w: p.w * f[0], h: p.h * f[1] });
	const pTop = scale(top, push.top);
	const pBot = scale(bot, push.bottom);
	const box = (t: Part, b: Part) => ({
		w: Math.max(t.w, b.w),
		h: t.h * (kind === 'mushroom' ? 1.15 : 1) + b.h
	});
	const place = (t: Part, b: Part, cx: number, base: number, k: number, attrs: string) =>
		`<g transform="translate(${n(cx)} ${n(base)}) scale(${n(k)})" ${attrs}>${twoPartShape(kind, t, b)}</g>`;
	const outline = (k: number) =>
		`fill="none" stroke="${INK}" stroke-width="${u(4 / k)}" stroke-linejoin="round"`;
	// Prompt: original on the left, an arrow, the instruction on top.
	const ob = box(top, bot);
	const ko = Math.min((w * 0.36) / ob.w, (h * 0.62) / ob.h);
	const prompt =
		label({ x: w / 2, y: 52 }, push.text, 'middle', 30, INK) +
		place(top, bot, w * 0.27, h * 0.86, ko, `fill="${PALE}"`) +
		place(top, bot, w * 0.27, h * 0.86, ko, outline(ko)) +
		`<path d="M${n(w * 0.53)} ${n(h * 0.55)} L${n(w * 0.68)} ${n(h * 0.55)} M${n(w * 0.65)} ${n(h * 0.5)} L${n(w * 0.69)} ${n(h * 0.55)} L${n(w * 0.65)} ${n(h * 0.6)}" ${stroke(6, DOT)}/>` +
		label({ x: w * 0.83, y: h * 0.56 }, 'your', 'middle', 26, GUIDE) +
		label({ x: w * 0.83, y: h * 0.62 }, 'version', 'middle', 26, GUIDE);
	const pb = box(pTop, pBot);
	const kp = Math.min((w * 0.8) / pb.w, (h * 0.8) / pb.h);
	const answer = place(pTop, pBot, w / 2, h / 2 + (pb.h * kp) / 2, kp, outline(kp));
	return { prompt: svgDoc(w, h, prompt), answer: svgDoc(w, h, answer) };
}
