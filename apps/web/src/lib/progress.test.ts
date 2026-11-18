import { describe, expect, it } from 'vitest';
import {
	dailyScores,
	recommendedTier,
	recurringMistakes,
	skillAverages,
	streakDays,
	suggestTopic,
	type Attempt
} from './progress';

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

const DAY = 24 * 3600 * 1000;
const NOW = new Date(2026, 10, 19, 15).getTime();
const at = (daysAgo: number, extra: Partial<Attempt> = {}): Attempt => ({
	category: 'animals',
	topic: 'pets',
	tier: 1,
	score: 6,
	at: NOW - daysAgo * DAY,
	...extra
});

describe('progress analytics', () => {
	it('streaks count consecutive days and survive until midnight', () => {
		expect(streakDays([], NOW)).toBe(0);
		expect(streakDays([at(0), at(1), at(2), at(4)], NOW)).toBe(3);
		expect(streakDays([at(1), at(2)], NOW)).toBe(2); // nothing today yet
		expect(streakDays([at(2)], NOW)).toBe(0);
	});
	it('spots recurring mistakes in the last five drawings', () => {
		const h = [
			at(5, { fixes: ['too-big'] }),
			at(4, { fixes: ['too-big', 'shaky-lines'] }),
			at(3, { fixes: ['too-big'] }),
			at(2, { fixes: ['shaky-lines'] }),
			at(1, { fixes: ['too-big', 'shaky-lines'] }),
			at(0, { fixes: [] })
		];
		expect(recurringMistakes(h)).toEqual([
			{ ruleId: 'too-big', count: 3, window: 5 },
			{ ruleId: 'shaky-lines', count: 3, window: 5 }
		]);
		expect(recurringMistakes(h.slice(0, 2))).toEqual([]);
	});
	it('averages skills and groups scores by day', () => {
		const h = [
			at(1, { skills: { outline: 0.4 }, score: 4 }),
			at(1, { skills: { outline: 0.8 }, score: 8 }),
			at(0, { score: 9 })
		];
		expect(skillAverages(h).outline).toBeCloseTo(0.6);
		const d = dailyScores(h, 7, NOW);
		expect(d.map((x) => [x.avg, x.n])).toEqual([
			[6, 2],
			[9, 1]
		]);
	});
	it('surprise me favours weak and untried topics', () => {
		const h = [at(1, { topic: 'pets', score: 9.5 }), at(1, { topic: 'birds', score: 2 })];
		const picks = { pets: 0, birds: 0, insects: 0 } as Record<string, number>;
		let seed = 1;
		const rand = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
		for (let i = 0; i < 3000; i++)
			picks[suggestTopic('animals', ['pets', 'birds', 'insects'], h, rand)!]++;
		expect(picks.birds).toBeGreaterThan(picks.pets * 3);
		expect(picks.insects).toBeGreaterThan(picks.pets);
	});
});
