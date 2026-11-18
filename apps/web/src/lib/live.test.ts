import { describe, expect, it } from 'vitest';
import { fetchLiveReference } from './live';

const ok = (body: unknown, status = 200) =>
	(async () =>
		new Response(status === 204 ? null : JSON.stringify(body), { status })) as typeof fetch;

describe('fetchLiveReference', () => {
	it('returns the reference on 200 and builds the query', async () => {
		let url = '';
		const f = (async (u: string) => {
			url = u;
			return new Response(JSON.stringify({ id: 'px-1' }), { status: 200 });
		}) as unknown as typeof fetch;
		const r = await fetchLiveReference(
			{ category: 'animals', topic: 'pets', tier: 2, exclude: ['a', 'b'] },
			1000,
			f
		);
		expect(r?.id).toBe('px-1');
		expect(url).toBe('/api/ref?category=animals&topic=pets&tier=2&exclude=a%2Cb');
	});
	it('returns null on 204, errors and timeouts', async () => {
		expect(await fetchLiveReference({ category: 'animals' }, 1000, ok(null, 204))).toBeNull();
		const boom = (async () => {
			throw new Error('down');
		}) as unknown as typeof fetch;
		expect(await fetchLiveReference({ category: 'animals' }, 1000, boom)).toBeNull();
		const slow = ((_u: string, init?: RequestInit) =>
			new Promise((_, rej) =>
				init?.signal?.addEventListener('abort', () => rej(new Error('aborted')))
			)) as unknown as typeof fetch;
		expect(await fetchLiveReference({ category: 'animals' }, 20, slow)).toBeNull();
	});
});
