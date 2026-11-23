import { describe, expect, it } from 'vitest';
import {
	fromUnits,
	paintSize,
	panAfterZoom,
	rulerTicks,
	showLength,
	toUnits,
	unitPx,
	wheelZoomStep
} from './view-prefs';

describe('units', () => {
	it('turn pixels into centimetres and inches at the chosen ppi', () => {
		expect(unitPx('px', 300)).toBe(1);
		expect(unitPx('in', 300)).toBe(300);
		expect(unitPx('cm', 254)).toBeCloseTo(100);
		expect(toUnits(600, 'in', 300)).toBe(2);
		expect(toUnits(1181, 'cm', 300)).toBeCloseTo(10, 1);
		expect(fromUnits(2, 'in', 300)).toBe(600);
		expect(fromUnits(10, 'cm', 300)).toBe(1181);
		expect(fromUnits(1920, 'px', 72)).toBe(1920);
	});

	it('read well', () => {
		expect(showLength(1920.4, 'px', 300)).toBe('1920 px');
		expect(showLength(600, 'in', 300)).toBe('2 in');
		expect(showLength(1920, 'cm', 300)).toBe('16.3 cm');
		expect(showLength(100, 'cm', 300)).toBe('0.85 cm');
	});
});

describe('ruler ticks', () => {
	it('number pixels on a 1-2-5 step at least the gap apart', () => {
		// canvas pixel 0 at screen x 100, 1 canvas px = 0.5 screen px
		const t = rulerTicks(100, 0.5, 600, 'px', 300, 60);
		const labelled = t.filter((x) => x.label);
		// 60 screen px = 120 canvas px → steps of 200 px (100 screen px apart)
		expect(labelled.map((x) => x.label).slice(0, 3)).toEqual(['-200', '0', '200']);
		expect(labelled.find((x) => x.label === '0')!.at).toBe(100);
		expect(labelled.find((x) => x.label === '200')!.at).toBe(200);
		// plain ticks between
		expect(t.length).toBeGreaterThan(labelled.length * 3);
		expect(t.every((x) => x.at >= 0 && x.at <= 600)).toBe(true);
	});

	it('count centimetres and inches', () => {
		// 300 ppi, fitted at 0.25: an inch is 75 screen px
		const inches = rulerTicks(0, 0.25, 400, 'in', 300, 60).filter((x) => x.label);
		expect(inches.map((x) => [x.label, x.at])).toEqual([
			['0', 0],
			['1', 75],
			['2', 150],
			['3', 225],
			['4', 300],
			['5', 375]
		]);
		const cm = rulerTicks(0, 1, 400, 'cm', 300, 60).filter((x) => x.label);
		// a centimetre is ~118 screen px here: every one numbered
		expect(cm.map((x) => x.label)).toEqual(['0', '1', '2', '3']);
		const fine = rulerTicks(0, 4, 400, 'cm', 300, 60).filter((x) => x.label);
		expect(fine[1].label).toBe('0.2');
	});

	it('run backwards when the view is mirrored', () => {
		const t = rulerTicks(500, -1, 500, 'px', 300, 60).filter((x) => x.label);
		const zero = t.find((x) => x.label === '0')!;
		const next = t.find((x) => x.label === '100')!;
		expect(zero.at).toBe(500);
		expect(next.at).toBe(400);
	});

	it('give up quietly on nonsense', () => {
		expect(rulerTicks(0, 0, 500, 'px', 300)).toEqual([]);
		expect(rulerTicks(0, 1, 0, 'px', 300)).toEqual([]);
	});
});

describe('zoom', () => {
	it('keeps the point under the pointer still', () => {
		// a point 200 px right of the middle, the canvas panned 50 px: zoom ×2
		const pan = panAfterZoom(50, 2, 200);
		// before: the canvas point under the pointer was (200 - 50) / z from the canvas
		// middle; after, (200 - pan) / (2z): the same point
		expect((200 - pan) / 2).toBe(200 - 50);
		// towards the middle: the pan scales
		expect(panAfterZoom(50, 2)).toBe(100);
	});

	it('steps per wheel notch, scaled by the speed', () => {
		const notch = wheelZoomStep(-100, 0, 1);
		expect(notch).toBeCloseTo(Math.log(1.1));
		expect(wheelZoomStep(100, 0, 1)).toBeCloseTo(-Math.log(1.1));
		expect(wheelZoomStep(-100, 0, 2)).toBeCloseTo(2 * notch);
		// lines (Firefox) count as a notch
		expect(wheelZoomStep(-3, 1, 1)).toBeCloseTo(Math.log(1.1));
		// a trackpad's few pixels: a small step
		expect(Math.abs(wheelZoomStep(-4.5, 0, 1))).toBeLessThan(0.05);
	});

	it('never moves more than a notch per wheel event', () => {
		// WebKitGTK's notch comes as a fraction (e.g. 173.33 px): still one notch, not ×4
		expect(wheelZoomStep(173.33, 0, 1)).toBeCloseTo(-Math.log(1.1));
		expect(wheelZoomStep(39.5, 0, 1)).toBeGreaterThanOrEqual(-Math.log(1.1));
		// a big flick: one notch
		expect(wheelZoomStep(2000, 0, 1)).toBeCloseTo(-Math.log(1.1));
	});
});

describe('brush size', () => {
	it('is capped at the largest size', () => {
		expect(paintSize(5, 1, false, 3)).toBe(3);
		expect(paintSize(2, 4, false)).toBe(2);
	});

	it('keeps its size on screen when following the zoom', () => {
		// zoomed in ×4: a quarter of the size on the canvas, the same on screen
		expect(paintSize(2, 4, true)).toBe(0.5);
		expect(paintSize(2, 0.5, true)).toBe(4);
		expect(paintSize(8, 2, true, 6)).toBe(3);
	});
});
