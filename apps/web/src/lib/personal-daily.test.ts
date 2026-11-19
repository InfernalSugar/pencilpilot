import { describe, expect, it } from 'vitest';
import { categoryAverage, personalChallenge, personalDone, personalStreak } from './personal-daily';
import type { Attempt } from './progress';
import type { Reference } from './references';

const refs: Reference[] = [];
for (const category of ['anime', 'animals', 'real-life'])
	for (let i = 0; i < 6; i++)
		refs.push({
			id: `${category}-${i}`,
			category,
			tier: 1 + (i % 2),
			topic: i < 3 ? 'faces' : 'bodies'
		} as Reference);

const day = '2026-11-19';
const t = Date.parse(`${day}T12:00:00Z`);
const att = (
	category: string,
	score: number,
	daysAgo: number,
	extra: Partial<Attempt> = {}
): Attempt => ({
	category,
	topic: 'faces',
	tier: 1,
	score,
	at: t - daysAgo * 86_400_000,
	...extra
});

describe('personal daily challenge', () => {
	it('is the same all day for one person, and differs between people', () => {
		const h = [att('anime', 7, 1)];
		const a = personalChallenge(day, 'u:ann', h, refs)!;
		expect(personalChallenge(day, 'u:ann', h, refs)!.ref.id).toBe(a.ref.id);
		const ids = new Set(
			['u:1', 'u:2', 'u:3', 'u:4', 'u:5', 'u:6'].map(
				(k) => personalChallenge(day, k, h, refs)!.ref.id
			)
		);
		expect(ids.size).toBeGreaterThan(1);
	});

	it('mostly follows what someone draws, with some variety', () => {
		const h = [
			...Array.from({ length: 12 }, (_, i) => att('anime', 6, i + 1)),
			att('animals', 6, 3)
		];
		const days = Array.from({ length: 60 }, (_, i) =>
			new Date(t + i * 86_400_000).toISOString().slice(0, 10)
		);
		const cats = days.map((d) => personalChallenge(d, 'u:ann', h, refs, t)!.category);
		const anime = cats.filter((c) => c === 'anime').length;
		expect(anime).toBeGreaterThan(40);
		expect(anime).toBeLessThan(60);
		expect(personalChallenge(day, 'u:ann', h, refs)!.reason).toMatch(/Anime|change/);
	});

	it('starts newcomers somewhere and avoids pictures drawn lately', () => {
		const fresh = personalChallenge(day, 'g:x', [], refs)!;
		expect(fresh.reason).toMatch(/start you off/);
		const recent = refs
			.filter((r) => r.category === 'anime')
			.slice(0, 5)
			.map((r, i) => att('anime', 5, i + 1, { refId: r.id }));
		const c = personalChallenge(day, 'u:ann', recent, refs)!;
		if (c.category === 'anime') expect(c.ref.id).toBe('anime-5');
	});

	it('streaks, done today, earlier average', () => {
		const h = [
			att('anime', 6, 2, { personal: '2026-11-17' }),
			att('anime', 7, 1, { personal: '2026-11-18' }),
			att('anime', 8, 0, { personal: day })
		];
		expect(personalStreak(h, day)).toBe(3);
		expect(personalStreak(h.slice(0, 2), day)).toBe(2);
		expect(personalDone(h, day)?.score).toBe(8);
		expect(categoryAverage(h, 'anime', t - 1000)).toBe(6.5);
		expect(categoryAverage(h, 'animals', t)).toBeNull();
	});
});
