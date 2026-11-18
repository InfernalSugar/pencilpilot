import { describe, expect, it } from 'vitest';
import { openDb } from './db';
import {
	createPasswordUser,
	createSession,
	findUserByEmail,
	generateToken,
	passwordProblem,
	upsertGoogleUser,
	validateSession,
	verifyPassword,
	deleteUser,
	SESSION_DAYS
} from './auth';
import { RateLimiter } from './ratelimit';
import { cleanAttempt, snapshot, sync } from './progress-store';

const DAY = 86400000;

describe('accounts', () => {
	it('creates password users with hashed passwords', async () => {
		const db = openDb(':memory:');
		const id = await createPasswordUser(db, '  Ali@Example.com ', 'correct horse', 'Ali');
		const u = findUserByEmail(db, 'ali@example.com')!;
		expect(u.id).toBe(id);
		expect(u.password_hash).not.toContain('correct horse');
		expect(await verifyPassword(u.password_hash!, 'correct horse')).toBe(true);
		expect(await verifyPassword(u.password_hash!, 'wrong')).toBe(false);
		await expect(createPasswordUser(db, 'ali@example.com', 'another pw')).rejects.toThrow();
	});

	it('sessions validate, slide and expire; only the hash is stored', async () => {
		const db = openDb(':memory:');
		const id = await createPasswordUser(db, 'a@b.co', 'longenough');
		const token = generateToken();
		const t0 = Date.now();
		const s = createSession(db, token, id, t0);
		expect(s.id).not.toBe(token);
		expect(validateSession(db, token, t0)!.user.id).toBe(id);
		expect(validateSession(db, 'nope', t0)).toBeNull();
		// past half-life → extended
		const later = t0 + (SESSION_DAYS - 10) * DAY;
		expect(validateSession(db, token, later)!.session.expiresAt).toBe(later + SESSION_DAYS * DAY);
		// expired → gone
		expect(validateSession(db, token, later + (SESSION_DAYS + 1) * DAY)).toBeNull();
		expect(validateSession(db, token, t0)).toBeNull();
	});

	it('google sign-in links verified emails and never unverified ones', async () => {
		const db = openDb(':memory:');
		const id = await createPasswordUser(db, 'a@b.co', 'longenough');
		expect(upsertGoogleUser(db, { sub: 'g1', email: 'a@b.co', emailVerified: false })).not.toBe(id);
		expect(upsertGoogleUser(db, { sub: 'g2', email: 'A@b.co', emailVerified: true })).toBe(id);
		expect(upsertGoogleUser(db, { sub: 'g2' })).toBe(id);
	});

	it('password rules', () => {
		expect(passwordProblem('short')).toMatch(/8/);
		expect(passwordProblem('password1')).toMatch(/common/);
		expect(passwordProblem('me@x.io', 'me@x.io')).toMatch(/at least|email/);
		expect(passwordProblem('a good long one')).toBeNull();
	});

	it('rate limiter refills over time', () => {
		const r = new RateLimiter(2, 1000);
		expect([r.take('k', 0), r.take('k', 0), r.take('k', 0)]).toEqual([true, true, false]);
		expect(r.take('k', 1000)).toBe(true);
	});
});

describe('progress sync', () => {
	const now = Date.UTC(2026, 10, 19);
	const att = (at: number, extra = {}) => ({
		category: 'animals',
		topic: null,
		tier: 1,
		score: 7,
		at,
		...extra
	});

	it('cleans attempts and drops bad ones', () => {
		expect(
			cleanAttempt(att(now, { evil: '<script>', skills: { outline: 0.5, x: 9 } }), now)
		).toEqual({
			...att(now),
			skills: { outline: 0.5 }
		});
		expect(cleanAttempt(att(now, { score: 99 }), now)).toBeNull();
		expect(cleanAttempt(att(0), now)).toBeNull();
		expect(cleanAttempt('nope', now)).toBeNull();
	});

	it('merges two devices and deletes with the account', async () => {
		const db = openDb(':memory:');
		const id = await createPasswordUser(db, 'a@b.co', 'longenough');
		sync(db, id, { attempts: [att(now - 3000), att(now - 2000)], drills: [], thumbs: {} }, now);
		const s = sync(
			db,
			id,
			{
				attempts: [att(now - 2000), att(now - 1000)],
				drills: [{ id: 'ghosted-lines', at: now - 500, avg: 8.5, rounds: [8, 9] }],
				thumbs: { [String(now - 1000)]: 'data:image/jpeg;base64,AAAA', bad: 'x' }
			},
			now
		);
		expect(s.attempts.map((a) => a.at)).toEqual([now - 3000, now - 2000, now - 1000]);
		expect(s.drills).toHaveLength(1);
		expect(Object.keys(s.thumbs)).toEqual([String(now - 1000)]);
		deleteUser(db, id);
		expect(snapshot(db, id)).toEqual({ attempts: [], drills: [], thumbs: {} });
	});
});

describe('tip votes', async () => {
	const { cleanVote, recordVote, voteStats, wilson } = await import('./tip-votes');
	it('validates, de-duplicates per signed-in attempt and ranks by Wilson bound', async () => {
		const db = openDb(':memory:');
		const id = await createPasswordUser(db, 'v@b.co', 'longenough');
		expect(cleanVote({ ruleId: 'Bad Id!', vote: 1 })).toBeNull();
		expect(cleanVote({ ruleId: 'too-big', vote: 2 })).toBeNull();
		const v = cleanVote({ ruleId: 'too-big', vote: -1, attemptAt: 5, category: 'animals' })!;
		recordVote(db, v, id);
		recordVote(db, { ...v, vote: 1 }, id); // changed mind → replaces
		recordVote(db, { ruleId: 'too-big', vote: -1 }, null); // guest
		for (let i = 0; i < 8; i++) recordVote(db, { ruleId: 'shaky-lines', vote: 1 }, null);
		const s = voteStats(db);
		expect(s.find((r) => r.ruleId === 'too-big')).toMatchObject({ up: 1, down: 1 });
		expect(s[0].ruleId).toBe('too-big'); // least helpful first
		expect(wilson(0, 0)).toBe(0);
		expect(wilson(10, 10)).toBeGreaterThan(wilson(1, 1));
		deleteUser(db, id);
		expect(voteStats(db).find((r) => r.ruleId === 'too-big')).toMatchObject({ up: 0, down: 1 });
	});
});
