import { describe, expect, it } from 'vitest';
import { cutsIn, outputSize, placeCamera, plan, stageFile } from './exporting';
import { exportPasses, layersInView } from './stages';
import { addCut, newTimeline, setStage, setStatic, type Timeline } from './timeline';

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

describe('frame files', () => {
	it('numbers frames from the name, with the layer when there is one', async () => {
		const { frameFile } = await import('./exporting');
		expect(frameFile('/a/Walk', 7, 120, 'psd')).toBe('/a/Walk_0007.psd');
		expect(frameFile('/a/Walk', 7, 12000, 'png', 'Ink / colour')).toBe(
			'/a/Walk_Ink - colour_00007.png'
		);
	});
});

describe('transitions between cuts', () => {
	it('lay a flash and a dissolve over the frames, keeping the length', async () => {
		const { outputFrames, joinFrames } = await import('./exporting');
		const run = (cut: number, repeat: number) => ({
			source: { scene: 0, cut, frame: 0 },
			pairs: [] as [number, number][],
			repeat
		});
		// cut 0: 6 frames (two runs), cut 1: 14 frames, cut 2: 24 frames
		const runs = [run(0, 3), run(0, 3), run(1, 14), run(2, 24)];
		const kinds = ['flash', 'dissolve', undefined] as const;
		const out = outputFrames(runs, (s) => kinds[s.cut], 24);
		expect(out.length).toBe(44);
		// the flash: the last 3 of cut 0 darken, then white landing on cut 1
		expect(out.slice(3, 6).map((f) => f.dim)).toEqual([0.88, 0.76, 0.65]);
		expect(out.slice(6, 9).map((f) => f.white)).toEqual([1, 0.55, 0.2]);
		expect(out[9]).toEqual({ run: 2 });
		// the dissolve: 12 frames around the cut at frame 20, cut 1's last frame held under
		// cut 2's first, eased
		expect(out.filter((f) => f.over).length).toBe(12);
		expect(out[13].over).toBeUndefined();
		expect(out[14]).toMatchObject({ run: 2, over: { run: 3 } });
		expect(out[25]).toMatchObject({ run: 2, over: { run: 3 } });
		expect(out[26]).toEqual({ run: 3 });
		const alphas = out.slice(14, 26).map((f) => f.over!.alpha);
		expect(alphas.every((a, i) => i === 0 || a > alphas[i - 1])).toBe(true);
		expect(alphas[0]).toBeLessThan(0.05);
		expect(alphas[11]).toBeGreaterThan(0.95);
		// joined back into runs
		const j = joinFrames(out);
		expect(j.reduce((n, [, r]) => n + r, 0)).toBe(44);
		expect(j.at(-1)).toEqual([{ run: 3 }, 18]);
	});
});

describe('exporting by stage', () => {
	/** layer 1: a rough on twos; layer 2: its clean-up on ones; layer 3: untagged notes */
	function staged(): Timeline {
		const t = newTimeline([]);
		const cut = t.scenes[0].cuts[0];
		cut.tracks[1] = [
			{ drawing: 11, length: 2 },
			{ drawing: 12, length: 2 }
		];
		cut.tracks[2] = [
			{ drawing: 21, length: 1 },
			{ drawing: 22, length: 1 },
			{ drawing: 23, length: 1 },
			{ drawing: 24, length: 1 }
		];
		cut.tracks[3] = [{ drawing: 31, length: 4 }];
		setStage(t, 1, 'rough');
		setStage(t, 2, 'clean');
		return t;
	}
	const layers = [1, 2, 3].map((id) => ({ id, clip: false }));
	/** what each pass sends: its file label, and each run's pairs and how long it's held */
	const send = (t: Timeline, show: 'final' | 'view' | 'all' | 'each') =>
		exportPasses(show, t.stages, { id: 'rough' }).map((pass) => {
			const { shown } = layersInView(layers, t.stages, pass.view);
			const runs = plan(t, cutsIn(t, 'cut', { scene: 0, cut: 0 }, null), shown, 24, 24);
			return { label: pass.label, runs: runs.map((r) => [r.pairs.map(([, d]) => d), r.repeat]) };
		});

	it('leave the rough out of the final film, and send its frames on twos alone', () => {
		const t = staged();
		expect(send(t, 'final')).toEqual([
			{
				label: null,
				runs: [
					[[21, 31], 1],
					[[22, 31], 1],
					[[23, 31], 1],
					[[24, 31], 1]
				]
			}
		]);
		// the view (rough): its drawings held two frames each, sent once
		expect(send(t, 'view')).toEqual([
			{
				label: null,
				runs: [
					[[11, 31], 2],
					[[12, 31], 2]
				]
			}
		]);
		expect(send(t, 'all')[0].runs).toHaveLength(4);
	});

	it('make a pass per stage that has layers, each named after it', () => {
		const t = staged();
		expect(send(t, 'each').map((p) => [p.label, p.runs.length])).toEqual([
			['Rough', 2],
			['Clean-up', 4]
		]);
		// no stages: one pass of every layer
		expect(exportPasses('each', undefined, { id: 'all' })).toEqual([
			{ label: null, view: { id: 'all' } }
		]);
		expect(stageFile('/home/me/Walk cycle.mp4', 'Rough')).toBe('/home/me/Walk cycle_Rough.mp4');
		expect(stageFile('/home/me/Walk cycle.png', 'Tie-down')).toBe(
			'/home/me/Walk cycle_Tie-down.png'
		);
		expect(stageFile('/home/me/v1.2/film', 'Clean-up')).toBe('/home/me/v1.2/film_Clean-up');
	});
});
