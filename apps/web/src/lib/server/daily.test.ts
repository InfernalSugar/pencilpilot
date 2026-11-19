import { describe, expect, it } from 'vitest';
import { openDb } from './db';
import { dailyStreak, dayScores, histogram, myDaily, recordDaily } from './daily';

const setup = () => {
	const db = openDb(':memory:');
	for (const u of ['ann', 'bob'])
		db.prepare('INSERT INTO users (id, email, created_at) VALUES (?, ?, 0)').run(u, `${u}@x.test`);
	return db;
};
const entry = (day: string, score = 7) => ({ day, refId: 'r', score, attemptAt: 1 });

describe('daily scores', () => {
	it('counts the first try only and reports the day', () => {
		const db = setup();
		expect(recordDaily(db, 'ann', entry('2026-11-19', 7.234))).toBe(true);
		expect(recordDaily(db, 'ann', entry('2026-11-19', 9))).toBe(false);
		recordDaily(db, 'bob', entry('2026-11-19', 4));
		expect(dayScores(db, '2026-11-19')).toEqual([4, 7.23]);
		expect(myDaily(db, 'ann', '2026-11-19')).toBe(7.23);
		expect(myDaily(db, 'ann', '2026-11-18')).toBeNull();
		expect(histogram([1, 4, 7.23, 10])).toEqual([1, 0, 0, 1, 0, 0, 1, 0, 0, 1]);
	});

	it('streaks run up to today, or yesterday while today is still open', () => {
		const db = setup();
		for (const d of ['2026-11-15', '2026-11-17', '2026-11-18']) recordDaily(db, 'ann', entry(d));
		expect(dailyStreak(db, 'ann', '2026-11-19')).toBe(2);
		recordDaily(db, 'ann', entry('2026-11-19'));
		expect(dailyStreak(db, 'ann', '2026-11-19')).toBe(3);
		expect(dailyStreak(db, 'ann', '2026-11-21')).toBe(0);
		expect(dailyStreak(db, 'bob', '2026-11-19')).toBe(0);
	});
});
