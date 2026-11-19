/**
 * Daily challenge scores: the first scored try per signed-in person per day.
 * Scores come from the browser (grading runs there), so they're trusted only
 * as far as a practice comparison needs.
 */
import { dayOf } from '../daily';
import type { DB } from './db';
import { RateLimiter } from './ratelimit';

export const dailyByUser = new RateLimiter(10, 60_000);

/** Record a day's score; false if this person already has one for that day. */
export function recordDaily(
	db: DB,
	userId: string,
	d: { day: string; refId: string; score: number; attemptAt: number | null },
	now = Date.now()
): boolean {
	const r = db
		.prepare(
			`INSERT OR IGNORE INTO daily_scores (day, user_id, ref_id, score, attempt_at, created_at)
			 VALUES (?, ?, ?, ?, ?, ?)`
		)
		.run(d.day, userId, d.refId, Math.round(d.score * 100) / 100, d.attemptAt, now);
	return r.changes > 0;
}

export function dayScores(db: DB, day: string): number[] {
	return (
		db.prepare('SELECT score FROM daily_scores WHERE day = ? ORDER BY score').all(day) as {
			score: number;
		}[]
	).map((r) => r.score);
}

export function myDaily(db: DB, userId: string, day: string): number | null {
	const r = db
		.prepare('SELECT score FROM daily_scores WHERE day = ? AND user_id = ?')
		.get(day, userId) as { score: number } | undefined;
	return r?.score ?? null;
}

/** Days in a row with a daily challenge, ending today (or yesterday, if today isn't done yet). */
export function dailyStreak(db: DB, userId: string, today = dayOf()): number {
	const days = new Set(
		(
			db
				.prepare('SELECT day FROM daily_scores WHERE user_id = ? ORDER BY day DESC LIMIT 400')
				.all(userId) as { day: string }[]
		).map((r) => r.day)
	);
	const prev = (d: string) => dayOf(new Date(Date.parse(`${d}T00:00:00Z`) - 86_400_000));
	let d = days.has(today) ? today : prev(today);
	let n = 0;
	while (days.has(d)) {
		n++;
		d = prev(d);
	}
	return n;
}

/** Scores bucketed by whole points, 1–10 (index 0 = scores below 2). */
export function histogram(scores: number[]): number[] {
	const h = Array(10).fill(0);
	for (const s of scores) h[Math.min(9, Math.max(0, Math.floor(s) - 1))]++;
	return h;
}
