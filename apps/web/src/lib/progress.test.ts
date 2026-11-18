import { describe, expect, it } from 'vitest';
import { recommendedTier, type Attempt } from './progress';

const a = (tier: number, score: number, category = 'animals'): Attempt => ({
	category,
	topic: null,
	tier,
	score,
	at: 0
});

describe('recommendedTier', () => {
	it('starts at Beginner', () => {
		expect(recommendedTier('animals', [])).toBe(1);
	});
	it('moves up after three comfortable drawings in a row', () => {
		expect(recommendedTier('animals', [a(1, 8), a(1, 9)])).toBe(1);
		expect(recommendedTier('animals', [a(1, 8), a(1, 9), a(1, 7.5)])).toBe(2);
		expect(recommendedTier('animals', [a(1, 8), a(1, 9), a(1, 6)])).toBe(1);
	});
	it('climbs several tiers and is per category', () => {
		const h = [a(1, 9), a(1, 9), a(1, 9), a(2, 8), a(2, 8), a(2, 8)];
		expect(recommendedTier('animals', h)).toBe(3);
		expect(recommendedTier('anime', h)).toBe(1);
	});
	it('never goes past Hard', () => {
		const h = [1, 2, 3, 4].flatMap((t) => [a(t, 10), a(t, 10), a(t, 10)]);
		expect(recommendedTier('animals', h)).toBe(4);
	});
});
