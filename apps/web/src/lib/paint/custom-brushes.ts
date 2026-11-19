/**
 * "My brushes": a brush the student adjusted, saved in this browser. Each is a
 * built-in MyPaint brush plus a few tweaks applied to its .myb settings before
 * the engine loads it.
 */

export interface BrushTweaks {
	/** edge hardness, 0 (soft) – 1 (hard) */
	hardness?: number;
	/** dabs per brush radius: lower = gaps between dabs, higher = smooth line */
	spacing?: number;
	/** random dab offset (grain / scatter), 0 – 1.5 */
	jitter?: number;
	/** how much harder pressing makes the line thicker, 0 – 1 */
	pressureSize?: number;
	/** how much harder pressing makes the line darker, 0 – 1 */
	pressureOpacity?: number;
}

export interface CustomBrush {
	id: string;
	name: string;
	/** the built-in brush it starts from */
	base: string;
	tweaks: BrushTweaks;
}

export const isCustom = (id: string) => id.startsWith('my-');

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

/** Save a new brush, or update one with the same id. Returns it. */
export function saveCustomBrush(b: Omit<CustomBrush, 'id'> & { id?: string }): CustomBrush {
	const brush: CustomBrush = {
		id: b.id && isCustom(b.id) ? b.id : `my-${Date.now().toString(36)}`,
		name: b.name.trim().slice(0, 40) || 'My brush',
		base: b.base,
		tweaks: b.tweaks
	};
	const list = customBrushes().filter((x) => x.id !== brush.id);
	store([...list, brush]);
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

type Settings = Record<string, { base_value: number; inputs: Record<string, [number, number][]> }>;

/** The values a brush starts with, for the editor's sliders. */
export function tweaksOf(myb: string): Required<BrushTweaks> {
	const s = (JSON.parse(myb).settings ?? {}) as Settings;
	const base = (k: string, d: number) => s[k]?.base_value ?? d;
	const curve = (k: string) => s[k]?.inputs?.pressure ?? [];
	const span = (pts: [number, number][]) =>
		pts.length ? Math.max(...pts.map((p) => p[1])) - Math.min(...pts.map((p) => p[1])) : 0;
	return {
		hardness: base('hardness', 0.8),
		spacing: base('dabs_per_actual_radius', 2),
		jitter: base('offset_by_random', 0),
		pressureSize: Math.min(1, span(curve('radius_logarithmic')) / 1.6),
		pressureOpacity: Math.min(1, span(curve('opaque_multiply')))
	};
}

/** The .myb source with the tweaks applied. */
export function applyTweaks(myb: string, t: BrushTweaks): string {
	const doc = JSON.parse(myb);
	const s = (doc.settings ??= {}) as Settings;
	const setting = (k: string) => (s[k] ??= { base_value: 0, inputs: {} });
	if (t.hardness !== undefined) setting('hardness').base_value = clamp(t.hardness, 0, 1);
	if (t.spacing !== undefined)
		setting('dabs_per_actual_radius').base_value = clamp(t.spacing, 0.5, 10);
	if (t.jitter !== undefined) setting('offset_by_random').base_value = clamp(t.jitter, 0, 1.5);
	if (t.pressureSize !== undefined) {
		const p = clamp(t.pressureSize, 0, 1);
		const r = setting('radius_logarithmic');
		r.inputs = { ...r.inputs };
		if (p > 0)
			r.inputs.pressure = [
				[0, -1.2 * p],
				[1, 0.4 * p]
			];
		else delete r.inputs.pressure;
	}
	if (t.pressureOpacity !== undefined) {
		// opacity comes from the pressure mapping on top of a base: light pressure is
		// (1 - p) as dark as full pressure
		const p = clamp(t.pressureOpacity, 0, 1);
		const o = setting('opaque_multiply');
		o.base_value = 0;
		o.inputs = {
			...o.inputs,
			pressure: [
				[0, 1 - p],
				[1, 1]
			]
		};
	}
	return JSON.stringify(doc);
}

const clamp = (v: number, a: number, b: number) => Math.max(a, Math.min(b, v));
