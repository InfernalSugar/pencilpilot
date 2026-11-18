/**
 * 👍/👎 on tips (M8-T03): which tips help and which don't. Guests can vote
 * (rate-limited by IP); signed-in votes are de-duplicated per attempt and
 * deleted with the account.
 */
import type { DB } from './db';
import { RateLimiter } from './ratelimit';

export const votesByIp = new RateLimiter(60, 10_000);

export interface VoteIn {
	ruleId: string;
	vote: 1 | -1;
	category?: string;
	score?: number;
	attemptAt?: number;
}

export function cleanVote(v: unknown): VoteIn | null {
	if (!v || typeof v !== 'object') return null;
	const x = v as Record<string, unknown>;
	if (typeof x.ruleId !== 'string' || !/^[a-z0-9-]{1,60}$/.test(x.ruleId)) return null;
	if (x.vote !== 1 && x.vote !== -1) return null;
	const out: VoteIn = { ruleId: x.ruleId, vote: x.vote };
	if (typeof x.category === 'string' && /^[a-z-]{1,30}$/.test(x.category))
		out.category = x.category;
	if (typeof x.score === 'number' && x.score >= 1 && x.score <= 10) out.score = x.score;
	if (typeof x.attemptAt === 'number' && Number.isFinite(x.attemptAt)) out.attemptAt = x.attemptAt;
	return out;
}

export function recordVote(db: DB, v: VoteIn, userId: string | null, now = Date.now()) {
	if (userId && v.attemptAt !== undefined) {
		// changing your mind on the same tip for the same drawing replaces the vote
		db.prepare('DELETE FROM tip_votes WHERE user_id = ? AND attempt_at = ? AND rule_id = ?').run(
			userId,
			v.attemptAt,
			v.ruleId
		);
	}
	db.prepare(
		'INSERT INTO tip_votes (rule_id, vote, category, score, attempt_at, user_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)'
	).run(v.ruleId, v.vote, v.category ?? '', v.score ?? null, v.attemptAt ?? null, userId, now);
}

export interface RuleStats {
	ruleId: string;
	up: number;
	down: number;
	/** Wilson lower bound of the 👍 share (95%): ranks rules fairly with few votes */
	helpful: number;
}

export function voteStats(db: DB): RuleStats[] {
	const rows = db
		.prepare(
			`SELECT rule_id AS ruleId, SUM(vote = 1) AS up, SUM(vote = -1) AS down
			 FROM tip_votes GROUP BY rule_id`
		)
		.all() as { ruleId: string; up: number; down: number }[];
	return rows
		.map((r) => ({ ...r, helpful: wilson(r.up, r.up + r.down) }))
		.sort((a, b) => a.helpful - b.helpful);
}

/** Wilson score interval lower bound. */
export function wilson(pos: number, n: number, z = 1.96): number {
	if (!n) return 0;
	const p = pos / n;
	return (
		(p + (z * z) / (2 * n) - z * Math.sqrt((p * (1 - p) + (z * z) / (4 * n)) / n)) /
		(1 + (z * z) / n)
	);
}
