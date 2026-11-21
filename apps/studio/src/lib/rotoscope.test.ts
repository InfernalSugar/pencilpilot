import { describe, expect, it } from 'vitest';
import { fitInside, naturalOrder, placeFrames, sequenceName } from './rotoscope';
import { spell } from './frametools';

describe('rotoscope import', () => {
	it('orders numbered files as people number them', () => {
		const names = ['walk10.png', 'walk2.png', 'walk1.png'].map((name) => ({ name }));
		expect(naturalOrder(names).map((f) => f.name)).toEqual([
			'walk1.png',
			'walk2.png',
			'walk10.png'
		]);
	});

	it('fits a picture inside the canvas, centred', () => {
		expect(fitInside(1920, 1080, 1000, 1000)).toEqual({
			x: 0,
			y: 218.75,
			width: 1000,
			height: 562.5
		});
		expect(fitInside(100, 200, 400, 200)).toEqual({ x: 150, y: 0, width: 100, height: 200 });
	});

	it('places frames from the current one, held as asked', () => {
		const t = [{ drawing: 9, length: 3 }];
		placeFrames(t, 1, [4, 5], 2);
		expect(spell(t)).toEqual([9, 4, 4, 5, 5]);
		placeFrames(t, 7, [6]);
		expect(spell(t)).toEqual([9, 4, 4, 5, 5, 0, 0, 6]);
	});

	it('names the layer after the sequence', () => {
		expect(sequenceName('/shots/walk_0001.png')).toBe('walk');
		expect(sequenceName('C:\\v\\Dance take 3.mp4')).toBe('Dance take');
		expect(sequenceName('0001.png')).toBe('Reference');
	});
});
