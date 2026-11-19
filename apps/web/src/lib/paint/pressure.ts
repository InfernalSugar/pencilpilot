/**
 * Pen feel: a personal pressure curve, applied to every pen stroke before the
 * brush sees it. One "firmness" number: below 1 a light touch already draws
 * strong lines, above 1 you have to press harder.
 */

export const PRESSURE_PRESETS = [
	{ id: 'light', label: 'Light touch', firmness: 0.55 },
	{ id: 'normal', label: 'Normal', firmness: 1 },
	{ id: 'heavy', label: 'Heavy hand', firmness: 1.8 }
] as const;

const KEY = 'pp.pressureFirmness';
let firmness: number | null = null;
const listeners = new Set<(f: number) => void>();

export function pressureFirmness(): number {
	if (firmness !== null) return firmness;
	try {
		const v = Number(JSON.parse(localStorage.getItem(KEY) ?? '1'));
		firmness = Number.isFinite(v) ? clampF(v) : 1;
	} catch {
		firmness = 1;
	}
	return firmness;
}

export function setPressureFirmness(f: number) {
	firmness = clampF(f);
	try {
		localStorage.setItem(KEY, JSON.stringify(firmness));
	} catch {
		/* storage blocked */
	}
	for (const l of listeners) l(firmness);
}

export function onPressureFirmness(f: (v: number) => void) {
	listeners.add(f);
	return () => listeners.delete(f);
}

/** The pressure the brush gets for a raw pen pressure (0–1). */
export const applyCurve = (raw: number, f = pressureFirmness()) =>
	Math.max(0, Math.min(1, raw)) ** f;

const clampF = (v: number) => Math.max(0.3, Math.min(3, v));
