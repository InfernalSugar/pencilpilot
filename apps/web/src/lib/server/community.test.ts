import { describe, expect, it } from 'vitest';
import { openDb, type DB } from './db';
import {
	cleanReference,
	cleanRubric,
	deleteDrawing,
	drawingPng,
	expected,
	looksLikePng,
	moderate,
	myDrawings,
	nextPair,
	nextToRate,
	recordCloser,
	recordRubric,
	reportDrawing,
	REPORTS_TO_HIDE,
	shareDrawing
} from './community';
import { references } from '../references';

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]);
const LIVE = {
	id: 'live-123',
	live: true,
	category: 'animals',
	imageUrl: '/refimg/live-123.jpg',
	pageUrl: 'https://pixabay.com/photos/cat-123/',
	authorUrl: 'https://pixabay.com/users/someone-1/',
	author: 'someone',
	tags: ['cat', 'pet']
};

function setup(users = ['ann', 'bob', 'cat', 'dan', 'eve']): DB {
	const db = openDb(':memory:');
	for (const u of users)
		db.prepare('INSERT INTO users (id, email, created_at) VALUES (?, ?, 0)').run(u, `${u}@x.test`);
	return db;
}
const ref = cleanReference(LIVE)!;
const share = (db: DB, user: string, r = ref, at: number | null = null) =>
	shareDrawing(db, user, { png: PNG, reference: r, score: 5, attemptAt: at });

describe('cleanReference', () => {
	it('accepts curated references by id and well-formed live ones', () => {
		if (references.length) expect(cleanReference({ id: references[0].id })).toBe(references[0]);
		expect(ref.imageUrl).toBe('/refimg/live-123.jpg');
		expect(ref.license).toBe('Pixabay Content License');
	});
	it('rejects links that leave our image server or Pixabay', () => {
		expect(cleanReference({ ...LIVE, imageUrl: 'https://evil.test/x.jpg' })).toBeNull();
		expect(cleanReference({ ...LIVE, imageUrl: '/refimg/../../etc/passwd' })).toBeNull();
		expect(cleanReference({ ...LIVE, pageUrl: 'javascript:alert(1)' })).toBeNull();
		expect(cleanReference({ ...LIVE, live: false })).toBeNull();
		expect(cleanReference('nope')).toBeNull();
	});
});

describe('checks', () => {
	it('recognises PNGs and rubric scores', () => {
		expect(looksLikePng(PNG)).toBe(true);
		expect(looksLikePng(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9]))).toBe(false);
		expect(cleanRubric({ proportions: 3, lines: 5, shading: 1, overall: '4' })).toEqual({
			proportions: 3,
			lines: 5,
			shading: 1,
			overall: 4
		});
		expect(cleanRubric({ proportions: 6, lines: 5, shading: 1, overall: 4 })).toBeNull();
		expect(cleanRubric({ proportions: 3 })).toBeNull();
	});

	it('moderation fails closed', async () => {
		const ok = (body: unknown, status = 200) =>
			(async () => new Response(JSON.stringify(body), { status })) as unknown as typeof fetch;
		expect(await moderate(PNG, ok({ safe: true }))).toEqual({ safe: true });
		expect(await moderate(PNG, ok({ safe: false, reason: 'nsfw' }))).toEqual({
			safe: false,
			reason: 'nsfw'
		});
		expect((await moderate(PNG, ok({}, 500))).safe).toBe(false);
		const down = (async () => {
			throw new Error('ECONNREFUSED');
		}) as unknown as typeof fetch;
		expect(await moderate(PNG, down)).toEqual({ safe: false, reason: 'unavailable' });
	});
});

