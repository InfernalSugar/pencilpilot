import { describe, expect, it } from 'vitest';
import {
	addColour,
	addPalette,
	flatColours,
	MAX_COLOURS,
	readPalettes,
	removeColour,
	removePalette,
	renamePalette,
	setColour
} from './palettes';

/** a model sheet: paper, black outlines round flat colours (rects, [x0, y0, x1, y1, rgb]) */
function sheet(w: number, h: number, flats: [number, number, number, number, number[]][]) {
	const px = new Uint8ClampedArray(w * h * 4).fill(255);
	const put = (x: number, y: number, c: number[]) => px.set([...c, 255], (y * w + x) * 4);
	for (const [x0, y0, x1, y1, c] of flats)
		for (let y = y0; y < y1; y++)
			for (let x = x0; x < x1; x++) {
				const edge = x < x0 + 2 || y < y0 + 2 || x >= x1 - 2 || y >= y1 - 2;
				put(x, y, edge ? [16, 16, 20] : c);
			}
	return { px, put };
}

const close = (hex: string, rgb: number[]) => {
	const n = parseInt(hex.slice(1), 16);
	const got = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
	return got.every((v, i) => Math.abs(v - rgb[i]) <= 4);
};

describe('a model sheet’s flat colours', () => {
	it('are found, most used first, without the paper, the lines or the edges', () => {
		const skin = [242, 201, 168];
		const hair = [90, 58, 42];
		const shirt = [70, 110, 200];
		const { px, put } = sheet(120, 80, [
			[4, 4, 70, 70, skin],
			[74, 4, 116, 40, hair],
			[74, 44, 100, 70, shirt]
		]);
		// antialiasing between skin and its line, and a stray speck: not colours of their own
		for (let y = 10; y < 60; y++) put(6, y, [180, 140, 120]);
		put(40, 40, [0, 200, 0]);
		const got = flatColours(px, 120, 80);
		expect(got).toHaveLength(3);
		expect(close(got[0], skin)).toBe(true);
		expect(close(got[1], hair)).toBe(true);
		expect(close(got[2], shirt)).toBe(true);
	});

	it('takes close shades as one, keeps at most `max`, and finds none on blank paper', () => {
		const { px } = sheet(60, 30, [
			[2, 2, 30, 28, [200, 60, 60]],
			[30, 2, 58, 28, [204, 63, 58]]
		]);
		expect(flatColours(px, 60, 30)).toHaveLength(1);
		const many = sheet(
			200,
			20,
			Array.from(
				{ length: 10 },
				(_, i) =>
					[i * 20, 0, i * 20 + 20, 20, [i * 25, 100, 200 - i * 20]] as [
						number,
						number,
						number,
						number,
						number[]
					]
			)
		);
		expect(flatColours(many.px, 200, 20, 4)).toHaveLength(4);
		expect(flatColours(new Uint8ClampedArray(40 * 40 * 4).fill(255), 40, 40)).toEqual([]);
	});
});

describe('palettes', () => {
	it('are read safely: bad colours and entries dropped, names kept short', () => {
		const got = readPalettes([
			{
				name: 'Mina',
				colours: [{ name: 'Skin', hex: '#F2C9A8' }, { name: 'x', hex: 'red' }, null]
			},
			'nope',
			{ colours: [{ hex: '#123456' }] },
			{ name: 'y'.repeat(80), colours: 'no' }
		]);
		expect(got).toEqual([
			{ name: 'Mina', colours: [{ name: 'Skin', hex: '#f2c9a8' }] },
			{ name: 'Palette 2', colours: [{ name: '#123456', hex: '#123456' }] },
			{ name: 'y'.repeat(40), colours: [] }
		]);
		expect(readPalettes(null)).toEqual([]);
	});

	it('are made, named, filled and emptied without changing the ones given', () => {
		const a = addPalette([], 'Mina');
		const b = addColour(a, 0, '#F2C9A8', 'Skin');
		const c = addColour(b, 0, '#5a3a2a');
		expect(c[0].colours).toEqual([
			{ name: 'Skin', hex: '#f2c9a8' },
			{ name: 'Colour 2', hex: '#5a3a2a' }
		]);
		expect(a[0].colours).toEqual([]);
		expect(addColour(c, 0, 'nope')).toBe(c);
		const d = setColour(c, 0, 1, { name: 'Hair', hex: '#4b2f22' });
		expect(d[0].colours[1]).toEqual({ name: 'Hair', hex: '#4b2f22' });
		expect(setColour(d, 0, 1, { hex: 'bad' })[0].colours[1].hex).toBe('#4b2f22');
		expect(renamePalette(d, 0, '  ')[0].name).toBe('Mina');
		expect(renamePalette(d, 0, 'Mina (winter)')[0].name).toBe('Mina (winter)');
		expect(removeColour(d, 0, 0)[0].colours.map((c) => c.name)).toEqual(['Hair']);
		expect(removePalette(d, 0)).toEqual([]);
		let full = addPalette([], 'Full');
		for (let i = 0; i < MAX_COLOURS + 3; i++) full = addColour(full, 0, '#000000');
		expect(full[0].colours).toHaveLength(MAX_COLOURS);
	});
});
