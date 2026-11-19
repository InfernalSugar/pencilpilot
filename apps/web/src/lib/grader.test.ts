import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { beforeAll, describe, expect, it } from 'vitest';
import { initSync, grade, defaultWeights, feedback } from 'pencilpilot-grader-wasm';
import { gradeImages, humanize, measurement, toGray, type Feedback, type Grade } from './grader';

describe('toGray', () => {
	it('maps black, white and transparent pixels', () => {
		const rgba = new Uint8ClampedArray([0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 0]);
		expect(Array.from(toGray(rgba))).toEqual([0, 255, 255]);
	});
});

describe('grader wasm', () => {
	const W = 200;
	const H = 150;

	beforeAll(() => {
		// In node there is no fetch for the .wasm URL: load the bytes directly.
		const require = createRequire(import.meta.url);
		const wasm = require.resolve('pencilpilot-grader-wasm/grader_bg.wasm');
		initSync({ module: readFileSync(wasm) });
	});

	/** White canvas; `ring` draws a circle outline, otherwise a filled shaded disc. */
	function disc(ring: boolean): Uint8Array {
		const px = new Uint8Array(W * H).fill(255);
		for (let y = 0; y < H; y++) {
			for (let x = 0; x < W; x++) {
				const d = Math.hypot(x + 0.5 - 100, y + 0.5 - 75);
				if (ring ? Math.abs(d - 40) < 1.5 : d <= 40) px[y * W + x] = ring ? 20 : 90 + (x - 60);
			}
		}
		return px;
	}

	it('grades a drawing and returns plain camelCase objects', () => {
		const g = grade(W, H, disc(true), disc(false), {
			category: 'anime',
			strokes: [
				{
					brushId: 'pen',
					eraser: false,
					points: [
						{ x: 60, y: 75, pressure: 0.5, t: 0 },
						{ x: 80, y: 60, pressure: 0.5, t: 16 },
						{ x: 100, y: 35, pressure: 0.5, t: 32 }
					]
				}
			]
		}) as Grade;
		expect(g.score).toBeGreaterThanOrEqual(8.5);
		expect(g.category).toBe('anime');
		expect(g.grid.cells).toHaveLength(g.grid.cols * g.grid.rows);
		expect(g.keypoints).toBeNull();
		expect(typeof g.alignment.scale).toBe('number');
		expect(measurement(g, 'outline')).toBeGreaterThan(0.8);
		expect(measurement(g, 'line_confidence')).toBeTypeOf('number');
	});

	it('accepts JSON options and flags blank drawings', () => {
		const g = grade(
			W,
			H,
			new Uint8Array(W * H).fill(255),
			disc(false),
			JSON.stringify({ category: 'real-life' })
		) as Grade;
		expect(g.score).toBe(1);
		expect(g.flags).toContain('blank');
	});

	it('turns measurements into ranked tips with real numbers', () => {
		const fb = feedback(
			[
				{ name: 'scale_ratio', value: 1.4 },
				{ name: 'outline_recall', value: 0.3 },
				{ name: 'line_confidence', value: 0.9 },
				{ name: 'smoothness', value: 0.9 }
			],
			'animals',
			7
		) as Feedback;
		expect(fb.fixes.map((f) => f.id)).toEqual(['too-big', 'missing-parts']);
		expect(fb.fixes[0].title).toBe('Your drawing is 40% too big');
		expect(fb.fixes[0].lesson).toBe('measuring-with-units');
		expect(fb.praise?.skill).toBe('line_confidence');
		expect(humanize('measuring-with-units')).toBe('Measuring with units');
	});

	it('the real grade → feedback path has no unfilled placeholders', () => {
		const g = grade(W, H, disc(true), disc(false), { category: 'animals' }) as Grade;
		const fb = feedback(g.measurements, g.category, 1) as Feedback;
		for (const f of [...fb.fixes, ...(fb.praise ? [fb.praise] : [])]) {
			expect(f.title + f.text).not.toMatch(/[{}]/);
		}
	});

	it('rejects unknown categories', () => {
		expect(() => grade(W, H, disc(true), disc(false), { category: 'nope' })).toThrow();
		expect(defaultWeights('real-life').tone).toBeGreaterThan(defaultWeights('anime').tone);
	});
});

describe('analysis size', () => {
	it('keeps the classic canvas and shrinks big ones to about the same area', async () => {
		const { analysisSize, downscaleGray } = await import('./grader');
		expect(analysisSize(800, 600)).toEqual({ width: 800, height: 600, scale: 1 });
		const a = analysisSize(1600, 1200);
		expect(a).toMatchObject({ width: 800, height: 600 });
		const p = analysisSize(1000, 1414);
		expect(p.width * p.height).toBeGreaterThan(470_000);
		expect(p.width * p.height).toBeLessThan(490_000);
		expect(p.height / p.width).toBeCloseTo(1.414, 2);
		// area averaging: a 2×2 block of 0 and 255 becomes mid grey
		expect([...downscaleGray(new Uint8Array([0, 255, 255, 0]), 2, 2, 1, 1)]).toEqual([128]);
	});

	it('grades canvases bigger than 800 × 600 (scaled down first)', async () => {
		// regression: the scaled options used to carry keypoints: undefined, which the grader rejected
		const [w, h] = [1600, 1200];
		const px = new Uint8Array(w * h).fill(255);
		for (let x = 300; x < 1300; x++) for (let y = 598; y < 602; y++) px[y * w + x] = 0;
		const g = await gradeImages(w, h, px, px, {
			category: 'anime',
			mode: 'sketch',
			strokes: [
				{
					brushId: 'pencil',
					eraser: false,
					points: [
						{ x: 300, y: 600, pressure: 0.5, t: 0 },
						{ x: 1300, y: 600, pressure: 0.5, t: 400 }
					]
				}
			]
		});
		expect(g.score).toBeGreaterThanOrEqual(1);
	});
});
