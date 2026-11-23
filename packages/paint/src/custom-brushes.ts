/**
 * "My brushes": brushes the student made, adjusted or imported, saved in this
 * browser. Each is a MyPaint brush (a built-in one, or an imported brush's own
 * settings) plus tweaks applied to its .myb settings before the engine loads it.
 * Its grain and picture tip live with the brush textures (see brush-textures).
 */

export interface BrushTweaks {
	// ---- shape ----
	/** the brush's radius in px at 100% size (`radius_logarithmic`, stored as px) */
	size?: number;
	/** each dab a random size, 0 – 1 (`radius_by_random`) */
	sizeRandom?: number;
	/** edge hardness, 0 (soft) – 1 (hard) */
	hardness?: number;
	/** 1 = round, up to 10 = a flat, thin nib (`elliptical_dab_ratio`) */
	roundness?: number;
	/** the nib's angle in degrees, 0 – 180 (`elliptical_dab_angle`) */
	angle?: number;
	/** the nib turns as the stroke changes direction (a `direction` input on the angle) */
	followDirection?: boolean;

	// ---- stroke ----
	/** dabs per brush radius: lower = gaps between dabs, higher = smooth line */
	spacing?: number;
	/** the line trails behind the pen, smoothing it, 0 – 10 (`slow_tracking`) */
	smoothing?: number;
	/** random dab offset (grain / scatter), 0 – 1.5 (`offset_by_random`) */
	jitter?: number;
	/** a wobbly, hand-drawn line, 0 – 2 (`tracking_noise`) */
	shake?: number;

	// ---- pressure, speed, tilt ----
	/** how much harder pressing makes the line thicker, 0 – 1 */
	pressureSize?: number;
	/** how much harder pressing makes the line darker, 0 – 1 */
	pressureOpacity?: number;
	/** faster strokes thinner (−1) or thicker (+1) (a `speed1` input on the size) */
	speedSize?: number;
	/** leaning the pen over makes the line wider, 0 – 1 (a `tilt_declination` input) */
	tiltSize?: number;

	// ---- colour ----
	/** how solid each dab is at full pressure, 0 – 1 (`opaque`) */
	opacity?: number;
	/** blends with the paint already there, 0 – 1 (`smudge`) */
	smudge?: number;
	/** how long picked-up paint lasts, 0 (soon replaced) – 1 (carried far) (`smudge_length`) */
	smudgeLength?: number;
	/** each dab's hue varies by up to this, 0 – 0.5 (`change_color_h` from `random`) */
	hueJitter?: number;
	/** each dab lighter or darker by up to this, 0 – 0.5 (`change_color_v` from `random`) */
	shadeJitter?: number;
}

export type Tweaks = Required<BrushTweaks>;

export interface CustomBrush {
	id: string;
	name: string;
	/** the built-in brush it starts from (others drawing together see this one) */
	base: string;
	tweaks: BrushTweaks;
	/** an imported MyPaint brush's own settings (used instead of `base`'s) */
	source?: string;
}

export const isCustom = (id: string) => id.startsWith('my-');

/** what a brand-new brush starts from: a plain round brush */
export const NEW_BRUSH_BASE = 'experimental-basic';

const KEY = 'pp.myBrushes';
let cache: CustomBrush[] | null = null;
const listeners = new Set<() => void>();

export function customBrushes(): CustomBrush[] {
	if (cache) return cache;
	try {
		const v = JSON.parse(localStorage.getItem(KEY) ?? '[]');
		cache = Array.isArray(v) ? v.filter((b) => b && isCustom(String(b.id)) && b.base) : [];
	} catch {
		cache = [];
	}
	return cache!;
}

function store(list: CustomBrush[]) {
	cache = list;
	try {
		localStorage.setItem(KEY, JSON.stringify(list));
	} catch {
		/* storage blocked: kept for this visit */
	}
	for (const l of listeners) l();
}

export const customBrush = (id: string) => customBrushes().find((b) => b.id === id);

let made = 0;
/** a new brush id (several can be made in the same millisecond when importing a set) */
const newId = () => `my-${Date.now().toString(36)}${(made++ % 1296).toString(36)}`;

/** Save a new brush, or update one with the same id. Returns it. */
export function saveCustomBrush(b: Omit<CustomBrush, 'id'> & { id?: string }): CustomBrush {
	const ids = new Set(customBrushes().map((x) => x.id));
	let id = b.id && isCustom(b.id) ? b.id : newId();
	if (!b.id) while (ids.has(id)) id = newId();
	const brush: CustomBrush = {
		id,
		name: b.name.trim().slice(0, 40) || 'My brush',
		base: b.base,
		tweaks: b.tweaks,
		...(b.source ? { source: b.source } : {})
	};
	const list = customBrushes();
	const at = list.findIndex((x) => x.id === brush.id);
	// an edited brush keeps its place in the list
	store(at < 0 ? [...list, brush] : list.map((x, i) => (i === at ? brush : x)));
	return brush;
}

