import { describe, expect, it } from 'vitest';
import {
	DEFAULT_FILTERS,
	applyDecision,
	failure,
	keyAction,
	keptPercent,
	listQuery,
	moveOptions,
	reviewLabel,
	serialQueue,
	step,
	type Picture
} from './picture-review';

const pic = (over: Partial<Picture> = {}): Picture => ({
	id: 'px-1',
	imageUrl: '/api/admin/img/px-1.jpg',
	category: 'anime',
	topic: 'characters',
	tier: 2,
	source: 'pixabay',
	provider: 'Pixabay',
	kind: 'illustration',
	title: '',
	tags: ['girl'],
	credit: {
		author: 'Ann',
		authorUrl: '',
		license: 'Pixabay Content License',
		licenseUrl: '',
		pageUrl: '',
		attribution: ''
	},
	sorted: true,
	blocked: false,
	addedAt: 1,
	reports: { count: 0, reasons: [] },
	review: null,
	moveTo: ['anime', 'cartoon', 'fundamentals'],
	...over
});

describe('picture review', () => {
	it('builds the list query, leaving out empty filters', () => {
		expect(listQuery(DEFAULT_FILTERS, 1, 5)).toBe('status=unreviewed&sort=newest&page=1&per=48');
		const q = new URLSearchParams(
			listQuery(
				{ ...DEFAULT_FILTERS, category: 'anime', topic: 'scenery', sort: 'random', tier: '2' },
				3,
				42,
				10
			)
		);
		expect(Object.fromEntries(q)).toEqual({
			category: 'anime',
			topic: 'scenery',
			tier: '2',
			status: 'unreviewed',
			sort: 'random',
			seed: '42',
			page: '3',
			per: '10'
		});
	});

	it('maps the review keys', () => {
		expect(keyAction({ key: 'k' })).toBe('keep');
		expect(keyAction({ key: 'R' })).toBe('reject');
		expect(keyAction({ key: 'x' })).toBe('reject');
		expect(keyAction({ key: 'm' })).toBe('move');
		expect(keyAction({ key: 'ArrowRight' })).toBe('next');
		expect(keyAction({ key: 'l' })).toBe('next');
		expect(keyAction({ key: 'ArrowLeft' })).toBe('prev');
		expect(keyAction({ key: 'j' })).toBe('prev');
		expect(keyAction({ key: 'z' })).toBe('undo');
		expect(keyAction({ key: 'z', ctrlKey: true })).toBeNull();
		expect(keyAction({ key: 'r', metaKey: true })).toBeNull();
		expect(keyAction({ key: 'q' })).toBeNull();
	});

	it('applies decisions optimistically', () => {
		const p = pic();
		const kept = applyDecision(p, 'keep');
		expect(kept.review?.outcome).toBe('keep');
		expect(reviewLabel(kept)).toBe('Kept');
		const moved = applyDecision(p, 'move', { category: 'anime', topic: 'scenery' });
		expect([moved.category, moved.topic]).toEqual(['anime', 'scenery']);
		expect(moved.review?.outcome).toBe('move→anime/scenery');
		expect(reviewLabel(moved)).toBe('Moved from anime/characters');
		// moved back to where the sorter had it: a keep
		expect(applyDecision(moved, 'move', { topic: 'characters' }).review?.outcome).toBe('keep');
		const rej = applyDecision(p, 'reject');
		expect(rej.rejected).toBe(true);
		expect(reviewLabel(rej)).toBe('Rejected');
		expect(applyDecision(p, 'tier', { tier: 4 }).tier).toBe(4);
		expect(p.review).toBeNull(); // the original is untouched (for undo)
		expect(applyDecision(pic({ blocked: true }), 'keep').blocked).toBe(false);
		expect(reviewLabel(pic({ blocked: true }))).toBe('Reported');
	});

	it('offers only topics the style rules allow, minus the current one', () => {
		const topics = {
			anime: [
				{ id: 'characters', name: 'Characters' },
				{ id: 'scenery', name: 'Scenery' }
			],
			'real-life': [{ id: 'people', name: 'People' }],
			cartoon: [{ id: 'objects', name: 'Objects' }]
		};
		expect(moveOptions(pic(), topics)).toEqual([
			{ category: 'anime', topic: 'scenery', name: 'Scenery' },
			{ category: 'cartoon', topic: 'objects', name: 'Objects' }
		]);
	});

	it('computes the kept share and steps within bounds', () => {
		expect(keptPercent({ reviewed: 0, kept: 0 })).toBeNull();
		expect(keptPercent({ reviewed: 3, kept: 2 })).toBe(67);
		expect(step(0, -1, 5)).toBe(0);
		expect(step(4, 1, 5)).toBe(4);
		expect(step(2, 1, 5)).toBe(3);
		expect(step(0, 1, 0)).toBe(0);
	});

	it('runs queued jobs in order, even after a failure', async () => {
		const q = serialQueue();
		const seen: number[] = [];
		const slow = (n: number, ms: number) => () =>
			new Promise<void>((r) =>
				setTimeout(() => {
					seen.push(n);
					r();
				}, ms)
			);
		const a = q(slow(1, 20));
		const b = q(() => Promise.reject(new Error('boom')));
		const c = q(slow(3, 1));
		await a;
		await expect(b).rejects.toThrow('boom');
		await c;
		expect(seen).toEqual([1, 3]);
	});

	it('explains failures', async () => {
		expect(await failure(new Response('', { status: 404 }))).toMatch(/REF_ADMIN=1/);
		expect(
			await failure(
				new Response(JSON.stringify({ error: 'unknown topic' }), {
					status: 400,
					headers: { 'content-type': 'application/json' }
				})
			)
		).toBe('unknown topic');
		expect(await failure(new Response('only from this computer', { status: 403 }))).toBe(
			'only from this computer'
		);
	});
});
