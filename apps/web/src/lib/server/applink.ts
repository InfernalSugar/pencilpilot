/**
 * Linking the desktop app to an account, for "Draw together" (ADR-0013). The usual way for
 * desktop apps (RFC 8252, with PKCE):
 *
 * 1. The app opens the browser at /app/link with a loopback address to come back to
 *    (http://127.0.0.1:{port}/…), a random state and a code challenge (the SHA-256 of a
 *    secret only the app knows).
 * 2. The person signs in here as usual and confirms. The site sends the browser back to the
 *    app with a one-time code (5 minutes).
 * 3. The app trades the code and its secret for an app token (POST /api/app/token), which
 *    it keeps and shows when asking for passes (POST /api/app/pass).
 *
 * Codes and tokens are stored hashed (SHA-256), so a leaked database can't be used. Each
 * linked app is listed on the Account page, where it can be unlinked.
 */
import { createHash, randomBytes } from 'node:crypto';
import type { DB } from './db';

const CODE_MS = 5 * 60_000;

const hash = (s: string) => createHash('sha256').update(s).digest('hex');
const random = (bytes: number) => randomBytes(bytes).toString('base64url');

/** The app's loopback address, as given; null unless it's http://127.0.0.1 or [::1] with a port. */
export function loopbackRedirect(raw: unknown): string | null {
	if (typeof raw !== 'string' || raw.length > 200) return null;
	try {
		const u = new URL(raw);
		const loopback = u.hostname === '127.0.0.1' || u.hostname === '[::1]';
		if (u.protocol !== 'http:' || !loopback || !u.port || u.username || u.password) return null;
		if (u.search || u.hash) return null;
		return u.href;
	} catch {
		return null;
	}
}

/** A PKCE challenge: base64url of a SHA-256 (43 characters). */
export const validChallenge = (s: unknown): s is string =>
	typeof s === 'string' && /^[A-Za-z0-9_-]{43}$/.test(s);

/** The app's state, passed back untouched. */
export const validState = (s: unknown): s is string =>
	typeof s === 'string' && /^[A-Za-z0-9_-]{16,128}$/.test(s);

/** What the app calls itself on the Account page ("PencilPilot on Linux"). */
export function appLabel(raw: unknown): string {
	const s =
		typeof raw === 'string'
			? raw
					.replace(/\p{Cc}/gu, '')
					.replace(/\s+/g, ' ')
					.trim()
			: '';
	return s.slice(0, 60) || 'PencilPilot app';
}

/** The code challenge for a verifier (S256). */
export const challengeOf = (verifier: string) =>
	createHash('sha256').update(verifier).digest('base64url');

/** A one-time code for the app at `redirect`, for `userId`. */
export function createLinkCode(
	db: DB,
	userId: string,
	o: { challenge: string; redirect: string; label: string },
	now = Date.now()
): string {
	const code = random(32);
	db.prepare('DELETE FROM app_link_codes WHERE expires_at <= ?').run(now);
	db.prepare(
		`INSERT INTO app_link_codes (id, user_id, challenge, redirect_uri, label, expires_at)
		 VALUES (?, ?, ?, ?, ?, ?)`
	).run(hash(code), userId, o.challenge, o.redirect, o.label, now + CODE_MS);
	return code;
}

/**
 * Trade a one-time code (and the app's secret) for an app token. The code is used up by
 * the first try, right or wrong. Null when it doesn't match.
 */
export function exchangeCode(
	db: DB,
	o: { code: unknown; verifier: unknown; redirect: unknown },
	now = Date.now()
): { token: string; user: { id: string; name: string } } | null {
	if (typeof o.code !== 'string' || typeof o.verifier !== 'string') return null;
	if (!/^[A-Za-z0-9._~-]{43,128}$/.test(o.verifier)) return null;
	const id = hash(o.code);
	const row = db
		.prepare(
			`SELECT c.user_id, c.challenge, c.redirect_uri, c.label, c.expires_at, u.name
			 FROM app_link_codes c JOIN users u ON u.id = c.user_id WHERE c.id = ?`
		)
		.get(id) as
		| {
				user_id: string;
				challenge: string;
				redirect_uri: string;
				label: string;
				expires_at: number;
				name: string;
		  }
		| undefined;
	if (!row) return null;
	db.prepare('DELETE FROM app_link_codes WHERE id = ?').run(id);
	if (now >= row.expires_at) return null;
	if (row.redirect_uri !== o.redirect || challengeOf(o.verifier) !== row.challenge) return null;
	const token = 'ppa_' + random(32);
	db.prepare(
		`INSERT INTO app_links (id, token_hash, user_id, label, created_at)
		 VALUES (?, ?, ?, ?, ?)`
	).run(random(9), hash(token), row.user_id, row.label, now);
	return { token, user: { id: row.user_id, name: row.name } };
}

/** The account an app token belongs to (and notes the app was seen), or null. */
export function appUser(
	db: DB,
	token: string,
	now = Date.now()
): { link: string; user: { id: string; name: string } } | null {
	const row = db
		.prepare(
			`SELECT l.id, u.id AS user_id, u.name FROM app_links l
			 JOIN users u ON u.id = l.user_id WHERE l.token_hash = ?`
		)
		.get(hash(token)) as { id: string; user_id: string; name: string } | undefined;
	if (!row) return null;
	db.prepare('UPDATE app_links SET last_used_at = ? WHERE id = ?').run(now, row.id);
	return { link: row.id, user: { id: row.user_id, name: row.name } };
}

/** The token from an `Authorization: Bearer …` header. */
export function bearer(header: string | null): string | null {
	const m = header?.match(/^Bearer ([A-Za-z0-9_-]{8,200})$/);
	return m ? m[1] : null;
}

export interface LinkedApp {
	id: string;
	label: string;
	created_at: number;
	last_used_at: number | null;
}

export function listLinks(db: DB, userId: string): LinkedApp[] {
	return db
		.prepare(
			`SELECT id, label, created_at, last_used_at FROM app_links
			 WHERE user_id = ? ORDER BY created_at DESC`
		)
		.all(userId) as LinkedApp[];
}

/** Unlink one of the account's apps; its token stops working. */
export function unlink(db: DB, userId: string, id: string): boolean {
	return (
		db.prepare('DELETE FROM app_links WHERE id = ? AND user_id = ?').run(id, userId).changes > 0
	);
}

/** The app signing out: its own token stops working. */
export function revokeToken(db: DB, token: string): boolean {
	return db.prepare('DELETE FROM app_links WHERE token_hash = ?').run(hash(token)).changes > 0;
}
