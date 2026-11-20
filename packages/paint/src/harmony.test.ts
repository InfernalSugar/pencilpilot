import { describe, expect, it } from 'vitest';
import { harmony, HARMONIES } from './harmony';

describe('colour harmony', () => {
	it('complementary is the opposite hue', () => {
		expect(harmony('#ff0000', 'complementary')).toEqual(['#ff0000', '#00ffff']);
	});

	it('triadic and split are spread around the wheel', () => {
		expect(harmony('#ff0000', 'triadic')).toEqual(['#ff0000', '#00ff00', '#0000ff']);
		expect(harmony('#ff0000', 'split')).toEqual(['#ff0000', '#00ff80', '#0080ff']);
		expect(harmony('#ff0000', 'analogous')).toEqual(['#ff0000', '#ff0080', '#ff8000']);
	});

	it('shades keep the hue, lighter then darker', () => {
		const [, ...s] = harmony('#3366cc', 'shades');
		expect(s).toHaveLength(4);
		const lum = (c: string) => {
			const n = parseInt(c.slice(1), 16);
			return ((n >> 16) & 255) + ((n >> 8) & 255) + (n & 255);
		};
		const l = s.map(lum);
		expect(l[0]).toBeGreaterThan(l[1]);
		expect(l[1]).toBeGreaterThan(lum('#3366cc'));
		expect(l[2]).toBeLessThan(lum('#3366cc'));
		expect(l[3]).toBeLessThan(l[2]);
	});

	it('greys have no hue to turn: no repeats', () => {
		for (const h of HARMONIES)
			for (const c of harmony('#808080', h.id))
				expect(harmony('#808080', h.id).filter((x) => x === c)).toHaveLength(1);
		expect(harmony('#808080', 'complementary')).toEqual(['#808080']);
	});
});
