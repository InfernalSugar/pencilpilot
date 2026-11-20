import { describe, expect, it } from 'vitest';
import { timeline } from './replay';
import type { Stroke } from '@pencilpilot/paint/strokes';

const s = (ts: number[]): Stroke => ({
	brushId: 'b',
	eraser: false,
	points: ts.map((t, i) => ({ x: i, y: 0, pressure: 0.5, t }))
});

describe('replay timeline', () => {
	it('orders strokes by time and shortens long pauses', () => {
		const tl = timeline([s([5000, 5100]), s([0, 100, 200]), s([])], 600);
		expect(tl.strokes.map((x) => x.points[0].t)).toEqual([0, 5000]);
		// 0,100,200 then a 4.8 s pause shortened to 0.6 s, then +100
		expect(tl.times).toEqual([
			[0, 100, 200],
			[800, 900]
		]);
		expect(tl.ends).toEqual([200, 900]);
		expect(tl.duration).toBe(900);
	});
});
