import { describe, expect, it } from 'vitest';
import {
	addCut,
	addScene,
	cameraOf,
	cutLength,
	deleteFrame,
	deleteFrameAll,
	drawingAt,
	drawingNumbers,
	drawingsIn,
	findDrawing,
	fitCamera,
	followTiming,
	fromFile,
	inset,
	insertExposure,
	insertFrame,
	insertFrameAll,
	moveExposure,
	newTimeline,
	removeExposure,
	setDrawing,
	setKey,
	setLength,
	setNote,
	setStage,
	setStatic,
	setStaticDrawing,
	setTiming,
	showing,
	stageOf,
	stagesFromFile,
	stagesToFile,
	staticDrawing,
	staticsToFile,
	toFile,
	type Cut,
	type Timeline,
	type Track
} from './timeline';

const frames = (t: Track, n = 8) => Array.from({ length: n }, (_, f) => drawingAt(t, f));

describe('timeline', () => {
	it('shows each drawing for its exposure', () => {
		const t: Track = [
			{ drawing: 1, length: 2 },
			{ drawing: 0, length: 1 },
			{ drawing: 2, length: 3 }
		];
		expect(frames(t)).toEqual([1, 1, 0, 2, 2, 2, 0, 0]);
	});

	it('drawing on an empty cel or past the end starts a drawing there', () => {
		const t: Track = [{ drawing: 1, length: 1 }];
		setDrawing(t, 3, 5); // past the end: empty cels fill the gap
		expect(t).toEqual([
			{ drawing: 1, length: 1 },
			{ drawing: 0, length: 2 },
			{ drawing: 5, length: 1 }
		]);
		setDrawing(t, 2, 6); // the empty cel (frames 1–2) becomes drawing 6
		expect(frames(t, 4)).toEqual([1, 6, 6, 5]);
	});

	it('changes timing: hold, ones / twos / threes, insert and delete frames', () => {
		const t: Track = [
			{ drawing: 1, length: 1 },
			{ drawing: 2, length: 1 }
		];
		setTiming(t, 2);
		expect(frames(t, 5)).toEqual([1, 1, 2, 2, 0]);
		setLength(t, 0, 3);
		expect(frames(t, 6)).toEqual([1, 1, 1, 2, 2, 0]);
		insertFrame(t, 4);
		expect(frames(t, 7)).toEqual([1, 1, 1, 2, 2, 2, 0]);
		deleteFrame(t, 0);
		deleteFrame(t, 0);
		deleteFrame(t, 0);
		expect(t).toEqual([{ drawing: 2, length: 3 }]);
		setLength(t, 0, 0);
		expect(t[0].length).toBe(1);
	});

	it('inserts, removes and moves exposures', () => {
		const t: Track = [
			{ drawing: 1, length: 1 },
			{ drawing: 2, length: 2 }
		];
		insertExposure(t, 1, { drawing: 3, length: 1 });
		expect(frames(t, 4)).toEqual([1, 3, 2, 2]);
		moveExposure(t, 0, 2);
		expect(frames(t, 4)).toEqual([3, 2, 2, 1]);
		removeExposure(t, 1);
		expect(frames(t, 3)).toEqual([3, 1, 0]);
		// empty cels at the end don't count
		insertExposure(t, 2, { drawing: 0, length: 4 });
		removeExposure(t, 0);
		expect(t).toEqual([{ drawing: 1, length: 1 }]);
	});

	it('works on every layer of a cut', () => {
		const cut: Cut = {
			id: 1,
			name: 'Cut 1',
			tracks: { 1: [{ drawing: 1, length: 2 }], 2: [{ drawing: 9, length: 4 }] }
		};
		expect(cutLength(cut)).toBe(4);
		expect(showing(cut, [1, 2, 3], 1)).toEqual([
			[1, 1],
			[2, 9],
			[3, 0]
		]);
		insertFrameAll(cut, 0);
		expect(cutLength(cut)).toBe(5);
		expect(drawingAt(cut.tracks[1], 2)).toBe(1);
		deleteFrameAll(cut, 0);
		deleteFrameAll(cut, 0);
		expect(cut.tracks[1]).toEqual([{ drawing: 1, length: 1 }]);
		expect(cutLength({ id: 2, name: '', tracks: {} })).toBe(1);
	});

	it('finds drawings, and adds scenes and cuts', () => {
		const tl = newTimeline([
			{ id: 1, drawing: 4 },
			{ id: 2, drawing: 0 }
		]);
		expect(tl.scenes[0].cuts[0].tracks).toEqual({ 1: [{ drawing: 4, length: 1 }], 2: [] });
		const c = addCut(tl.scenes[0], 0);
		setDrawing(tl.scenes[0].cuts[c].tracks[2] ?? (tl.scenes[0].cuts[c].tracks[2] = []), 2, 7);
		expect(findDrawing(tl, 7)).toEqual({ scene: 0, cut: 1, frame: 2, layer: 2 });
		expect(findDrawing(tl, 99)).toBeNull();
		expect(drawingsIn(tl).sort()).toEqual([4, 7]);
		expect(addScene(tl, 0)).toBe(1);
		expect(tl.scenes.map((s) => s.name)).toEqual(['Scene 1', 'Scene 2']);
	});

	it('goes to the file and back, for the layers that exist', () => {
		const tl = newTimeline([{ id: 3, drawing: 8 }]);
		tl.scenes[0].cuts[0].tracks[3].push({ drawing: 0, length: 2 }, { drawing: 9, length: 1 });
		tl.scenes[0].cuts[0].tracks[5] = [{ drawing: 10, length: 1 }]; // a deleted layer
		const file = toFile(tl, [3]);
		expect(file[0].cuts[0].tracks).toEqual({
			3: [
				{ drawing: 'D8', length: 1 },
				{ drawing: null, length: 2 },
				{ drawing: 'D9', length: 1 }
			]
		});
		// reopened, the layer has a new id on the canvas
		const back = fromFile(file, new Map([[3, 12]]));
		expect(back.scenes[0].cuts[0].tracks[12]).toEqual(tl.scenes[0].cuts[0].tracks[3]);
	});

	it('static layers show one drawing all cut long', () => {
		const tl = newTimeline([
			{ id: 1, drawing: 1 },
			{ id: 2, drawing: 0 }
		]);
		const cut = tl.scenes[0].cuts[0];
		cut.tracks[1].push({ drawing: 2, length: 1 }, { drawing: 3, length: 1 });
		cut.tracks[2] = [
			{ drawing: 0, length: 1 },
			{ drawing: 7, length: 9 },
			{ drawing: 8, length: 1 }
		];
		expect(cutLength(cut)).toBe(11);
		setStatic(tl, 2, true);
		setStatic(tl, 2, true); // (once)
		expect(tl.statics).toEqual([2]);
		// its first drawing on every frame; it doesn't make the cut longer
		expect(cutLength(cut, tl.statics)).toBe(3);
		expect([0, 1, 2].map((f) => showing(cut, [1, 2], f, tl.statics))).toEqual([
			[
				[1, 1],
				[2, 7]
			],
			[
				[1, 2],
				[2, 7]
			],
			[
				[1, 3],
				[2, 7]
			]
		]);
		// frames go in and out of the cel layers only
		insertFrameAll(cut, 0, tl.statics);
		expect(cut.tracks[2][1].length).toBe(9);
		deleteFrameAll(cut, 0, tl.statics);
		// drawing on it changes its one drawing; an empty one gets one
		setStaticDrawing(cut.tracks[2], 9);
		expect(staticDrawing(cut.tracks[2])).toBe(9);
		const empty: Track = [];
		setStaticDrawing(empty, 4);
		expect(empty).toEqual([{ drawing: 4, length: 1 }]);
		// a cel layer again: it animates as before
		setStatic(tl, 2, false);
		expect(tl.statics).toEqual([]);
		expect(drawingAt(cut.tracks[2], 10)).toBe(8);
	});

	it('frames each cut with a camera', () => {
		const canvas = { width: 1920, height: 1080 };
		const cut: Cut = { id: 1, name: 'Cut 1', tracks: {} };
		expect(cameraOf(cut, canvas)).toEqual({ x: 0, y: 0, width: 1920, height: 1080 });
		expect(fitCamera(1, canvas)).toEqual({ x: 420, y: 0, width: 1080, height: 1080 });
		expect(fitCamera(2.39, canvas)).toEqual({ x: 0, y: 139, width: 1920, height: 803 });
		// kept on the canvas
		cut.camera = { x: 1800, y: -50, width: 400, height: 300 };
		expect(cameraOf(cut, canvas)).toEqual({ x: 1520, y: 0, width: 400, height: 300 });
		expect(inset({ x: 0, y: 0, width: 100, height: 50 }, 0.8)).toEqual({
			x: 10,
			y: 5,
			width: 80,
			height: 40
		});
		// a new cut is framed like the one before it
		const tl = newTimeline([]);
		tl.scenes[0].cuts[0].camera = { x: 1, y: 2, width: 3, height: 4 };
		addCut(tl.scenes[0], 0);
		expect(tl.scenes[0].cuts[1].camera).toEqual({ x: 1, y: 2, width: 3, height: 4 });
	});

	it('keeps static layers and cameras in the file', () => {
		const tl = newTimeline([
			{ id: 3, drawing: 8 },
			{ id: 4, drawing: 9 }
		]);
		tl.scenes[0].cuts[0].camera = { x: 0, y: 0, width: 800, height: 600 };
		setStatic(tl, 4, true);
		setStatic(tl, 6, true); // a deleted layer
		const file = toFile(tl, [3, 4]);
		expect(staticsToFile(tl, [3, 4])).toEqual([4]);
		const back = fromFile(
			file,
			new Map([
				[3, 12],
				[4, 13]
			]),
			[4]
		);
		expect(back.statics).toEqual([13]);
		expect(back.scenes[0].cuts[0].camera).toEqual({ x: 0, y: 0, width: 800, height: 600 });
		// a broken camera is dropped
		file[0].cuts[0].camera = { x: 0, y: 0, width: 0, height: 5 };
		expect(fromFile(file, new Map()).scenes[0].cuts[0].camera).toBeUndefined();
	});

	it('writes drawings and notes as on an exposure sheet', () => {
		const t: Track = [
			{ drawing: 1, length: 4 },
			{ drawing: 2, length: 2 }
		];
		// a key mid-hold: from there to the end of that hold
		setKey(t, 2, 3);
		expect(frames(t, 6)).toEqual([1, 1, 3, 3, 2, 2]);
		// an empty cel, and a key past the end
		setKey(t, 4, 0);
		setKey(t, 8, 1);
		expect(frames(t, 10)).toEqual([1, 1, 3, 3, 0, 0, 0, 0, 1, 0]);
		// a drawing exposed again keeps its number
		expect([...drawingNumbers(t)]).toEqual([
			[1, 1],
			[3, 2]
		]);

		const cut: Cut = { id: 1, name: 'Cut 1', tracks: { 1: t } };
		setNote(cut, 'dialogue', 2, ' hel- ');
		setNote(cut, 'dialogue', 4, 'lo');
		setNote(cut, 'action', 0, 'turns');
		expect(cut.dialogue).toEqual({ 2: 'hel-', 4: 'lo' });
		// notes move with inserted and deleted frames
		insertFrameAll(cut, 1);
		expect(cut.dialogue).toEqual({ 3: 'hel-', 5: 'lo' });
		expect(cut.action).toEqual({ 0: 'turns' });
		deleteFrameAll(cut, 3);
		expect(cut.dialogue).toEqual({ 4: 'lo' });
		setNote(cut, 'action', 0, '');
		expect(cut.action).toBeUndefined();
		// kept in the file
		const tl = newTimeline([]);
		tl.scenes[0].cuts[0] = cut;
		const back = fromFile(toFile(tl, [1]), new Map([[1, 1]]));
		expect(back.scenes[0].cuts[0].dialogue).toEqual({ 4: 'lo' });
		expect(back.scenes[0].cuts[0].action).toBeUndefined();
	});
});

