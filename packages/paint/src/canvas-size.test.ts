import { describe, expect, it } from 'vitest';
import { checkSize, sizeNote, CANVAS_PRESETS } from './canvas-size';

describe('canvas sizes', () => {
	it('allows up to 8K, refuses bigger', () => {
		expect(checkSize(3840, 2160).ok).toBe(true);
		expect(checkSize(7680, 4320).ok).toBe(true);
		expect(checkSize(4320, 7680).ok).toBe(true);
		expect(checkSize(8193, 4000).ok).toBe(false);
		expect(checkSize(8192, 8192).ok).toBe(false); // over the 8K area
		expect(checkSize(255, 600).ok).toBe(false);
	});
	it('every preset is valid', () => {
		for (const p of CANVAS_PRESETS) expect(checkSize(p.width, p.height).ok, p.id).toBe(true);
	});
	it('warns about memory on big canvases only', () => {
		expect(sizeNote({ width: 800, height: 600 })).toBeNull();
		expect(sizeNote({ width: 1920, height: 1080 })).toBeNull();
		expect(sizeNote({ width: 2560, height: 1440 })).toMatch(/Big canvas/);
		expect(sizeNote({ width: 7680, height: 4320 })).toMatch(/Very big/);
	});
});
