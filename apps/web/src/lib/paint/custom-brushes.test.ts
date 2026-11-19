import { describe, expect, it } from 'vitest';
import { applyCurve } from './pressure';
import { applyTweaks, tweaksOf } from './custom-brushes';

const MYB = JSON.stringify({
	version: 3,
	settings: {
		radius_logarithmic: {
			base_value: 0.75,
			inputs: {
				pressure: [
					[0, -0.6],
					[1, 0.3]
				]
			}
		},
		hardness: { base_value: 0.2, inputs: {} },
		dabs_per_actual_radius: { base_value: 4, inputs: {} },
		opaque_multiply: {
			base_value: 0,
			inputs: {
				pressure: [
					[0, 0],
					[1, 1]
				]
			}
		}
	}
});

describe('custom brushes', () => {
	it('reads the starting values and writes tweaks into the .myb', () => {
		const t = tweaksOf(MYB);
		expect(t.hardness).toBeCloseTo(0.2);
		expect(t.spacing).toBe(4);
		expect(t.pressureOpacity).toBeCloseTo(1);
		const out = JSON.parse(
			applyTweaks(MYB, {
				hardness: 0.9,
				spacing: 1,
				jitter: 0.4,
				pressureSize: 0,
				pressureOpacity: 0.5
			})
		).settings;
		expect(out.hardness.base_value).toBe(0.9);
		expect(out.dabs_per_actual_radius.base_value).toBe(1);
		expect(out.offset_by_random.base_value).toBe(0.4);
		expect(out.radius_logarithmic.inputs.pressure).toBeUndefined();
		expect(out.radius_logarithmic.base_value).toBe(0.75); // size untouched
		expect(out.opaque_multiply.inputs.pressure).toEqual([
			[0, 0.5],
			[1, 1]
		]);
		// round trip: the editor reads back what it wrote
		expect(tweaksOf(applyTweaks(MYB, { pressureSize: 0.5 })).pressureSize).toBeCloseTo(0.5);
	});

	it('clamps silly values', () => {
		const out = JSON.parse(applyTweaks(MYB, { hardness: 5, spacing: -3 })).settings;
		expect(out.hardness.base_value).toBe(1);
		expect(out.dabs_per_actual_radius.base_value).toBe(0.5);
	});
});

describe('pressure curve', () => {
	it('firmer needs more pressure, lighter needs less; ends stay put', () => {
		expect(applyCurve(0.5, 1)).toBeCloseTo(0.5);
		expect(applyCurve(0.5, 2)).toBeCloseTo(0.25);
		expect(applyCurve(0.5, 0.5)).toBeGreaterThan(0.7);
		for (const f of [0.5, 1, 2]) {
			expect(applyCurve(0, f)).toBe(0);
			expect(applyCurve(1, f)).toBe(1);
		}
	});
});
