import { describe, expect, it } from 'vitest';
import { StrokeLog, type Stroke } from './strokes';

const s = (id: string): Stroke => ({
	brushId: id,
	eraser: false,
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

	it('clear is undoable and a new stroke drops redo', () => {
		const log = new StrokeLog();
		log.add(s('a'));
		log.clear();
		expect(ids(log)).toEqual([]);
		log.undo();
		expect(ids(log)).toEqual(['a']);
		log.redo();
		expect(ids(log)).toEqual([]);
		log.undo();
		log.undo();
		log.add(s('c'));
		expect(log.redo()).toBe(false);
		expect(ids(log)).toEqual(['c']);
	});

	it('ignores empty strokes and respects the limit', () => {
		const log = new StrokeLog(3);
		log.add({ brushId: 'x', eraser: false, points: [] });
		expect(log.strokes.length).toBe(0);
		for (const id of ['1', '2', '3', '4', '5']) log.add(s(id));
		let n = 0;
		while (log.undo()) n++;
		expect(n).toBe(3);
		expect(ids(log)).toEqual(['1', '2']);
	});
});
