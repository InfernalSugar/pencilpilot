import { describe, expect, it } from 'vitest';
import { dailyPool, dailyReference, dayOf, isDay, msToNextDay, percentile } from './daily';
import type { Reference } from './references';

const ref = (id: string, tier: number) => ({ id, tier, category: 'animals' }) as Reference;
const all = [ref('a', 1), ref('b', 2), ref('c', 3), ref('d', 1), ref('e', 4), ref('f', 2)];

describe('daily challenge', () => {
	it('uses calm pictures, the same for everyone on a day, without repeats in a cycle', () => {
		expect(dailyPool(all).map((r) => r.id)).toEqual(['a', 'b', 'd', 'f']);
		expect(dailyReference('2026-11-19', all)).toBe(dailyReference('2026-11-19', all));
		const week = ['2026-01-01', '2026-01-02', '2026-01-03', '2026-01-04'].map(
			(d) => dailyReference(d, all)!.id
		);
		expect(new Set(week).size).toBe(4);
		expect(dailyReference('nope', all)).toBeUndefined();
		expect(dailyReference('2026-02-30', all)).toBeUndefined();
	});

	it('days, countdown, percentile', () => {
		expect(dayOf(new Date('2026-11-19T23:59:00Z'))).toBe('2026-11-19');
		expect(isDay('2026-11-19')).toBe(true);
		expect(isDay('2026-13-01')).toBe(false);
		expect(msToNextDay(new Date('2026-11-19T23:00:00Z'))).toBe(3_600_000);
		expect(percentile(7, [7, 5, 6, 8, 9])).toBe(0.5);
		expect(percentile(7, [7])).toBe(1);
		expect(percentile(5, [5, 5, 3])).toBe(0.75);
	});
});
