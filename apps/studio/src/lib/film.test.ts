import { describe, expect, it } from 'vitest';
import { ease, liveSchedule, liveSteps, runs, strokesOfFrame } from './film';
import type { Stroke } from '@pencilpilot/paint/strokes';

const stroke = (layer: number, ts: number[], drawing?: number): Stroke => ({
	brushId: 'pen',
	eraser: false,
	layer,
	drawing,
	points: ts.map((t, i) => ({ x: i, y: 0, pressure: 0.5, t }))
});

describe('films', () => {
	it('times strokes as drawn, with pauses kept short', () => {
		const s = [stroke(1, [0, 100, 200]), stroke(1, [5000, 5100])];
		const { events, duration } = liveSchedule(s);
		// 0.1 s in, 0.2 s of stroke, a 4.8 s pause cut to 0.5, 0.1 s of stroke
		expect(duration).toBeCloseTo(0.9);
		expect(events.filter((e) => e.stroke === 0).map((e) => e.points)).toEqual([1, 2, 3, 4]);
		expect(events.find((e) => e.stroke === 1)!.t).toBeCloseTo(0.8);
		// twice as fast: half as long
		expect(liveSchedule(s, 2).duration).toBeCloseTo(0.45);
		// a pen held still mid-stroke counts for at most a quarter second
		expect(liveSchedule([stroke(1, [0, 3000])]).duration).toBeCloseTo(0.35);
		// a later session (times start again) gets a short pause
		expect(liveSchedule([stroke(1, [9000, 9100]), stroke(1, [0, 100])]).duration).toBeCloseTo(
			0.1 + 0.1 + 0.25 + 0.1
		);
	});

	it('picks the strokes of the drawings showing', () => {
		const all = [stroke(1, [0], 10), stroke(1, [1], 11), stroke(2, [2]), stroke(3, [3], 30)];
		const one = (l: number) => (l === 2 ? 1 : 5);
		expect(
			strokesOfFrame(
				all,
				[
					[1, 11],
					[2, 20],
					[3, 0]
				],
				one
			)
		).toEqual([all[1], all[2]]);
		// a layer with many drawings: strokes that don't say which are left out
		expect(strokesOfFrame(all, [[2, 20]], () => 3)).toEqual([]);
	});

	it('eases in and out', () => {
		expect(ease(0)).toBe(0);
		expect(ease(0.5)).toBe(0.5);
		expect(ease(1)).toBe(1);
		expect(ease(0.1)).toBeLessThan(0.1);
		expect(ease(2)).toBe(1);
	});

	it('holds a drawn-live picture until a stroke moves', () => {
		// 10 fps: events at 0.05, 0.15, 0.16 and 0.55 s over 8 frames
		const steps = liveSteps([0.05, 0.15, 0.16, 0.55], 8, 10);
		expect(steps).toEqual([
			{ upTo: 1, repeat: 1 },
			{ upTo: 3, repeat: 4 },
			{ upTo: 4, repeat: 3 }
		]);
		// every frame is covered, once
		expect(steps.reduce((n, s) => n + s.repeat, 0)).toBe(8);
		// nothing drawn yet still makes a first picture
		expect(liveSteps([0.5], 3, 10)).toEqual([{ upTo: 0, repeat: 3 }]);
	});

	it('joins repeats', () => {
		expect(runs([1, 1, 2, 3, 3, 3])).toEqual([
			[1, 2],
			[2, 1],
			[3, 3]
		]);
	});
});
