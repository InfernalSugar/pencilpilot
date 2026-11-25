import { describe, expect, it } from 'vitest';
import {
	BRUSH_DEFAULTS,
	DEFAULT_STRANDS,
	NO_TEXTURE,
	STRANDS_MAX,
	brushTexture,
	readStrands,
	setBrushTexture,
	strandStrokes,
	strandsOn,
	strandsReach,
	styleStrokes,
	textureOn
} from './brush-textures';

const HAIR = 'pencilpilot-hair-strands';

describe('strands', () => {
	it('reads strand settings back checked and clamped', () => {
		expect(readStrands({ count: 6, spread: 12, wobble: 0.5, taper: 0.25 })).toEqual({
			count: 6,
			spread: 12,
			wobble: 0.5,
			taper: 0.25
		});
		expect(readStrands({ count: 99.4, spread: 1000, wobble: -1, taper: 'x' })).toEqual({
			count: STRANDS_MAX,
			spread: 30,
			wobble: 0,
			taper: DEFAULT_STRANDS.taper
		});
		// fewer than two strands is one line: no strands
		expect(readStrands({ count: 1, spread: 5, wobble: 0, taper: 0 })).toBeUndefined();
		expect(readStrands(null)).toBeUndefined();
		expect(readStrands('8')).toBeUndefined();
	});

	it('counts as a change to how strokes look', () => {
		expect(textureOn(NO_TEXTURE)).toBe(false);
		const t = { ...NO_TEXTURE, strands: { ...DEFAULT_STRANDS } };
		expect(strandsOn(t)).toBe(true);
		expect(textureOn(t)).toBe(true);
		expect(textureOn({ ...t, strands: null })).toBe(false);
	});

	it('reaches the bundle’s half width (and its wobble) past the brush’s own reach', () => {
		expect(strandsReach({ count: 4, spread: 10, wobble: 0, taper: 0 }, 2.5)).toBe(7.5);
		expect(strandsReach({ count: 4, spread: 10, wobble: 1, taper: 0 }, 2.5)).toBeCloseTo(13.5);
	});

	it('gives the engine its strands (or none) with the rest of the dab style', () => {
		const calls: number[][] = [];
		const engine = {
			setStrands: (...a: number[]) => void calls.push(a),
			setTexture: () => true,
			setDabStyle: () => {}
		};
		strandStrokes(engine, {
			...NO_TEXTURE,
			strands: { count: 5, spread: 8, wobble: 0.2, taper: 1 }
		});
		strandStrokes(engine, NO_TEXTURE);
		styleStrokes(
			engine,
			{ ...NO_TEXTURE, strands: { count: 3, spread: 2, wobble: 0, taper: 0 } },
			new Map()
		);
		styleStrokes(engine, null, new Map());
		expect(calls).toEqual([
			[5, 8, 0.2, 1],
			[0, 0, 0, 0],
			[3, 2, 0, 0],
			[0, 0, 0, 0]
		]);
	});

	it('comes with the Hair strands brush, and stays as it’s changed', () => {
		expect(BRUSH_DEFAULTS[HAIR]?.strands).toBeTruthy();
		expect(brushTexture(HAIR).strands).toEqual(BRUSH_DEFAULTS[HAIR].strands);
		// changed: kept
		const more = { ...brushTexture(HAIR), strands: { count: 16, spread: 20, wobble: 1, taper: 0 } };
		setBrushTexture(HAIR, more);
		expect(brushTexture(HAIR).strands).toEqual(more.strands);
		// switched off: stays off (its own settings don't come back)
		setBrushTexture(HAIR, { ...more, strands: null });
		expect(strandsOn(brushTexture(HAIR))).toBe(false);
		// forgotten: its own again
		setBrushTexture(HAIR, null);
		expect(brushTexture(HAIR).strands).toEqual(BRUSH_DEFAULTS[HAIR].strands);
	});

	it('keeps any brush’s strands with its other settings', () => {
		const id = 'my-test-strands';
		expect(brushTexture(id).strands).toBeUndefined();
		const t = { ...NO_TEXTURE, strands: { count: 7, spread: 6, wobble: 0.3, taper: 0.4 } };
		setBrushTexture(id, t);
		expect(brushTexture(id)).toEqual(t);
		// nothing on: nothing kept
		setBrushTexture(id, { ...t, strands: null });
		expect(brushTexture(id)).toEqual(NO_TEXTURE);
	});
});
