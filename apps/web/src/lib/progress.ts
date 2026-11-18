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
