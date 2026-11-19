/**
 * Community critique (M9-T02), behind the COMMUNITY flag.
 *
 * Signed-in students may share a drawing. Other students then judge shared
 * drawings in two ways, both without free text (nothing to moderate but the
 * pictures themselves):
 * - "Which is closer?": two drawings of the same reference, pick the closer one.
 *   Each drawing keeps an Elo-style rating; these pairwise votes double as
 *   human calibration data for the grader.
 * - A 1–5 rubric: proportions, lines, light & dark, overall.
 * Pairwise votes are kept with both drawings' grader scores, so they can later
 * feed grader calibration.
 *
 * Every shared drawing passes the NSFW guard (ref-service /api/moderate) first,
 * and is hidden after REPORTS_TO_HIDE reports.
 */
import { randomUUID } from 'node:crypto';
import { COMMUNITY, REF_SERVICE_URL } from '$app/env/private';
import { byId, type Reference } from '../references';
import { SOURCES, defaultLicense, safeLink } from '../credit';
import type { DB } from './db';
import { RateLimiter } from './ratelimit';

export const communityEnabled = () => COMMUNITY === 'on';

export const RUBRIC = [
	{ id: 'proportions', label: 'Proportions', hint: 'Sizes and placement match the reference' },
	{ id: 'lines', label: 'Lines', hint: 'Confident, clean outlines' },
	{
		id: 'shading',
		label: 'Light & dark',
		hint: 'Shading follows the reference (give line art a 3)'
	},
	{ id: 'overall', label: 'Overall', hint: 'How close is it, all things considered?' }
] as const;
export type RubricId = (typeof RUBRIC)[number]['id'];
export type RubricScores = Record<RubricId, number>;

/** Reports from different people before a drawing is hidden for everyone. */
export const REPORTS_TO_HIDE = 3;
/** Largest PNG we keep (an 800×600 drawing is usually 50–300 kB). */
export const MAX_PNG_BYTES = 1_500_000;
const ELO_K = 24;

/** 20 shares a day per person, 300 judgements an hour. */
export const sharesByUser = new RateLimiter(20, (24 * 60 * 60 * 1000) / 20);
export const judgementsByUser = new RateLimiter(300, (60 * 60 * 1000) / 300);

export interface SharedDrawing {
	id: string;
	referenceId: string;
	reference: Reference;
	category: string;
	score: number | null;
	createdAt: number;
}

export interface MySharedDrawing extends SharedDrawing {
	rating: number;
	comparisons: number;
	wins: number;
	hidden: boolean;
	rubric: (RubricScores & { n: number }) | null;
}

const LIVE_IMAGE = /^\/refimg\/[A-Za-z0-9_-]{1,36}\.jpg$/;

/**
 * The reference a shared drawing was made from. Curated ones are looked up by
 * id; Endless (live) ones come from the browser, so only known-shape fields are
 * kept and their links must point at our image server and the picture
 * sources (`safeLink`).
 */
export function cleanReference(raw: unknown): Reference | null {
	if (!raw || typeof raw !== 'object') return null;
	const r = raw as Record<string, unknown>;
	if (typeof r.id !== 'string') return null;
	const curated = byId(r.id);
	if (curated) return curated;
	if (r.live !== true) return null;
	const str = (v: unknown, max: number) => (typeof v === 'string' && v.length <= max ? v : null);
	const imageUrl = str(r.imageUrl, 60);
	const pageUrl = safeLink(r.pageUrl);
	const authorUrl = safeLink(r.authorUrl);
	const source = SOURCES.find((s) => s === r.source) ?? 'pixabay';
	const fallback = defaultLicense(source);
	const category = str(r.category, 30);
	if (!imageUrl || !LIVE_IMAGE.test(imageUrl)) return null;
	if (!pageUrl) return null;
	if (!category || !/^[a-z-]+$/.test(category) || !/^[A-Za-z0-9_-]{1,40}$/.test(r.id)) return null;
	const tags = Array.isArray(r.tags)
		? r.tags.filter((t): t is string => typeof t === 'string' && t.length <= 40).slice(0, 12)
		: [];
	const num = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : undefined);
	return {
		id: r.id,
		source,
		sourceId: num(r.sourceId) ?? 0,
		category,
		pageUrl,
		author: str(r.author, 80) ?? 'Unknown',
		authorUrl,
		license: str(r.license, 60) ?? fallback.license,
		licenseUrl: safeLink(r.licenseUrl) || fallback.licenseUrl,
		provider: str(r.provider, 80) ?? undefined,
		attribution: str(r.attribution, 400) ?? undefined,
		tags,
		width: num(r.width) ?? 0,
		height: num(r.height) ?? 0,
		title: str(r.title, 80) ?? tags[0] ?? 'Reference',
		keypoints: [],
		topic: str(r.topic, 40) ?? undefined,
		tier: num(r.tier),
		imageUrl,
		live: true
	};
}

