import { describe, expect, it } from 'vitest';
import { StrokeLog, type Stroke } from './strokes';

const s = (id: string, layer = 1): Stroke => ({
	brushId: id,
	eraser: false,
	layer,
	points: [{ x: 0, y: 0, pressure: 0.5, t: 0 }]
});
const ids = (log: StrokeLog) => log.strokes.map((x) => x.brushId);

describe('StrokeLog', () => {
	it('undo/redo strokes', () => {
		const log = new StrokeLog();
		log.add(s('a'));
		log.add(s('b'));
		expect(log.undo()).toBe(true);
		expect(ids(log)).toEqual(['a']);
		expect(log.redo()).toBe(true);
		expect(ids(log)).toEqual(['a', 'b']);
	});

	it('clearing a layer only removes its strokes, undoably; a new stroke drops redo', () => {
		const log = new StrokeLog();
		log.add(s('a', 1));
		log.add(s('b', 2));
		log.clear(1);
		expect(ids(log)).toEqual(['b']);
		log.undo();
		expect(ids(log).sort()).toEqual(['a', 'b']);
		log.redo();
		expect(ids(log)).toEqual(['b']);
		log.undo();
		log.undo();
		log.add(s('c'));
		expect(log.redo()).toBe(false);
		expect(ids(log)).toEqual(['a', 'c']);
	});

	it('merging moves strokes to the lower layer and back on undo', () => {
		const log = new StrokeLog();
		log.add(s('a', 1));
		log.add(s('b', 2));
		log.merge(2, 1);
		expect(log.on(new Set([1])).map((x) => x.brushId)).toEqual(['a', 'b']);
		log.undo();
		expect(log.on(new Set([1])).map((x) => x.brushId)).toEqual(['a']);
		expect(log.on(new Set([2])).map((x) => x.brushId)).toEqual(['b']);
	});

	it('layer changes and empty strokes are undo steps too (one-to-one with the engine)', () => {
		const log = new StrokeLog();
		log.add(s('a'));
		log.layerChange();
		log.add({ brushId: 'x', eraser: false, layer: 1, points: [] });
		expect(ids(log)).toEqual(['a']);
		expect(log.undo()).toBe(true); // empty stroke
		expect(log.undo()).toBe(true); // layer change
		expect(ids(log)).toEqual(['a']);
		expect(log.undo()).toBe(true);
		expect(ids(log)).toEqual([]);
	});

	it('respects the limit', () => {
		const log = new StrokeLog(3);
		for (const id of ['1', '2', '3', '4', '5']) log.add(s(id));
		let n = 0;
		while (log.undo()) n++;
		expect(n).toBe(3);
		expect(ids(log)).toEqual(['1', '2']);
	});
});

describe('StrokeLog batch', () => {
	it('records several actions as one undo step', () => {
		const log = new StrokeLog();
		const s = (layer: number) => ({
			brushId: 'b',
			eraser: false,
			layer,
			points: [{ x: 0, y: 0, pressure: 1, t: 0 }]
		});
		log.add(s(2));
		log.add(s(3));
		log.batch(() => {
			log.merge(3, 1);
			log.merge(2, 1);
		});
		expect(log.strokes.every((x) => x.layer === 1)).toBe(true);
		expect(log.undo()).toBe(true);
		expect(log.strokes.map((x) => x.layer)).toEqual([2, 3]);
		expect(log.redo()).toBe(true);
		expect(log.strokes.every((x) => x.layer === 1)).toBe(true);
		expect(log.undo() && log.undo() && log.undo()).toBe(true);
		expect(log.strokes).toHaveLength(0);
	});
});
