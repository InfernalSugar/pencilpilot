/**
 * Server copy of a user's practice history (attempts, drill results, thumbnails).
 * Browsers keep using localStorage; signed-in browsers sync with this store so
 * progress follows the account. Everything from the client is validated.
 */
import type { DB } from './db';

export const LIMITS = { attempts: 2000, drills: 1000, thumbs: 24, thumbBytes: 40_000 };

type Json = Record<string, unknown>;

const num = (v: unknown, lo: number, hi: number) =>
	typeof v === 'number' && Number.isFinite(v) && v >= lo && v <= hi;
const str = (v: unknown, max = 200) => typeof v === 'string' && v.length <= max;
const MIN_AT = Date.UTC(2026, 0, 1);

/** Keep only well-formed attempts, with known fields. */
export function cleanAttempt(a: unknown, now = Date.now()): Json | null {
	if (!a || typeof a !== 'object') return null;
	const x = a as Json;
	if (!num(x.at, MIN_AT, now + 60_000) || !num(x.score, 1, 10) || !num(x.tier, 1, 4)) return null;
	if (!str(x.category, 40) || !(x.topic === null || str(x.topic, 40))) return null;
	const out: Json = {
		at: x.at,
		score: x.score,
		tier: x.tier,
		category: x.category,
		topic: x.topic
	};
	if (str(x.refId, 40)) out.refId = x.refId;
	if (str(x.title, 120)) out.title = x.title;
	if (Array.isArray(x.fixes) && x.fixes.length <= 10 && x.fixes.every((f) => str(f, 60)))
		out.fixes = x.fixes;
	if (x.skills && typeof x.skills === 'object') {
		const sk = Object.entries(x.skills as Json).filter(([k, v]) => str(k, 40) && num(v, 0, 1));
		if (sk.length <= 10) out.skills = Object.fromEntries(sk);
	}
	return out;
}

export function cleanDrill(d: unknown, now = Date.now()): Json | null {
	if (!d || typeof d !== 'object') return null;
	const x = d as Json;
	if (!str(x.id, 60) || !num(x.at, MIN_AT, now + 60_000) || !num(x.avg, 1, 10)) return null;
	if (!Array.isArray(x.rounds) || x.rounds.length > 30 || !x.rounds.every((r) => num(r, 1, 10)))
		return null;
	return { id: x.id, at: x.at, avg: x.avg, rounds: x.rounds };
}

const cleanThumb = (at: string, url: unknown) =>
	/^\d{13}$/.test(at) &&
	typeof url === 'string' &&
	url.length <= LIMITS.thumbBytes &&
	/^data:image\/(jpeg|png);base64,[A-Za-z0-9+/=]+$/.test(url);

export interface Snapshot {
	attempts: Json[];
	drills: Json[];
	thumbs: Record<string, string>;
}

/** Merge client data in (upsert by timestamp), then return everything stored. */
export function sync(
	db: DB,
	userId: string,
	incoming: Partial<Snapshot>,
	now = Date.now()
): Snapshot {
	const putA = db.prepare(
		'INSERT INTO attempts (user_id, at, data) VALUES (?, ?, ?) ON CONFLICT(user_id, at) DO UPDATE SET data = excluded.data'
	);
	const putD = db.prepare(
		'INSERT INTO drill_results (user_id, at, data) VALUES (?, ?, ?) ON CONFLICT(user_id, at) DO NOTHING'
	);
	const putT = db.prepare(
		'INSERT INTO thumbs (user_id, at, data_url) VALUES (?, ?, ?) ON CONFLICT(user_id, at) DO NOTHING'
	);
	db.transaction(() => {
		for (const a of (incoming.attempts ?? []).slice(-LIMITS.attempts)) {
			const c = cleanAttempt(a, now);
			if (c) putA.run(userId, c.at, JSON.stringify(c));
		}
		for (const d of (incoming.drills ?? []).slice(-LIMITS.drills)) {
			const c = cleanDrill(d, now);
			if (c) putD.run(userId, c.at, JSON.stringify(c));
		}
		for (const [at, url] of Object.entries(incoming.thumbs ?? {}).slice(-LIMITS.thumbs)) {
			if (cleanThumb(at, url)) putT.run(userId, Number(at), url);
		}
		// keep only the newest N of each
		db.prepare(
			`DELETE FROM attempts WHERE user_id = ? AND at NOT IN (SELECT at FROM attempts WHERE user_id = ? ORDER BY at DESC LIMIT ${LIMITS.attempts})`
		).run(userId, userId);
		db.prepare(
			`DELETE FROM drill_results WHERE user_id = ? AND at NOT IN (SELECT at FROM drill_results WHERE user_id = ? ORDER BY at DESC LIMIT ${LIMITS.drills})`
		).run(userId, userId);
		db.prepare(
			`DELETE FROM thumbs WHERE user_id = ? AND at NOT IN (SELECT at FROM thumbs WHERE user_id = ? ORDER BY at DESC LIMIT ${LIMITS.thumbs})`
		).run(userId, userId);
	})();
	return snapshot(db, userId);
}

export function snapshot(db: DB, userId: string): Snapshot {
	const rows = (table: string) =>
		(
			db.prepare(`SELECT data FROM ${table} WHERE user_id = ? ORDER BY at`).all(userId) as {
				data: string;
			}[]
		).map((r) => JSON.parse(r.data) as Json);
	const thumbs = Object.fromEntries(
		(
			db.prepare('SELECT at, data_url FROM thumbs WHERE user_id = ? ORDER BY at').all(userId) as {
				at: number;
				data_url: string;
			}[]
		).map((t) => [String(t.at), t.data_url])
	);
	return { attempts: rows('attempts'), drills: rows('drill_results'), thumbs };
}
