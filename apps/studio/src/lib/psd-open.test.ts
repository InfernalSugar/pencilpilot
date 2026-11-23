import { describe, expect, it } from 'vitest';
import { psdStem, psdToDoc, tooBig } from './psd-open';

describe('opening a Photoshop file', () => {
	it('make its layers and groups a project, the top layer active', () => {
		const px = (v: number) => new Uint8Array(2 * 2 * 4).fill(v);
		const doc = psdToDoc(
			{
				width: 2,
				height: 2,
				groups: [{ name: 'Hero', opacity: 0.5, visible: true }],
				layers: [
					{
						name: 'Paper',
						rgba: px(255),
						opacity: 1,
						blend: 'normal',
						visible: true,
						clip: true,
						group: 0
					},
					{
						name: 'Line',
						rgba: px(9),
						opacity: 0.8,
						blend: 'multiply',
						visible: false,
						clip: true,
						group: 1,
						mask: new Uint8Array(4).fill(255)
					}
				],
				notes: []
			},
			24
		);
		expect(doc.groups).toEqual([{ id: 1, name: 'Hero', opacity: 0.5, visible: true }]);
		expect(doc.layers.map((l) => [l.id, l.name, l.group, l.clip, l.blend, l.visible])).toEqual([
			[1, 'Paper', 0, false, 'normal', true],
			[2, 'Line', 1, true, 'multiply', false]
		]);
		expect(doc.layers[1].mask).toHaveLength(4);
		expect(doc.layers[0].mask).toBeUndefined();
		expect(doc.activeLayer).toBe(2);
		expect([doc.width, doc.height, doc.fps]).toEqual([2, 2, 24]);
	});

	it('give a file with nothing to keep an empty layer', () => {
		const doc = psdToDoc({ width: 3, height: 1, groups: [], layers: [], notes: [] }, 12);
		expect(doc.layers).toHaveLength(1);
		expect(doc.layers[0].rgba).toHaveLength(12);
	});

	it('refuse pictures bigger than a canvas can be', () => {
		expect(tooBig(4000, 3000)).toBe('');
		expect(tooBig(9000, 100)).toMatch(/9000 × 100 px/);
		expect(tooBig(8000, 8000)).not.toBe('');
	});

	it('name the project after the file', () => {
		expect(psdStem('Hero sheet.PSD')).toBe('Hero sheet');
		expect(psdStem('big.psb')).toBe('big');
	});
});