describe('layer stages', () => {
	it('set, clear, and go away when none is left', () => {
		const tl = newTimeline([]);
		setStage(tl, 3, 'rough');
		setStage(tl, 4, 'clean');
		expect(stageOf(tl, 3)).toBe('rough');
		expect(stageOf(tl, 5)).toBeNull();
		setStage(tl, 3, 'tiedown');
		expect(tl.stages).toEqual({ 3: 'tiedown', 4: 'clean' });
		setStage(tl, 3, null);
		setStage(tl, 4, null);
		expect(tl.stages).toBeUndefined();
		// (layer 0 is no layer)
		setStage(tl, 0, 'rough');
		expect(tl.stages).toBeUndefined();
	});

	it('round-trip through the file, for layers that exist (unknown stages left out)', () => {
		const tl = newTimeline([]);
		setStage(tl, 3, 'rough');
		setStage(tl, 4, 'clean');
		setStage(tl, 9, 'comp');
		const file = stagesToFile(tl, [3, 4]);
		expect(file).toEqual({ 3: 'rough', 4: 'clean' });
		// opened, the canvas gives the layers new ids
		const ids = new Map([
			[3, 30],
			[4, 40]
		]);
		expect(stagesFromFile(file, ids)).toEqual({ 30: 'rough', 40: 'clean' });
		expect(stagesFromFile({ 3: 'wobbly', 4: 'flats', 7: 'rough' }, ids)).toEqual({ 40: 'flats' });
		expect(stagesFromFile(undefined, ids)).toEqual({});
	});
});

