import { error, json } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import {
	dailyByUser,
	dailyStreak,
	dayScores,
	histogram,
	myDaily,
	recordDaily
} from '#lib/server/daily.js';
import { dailyReference, dayOf, isDay } from '#lib/daily.js';
import type { RequestHandler } from './$types';

/** How the day is going: player count, score histogram, and your own score + streak. */
export const GET: RequestHandler = ({ url, locals }) => {
	const day = url.searchParams.get('day') ?? dayOf();
	if (!isDay(day)) error(400, 'Bad day');
	const db = getDb();
	const scores = dayScores(db, day);
	return json({
		day,
		count: scores.length,
		histogram: histogram(scores),
		scores,
		mine: locals.user ? myDaily(db, locals.user.id, day) : null,
		streak: locals.user ? dailyStreak(db, locals.user.id) : 0
	});
};

/** Count today's challenge (the first try only). */
export const POST: RequestHandler = async ({ request, locals }) => {
	if (!locals.user) error(401, 'Sign in to be counted');
	if (!dailyByUser.take(locals.user.id)) error(429, 'Slow down');
	let body: { day?: unknown; refId?: unknown; score?: unknown; attemptAt?: unknown };
	try {
		body = await request.json();
	} catch {
		error(400, 'Bad JSON');
	}
	const day = body.day;
	// today only (UTC); an hour of grace just after midnight for drawings started yesterday
	const today = dayOf();
	const yesterday = dayOf(new Date(Date.now() - 3_600_000));
	if (!isDay(day) || (day !== today && day !== yesterday)) error(400, 'That challenge is closed');
	const ref = dailyReference(day);
	if (!ref || body.refId !== ref.id) error(400, 'Not that day’s picture');
	const score = Number(body.score);
	if (!Number.isFinite(score) || score < 1 || score > 10) error(400, 'Bad score');
	const attemptAt = Number.isInteger(body.attemptAt) ? (body.attemptAt as number) : null;
	const db = getDb();
	const counted = recordDaily(db, locals.user.id, { day, refId: ref.id, score, attemptAt });
	return json({
		counted,
		mine: myDaily(db, locals.user.id, day),
		streak: dailyStreak(db, locals.user.id)
	});
};