describe('which is closer?', () => {
	it('never shows your own drawing and never repeats a judged pair', () => {
		const db = setup();
		const a = share(db, 'ann');
		const b = share(db, 'bob');
		expect(nextPair(db, 'ann')).toBeNull(); // only one drawing that isn't hers
		const pair = nextPair(db, 'cat')!;
		expect(new Set([pair.a.id, pair.b.id])).toEqual(new Set([a, b]));
		expect(recordCloser(db, 'cat', pair.a.id, pair.b.id, pair.a.id)).toBeNull();
		expect(recordCloser(db, 'cat', pair.b.id, pair.a.id, pair.a.id)).toBe('already_judged');
		expect(nextPair(db, 'cat')).toBeNull();
		expect(recordCloser(db, 'ann', a, b, a)).toBe('own_drawing');
		expect(recordCloser(db, 'dan', a, b, 'other')).toBe('mismatch');
	});

	it('moves ratings Elo-style', () => {
		const db = setup();
		const a = share(db, 'ann');
		const b = share(db, 'bob');
		recordCloser(db, 'cat', a, b, a);
		recordCloser(db, 'dan', a, b, a);
		const [mine] = myDrawings(db, 'ann');
		expect(mine.rating).toBeGreaterThan(1000);
		expect(mine.comparisons).toBe(2);
		expect(mine.wins).toBe(2);
		expect(myDrawings(db, 'bob')[0].rating).toBeCloseTo(2000 - mine.rating, 6);
		expect(expected(1000, 1000)).toBeCloseTo(0.5);
	});

	it('only pairs drawings of the same reference', () => {
		const db = setup();
		const other = cleanReference({ ...LIVE, id: 'live-999', imageUrl: '/refimg/live-999.jpg' })!;
		const a = share(db, 'ann');
		const b = share(db, 'bob', other);
		expect(nextPair(db, 'cat')).toBeNull();
		expect(recordCloser(db, 'cat', a, b, a)).toBe('mismatch');
	});
});

describe('rubric, reports, ownership', () => {
	it('rates each drawing once and averages for the owner', () => {
		const db = setup();
		const a = share(db, 'ann');
		expect(nextToRate(db, 'ann')).toBeNull();
		expect(nextToRate(db, 'bob')?.id).toBe(a);
		const s = { proportions: 4, lines: 2, shading: 3, overall: 3 };
		expect(recordRubric(db, 'bob', a, s)).toBeNull();
		expect(recordRubric(db, 'bob', a, s)).toBe('already_judged');
		expect(recordRubric(db, 'cat', a, { ...s, lines: 4 })).toBeNull();
		expect(recordRubric(db, 'ann', a, s)).toBe('own_drawing');
		expect(nextToRate(db, 'bob')).toBeNull();
		expect(myDrawings(db, 'ann')[0].rubric).toMatchObject({ n: 2, lines: 3, proportions: 4 });
	});

	it(`hides a drawing after ${REPORTS_TO_HIDE} reports, except from its owner`, () => {
		const db = setup();
		const a = share(db, 'ann');
		share(db, 'bob');
		expect(reportDrawing(db, 'ann', a, 'unsafe')).toBe('not_found');
		expect(reportDrawing(db, 'bob', a, 'unsafe')).toBe(false);
		expect(reportDrawing(db, 'bob', a, 'unsafe')).toBe(false); // same person twice counts once
		expect(reportDrawing(db, 'cat', a, 'unsafe')).toBe(false);
		expect(reportDrawing(db, 'dan', a, 'unsafe')).toBe(true);
		expect(drawingPng(db, a, 'eve')).toBeNull();
		expect(drawingPng(db, a, 'ann')).not.toBeNull();
		expect(nextToRate(db, 'eve')?.id).not.toBe(a);
		expect(myDrawings(db, 'ann')[0].hidden).toBe(true);
	});

	it('sharing an attempt twice keeps one copy; owners can delete; accounts cascade', () => {
		const db = setup();
		const a = share(db, 'ann', ref, 42);
		expect(share(db, 'ann', ref, 42)).toBe(a);
		expect(deleteDrawing(db, 'bob', a)).toBe(false);
		expect(deleteDrawing(db, 'ann', a)).toBe(true);
		const b = share(db, 'ann');
		recordRubric(db, 'bob', b, { proportions: 1, lines: 1, shading: 1, overall: 1 });
		db.prepare('DELETE FROM users WHERE id = ?').run('ann');
		expect(db.prepare('SELECT COUNT(*) AS n FROM shared_drawings').get()).toEqual({ n: 0 });
		expect(db.prepare('SELECT COUNT(*) AS n FROM rubric_ratings').get()).toEqual({ n: 0 });
	});
});
