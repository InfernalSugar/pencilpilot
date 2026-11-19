// Logic for the owner's picture review page (/dev/pictures, dev only).
// The ref-service's /api/admin/* endpoints do the work (crates/ref-service/src/admin.rs).

export type Decision = 'keep' | 'reject' | 'move' | 'tier';

export interface Review {
	decision: string;
	/** "keep", "reject" or "move→category/topic", measured against `from` */
	outcome: string;
	/** where the sorter had filed it: "category/topic" */
	from: string;
	seq: number;
	at: number;
}

export interface Picture {
	id: string;
	imageUrl: string;
	category: string;
	topic: string;
	tier: number;
	source: string;
	provider: string;
	kind: string;
	title: string;
	tags: string[];
	credit: {
		author: string;
		authorUrl: string;
		license: string;
		licenseUrl: string;
		pageUrl: string;
		attribution: string;
	};
	/** placed by the picture sorter (ADR-0009), not just by the search */
	sorted: boolean;
	/** blocked by a student's report */
	blocked: boolean;
	addedAt: number;
	reports: { count: number; reasons: string[] };
	review: Review | null;
	/** categories this picture's kind may go in (style rules) */
	moveTo: string[];
	/** client-only: rejected on this page (kept in view so it can be undone) */
	rejected?: boolean;
}

export interface TopicStats {
	category: string;
	topic: string;
	pictures: number;
	reviewed: number;
	kept: number;
	moved: number;
	rejected: number;
}

export interface Filters {
	category: string;
	topic: string;
	source: string;
	tier: string;
	status: 'unreviewed' | 'reviewed' | 'all';
	sort: 'newest' | 'reported' | 'random';
}

export const DEFAULT_FILTERS: Filters = {
	category: '',
	topic: '',
	source: '',
	tier: '',
	status: 'unreviewed',
	sort: 'newest'
};

export const SOURCES = ['pixabay', 'openverse', 'artic', 'met'] as const;

export const PER_PAGE = 48;

/** Query string for GET /api/admin/pictures. Empty filters are left out. */
export function listQuery(f: Filters, page: number, seed: number, per = PER_PAGE): string {
	const q = new URLSearchParams();
	for (const k of ['category', 'topic', 'source', 'tier', 'status', 'sort'] as const) {
		if (f[k]) q.set(k, f[k]);
	}
	if (f.sort === 'random') q.set('seed', String(seed));
	q.set('page', String(page));
	q.set('per', String(per));
	return q.toString();
}

export type KeyAction = 'keep' | 'reject' | 'move' | 'next' | 'prev' | 'undo' | 'focus' | 'close';

/** What a key does on the review page (null: nothing; modifiers are ignored). */
export function keyAction(e: {
	key: string;
	ctrlKey?: boolean;
	metaKey?: boolean;
	altKey?: boolean;
}): KeyAction | null {
	if (e.ctrlKey || e.metaKey || e.altKey) return null;
	switch (e.key) {
		case 'k':
		case 'K':
			return 'keep';
		case 'r':
		case 'R':
		case 'x':
		case 'X':
			return 'reject';
		case 'm':
		case 'M':
			return 'move';
		case 'ArrowRight':
		case 'ArrowDown':
		case 'l':
		case 'L':
			return 'next';
		case 'ArrowLeft':
		case 'ArrowUp':
		case 'j':
		case 'J':
			return 'prev';
		case 'z':
		case 'Z':
			return 'undo';
		case 'f':
		case 'F':
			return 'focus';
		case 'Escape':
			return 'close';
		default:
			return null;
	}
}

/** The picture as it will look once `decision` is applied (optimistic UI). */
export function applyDecision(
	p: Picture,
	decision: Decision,
	target: { category?: string; topic?: string; tier?: number } = {}
): Picture {
	const from = p.review?.from ?? `${p.category}/${p.topic}`;
	const at = Math.floor(Date.now() / 1000);
	const pending = (outcome: string): Review => ({ decision, outcome, from, seq: 0, at });
	switch (decision) {
		case 'reject':
			return { ...p, rejected: true, review: pending('reject') };
		case 'keep':
			return { ...p, blocked: false, review: pending(outcomeAt(from, p.category, p.topic)) };
		case 'move': {
			const category = target.category ?? p.category;
			const topic = target.topic ?? p.topic;
			return {
				...p,
				category,
				topic,
				blocked: false,
				review: pending(outcomeAt(from, category, topic))
			};
		}
		case 'tier':
			return {
				...p,
				tier: target.tier ?? p.tier,
				review: pending(outcomeAt(from, p.category, p.topic))
			};
	}
}

const outcomeAt = (from: string, category: string, topic: string) =>
	from === `${category}/${topic}` ? 'keep' : `move→${category}/${topic}`;

/** Short label for a picture's review state. */
export function reviewLabel(p: Picture): string {
	if (p.rejected || p.review?.outcome === 'reject') return 'Rejected';
	if (!p.review) return p.blocked ? 'Reported' : 'Not reviewed';
	if (p.review.outcome === 'keep') return 'Kept';
	return `Moved from ${p.review.from}`;
}

/** Kept-unchanged share of the reviewed pictures, in whole percent (the sorter's precision). */
export function keptPercent(s: Pick<TopicStats, 'reviewed' | 'kept'>): number | null {
	return s.reviewed ? Math.round((100 * s.kept) / s.reviewed) : null;
}

/** Index after a step of `delta`, kept inside 0..len-1. */
export const step = (i: number, delta: number, len: number) =>
	len ? Math.min(len - 1, Math.max(0, i + delta)) : 0;

/** Topic choices for moving `p`: allowed categories only, its own place left out. */
export function moveOptions(
	p: Pick<Picture, 'category' | 'topic' | 'moveTo'>,
	topics: Record<string, { id: string; name: string }[]>
): { category: string; topic: string; name: string }[] {
	return p.moveTo.flatMap((c) =>
		(topics[c] ?? [])
			.filter((t) => !(c === p.category && t.id === p.topic))
			.map((t) => ({ category: c, topic: t.id, name: t.name }))
	);
}

/** Runs async jobs one after another, so decisions reach the server in order. */
export function serialQueue() {
	let tail: Promise<unknown> = Promise.resolve();
	return <T>(job: () => Promise<T>): Promise<T> => {
		const run = tail.then(job, job);
		tail = run.catch(() => {});
		return run;
	};
}

/** A readable message for a failed API call. */
export async function failure(res: Response): Promise<string> {
	if (res.status === 404 && !res.headers.get('content-type')?.includes('json'))
		return 'The review API is off. Run the ref-service with REF_ADMIN=1 (pnpm dev does) and reload.';
	try {
		const body = await res.clone().json();
		if (body?.error) return String(body.error);
	} catch {
		/* not JSON */
	}
	const text = await res.text().catch(() => '');
	return text || `HTTP ${res.status}`;
}