/** PNG signature + a sane size; the moderation step decodes it properly. */
export function looksLikePng(bytes: Uint8Array) {
	const sig = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
	return (
		bytes.length > sig.length &&
		bytes.length <= MAX_PNG_BYTES &&
		sig.every((b, i) => bytes[i] === b)
	);
}

export type ModerationResult = { safe: true } | { safe: false; reason: string };

/** Ask the ref-service's NSFW guard. Anything that goes wrong counts as unsafe. */
export async function moderate(
	png: Uint8Array,
	fetcher: typeof fetch = fetch
): Promise<ModerationResult> {
	const base = REF_SERVICE_URL || 'http://127.0.0.1:8787';
	try {
		const res = await fetcher(`${base}/api/moderate`, {
			method: 'POST',
			body: new Blob([new Uint8Array(png)], { type: 'image/png' }),
			headers: { 'content-type': 'image/png' },
			signal: AbortSignal.timeout(15_000)
		});
		if (!res.ok) return { safe: false, reason: 'unavailable' };
		const v = (await res.json()) as { safe?: unknown; reason?: unknown };
		return v.safe === true ? { safe: true } : { safe: false, reason: String(v.reason ?? 'unsafe') };
	} catch {
		return { safe: false, reason: 'unavailable' };
	}
}

export function shareDrawing(
	db: DB,
	userId: string,
	d: { png: Uint8Array; reference: Reference; score: number | null; attemptAt: number | null },
	now = Date.now()
): string {
	// Re-sharing the same attempt replaces nothing: it returns the existing one.
	if (d.attemptAt != null) {
		const prev = db
			.prepare('SELECT id FROM shared_drawings WHERE user_id = ? AND attempt_at = ?')
			.get(userId, d.attemptAt) as { id: string } | undefined;
		if (prev) return prev.id;
	}
	const id = randomUUID();
	db.prepare(
		`INSERT INTO shared_drawings (id, user_id, attempt_at, reference_id, reference_json, category, png, score, created_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`
	).run(
		id,
		userId,
		d.attemptAt,
		d.reference.id,
		JSON.stringify(d.reference),
		d.reference.category,
		Buffer.from(d.png),
		d.score,
		now
	);
	return id;
}

interface Row {
	id: string;
	reference_id: string;
	reference_json: string;
	category: string;
	score: number | null;
	created_at: number;
}
const toShared = (r: Row): SharedDrawing => ({
	id: r.id,
	referenceId: r.reference_id,
	reference: JSON.parse(r.reference_json),
	category: r.category,
	score: r.score,
	createdAt: r.created_at
});
const COLS = 'd.id, d.reference_id, d.reference_json, d.category, d.score, d.created_at';

/**
 * Two drawings of the same reference for `voterId` to compare: never their own,
 * never a pair they already judged, preferring drawings with few comparisons.
 */
