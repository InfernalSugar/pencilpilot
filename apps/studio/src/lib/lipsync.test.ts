import { describe, expect, it } from 'vitest';
import {
	autoChart,
	dialogueText,
	drawingFor,
	extendedOf,
	shapeFrames,
	writeMouths,
	type Cue
} from './lipsync';
import { fromFile, mouthsToFile, drawingsIn, layerOfDrawing, type MouthChart } from './timeline';
import { spell } from './frametools';

const cues = (...c: [number, number, string][]): Cue[] =>
	c.map(([start, end, value]) => ({ start, end, value }));

describe('lip-sync', () => {
	it('lays shapes on twos, the shape covering most of each pair', () => {
		// 24 fps: a frame is 1/24 s
		const f = 1 / 24;
		const c = cues([0, 2 * f, 'X'], [2 * f, 5 * f, 'D'], [5 * f, 8 * f, 'C']);
		expect(shapeFrames(c, 24, 8, 2)).toEqual(['X', 'X', 'D', 'D', 'D', 'D', 'C', 'C']);
		expect(shapeFrames(c, 24, 8, 1)).toEqual(['X', 'X', 'D', 'D', 'D', 'C', 'C', 'C']);
	});

	it('never loses a closed mouth to the twos', () => {
		const f = 1 / 24;
		// an M for half a frame between two open vowels
		const c = cues([0, 3 * f, 'D'], [3 * f, 3.5 * f, 'A'], [3.5 * f, 8 * f, 'C']);
		expect(shapeFrames(c, 24, 8, 2)).toEqual(['D', 'D', 'A', 'A', 'C', 'C', 'C', 'C']);
	});

	it('leads the sound by some frames, and rests past the end', () => {
		const f = 1 / 24;
		const c = cues([0, 4 * f, 'X'], [4 * f, 8 * f, 'F']);
		expect(shapeFrames(c, 24, 10, 1, 2)).toEqual([
			'X',
			'X',
			'F',
			'F',
			'F',
			'F',
			'X',
			'X',
			'X',
			'X'
		]);
	});

	it('finds a stand-in for shapes the chart lacks', () => {
		const chart: MouthChart = { layer: 1, shapes: { A: 10, C: 12, D: 13, F: 15 } };
		expect(drawingFor(chart, 'D')).toBe(13);
		expect(drawingFor(chart, 'X')).toBe(10);
		expect(drawingFor(chart, 'B')).toBe(12);
		expect(drawingFor(chart, 'E')).toBe(12);
		expect(drawingFor({ layer: 1, shapes: {} }, 'A')).toBe(0);
		expect(extendedOf(chart)).toBe('');
		expect(extendedOf({ layer: 1, shapes: { A: 1, X: 2, G: 3 } })).toBe('GX');
	});

	it('fills a chart from a layer’s drawings in order', () => {
		const chart: MouthChart = { layer: 1, shapes: { B: 7 } };
		autoChart(chart, [5, 7, 8, 9, 0, 11]);
		expect(chart.shapes).toEqual({ A: 5, B: 7, C: 8, D: 9, E: 11 });
	});

	it('writes the mouths onto the frames, leaving the rest', () => {
		const t = [
			{ drawing: 1, length: 2 },
			{ drawing: 2, length: 6 }
		];
		const chart: MouthChart = { layer: 1, shapes: { A: 10, D: 13 } };
		writeMouths(t, 2, ['A', 'A', 'D', 'D'], chart);
		expect(spell(t)).toEqual([1, 1, 10, 10, 13, 13, 2, 2]);
		// past the end, the track grows
		writeMouths(t, 9, ['D'], chart);
		expect(spell(t)).toEqual([1, 1, 10, 10, 13, 13, 2, 2, 0, 13]);
	});

	it('reads the dialogue notes as one line', () => {
		expect(dialogueText({ 8: 'world', 2: 'hel-', 4: 'lo', 30: 'later' }, 0, 24)).toBe(
			'hello world'
		);
	});

	it('keeps mouth charts (and their unshown drawings) in the file', () => {
		const tl = fromFile(
			[{ name: 'S', cuts: [{ name: 'C', tracks: { '3': [{ drawing: 'D5', length: 1 }] } }] }],
			new Map([[3, 30]]),
			[],
			[{ layer: 3, shapes: { A: 'D5', X: 'D6', Q: 'D7' } }]
		);
		expect(tl.mouths).toEqual([{ layer: 30, shapes: { A: 5, X: 6 } }]);
		expect(drawingsIn(tl).sort()).toEqual([5, 6]);
		expect(layerOfDrawing(tl, 6)).toBe(30);
		expect(mouthsToFile(tl, [30])).toEqual([{ layer: 30, shapes: { A: 'D5', X: 'D6' } }]);
		expect(mouthsToFile(tl, [1])).toEqual([]);
	});
});
