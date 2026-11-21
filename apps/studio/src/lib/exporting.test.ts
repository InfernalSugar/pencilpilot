import { describe, expect, it } from 'vitest';
import { cutsIn, outputSize, placeCamera, plan } from './exporting';
import { addCut, newTimeline, setStatic, type Timeline } from './timeline';

/** layer 1: A B on twos, C held 3; layer 2: a static background */
function tl(): Timeline {
	const t = newTimeline([
		{ id: 1, drawing: 0 },
		{ id: 2, drawing: 0 }
	]);
	const cut = t.scenes[0].cuts[0];
	cut.tracks[1] = [
		{ drawing: 1, length: 2 },
		{ drawing: 2, length: 2 },
		{ drawing: 3, length: 3 }
	];
	cut.tracks[2] = [{ drawing: 9, length: 1 }];
	setStatic(t, 2, true);
	return t;
}

describe('export plans', () => {
	it('send each drawing once, held for its frames', () => {
		const t = tl();
		const runs = plan(t, cutsIn(t, 'cut', { scene: 0, cut: 0 }, null), [1, 2], 24, 24);
		expect(runs.map((r) => [r.pairs, r.repeat])).toEqual([
			[
				[
					[1, 1],
					[2, 9]
				],
				2
			],
			[
				[
					[1, 2],
					[2, 9]
				],
				2
			],
			[
				[
					[1, 3],
					[2, 9]
				],
				3
			]
		]);
	});

	it('follow time at another frame rate', () => {
		const t = tl();
		const cuts = cutsIn(t, 'cut', { scene: 0, cut: 0 }, null);
		// 7 frames at 24 → about 3.5 at 12: 4 output frames
		const half = plan(t, cuts, [1], 24, 12);
		expect(half.map((r) => [r.source.frame, r.repeat])).toEqual([
			[0, 1],
			[2, 1],
			[4, 2]
		]);
		const double = plan(t, cuts, [1], 24, 48);
		expect(double.reduce((n, r) => n + r.repeat, 0)).toBe(14);
	});

	it('cover the loop range, the scene or everything', () => {
		const t = tl();
		addCut(t.scenes[0], 0);
		expect(cutsIn(t, 'loop', { scene: 0, cut: 0 }, { from: 2, to: 4 })).toEqual([
			{ scene: 0, cut: 0, from: 2, to: 4 }
		]);
		expect(cutsIn(t, 'scene', { scene: 0, cut: 0 }, null)).toEqual([
			{ scene: 0, cut: 0, from: 0, to: 6 },
			{ scene: 0, cut: 1, from: 0, to: 0 }
		]);
		// two cuts that look the same still go as two runs (each has its camera)
		const runs = plan(t, cutsIn(t, 'all', { scene: 0, cut: 0 }, null), [1], 24, 24);
		expect(runs.at(-1)!.source).toEqual({ scene: 0, cut: 1, frame: 0 });
	});

	it('size the output from the camera, even for video', () => {
		expect(outputSize({ x: 0, y: 0, width: 1080, height: 1920 }, 0.5, true)).toEqual({
			width: 540,
			height: 960
		});
		expect(outputSize({ x: 0, y: 0, width: 101, height: 57 }, 1, true)).toEqual({
			width: 100,
			height: 56
		});
		expect(outputSize({ x: 0, y: 0, width: 101, height: 57 }, 1, false).width).toBe(101);
		// a 16:9 cut in a square output: bars above and below
		expect(
			placeCamera({ x: 0, y: 0, width: 160, height: 90 }, { width: 100, height: 100 })
		).toEqual({
			x: 0,
			y: 21.875,
			width: 100,
			height: 56.25,
			k: 0.625
		});
	});
});
