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

	it('takes a new limit, dropping the oldest steps', () => {
		const log = new StrokeLog(10);
		for (const id of ['1', '2', '3', '4', '5']) log.add(s(id));
		log.setLimit(2);
		let n = 0;
		while (log.undo()) n++;
		expect(n).toBe(2);
		expect(ids(log)).toEqual(['1', '2', '3']);
	});
});

describe('StrokeLog effects', () => {
	it('undoes and redoes the app’s own part of a step with it, joined steps too', () => {
		const log = new StrokeLog();
		const seen: string[] = [];
		const fx = (n: string) => ({ undo: () => seen.push(`-${n}`), redo: () => seen.push(`+${n}`) });
		log.add(s('a'));
		log.attachLast(fx('a'));
		log.layerChange();
		log.attachLast(fx('b'));
		log.layerChange();
		log.attachLast(fx('c'));
		log.joinLast();
		log.undo();
		expect(seen).toEqual(['-c', '-b']);
		log.undo();
		expect(seen).toEqual(['-c', '-b', '-a']);
		expect(ids(log)).toEqual([]);
		log.redo();
		log.redo();
		expect(seen.slice(3)).toEqual(['+a', '+b', '+c']);
		expect(ids(log)).toEqual(['a']);
	});

	it('keeps an effect already attached: the later one undoes first, redoes last', () => {
		const log = new StrokeLog();
		const seen: string[] = [];
		const fx = (n: string) => ({ undo: () => seen.push(`-${n}`), redo: () => seen.push(`+${n}`) });
		log.add(s('a'));
		log.attachLast(fx('stroke'));
		log.attachLast(fx('baked'));
		log.undo();
		log.redo();
		expect(seen).toEqual(['-baked', '-stroke', '+stroke', '+baked']);
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

	it('names each step for the history panel', () => {
		const log = new StrokeLog();
		log.add(s('a'));
		log.add({ ...s('b'), eraser: true });
		log.layerChange();
		log.labelLast('Fill');
		log.clear(1);
		expect(log.history()).toEqual({
			undo: ['Brush stroke', 'Eraser', 'Fill', 'Clear layer'],
			redo: []
		});
		log.undo();
		log.undo();
		// redo lists the next step first
		expect(log.history()).toEqual({
			undo: ['Brush stroke', 'Eraser'],
			redo: ['Fill', 'Clear layer']
		});
		log.batch(() => {
			log.layerChange();
			log.labelLast('Gradient');
		});
		// a batch takes its first action's name; the step before it keeps its own
		expect(log.history().undo).toEqual(['Brush stroke', 'Eraser', 'Gradient']);
	});

	it('moves and scales the logged strokes with a canvas size change, undoably', () => {
		const log = new StrokeLog();
		const a: Stroke = {
			...s('a'),
			size: 2,
			points: [
				{ x: 10, y: 20, pressure: 0.5, t: 0 },
				{ x: 30, y: 40, pressure: 0.7, t: 5 }
			]
		};
		const cloned: Stroke = { ...s('c'), clone: { dx: -8, dy: 4, heal: false, all: false } };
		log.add(a);
		log.add(cloned);
		// canvas twice the size, the drawing scaled with it
		log.layerChange();
		log.attachLast(log.transformStep([2, 0, 0, 2, 0, 0]));
		const now = log.strokes;
		expect(now[0].points.map((p) => [p.x, p.y, p.pressure, p.t])).toEqual([
			[20, 40, 0.5, 0],
			[60, 80, 0.7, 5]
		]);
		expect(now[0].size).toBe(4);
		expect(now[1].clone).toMatchObject({ dx: -16, dy: 8 });
		// the strokes as they were are kept as they were (the undo step has them)
		expect(a.points[0]).toEqual({ x: 10, y: 20, pressure: 0.5, t: 0 });
		log.undo();
		expect(log.strokes[0].points[0]).toMatchObject({ x: 10, y: 20 });
		expect(log.strokes[0].size).toBe(2);
		// undoing a stroke from before still finds it
		log.undo();
		expect(ids(log)).toEqual(['a']);
		log.redo();
		log.redo();
		expect(log.strokes[0].points[1]).toMatchObject({ x: 60, y: 80 });
		// more room (moved, not scaled): sizes stay
		log.layerChange();
		log.attachLast(log.transformStep([1, 0, 0, 1, 100, 50]));
		expect(log.strokes[0].points[0]).toMatchObject({ x: 120, y: 90 });
		expect(log.strokes[0].size).toBe(4);
		expect(log.strokes[1].clone).toMatchObject({ dx: -16, dy: 8 });
	});
});
