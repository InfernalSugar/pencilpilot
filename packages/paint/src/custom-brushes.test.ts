import { describe, expect, it } from 'vitest';
import { applyCurve } from './pressure';
import {
	applyTweaks,
	changedTweaks,
	customBrushes,
	deleteCustomBrush,
	saveCustomBrush,
	tweaksOf
} from './custom-brushes';

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
		// the pressure curve on opaque_multiply is lost to the app's opacity slider
		expect(t.pressureOpacity).toBe(0);
		expect(t.opacity).toBe(1);
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
		// pressure → opacity goes on `opaque` (the app's opacity slider owns opaque_multiply)
		expect(out.opaque.inputs.pressure).toEqual([
			[0, 0],
			[1, 0.5]
		]);
		expect(out.opaque.base_value).toBeCloseTo(0.5);
		expect(out.opaque_multiply).toEqual({ base_value: 0, inputs: {} });
		// round trip: the editor reads back what it wrote
		expect(tweaksOf(applyTweaks(MYB, { pressureSize: 0.5 })).pressureSize).toBeCloseTo(0.5);
	});

	it('reads back every setting it writes', () => {
		const t = {
			size: 12,
			sizeRandom: 0.3,
			hardness: 0.6,
			roundness: 4,
			angle: 30,
			followDirection: true,
			spacing: 3,
			smoothing: 2.5,
			jitter: 0.7,
			shake: 0.4,
			pressureSize: 0.6,
			pressureOpacity: 0.7,
			speedSize: -0.5,
			tiltSize: 0.8,
			opacity: 0.8,
			smudge: 0.4,
			smudgeLength: 0.3,
			hueJitter: 0.1,
			shadeJitter: 0.2
		};
		const back = tweaksOf(applyTweaks(MYB, t));
		for (const [k, v] of Object.entries(t))
			if (typeof v === 'boolean') expect(back[k as keyof typeof t], k).toBe(v);
			else expect(back[k as keyof typeof t] as number, k).toBeCloseTo(v, 5);
		const s = JSON.parse(applyTweaks(MYB, t)).settings;
		expect(s.radius_logarithmic.base_value).toBeCloseTo(Math.log(12));
		expect(s.elliptical_dab_angle.inputs.direction).toEqual([
			[0, 0],
			[180, 180]
		]);
		expect(s.change_color_h.inputs.random).toEqual([
			[0, -0.1],
			[1, 0.1]
		]);
		// pressure and speed both shape the size
		expect(Object.keys(s.radius_logarithmic.inputs).sort()).toEqual([
			'pressure',
			'speed1',
			'tilt_declination'
		]);
		// switched off again: the curves go
		const off = JSON.parse(
			applyTweaks(applyTweaks(MYB, t), {
				followDirection: false,
				speedSize: 0,
				tiltSize: 0,
				hueJitter: 0
			})
		).settings;
		expect(off.elliptical_dab_angle.inputs.direction).toBeUndefined();
		expect(off.radius_logarithmic.inputs.speed1).toBeUndefined();
		expect(off.radius_logarithmic.inputs.tilt_declination).toBeUndefined();
		expect(off.change_color_h.inputs.random).toBeUndefined();
	});

	it('opacity alone keeps the pressure share, and the reverse', () => {
		const half = applyTweaks(MYB, { opacity: 1, pressureOpacity: 0.5 });
		const t = tweaksOf(applyTweaks(half, { opacity: 0.4 }));
		expect(t.opacity).toBeCloseTo(0.4);
		expect(t.pressureOpacity).toBeCloseTo(0.5);
	});

	it('keeps only what changed (the rest stays as the brush had it)', () => {
		const start = tweaksOf(MYB);
		expect(changedTweaks(start, { ...start })).toEqual({});
		expect(changedTweaks(start, { ...start, hardness: 0.5, followDirection: true })).toEqual({
			hardness: 0.5,
			followDirection: true
		});
		// opacity and its pressure share go together
		expect(changedTweaks(start, { ...start, pressureOpacity: 0.3 })).toEqual({
			opacity: start.opacity,
			pressureOpacity: 0.3
		});
	});

	it('an old brush (the five first tweaks) still loads', () => {
		const old = applyTweaks(MYB, {
			hardness: 0.5,
			spacing: 2,
			jitter: 0.1,
			pressureSize: 0.4,
			pressureOpacity: 0.6
		});
		const t = tweaksOf(old);
		expect(t.hardness).toBe(0.5);
		expect(t.pressureSize).toBeCloseTo(0.4);
		expect(t.pressureOpacity).toBeCloseTo(0.6);
	});

	it('saves, updates in place and deletes', () => {
		const a = saveCustomBrush({ name: '  Inky  ', base: 'b', tweaks: { hardness: 1 } });
		const b = saveCustomBrush({ name: 'Second', base: 'b', tweaks: {} });
		expect(a.id).not.toBe(b.id);
		expect(a.name).toBe('Inky');
		saveCustomBrush({ ...a, name: 'Inky 2' });
		expect(customBrushes().map((x) => x.name)).toEqual(['Inky 2', 'Second']);
		deleteCustomBrush(a.id);
		deleteCustomBrush(b.id);
		expect(customBrushes()).toEqual([]);
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