export function nextPair(
	db: DB,
	voterId: string,
	rand = Math.random
): { a: SharedDrawing; b: SharedDrawing } | null {
	const refs = db
		.prepare(
			`SELECT reference_id, SUM(comparisons) AS c FROM shared_drawings
			 WHERE hidden = 0 AND user_id != ? GROUP BY reference_id HAVING COUNT(*) >= 2
			 ORDER BY c ASC LIMIT 20`
		)
		.all(voterId) as { reference_id: string }[];
	const voted = db.prepare(
		'SELECT 1 FROM closer_votes WHERE voter_id = ? AND a_id = ? AND b_id = ?'
	);
	const candidates = db.prepare(
		`SELECT ${COLS} FROM shared_drawings d
		 WHERE d.reference_id = ? AND d.hidden = 0 AND d.user_id != ?
		 ORDER BY d.comparisons ASC, d.created_at DESC LIMIT 12`
	);
	// try references with the fewest judgements first, with a little shuffle so
	// two people opening the page at once don't see the identical pair
	const order = refs.map((r, i) => ({ r, k: i + rand() * 3 })).sort((x, y) => x.k - y.k);
	for (const { r } of order) {
		const ds = (candidates.all(r.reference_id, voterId) as Row[]).map(toShared);
		for (let i = 0; i < ds.length; i++)
			for (let j = i + 1; j < ds.length; j++) {
				const [lo, hi] = ds[i].id < ds[j].id ? [ds[i], ds[j]] : [ds[j], ds[i]];
				if (!voted.get(voterId, lo.id, hi.id))
					return rand() < 0.5 ? { a: lo, b: hi } : { a: hi, b: lo };
			}
	}
	return null;
}

/** Elo expected score of a against b. */
export const expected = (ra: number, rb: number) => 1 / (1 + 10 ** ((rb - ra) / 400));

export type JudgeError = 'not_found' | 'own_drawing' | 'already_judged' | 'mismatch';

export function recordCloser(
	db: DB,
	voterId: string,
	aId: string,
	bId: string,
	winnerId: string,
	now = Date.now()
): JudgeError | null {
	if (aId === bId || (winnerId !== aId && winnerId !== bId)) return 'mismatch';
	const get = db.prepare(
		'SELECT id, user_id, reference_id, rating, hidden FROM shared_drawings WHERE id = ?'
	);
	type R = { id: string; user_id: string; reference_id: string; rating: number; hidden: number };
	const a = get.get(aId) as R | undefined;
	const b = get.get(bId) as R | undefined;
	if (!a || !b || a.hidden || b.hidden) return 'not_found';
	if (a.reference_id !== b.reference_id) return 'mismatch';
	if (a.user_id === voterId || b.user_id === voterId) return 'own_drawing';
	const [lo, hi] = a.id < b.id ? [a, b] : [b, a];
	return db.transaction((): JudgeError | null => {
		const ins = db
			.prepare(
				'INSERT OR IGNORE INTO closer_votes (voter_id, a_id, b_id, winner_id, created_at) VALUES (?, ?, ?, ?, ?)'
			)
			.run(voterId, lo.id, hi.id, winnerId, now);
		if (ins.changes === 0) return 'already_judged';
		const [w, l] = winnerId === a.id ? [a, b] : [b, a];
		const delta = ELO_K * (1 - expected(w.rating, l.rating));
		const upd = db.prepare(
			'UPDATE shared_drawings SET rating = rating + ?, comparisons = comparisons + 1, wins = wins + ? WHERE id = ?'
		);
		upd.run(delta, 1, w.id);
		upd.run(-delta, 0, l.id);
		return null;
	})();
}

/** A drawing `raterId` hasn't rated (and didn't make), fewest ratings first. */
export function nextToRate(db: DB, raterId: string): SharedDrawing | null {
	const row = db
		.prepare(
			`SELECT ${COLS} FROM shared_drawings d
			 WHERE d.hidden = 0 AND d.user_id != ?
			   AND NOT EXISTS (SELECT 1 FROM rubric_ratings r WHERE r.rater_id = ? AND r.drawing_id = d.id)
			 ORDER BY (SELECT COUNT(*) FROM rubric_ratings r WHERE r.drawing_id = d.id) ASC, d.created_at DESC
			 LIMIT 1`
		)
		.get(raterId, raterId) as Row | undefined;
	return row ? toShared(row) : null;
}

export function cleanRubric(raw: Record<string, unknown>): RubricScores | null {
	const out = {} as RubricScores;
	for (const { id } of RUBRIC) {
		const v = Number(raw[id]);
		if (!Number.isInteger(v) || v < 1 || v > 5) return null;
		out[id] = v;
	}
	return out;
}

