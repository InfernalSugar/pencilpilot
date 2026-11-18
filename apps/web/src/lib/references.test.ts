import { describe, expect, it } from 'vitest';
import { inCategory, pickReference, references, titleOf } from './references';

describe('references', () => {
	it('every approved reference has licence + attribution', () => {
		expect(Array.isArray(references)).toBe(true); // also holds with zero references
		for (const r of references) {
			expect(r.id).toMatch(/^px-\d+$/);
			expect(r.author).not.toBe('');
			expect(r.license).not.toBe('');
			expect(r.licenseUrl).toMatch(/^https:\/\//);
			expect(r.pageUrl).toMatch(/^https:\/\/pixabay\.com\//);
		}
	});

	it('picks from the right category and avoids repeats when possible', () => {
		for (const c of ['anime', 'real-life', 'animals']) {
			const pool = inCategory(c);
			const first = pickReference(c, undefined, () => 0);
			if (!pool.length) {
				expect(first).toBeUndefined();
				continue;
			}
			expect(first!.category).toBe(c);
			if (pool.length > 1) expect(pickReference(c, first!.id, () => 0)!.id).not.toBe(first!.id);
		}
		expect(pickReference('no-such-category')).toBeUndefined();
	});

	it('titles fall back to tags', () => {
		const r = { ...references[0], title: '', tags: ['sleepy', 'cat', 'pet'] };
		expect(titleOf(r)).toBe('Sleepy cat');
	});
});
