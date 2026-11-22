import { describe, expect, it } from 'vitest';
import { FILTERS, pickFilter, reseed } from './filters';

describe('filters', () => {
	it('start with settings that show something', () => {
		for (const f of FILTERS) {
			const s = pickFilter(f.id);
			expect(s.kind).toBe(f.id);
			// every slider starts inside its range, and not all at zero
			for (const sl of f.sliders) {
				const v = s[sl.key] * sl.scale;
				expect(v).toBeGreaterThanOrEqual(sl.min);
				expect(v).toBeLessThanOrEqual(sl.max);
			}
			expect(f.sliders.some((sl) => s[sl.key] !== 0)).toBe(true);
		}
	});

	it('keep the pattern and grey switch when picking another', () => {
		const s = pickFilter('glitch', { ...pickFilter('noise'), mono: true, seed: 42 });
		expect(s).toMatchObject({ kind: 'glitch', mono: true, seed: 42, a: 0.4 });
		expect(reseed(s).seed).not.toBe(42);
	});
});
