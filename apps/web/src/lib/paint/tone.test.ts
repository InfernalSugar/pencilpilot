import { describe, expect, it } from 'vitest';
import {
	curveTable,
	levelsTable,
	mapTable,
	NO_CURVES,
	NO_LEVELS,
	NO_SPEC,
	parseSpec,
	specActive,
	specName,
	toneTables
} from './tone';

describe('curves', () => {
	it('a straight line changes nothing', () => {
		const t = curveTable([
			[0, 0],
			[1, 1]
		]);
		expect(t[0]).toBe(0);
		expect(t[128]).toBe(128);
		expect(t[255]).toBe(255);
	});

	it('passes through its points and never overshoots', () => {
		const t = curveTable([
			[0, 0],
			[0.25, 0.6],
			[0.5, 0.62],
			[1, 1]
		]);
		expect(t[Math.round(0.25 * 255)]).toBeCloseTo(0.6 * 255, -1);
		for (let i = 1; i < 256; i++) expect(t[i]).toBeGreaterThanOrEqual(t[i - 1]);
	});

	it('is flat outside its first and last points', () => {
		const t = curveTable([
			[0.2, 0.1],
			[0.8, 0.9]
		]);
		expect(t[0]).toBe(Math.round(0.1 * 255));
		expect(t[255]).toBe(Math.round(0.9 * 255));
	});
});

describe('levels', () => {
	it('moving the black and white points stretches the values', () => {
		const t = levelsTable({ ...NO_LEVELS, inBlack: 0.2, inWhite: 0.8 });
		expect(t[Math.round(0.2 * 255)]).toBe(0);
		expect(t[Math.round(0.8 * 255)]).toBe(255);
		expect(t[128]).toBeGreaterThan(120);
	});

	it('gamma above 1 brightens the middle', () => {
		expect(levelsTable({ ...NO_LEVELS, gamma: 2 })[128]).toBeGreaterThan(160);
	});
});

describe('tone tables', () => {
	it('are empty when nothing changes', () => {
		expect(toneTables({ curves: NO_CURVES, levels: NO_LEVELS })).toHaveLength(0);
		expect(toneTables({ curves: null, levels: null })).toHaveLength(0);
	});

	it('a red-only curve leaves green and blue alone', () => {
		const t = toneTables({
			curves: {
				...NO_CURVES,
				r: [
					[0, 1],
					[1, 0]
				]
			},
			levels: null
		});
		expect(t).toHaveLength(768);
		expect(t[0]).toBe(255); // red inverted
		expect(t[256 + 10]).toBe(10); // green unchanged
		expect(t[512 + 200]).toBe(200);
	});
});

describe('gradient map', () => {
	it('runs from the first colour to the last', () => {
		const t = mapTable([
			{ at: 0, color: '#000000' },
			{ at: 1, color: '#ff8000' }
		]);
		expect([t[0], t[1], t[2]]).toEqual([0, 0, 0]);
		expect([t[765], t[766], t[767]]).toEqual([255, 128, 0]);
	});

	it('is empty without stops', () => {
		expect(mapTable(null)).toHaveLength(0);
		expect(mapTable([])).toHaveLength(0);
	});
});

describe('specs', () => {
	it('know when they do something, and name themselves', () => {
		expect(specActive(NO_SPEC)).toBe(false);
		expect(specActive({ ...NO_SPEC, hue: 30 })).toBe(true);
		expect(specName({ ...NO_SPEC, map: [{ at: 0, color: '#000000' }] })).toBe('Gradient map');
		expect(specName({ ...NO_SPEC, levels: { ...NO_LEVELS, gamma: 2 } })).toBe('Levels');
	});

	it('round-trip through JSON and survive junk', () => {
		const s = { ...NO_SPEC, contrast: 0.3 };
		expect(parseSpec(JSON.stringify(s))).toEqual(s);
		expect(parseSpec('nope')).toEqual(NO_SPEC);
	});
});
