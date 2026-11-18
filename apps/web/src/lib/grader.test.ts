import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { beforeAll, describe, expect, it } from 'vitest';
import { initSync, grade, defaultWeights } from 'pencilpilot-grader-wasm';
import { measurement, toGray, type Grade } from './grader';

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

	it('rejects unknown categories', () => {
		expect(() => grade(W, H, disc(true), disc(false), { category: 'nope' })).toThrow();
		expect(defaultWeights('real-life').tone).toBeGreaterThan(defaultWeights('anime').tone);
	});
});