describe('a layer timed as another (to trace it)', () => {
	const tl = (): Timeline => ({
		scenes: [
			{
				id: 1,
				name: 'Scene 1',
				cuts: [
					// layer 1: A held 3, an empty cel for 2, B, then A again (a cycle)
					{
						id: 1,
						name: 'Cut 1',
						tracks: {
							1: [
								{ drawing: 10, length: 3 },
								{ drawing: 0, length: 2 },
								{ drawing: 11, length: 1 },
								{ drawing: 10, length: 2 }
							],
							2: [{ drawing: 20, length: 8 }]
						}
					},
					// layer 1 has no track here: none made
					{ id: 2, name: 'Cut 2', tracks: { 2: [{ drawing: 21, length: 4 }] } }
				]
			},
			{
				id: 2,
				name: 'Scene 2',
				cuts: [{ id: 1, name: 'Cut 1', tracks: { 1: [{ drawing: 12, length: 2 }] } }]
			}
		],
		statics: [],
		kinds: { 10: 'key', 11: 'inbetween' }
	});
	const counter = () => {
		let n = 100;
		return () => ++n;
	};

	it('copies holds and gaps, with a new drawing for each one shown', () => {
		const t = tl();
		const made = followTiming(t, 1, 5, counter());
		expect(made).toEqual([101, 102, 103]);
		expect(t.scenes[0].cuts[0].tracks[5]).toEqual([
			{ drawing: 101, length: 3 },
			{ drawing: 0, length: 2 },
			{ drawing: 102, length: 1 },
			// the same drawing again: the same new one
			{ drawing: 101, length: 2 }
		]);
		expect(frames(t.scenes[0].cuts[0].tracks[5])).toEqual([101, 101, 101, 0, 0, 102, 101, 101]);
		// the source is unchanged
		expect(t.scenes[0].cuts[0].tracks[1][0]).toEqual({ drawing: 10, length: 3 });
	});

	it('does every cut the layer has a track in, and only those', () => {
		const t = tl();
		followTiming(t, 1, 5, counter());
		expect(t.scenes[0].cuts[1].tracks[5]).toBeUndefined();
		expect(t.scenes[1].cuts[0].tracks[5]).toEqual([{ drawing: 103, length: 2 }]);
		expect(cutLength(t.scenes[0].cuts[0])).toBe(8);
	});

	it('marks the new drawings as their own are (keys, in-betweens)', () => {
		const t = tl();
		followTiming(t, 1, 5, counter());
		expect(t.kinds).toEqual({ 10: 'key', 11: 'inbetween', 101: 'key', 102: 'inbetween' });
	});

	it('a static layer’s is static too', () => {
		const t = tl();
		setStatic(t, 2, true);
		const made = followTiming(t, 2, 6, counter());
		expect(made).toEqual([101, 102]);
		expect(t.statics).toEqual([2, 6]);
		expect(staticDrawing(t.scenes[0].cuts[0].tracks[6])).toBe(101);
		expect(staticDrawing(t.scenes[0].cuts[1].tracks[6])).toBe(102);
		// a cel layer's stays a cel layer
		followTiming(t, 1, 7, counter());
		expect(t.statics).not.toContain(7);
	});

	it('an empty track gives an empty track', () => {
		const t = tl();
		t.scenes[0].cuts[1].tracks[1] = [];
		followTiming(t, 1, 5, counter());
		expect(t.scenes[0].cuts[1].tracks[5]).toEqual([]);
	});
});
