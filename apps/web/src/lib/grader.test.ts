import { describe, expect, it } from 'vitest';
import { toGray } from './grader';

describe('toGray', () => {
	it('maps black, white and transparent pixels', () => {
		const rgba = new Uint8ClampedArray([0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 0]);
		expect(Array.from(toGray(rgba))).toEqual([0, 255, 255]);
	});
});
