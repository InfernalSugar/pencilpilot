import { queueSync } from './sync';

/**
 * Per-browser practice history (until accounts arrive in M7), used to suggest
 * when to move up a difficulty tier. Stored in localStorage; every access is
 * guarded so the site works with storage blocked.
 */
export interface Attempt {
	category: string;
	topic: string | null;
	tier: number;
	score: number;
	at: number;
	/** reference id and title, for the history list */
	refId?: string;
	title?: string;
	/** rule ids of the fixes shown (feedback engine), for pattern spotting */
	fixes?: string[];
	/** 0–1 per skill: placement, outline, silhouette, tone, line_confidence */
	skills?: Record<string, number>;
}

const KEY = 'pp.attempts';
const MAX = 300;

export function attempts(): Attempt[] {
	try {
		return JSON.parse(localStorage.getItem(KEY) ?? '[]');
	} catch {
		return [];
	}
}

export function recordAttempt(a: Attempt) {
	try {
		const list = [...attempts(), a].slice(-MAX);
		localStorage.setItem(KEY, JSON.stringify(list));
		queueSync();
	} catch {
		/* storage unavailable: history is skipped */
	}
}

/** Scores at or above this count as "comfortable" at a tier. */
export const LEVEL_UP_SCORE = 7.5;
/** How many recent comfortable drawings unlock the next tier suggestion. */
export const LEVEL_UP_COUNT = 3;

/**
 * The tier to suggest for a category: start at Beginner; move up once the last
 * LEVEL_UP_COUNT drawings at the current tier all scored LEVEL_UP_SCORE or more.
 */
export function recommendedTier(category: string, history: Attempt[] = attempts()): number {
	let tier = 1;
	for (;;) {
		const recent = history
			.filter((a) => a.category === category && a.tier === tier)
			.slice(-LEVEL_UP_COUNT);
		if (
			tier < 4 &&
			recent.length === LEVEL_UP_COUNT &&
			recent.every((a) => a.score >= LEVEL_UP_SCORE)
		)
			tier++;
		else return tier;
	}
}

// ── Analytics for the progress page ─────────────────────────────────────

const DAY = 24 * 3600 * 1000;
const dayKey = (t: number) => {
	const d = new Date(t);
	return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
};

/** Consecutive days with at least one drawing, ending today (or yesterday, so a streak survives until midnight). */
export function streakDays(history: Attempt[], now = Date.now()): number {
	const days = new Set(history.map((a) => dayKey(a.at)));
	let t = now;
	if (!days.has(dayKey(t))) t -= DAY;
	let n = 0;
	while (days.has(dayKey(t))) {
		n++;
		t -= DAY;
	}
	return n;
}

/** Average score of attempts in the last `days` days (null if none). */
export function averageScore(history: Attempt[], days: number, now = Date.now()): number | null {
	const recent = history.filter((a) => now - a.at <= days * DAY);
	return recent.length ? recent.reduce((s, a) => s + a.score, 0) / recent.length : null;
}

export const bestScore = (history: Attempt[]) =>
	history.length ? Math.max(...history.map((a) => a.score)) : null;

/** Mean of each skill over the last `n` attempts that recorded skills. */
export function skillAverages(history: Attempt[], n = 20): Record<string, number> {
	const recent = history.filter((a) => a.skills).slice(-n);
	const sums: Record<string, [number, number]> = {};
	for (const a of recent)
		for (const [k, v] of Object.entries(a.skills!)) {
			const e = (sums[k] ??= [0, 0]);
			e[0] += v;
			e[1]++;
		}
	return Object.fromEntries(Object.entries(sums).map(([k, [s, c]]) => [k, s / c]));
}

export interface Pattern {
	ruleId: string;
	count: number;
	window: number;
}

/**
 * Mistakes that keep coming back: rule ids that appeared in at least `min` of
 * the last `window` graded drawings, most frequent first.
 */
export function recurringMistakes(history: Attempt[], window = 5, min = 3): Pattern[] {
	const recent = history.filter((a) => a.fixes).slice(-window);
	const counts = new Map<string, number>();
	for (const a of recent) for (const f of new Set(a.fixes)) counts.set(f, (counts.get(f) ?? 0) + 1);
	return [...counts.entries()]
		.filter(([, c]) => c >= min)
		.sort((a, b) => b[1] - a[1])
		.map(([ruleId, count]) => ({ ruleId, count, window: recent.length }));
}

/** Daily average scores for a chart: `[{ day: ms at midnight, avg, n }]`, oldest first. */
export function dailyScores(history: Attempt[], days: number | null, now = Date.now()) {
	const from = days === null ? -Infinity : now - days * DAY;
	const by = new Map<string, { day: number; sum: number; n: number }>();
	for (const a of history) {
		if (a.at < from) continue;
		const d = new Date(a.at);
		const day = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
		const e = by.get(dayKey(a.at)) ?? { day, sum: 0, n: 0 };
		e.sum += a.score;
		e.n++;
		by.set(dayKey(a.at), e);
	}
	return [...by.values()]
		.sort((a, b) => a.day - b.day)
		.map((e) => ({ day: e.day, avg: e.sum / e.n, n: e.n }));
}

/**
 * For "Surprise me": pick a topic, favouring ones the student scores lowest on
 * (and ones they haven't tried), so practice goes where it helps most.
 */
export function suggestTopic(
	category: string,
	topics: string[],
	history: Attempt[] = attempts(),
	rand = Math.random
): string | undefined {
	if (!topics.length) return undefined;
	const weights = topics.map((t) => {
		const scores = history
			.filter((a) => a.category === category && a.topic === t)
			.map((a) => a.score);
		if (!scores.length) return 6; // untried: worth exploring
		const avg = scores.reduce((s, x) => s + x, 0) / scores.length;
		return Math.max(1, 11 - avg); // weaker topic → more likely
	});
	let r = rand() * weights.reduce((s, w) => s + w, 0);
	for (let i = 0; i < topics.length; i++) {
		r -= weights[i];
		if (r <= 0) return topics[i];
	}
	return topics.at(-1);
}

// ── Thumbnails of recent drawings (small JPEGs, last 24) ────────────────

const THUMBS = 'pp.thumbs';

export function thumbs(): Record<string, string> {
	try {
		return JSON.parse(localStorage.getItem(THUMBS) ?? '{}');
	} catch {
		return {};
	}
}

/** Store a ~200 px JPEG of a drawing under its attempt time. */
export async function saveThumb(at: number, png: string) {
	try {
		const img = new Image();
		img.src = png;
		await img.decode();
		const c = document.createElement('canvas');
		c.width = 200;
		c.height = 150;
		const ctx = c.getContext('2d')!;
		ctx.fillStyle = '#fff';
		ctx.fillRect(0, 0, 200, 150);
		// any canvas shape: fit inside the 4:3 thumbnail, centred
		const s = Math.min(200 / img.naturalWidth, 150 / img.naturalHeight);
		const [w, h] = [img.naturalWidth * s, img.naturalHeight * s];
		ctx.drawImage(img, (200 - w) / 2, (150 - h) / 2, w, h);
		const all = thumbs();
		all[at] = c.toDataURL('image/jpeg', 0.7);
		const keep = Object.keys(all)
			.map(Number)
			.sort((a, b) => b - a)
			.slice(0, 24);
		localStorage.setItem(THUMBS, JSON.stringify(Object.fromEntries(keep.map((k) => [k, all[k]]))));
		queueSync();
	} catch {
		/* storage full or blocked: thumbnails are optional */
	}
}