export function deleteCustomBrush(id: string) {
	store(customBrushes().filter((b) => b.id !== id));
}

/** Get told when "My brushes" change (returns an unsubscribe). */
export function onCustomBrushes(f: () => void) {
	listeners.add(f);
	return () => listeners.delete(f);
}

type Pts = [number, number][];
type Settings = Record<string, { base_value: number; inputs: Record<string, Pts> }>;

/** a piecewise-linear MyPaint curve at `x` (as the engine reads it) */
function at(pts: Pts | undefined, x: number): number {
	if (!pts?.length) return 0;
	if (pts.length === 1) return pts[0][1];
	let [x0, y0] = pts[0];
	let [x1, y1] = pts[1];
	for (let i = 2; i < pts.length && x > x1; i++) {
		[x0, y0] = [x1, y1];
		[x1, y1] = pts[i];
	}
	if (x0 === x1 || y0 === y1) return y0;
	return (y1 * (x - x0) + y0 * (x1 - x)) / (x1 - x0);
}

const span = (pts: Pts | undefined) =>
	pts?.length ? Math.max(...pts.map((p) => p[1])) - Math.min(...pts.map((p) => p[1])) : 0;

/** the size slider's limits, px */
export const SIZE_RANGE: [number, number] = [0.5, 120];

/**
 * What a brush does, as the creator's settings. These are the values the brush
 * really draws with in the app (where the opacity slider sets `opaque_multiply`,
 * so a pressure curve there does nothing: only one on `opaque` counts).
 */
export function tweaksOf(myb: string): Tweaks {
	const s = (JSON.parse(myb).settings ?? {}) as Settings;
	const base = (k: string, d = 0) => s[k]?.base_value ?? d;
	const input = (k: string, i: string) => s[k]?.inputs?.[i];
	const opaque = base('opaque', 1);
	const opCurve = input('opaque', 'pressure');
	const full = clamp(opaque + at(opCurve, 1), 0, 1);
	const light = clamp(opaque + at(opCurve, 0), 0, 1);
	const speed = input('radius_logarithmic', 'speed1');
	const tilt = input('radius_logarithmic', 'tilt_declination');
	return {
		size: clamp(Math.exp(base('radius_logarithmic', 2)), ...SIZE_RANGE),
		sizeRandom: clamp(base('radius_by_random'), 0, 1),
		hardness: clamp(base('hardness', 0.8), 0, 1),
		roundness: clamp(base('elliptical_dab_ratio', 1), 1, 10),
		angle: clamp(base('elliptical_dab_angle', 90), 0, 180),
		followDirection: span(input('elliptical_dab_angle', 'direction')) > 0,
		spacing: clamp(base('dabs_per_actual_radius', 2) + base('dabs_per_basic_radius'), 0.5, 10),
		smoothing: clamp(base('slow_tracking'), 0, 10),
		jitter: clamp(base('offset_by_random'), 0, 1.5),
		shake: clamp(base('tracking_noise'), 0, 2),
		pressureSize: Math.min(1, span(input('radius_logarithmic', 'pressure')) / 1.6),
		pressureOpacity: full > 0 ? clamp((full - light) / full, 0, 1) : 0,
		speedSize: speed?.length ? clamp((at(speed, 4) - at(speed, 0)) / 2, -1, 1) : 0,
		tiltSize: tilt?.length ? clamp((at(tilt, 0) - at(tilt, 90)) / 1.2, 0, 1) : 0,
		opacity: full,
		smudge: clamp(base('smudge') + at(input('smudge', 'pressure'), 1), 0, 1),
		smudgeLength: clamp(base('smudge_length', 0.5), 0, 1),
		hueJitter: clamp(span(input('change_color_h', 'random')) / 2, 0, 0.5),
		shadeJitter: clamp(span(input('change_color_v', 'random')) / 2, 0, 0.5)
	};
}

