import { describe, expect, it } from 'vitest';
import { cloneBrushSource, cloneOffset } from './clone';
import { StrokeLog } from './strokes';

describe('cloneOffset', () => {
	it('needs a source first', () => {
		expect(cloneOffset(null, [5, 5], true, null)).toBeNull();
	});

	it('aligned: the first stroke sets the offset and later strokes keep it', () => {
		const first = cloneOffset([10, 20], [50, 20], true, null);
		expect(first).toEqual([-40, 0]);
		expect(cloneOffset([10, 20], [90, 70], true, first)).toEqual([-40, 0]);
	});

	it('not aligned: each stroke starts from the source point again', () => {
		const kept = cloneOffset([10, 20], [50, 20], false, null);
		expect(cloneOffset([10, 20], [90, 70], false, kept)).toEqual([-80, -50]);
	});
});

describe('clone strokes in the history', () => {
	it('are named Clone or Heal', () => {
		const log = new StrokeLog();
		const pts = [{ x: 0, y: 0, pressure: 0.5, t: 0 }];
		const clone = { dx: 1, dy: 2, heal: false, all: false };
		log.add({ brushId: 'a', eraser: false, points: pts });
		log.add({ brushId: 'a', eraser: false, points: pts, clone });
		log.add({ brushId: 'a', eraser: false, points: pts, clone: { ...clone, heal: true } });
		expect(log.history().undo).toEqual(['Brush stroke', 'Clone', 'Heal']);
	});
});

describe('cloneBrushSource', () => {
	it('copies at full strength, never smudging or erasing, and keeps the rest', () => {
		const myb = JSON.stringify({
			version: 3,
			settings: {
				opaque: {
					base_value: 0.3,
					inputs: {
						pressure: [
							[0, 0],
							[1, 1]
						]
					}
				},
				smudge: { base_value: 0.5, inputs: {} },
				radius_logarithmic: { base_value: 2, inputs: {} }
			}
		});
		const s = JSON.parse(cloneBrushSource(myb)).settings;
		expect(s.opaque).toEqual({ base_value: 1, inputs: {} });
		expect(s.smudge.base_value).toBe(0);
		expect(s.eraser.base_value).toBe(0);
		expect(s.radius_logarithmic.base_value).toBe(2);
	});
});
