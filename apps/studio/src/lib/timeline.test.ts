import { describe, expect, it } from 'vitest';
import {
	addCut,
	addScene,
	cutLength,
	deleteFrame,
	deleteFrameAll,
	drawingAt,
	drawingsIn,
	findDrawing,
	fromFile,
	insertExposure,
	insertFrame,
	insertFrameAll,
	moveExposure,
	newTimeline,
	removeExposure,
	setDrawing,
	setLength,
	setTiming,
	showing,
	toFile,
	type Cut,
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
});