export function recordRubric(
	db: DB,
	raterId: string,
	drawingId: string,
	s: RubricScores,
	now = Date.now()
): JudgeError | null {
	const d = db
		.prepare('SELECT user_id, hidden FROM shared_drawings WHERE id = ?')
		.get(drawingId) as { user_id: string; hidden: number } | undefined;
	if (!d || d.hidden) return 'not_found';
	if (d.user_id === raterId) return 'own_drawing';
	const r = db
		.prepare(
			`INSERT OR IGNORE INTO rubric_ratings (rater_id, drawing_id, proportions, lines, shading, overall, created_at)
			 VALUES (?, ?, ?, ?, ?, ?, ?)`
		)
		.run(raterId, drawingId, s.proportions, s.lines, s.shading, s.overall, now);
	return r.changes ? null : 'already_judged';
}

/** Report a drawing; returns true once it is hidden. Owners can't report their own. */
export function reportDrawing(
	db: DB,
	reporterId: string,
	drawingId: string,
	reason: string,
	now = Date.now()
): boolean | 'not_found' {
	const d = db.prepare('SELECT user_id FROM shared_drawings WHERE id = ?').get(drawingId) as
		{ user_id: string } | undefined;
	if (!d || d.user_id === reporterId) return 'not_found';
	return db.transaction(() => {
		db.prepare(
			'INSERT OR IGNORE INTO drawing_reports (reporter_id, drawing_id, reason, created_at) VALUES (?, ?, ?, ?)'
		).run(reporterId, drawingId, reason.slice(0, 40), now);
		const { n } = db
			.prepare('SELECT COUNT(*) AS n FROM drawing_reports WHERE drawing_id = ?')
			.get(drawingId) as { n: number };
		if (n >= REPORTS_TO_HIDE)
			db.prepare('UPDATE shared_drawings SET hidden = 1 WHERE id = ?').run(drawingId);
		return n >= REPORTS_TO_HIDE;
	})();
}

export function myDrawings(db: DB, userId: string): MySharedDrawing[] {
	const rows = db
		.prepare(
			`SELECT ${COLS}, d.rating, d.comparisons, d.wins, d.hidden,
			   (SELECT COUNT(*) FROM rubric_ratings r WHERE r.drawing_id = d.id) AS n,
			   (SELECT AVG(proportions) FROM rubric_ratings r WHERE r.drawing_id = d.id) AS proportions,
			   (SELECT AVG(lines) FROM rubric_ratings r WHERE r.drawing_id = d.id) AS lines,
			   (SELECT AVG(shading) FROM rubric_ratings r WHERE r.drawing_id = d.id) AS shading,
			   (SELECT AVG(overall) FROM rubric_ratings r WHERE r.drawing_id = d.id) AS overall
			 FROM shared_drawings d WHERE d.user_id = ? ORDER BY d.created_at DESC`
		)
		.all(userId) as (Row & {
		rating: number;
		comparisons: number;
		wins: number;
		hidden: number;
		n: number;
		proportions: number;
		lines: number;
		shading: number;
		overall: number;
	})[];
	return rows.map((r) => ({
		...toShared(r),
		rating: r.rating,
		comparisons: r.comparisons,
		wins: r.wins,
		hidden: !!r.hidden,
		rubric: r.n
			? {
					n: r.n,
					proportions: r.proportions,
					lines: r.lines,
					shading: r.shading,
					overall: r.overall
				}
			: null
	}));
}

export function deleteDrawing(db: DB, userId: string, id: string): boolean {
	return (
		db.prepare('DELETE FROM shared_drawings WHERE id = ? AND user_id = ?').run(id, userId).changes >
		0
	);
}

/** The PNG, for anyone signed in while it's visible; always for its owner. */
export function drawingPng(db: DB, id: string, viewerId: string): Buffer | null {
	const r = db.prepare('SELECT png, user_id, hidden FROM shared_drawings WHERE id = ?').get(id) as
		{ png: Buffer; user_id: string; hidden: number } | undefined;
	if (!r || (r.hidden && r.user_id !== viewerId)) return null;
	return r.png;
}
