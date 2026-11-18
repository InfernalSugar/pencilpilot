import type { Reference } from './references';

/**
 * Endless references (M5b): ask the ref-service for a vetted Pixabay picture.
 * Resolves to `null` when nothing is ready, the service is down, or it takes
 * longer than `timeoutMs`, so callers can fall back to curated references.
 */
export async function fetchLiveReference(
	q: { category: string; topic?: string | null; tier?: number | null; exclude?: string[] },
	timeoutMs = 1500,
	fetcher: typeof fetch = fetch
): Promise<Reference | null> {
	const params = new URLSearchParams({ category: q.category });
	if (q.topic) params.set('topic', q.topic);
	if (q.tier) params.set('tier', String(q.tier));
	if (q.exclude?.length) params.set('exclude', q.exclude.slice(-30).join(','));
	try {
		const res = await fetcher(`/api/ref?${params}`, { signal: AbortSignal.timeout(timeoutMs) });
		if (res.status !== 200) return null;
		return (await res.json()) as Reference;
	} catch {
		return null;
	}
}

export async function reportReference(id: string, reason: string, fetcher: typeof fetch = fetch) {
	const res = await fetcher('/api/report', {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify({ id, reason })
	});
	return res.ok;
}

export async function liveAvailable(fetcher: typeof fetch = fetch): Promise<boolean> {
	try {
		const res = await fetcher('/api/health', { signal: AbortSignal.timeout(800) });
		return res.ok;
	} catch {
		return false;
	}
}

const RECENT = 'pp.recentRefs';

/** Ids this browser saw recently, so we don't repeat pictures. */
export function recentRefs(): string[] {
	try {
		return JSON.parse(localStorage.getItem(RECENT) ?? '[]');
	} catch {
		return [];
	}
}

export function rememberRef(id: string) {
	try {
		const list = [...recentRefs().filter((x) => x !== id), id].slice(-60);
		localStorage.setItem(RECENT, JSON.stringify(list));
	} catch {
		/* storage unavailable */
	}
}