/** The .myb source with the tweaks applied (only the ones given). */
export function applyTweaks(myb: string, t: BrushTweaks): string {
	const doc = JSON.parse(myb);
	const s = (doc.settings ??= {}) as Settings;
	const setting = (k: string) => {
		const v = (s[k] ??= { base_value: 0, inputs: {} });
		v.inputs = { ...v.inputs };
		return v;
	};
	const put = (k: string, v: number) => (setting(k).base_value = v);
	/** set (or with `pts` null, remove) one input curve of a setting */
	const curve = (k: string, input: string, pts: Pts | null) => {
		const v = setting(k);
		if (pts) v.inputs[input] = pts;
		else delete v.inputs[input];
	};
	if (t.size !== undefined) put('radius_logarithmic', Math.log(clamp(t.size, ...SIZE_RANGE)));
	if (t.sizeRandom !== undefined) put('radius_by_random', clamp(t.sizeRandom, 0, 1));
	if (t.hardness !== undefined) put('hardness', clamp(t.hardness, 0, 1));
	if (t.roundness !== undefined) put('elliptical_dab_ratio', clamp(t.roundness, 1, 10));
	if (t.angle !== undefined) put('elliptical_dab_angle', clamp(t.angle, 0, 180));
	if (t.followDirection !== undefined)
		curve(
			'elliptical_dab_angle',
			'direction',
			t.followDirection
				? [
						[0, 0],
						[180, 180]
					]
				: null
		);
	if (t.spacing !== undefined) {
		put('dabs_per_actual_radius', clamp(t.spacing, 0.5, 10));
		put('dabs_per_basic_radius', 0);
	}
	if (t.smoothing !== undefined) put('slow_tracking', clamp(t.smoothing, 0, 10));
	if (t.jitter !== undefined) put('offset_by_random', clamp(t.jitter, 0, 1.5));
	if (t.shake !== undefined) put('tracking_noise', clamp(t.shake, 0, 2));
	if (t.pressureSize !== undefined) {
		const p = clamp(t.pressureSize, 0, 1);
		curve(
			'radius_logarithmic',
			'pressure',
			p > 0
				? [
						[0, -1.2 * p],
						[1, 0.4 * p]
					]
				: null
		);
	}
	if (t.speedSize !== undefined) {
		const v = clamp(t.speedSize, -1, 1);
		curve(
			'radius_logarithmic',
			'speed1',
			v
				? [
						[0, 0],
						[4, 2 * v]
					]
				: null
		);
	}
	if (t.tiltSize !== undefined) {
		const v = clamp(t.tiltSize, 0, 1);
		// declination: 90° is upright, 0° lying flat
		curve(
			'radius_logarithmic',
			'tilt_declination',
			v > 0
				? [
						[0, 1.2 * v],
						[90, 0]
					]
				: null
		);
	}
	if (t.opacity !== undefined || t.pressureOpacity !== undefined) {
		// on `opaque`: the app's opacity slider sets `opaque_multiply`, so a pressure curve
		// there would be lost; light pressure is (1 - p) as solid as full pressure
		const was = t.opacity === undefined || t.pressureOpacity === undefined ? tweaksOf(myb) : null;
		const o = clamp(t.opacity ?? was!.opacity, 0, 1);
		const p = clamp(t.pressureOpacity ?? was!.pressureOpacity, 0, 1);
		put('opaque', o * (1 - p));
		curve(
			'opaque',
			'pressure',
			p > 0
				? [
						[0, 0],
						[1, o * p]
					]
				: null
		);
		s.opaque_multiply = { base_value: 0, inputs: {} };
	}
	if (t.smudge !== undefined) {
		put('smudge', clamp(t.smudge, 0, 1));
		curve('smudge', 'pressure', null);
	}
	if (t.smudgeLength !== undefined) {
		put('smudge_length', clamp(t.smudgeLength, 0, 1));
		curve('smudge_length', 'stroke', null);
	}
	const jitter = (k: string, v: number) =>
		curve(
			k,
			'random',
			v > 0
				? [
						[0, -v],
						[1, v]
					]
				: null
		);
	if (t.hueJitter !== undefined) jitter('change_color_h', clamp(t.hueJitter, 0, 0.5));
	if (t.shadeJitter !== undefined) jitter('change_color_v', clamp(t.shadeJitter, 0, 0.5));
	return JSON.stringify(doc);
}

/** The settings in `t` that differ from `from` (what to save: the rest stays as the brush had it). */
export function changedTweaks(from: Tweaks, t: Tweaks): BrushTweaks {
	const out: Record<string, number | boolean> = {};
	for (const k of Object.keys(t) as (keyof Tweaks)[]) {
		const [a, b] = [from[k], t[k]];
		if (typeof b === 'boolean' ? a !== b : Math.abs((a as number) - b) > 1e-6) out[k] = b;
	}
	// opacity and its pressure curve are written together
	if ('opacity' in out || 'pressureOpacity' in out) {
		out.opacity = t.opacity;
		out.pressureOpacity = t.pressureOpacity;
	}
	return out as BrushTweaks;
}

const clamp = (v: number, a: number, b: number) => Math.max(a, Math.min(b, v));
