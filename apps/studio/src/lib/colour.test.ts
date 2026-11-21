import { describe, expect, it } from 'vitest';
import { bestMatch, colourFor, lineStartsAfter, pixelsOf, regions, seedIn, walls } from './colour';
import { spell } from './frametools';

/** a w×h line drawing from a picture: '#' is a line */
function lines(rows: string[]) {
	const [w, h] = [rows[0].length, rows.length];
	const rgba = new Uint8Array(w * h * 4);
	rows.forEach((r, y) => [...r].forEach((c, x) => c === '#' && (rgba[(y * w + x) * 4 + 3] = 255)));
	return { rgba, w, h };
}

describe('colour across frames', () => {
	it('cuts a line drawing into shapes, and finds the one that moved', () => {
		// a box on the left, then the same box one pixel to the right
		const a = lines(['########', '#..#....', '#..#....', '########']);
		const b = lines(['########', '.#..#...', '.#..#...', '########']);
		const ra = regions(walls(a.rgba, a.w, a.h, 96, 0), a.w, a.h);
		const box = pixelsOf(ra.labels, ra.labels[1 * a.w + 1]);
		expect(box.length).toBe(4);
		const rb = regions(walls(b.rgba, b.w, b.h, 96, 0), b.w, b.h);
		const m = bestMatch(rb.labels, rb.sizes, box);
		expect(rb.labels[1 * b.w + 2]).toBe(m.label);
		expect(m.score).toBeCloseTo(2 / 6);
		const seed = seedIn(rb.labels, m.label, box, b.w)!;
		expect(rb.labels[seed.y * b.w + seed.x]).toBe(m.label);
	});

	it('closes small gaps by growing the lines', () => {
		const a = lines(['#####', '#...#', '#....', '#####']);
		expect(regions(walls(a.rgba, a.w, a.h, 96, 0), a.w, a.h).sizes.length - 1).toBe(1);
		const grown = walls(a.rgba, a.w, a.h, 96, 1);
		expect(grown[2 * a.w + 3]).toBe(1);
	});

	it('gives colour drawings the line drawings’ timing', () => {
		// lines: A ×2, B ×3; colour: c on frame 0 only
		const line = [
			{ drawing: 1, length: 2 },
			{ drawing: 2, length: 3 }
		];
		const colour = [{ drawing: 10, length: 1 }];
		let n = 20;
		const made: number[] = [];
		const make = (from: number) => (made.push(from), n++);
		expect(colourFor(colour, line, 0, make)).toBe(10);
		expect(spell(colour)).toEqual([10, 10]);
		// the next span: a new colour drawing
		expect(colourFor(colour, line, 3, make)).toBe(20);
		expect(spell(colour)).toEqual([10, 10, 20, 20, 20]);
		expect(made).toEqual([0]);
		// a colour drawing held across both spans is copied for the second
		const held = [{ drawing: 30, length: 5 }];
		expect(colourFor(held, line, 2, make)).toBe(21);
		expect(made).toEqual([0, 30]);
		expect(spell(held)).toEqual([30, 30, 21, 21, 21]);
		expect(colourFor(held, [{ drawing: 0, length: 3 }], 1, make)).toBe(0);
	});

	it('lists where the next line drawings start', () => {
		const line = [
			{ drawing: 1, length: 2 },
			{ drawing: 0, length: 1 },
			{ drawing: 2, length: 1 },
			{ drawing: 3, length: 2 }
		];
		expect(lineStartsAfter(line, 0, 99)).toEqual([3, 4]);
		expect(lineStartsAfter(line, 0, 3)).toEqual([3]);
	});
});
