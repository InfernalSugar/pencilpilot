/**
 * Lobby tickets: a short signed note from the web server ("this browser is
 * player X called Y, signed in as Z") that the socket checks on join.
 */
import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto';
import type { Identity } from './hub';

const TTL_MS = 12 * 3600_000;

function secret(): Buffer {
	const g = globalThis as unknown as { __ppLobbySecret?: Buffer };
	g.__ppLobbySecret ??= randomBytes(32);
	return g.__ppLobbySecret;
}

const sign = (body: string) => createHmac('sha256', secret()).update(body).digest('base64url');

export function issueTicket(code: string, who: Identity, now = Date.now()): string {
	const body = Buffer.from(JSON.stringify({ code, ...who, exp: now + TTL_MS })).toString(
		'base64url'
	);
	return `${body}.${sign(body)}`;
}

export function readTicket(code: string, ticket: string, now = Date.now()): Identity | null {
	const [body, mac] = String(ticket).split('.');
	if (!body || !mac) return null;
	const want = Buffer.from(sign(body));
	const got = Buffer.from(mac);
	if (want.length !== got.length || !timingSafeEqual(want, got)) return null;
	try {
		const t = JSON.parse(Buffer.from(body, 'base64url').toString());
		if (t.code !== code || typeof t.exp !== 'number' || t.exp < now) return null;
		return { playerId: String(t.playerId), name: String(t.name), userId: t.userId ?? null };
	} catch {
		return null;
	}
}

/** A display name: 1–24 visible characters. */
export function cleanName(raw: unknown): string | null {
	if (typeof raw !== 'string') return null;
	const n = raw
		.replace(/\p{Cc}/gu, '') // control characters out
		.replace(/\s+/g, ' ')
		.trim()
		.slice(0, 24);
	return n.length ? n : null;
}
