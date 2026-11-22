/**
 * Stroke smoothing, the "pulled string" way (Photoshop's Smoothing, Procreate's
 * StreamLine): the brush tip hangs on a string behind the pointer and only moves when
 * the string is pulled tight, so wobbles shorter than the string never reach the
 * paper. Pressure is smoothed the same way. On pen-up the tip catches up to where the
 * pen lifted, so the stroke still ends there. A string of length 0 is no smoothing.
 */

export interface TipPoint {
	x: number;
	y: number;
	/** pressure, 0–1 */
	p: number;
}

/** The painting tools smoothing applies to (each keeps its own amount). */
export type SmoothTool = 'brush' | 'eraser' | 'smudge';

/** Which tool's smoothing a stroke uses (null: the tool doesn't paint strokes). */
export function smoothTool(tool: string, eraser: boolean): SmoothTool | null {
	if (tool === 'smudge') return 'smudge';
	if (tool === 'brush') return eraser ? 'eraser' : 'brush';
	return null;
}

/** The string's length in screen px for an amount (0–1): gentle at first, long at the top. */
export const stringLength = (amount: number) =>
	80 * Math.pow(Math.min(1, Math.max(0, amount)), 1.5);

export class Stabilizer {
	/** where the brush is */
	tip: TipPoint | null = null;
	/** where the pointer last was */
	last: TipPoint | null = null;

	/** `length`: the string, in canvas px (0: the tip is the pointer) */
	constructor(public length = 0) {}

	/** start a stroke (or carry on from a point, as after a Shift+click line) */
	reset(at: TipPoint | null = null) {
		this.tip = at && { ...at };
		this.last = at && { ...at };
	}

	/** The pointer moved: where the tip goes, or null when the string is still slack. */
	push(x: number, y: number, p: number): TipPoint | null {
		this.last = { x, y, p };
		const tip = this.tip;
		if (!tip || this.length <= 0) return (this.tip = { x, y, p });
		const [dx, dy] = [x - tip.x, y - tip.y];
		const d = Math.hypot(dx, dy);
		if (d <= this.length) return null;
		// pulled tight: the tip moves straight towards the pointer, the string's length behind it
		const moved = d - this.length;
		const k = moved / d;
		// pressure follows as far as the tip went, measured against the string
		const f = moved / (moved + this.length);
		return (this.tip = { x: tip.x + dx * k, y: tip.y + dy * k, p: tip.p + (p - tip.p) * f });
	}

	/** Pen-up: the tip's way to where the pointer stopped (`step` canvas px apart; empty when there). */
	catchUp(step = 4): TipPoint[] {
		const { tip, last } = this;
		if (!tip || !last) return [];
		const d = Math.hypot(last.x - tip.x, last.y - tip.y);
		if (d < 0.01 && Math.abs(last.p - tip.p) < 0.001) return [];
		const n = Math.min(32, Math.max(1, Math.ceil(d / Math.max(0.5, step))));
		const out: TipPoint[] = [];
		for (let k = 1; k <= n; k++) {
			const t = k / n;
			out.push({
				x: tip.x + (last.x - tip.x) * t,
				y: tip.y + (last.y - tip.y) * t,
				p: tip.p + (last.p - tip.p) * t
			});
		}
		this.tip = { ...last };
		return out;
	}
}
