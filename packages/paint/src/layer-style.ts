/**
 * Layer styles: an outline (stroke), drop shadow and outer glow drawn around a
 * layer's paint by the engine (crates/brush/src/style.rs), editable any time.
 * Each effect keeps its settings while it's off.
 */

export type StrokePosition = 'outside' | 'inside' | 'centre';

export interface LayerStyle {
	stroke: {
		on: boolean;
		/** px, 1–100 */
		size: number;
		/** #rrggbb */
		colour: string;
		position: StrokePosition;
		/** 0–1 */
		opacity: number;
	};
	shadow: {
		on: boolean;
		colour: string;
		opacity: number;
		/** where the light comes from, degrees anticlockwise from the right (120 = upper left) */
		angle: number;
		/** px, 0–200 */
		distance: number;
		/** softness, px, 0–100 */
		blur: number;
	};
	glow: {
		on: boolean;
		colour: string;
		/** px, 1–100 */
		size: number;
		opacity: number;
	};
}

/** What a layer starts with (all off); the engine's default too. */
export const NO_STYLE: LayerStyle = {
	stroke: { on: false, size: 4, colour: '#000000', position: 'outside', opacity: 1 },
	shadow: { on: false, colour: '#000000', opacity: 0.5, angle: 120, distance: 10, blur: 8 },
	glow: { on: false, colour: '#ffed87', size: 12, opacity: 0.75 }
};

const POSITIONS: StrokePosition[] = ['outside', 'inside', 'centre'];

/** Any effect on. */
export const hasStyle = (s: LayerStyle | null | undefined) =>
	!!s && (s.stroke.on || s.shadow.on || s.glow.on);

const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);

/** The style as the numbers the engine reads (`setLayer`); none: empty. */
export function styleNumbers(s: LayerStyle | null | undefined): Float32Array {
	if (!s || sameStyle(s, null)) return new Float32Array();
	const { stroke: k, shadow: d, glow: g } = s;
	return new Float32Array([
		+k.on,
		k.size,
		...rgb(k.colour),
		POSITIONS.indexOf(k.position),
		k.opacity,
		+d.on,
		...rgb(d.colour),
		d.opacity,
		d.angle,
		d.distance,
		d.blur,
		+g.on,
		...rgb(g.colour),
		g.size,
		g.opacity
	]);
}

const num = (v: unknown, lo: number, hi: number, dflt: number) =>
	typeof v === 'number' && Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : dflt;
const colour = (v: unknown, dflt: string) =>
	typeof v === 'string' && /^#[0-9a-f]{6}$/i.test(v) ? v.toLowerCase() : dflt;
const part = (v: unknown) => (v && typeof v === 'object' ? (v as Record<string, unknown>) : {});

/**
 * A style from anywhere (a file, someone else's app): every field checked and kept in
 * range, missing ones as `NO_STYLE`. Null when it isn't an object or has nothing on and
 * nothing changed (the same as no style).
 */
export function readStyle(v: unknown): LayerStyle | null {
	if (!v || typeof v !== 'object') return null;
	const o = v as Record<string, unknown>;
	const [k, d, g] = [part(o.stroke), part(o.shadow), part(o.glow)];
	const n = NO_STYLE;
	const s: LayerStyle = {
		stroke: {
			on: k.on === true,
			size: num(k.size, 1, 100, n.stroke.size),
			colour: colour(k.colour, n.stroke.colour),
			position: POSITIONS.includes(k.position as StrokePosition)
				? (k.position as StrokePosition)
				: n.stroke.position,
			opacity: num(k.opacity, 0, 1, n.stroke.opacity)
		},
		shadow: {
			on: d.on === true,
			colour: colour(d.colour, n.shadow.colour),
			opacity: num(d.opacity, 0, 1, n.shadow.opacity),
			angle: ((num(d.angle, -1e6, 1e6, n.shadow.angle) % 360) + 360) % 360 || 0,
			distance: num(d.distance, 0, 200, n.shadow.distance),
			blur: num(d.blur, 0, 100, n.shadow.blur)
		},
		glow: {
			on: g.on === true,
			colour: colour(g.colour, n.glow.colour),
			size: num(g.size, 1, 100, n.glow.size),
			opacity: num(g.opacity, 0, 1, n.glow.opacity)
		}
	};
	return sameStyle(s, null) ? null : s;
}

/** The same look (null = `NO_STYLE`). */
export function sameStyle(a: LayerStyle | null | undefined, b: LayerStyle | null | undefined) {
	const x = a ?? NO_STYLE;
	const y = b ?? NO_STYLE;
	const close = (p: number, q: number) => Math.abs(p - q) < 1e-3;
	return (
		x.stroke.on === y.stroke.on &&
		close(x.stroke.size, y.stroke.size) &&
		x.stroke.colour === y.stroke.colour &&
		x.stroke.position === y.stroke.position &&
		close(x.stroke.opacity, y.stroke.opacity) &&
		x.shadow.on === y.shadow.on &&
		x.shadow.colour === y.shadow.colour &&
		close(x.shadow.opacity, y.shadow.opacity) &&
		close(x.shadow.angle, y.shadow.angle) &&
		close(x.shadow.distance, y.shadow.distance) &&
		close(x.shadow.blur, y.shadow.blur) &&
		x.glow.on === y.glow.on &&
		x.glow.colour === y.glow.colour &&
		close(x.glow.size, y.glow.size) &&
		close(x.glow.opacity, y.glow.opacity)
	);
}
