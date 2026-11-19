import { fetchLiveReference } from '../live';
import { filtered, imageUrl, titleOf, type Reference } from '../references';
import type { LobbyRef } from './protocol';

export function toLobbyRef(r: Reference): LobbyRef {
	return {
		id: r.id,
		category: r.category,
		title: titleOf(r),
		imageUrl: imageUrl(r),
		author: r.author,
		authorUrl: r.authorUrl,
		pageUrl: r.pageUrl,
		license: r.license,
		licenseUrl: r.licenseUrl,
		tier: r.tier,
		live: r.live,
		tags: r.tags,
		source: r.source,
		provider: r.provider,
		attribution: r.attribution
	};
}

/** `n` different pictures for a game: hand-picked first, Endless ones to fill up. */
export async function pickRefs(category: string, tier: number, n: number): Promise<LobbyRef[]> {
	const pool = filtered(category, { tier }).slice();
	for (let i = pool.length - 1; i > 0; i--) {
		const j = Math.floor(Math.random() * (i + 1));
		[pool[i], pool[j]] = [pool[j], pool[i]];
	}
	const out = pool.slice(0, n).map(toLobbyRef);
	for (let tries = 0; out.length < n && tries < n * 3; tries++) {
		const r = await fetchLiveReference({ category, tier, exclude: out.map((x) => x.id) }, 3000);
		if (r && !out.some((x) => x.id === r.id)) out.push(toLobbyRef(r));
	}
	return out;
}

/** The lobby picture as a grader/viewer reference. */
export const asReference = (r: LobbyRef) => r as unknown as Reference;
