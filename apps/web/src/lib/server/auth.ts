/**
 * Accounts and sessions (M7). Session tokens follow the Lucia guide: a random
 * token goes in an httpOnly cookie; only its SHA-256 is stored, so a leaked
 * database can't be used to log in. Passwords use Argon2id.
 */
import { hash, verify } from '@node-rs/argon2';
import { sha256 } from '@oslojs/crypto/sha2';
import { encodeBase32LowerCaseNoPadding, encodeHexLowerCase } from '@oslojs/encoding';
import type { Cookies } from '@sveltejs/kit';
import type { DB } from './db';

export interface SessionUser {
	id: string;
	email: string | null;
	name: string;
	hasPassword: boolean;
	hasGoogle: boolean;
}

const DAY = 24 * 3600 * 1000;
export const SESSION_DAYS = 30;
export const SESSION_COOKIE = 'pp_session';

export function generateToken(): string {
	const bytes = new Uint8Array(20);
	crypto.getRandomValues(bytes);
	return encodeBase32LowerCaseNoPadding(bytes);
}

const sessionId = (token: string) => encodeHexLowerCase(sha256(new TextEncoder().encode(token)));

export function newUserId(): string {
	return 'u_' + generateToken().slice(0, 16);
}

export function createSession(db: DB, token: string, userId: string, now = Date.now()) {
	const s = { id: sessionId(token), userId, expiresAt: now + SESSION_DAYS * DAY };
	db.prepare('INSERT INTO sessions (id, user_id, expires_at) VALUES (?, ?, ?)').run(
		s.id,
		s.userId,
		s.expiresAt
	);
	return s;
}

interface Row {
	sid: string;
	expires_at: number;
	id: string;
	email: string | null;
	name: string;
	password_hash: string | null;
	google_id: string | null;
}

/** Returns the session + user, extending the session when it's past half its life. */
export function validateSession(db: DB, token: string, now = Date.now()) {
	const sid = sessionId(token);
	const row = db
		.prepare(
			`SELECT s.id AS sid, s.expires_at, u.id, u.email, u.name, u.password_hash, u.google_id
			 FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.id = ?`
		)
		.get(sid) as Row | undefined;
	if (!row) return null;
	if (now >= row.expires_at) {
		db.prepare('DELETE FROM sessions WHERE id = ?').run(sid);
		return null;
	}
	let expiresAt = row.expires_at;
	if (now >= expiresAt - (SESSION_DAYS / 2) * DAY) {
		expiresAt = now + SESSION_DAYS * DAY;
		db.prepare('UPDATE sessions SET expires_at = ? WHERE id = ?').run(expiresAt, sid);
	}
	return { session: { id: sid, expiresAt }, user: toUser(row) };
}

function toUser(
	r: Pick<Row, 'id' | 'email' | 'name' | 'password_hash' | 'google_id'>
): SessionUser {
	return {
		id: r.id,
		email: r.email,
		name: r.name,
		hasPassword: !!r.password_hash,
		hasGoogle: !!r.google_id
	};
}

export function invalidateSession(db: DB, id: string) {
	db.prepare('DELETE FROM sessions WHERE id = ?').run(id);
}

export function invalidateUserSessions(db: DB, userId: string) {
	db.prepare('DELETE FROM sessions WHERE user_id = ?').run(userId);
}

export function setSessionCookie(cookies: Cookies, token: string, expiresAt: number) {
	cookies.set(SESSION_COOKIE, token, {
		path: '/',
		httpOnly: true,
		sameSite: 'lax',
		expires: new Date(expiresAt)
	});
}

export function clearSessionCookie(cookies: Cookies) {
	cookies.delete(SESSION_COOKIE, { path: '/' });
}

/** Start a session for `userId` and set the cookie. */
export function signIn(db: DB, cookies: Cookies, userId: string) {
	const token = generateToken();
	const s = createSession(db, token, userId);
	setSessionCookie(cookies, token, s.expiresAt);
}

// ── Passwords ──────────────────────────────────────────────────────────

const ARGON = { memoryCost: 19456, timeCost: 2, outputLen: 32, parallelism: 1 };

export const hashPassword = (pw: string) => hash(pw, ARGON);
export const verifyPassword = (h: string, pw: string) => verify(h, pw);

const COMMON = new Set([
	'password',
	'password1',
	'12345678',
	'123456789',
	'1234567890',
	'qwertyuiop',
	'iloveyou',
	'11111111',
	'abcdefgh',
	'pencilpilot',
	'drawing123'
]);

/** null if OK, else a friendly reason. */
export function passwordProblem(pw: string, email = ''): string | null {
	if (pw.length < 8) return 'Use at least 8 characters.';
	if (pw.length > 200) return 'That password is too long.';
	if (COMMON.has(pw.toLowerCase())) return 'That password is too common. Try another.';
	if (email && pw.toLowerCase() === email.toLowerCase())
		return 'Don’t use your email as your password.';
	return null;
}

export const normaliseEmail = (e: string) => e.trim().toLowerCase();
export const validEmail = (e: string) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(e) && e.length <= 254;

// ── Users ──────────────────────────────────────────────────────────────

export function findUserByEmail(db: DB, email: string) {
	return db.prepare('SELECT * FROM users WHERE email = ?').get(normaliseEmail(email)) as
		(Row & { created_at: number }) | undefined;
}

export async function createPasswordUser(db: DB, email: string, password: string, name = '') {
	const id = newUserId();
	db.prepare(
		'INSERT INTO users (id, email, name, password_hash, created_at) VALUES (?, ?, ?, ?, ?)'
	).run(
		id,
		normaliseEmail(email),
		name.trim().slice(0, 60),
		await hashPassword(password),
		Date.now()
	);
	return id;
}

/** Sign in with Google: match by Google id, else link a verified email, else create. */
export function upsertGoogleUser(
	db: DB,
	g: { sub: string; email?: string; emailVerified?: boolean; name?: string }
): string {
	const byGoogle = db.prepare('SELECT id FROM users WHERE google_id = ?').get(g.sub) as
		{ id: string } | undefined;
	if (byGoogle) return byGoogle.id;
	if (g.email && g.emailVerified) {
		const byEmail = findUserByEmail(db, g.email);
		if (byEmail) {
			db.prepare('UPDATE users SET google_id = ? WHERE id = ?').run(g.sub, byEmail.id);
			return byEmail.id;
		}
	}
	const id = newUserId();
	db.prepare(
		'INSERT INTO users (id, email, name, google_id, created_at) VALUES (?, ?, ?, ?, ?)'
	).run(
		id,
		g.email && g.emailVerified ? normaliseEmail(g.email) : null,
		(g.name ?? '').slice(0, 60),
		g.sub,
		Date.now()
	);
	return id;
}

export function deleteUser(db: DB, userId: string) {
	db.prepare('DELETE FROM users WHERE id = ?').run(userId); // cascades to sessions + data
}
