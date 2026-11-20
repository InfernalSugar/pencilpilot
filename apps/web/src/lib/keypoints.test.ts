import { describe, expect, it } from 'vitest';
import {
	cleanKeypoints,
	containRect,
	keypointPairs,
	labelIdeas,
	onCanvas,
	refKeypoints,
	tappedIndexes
} from './keypoints';
import { references } from './references';

const kp = (x: number, y: number, label = 'p') => ({ x, y, label });

describe('keypoints', () => {
	it('accepts good keypoints and rounds them', () => {
		expect(cleanKeypoints([kp(0.123456, 1, ' left eye ')])).toEqual([
			{ x: 0.1235, y: 1, label: 'left eye' }
		]);
		expect(cleanKeypoints([])).toEqual([]);
	});

	it('refuses anything malformed', () => {
		expect(cleanKeypoints('nope')).toBeNull();
		expect(cleanKeypoints([kp(1.2, 0.5)])).toBeNull();
		expect(cleanKeypoints([kp(Number.NaN, 0.5)])).toBeNull();
		expect(cleanKeypoints([kp(0.5, 0.5, '  ')])).toBeNull();
		expect(cleanKeypoints([{ x: 0.5, y: 0.5 }])).toBeNull();
		expect(cleanKeypoints(Array.from({ length: 13 }, () => kp(0.5, 0.5)))).toBeNull();
	});

	it('a reference needs at least three points for the step', () => {
		expect(refKeypoints({ keypoints: [kp(0, 0), kp(1, 1)] })).toEqual([]);
		expect(refKeypoints({ keypoints: [kp(0, 0), kp(1, 1), kp(0, 1)] })).toHaveLength(3);
		expect(refKeypoints({ keypoints: [kp(0, 0), kp(1, 1), kp(5, 1)] })).toEqual([]);
		expect(refKeypoints(undefined)).toEqual([]);
	});

	it('maps picture fractions to the canvas like the grader places the picture', () => {
		// a 2:1 picture in a 400×400 canvas: 400×200, centred vertically
		expect(containRect(200, 100, 400, 400)).toEqual({ x: 0, y: 100, w: 400, h: 200 });
		const ref = { width: 200, height: 100 };
		expect(onCanvas({ x: 0, y: 0 }, ref, 400, 400)).toEqual({ x: 0, y: 100 });
		expect(onCanvas({ x: 0.5, y: 1 }, ref, 400, 400)).toEqual({ x: 200, y: 300 });
	});

	it('pairs only the tapped points, and nothing if too few are left', () => {
		const ref = {
			width: 100,
			height: 100,
			keypoints: [kp(0, 0), kp(1, 0), kp(1, 1), kp(0, 1)]
		};
		const taps = [{ x: 1, y: 2 }, null, { x: 3, y: 4 }, { x: 5, y: 6 }];
		const pairs = keypointPairs(ref, taps, 100, 100);
		expect(pairs).toEqual([
			{ reference: { x: 0, y: 0 }, drawing: { x: 1, y: 2 } },
			{ reference: { x: 100, y: 100 }, drawing: { x: 3, y: 4 } },
			{ reference: { x: 0, y: 100 }, drawing: { x: 5, y: 6 } }
		]);
		expect(tappedIndexes(taps)).toEqual([0, 2, 3]);
		expect(keypointPairs(ref, [{ x: 1, y: 1 }, null, null, { x: 2, y: 2 }], 100, 100)).toEqual([]);
		expect(tappedIndexes([{ x: 1, y: 1 }, null])).toEqual([]);
	});

	it('suggests labels for every category', () => {
		for (const c of ['anime', 'real-life', 'animals', 'environment', 'cartoon', 'fundamentals'])
			expect(labelIdeas(c).length).toBeGreaterThan(3);
		expect(labelIdeas('real-life', 'hands')).toContain('thumb tip');
	});

	it('every marked reference in content/ has valid keypoints', () => {
		const bad = references
			.filter((r) => r.keypoints.length)
			.filter((r) => cleanKeypoints(r.keypoints)?.length !== r.keypoints.length)
			.map((r) => r.id);
		expect(bad).toEqual([]);
	});
});
