/**
 * "Your challenge": a daily picture picked for one person from their own
 * history, separate from the global daily challenge everyone shares.
 *
 * - Category: mostly the one they practise most; now and then another one,
 *   for variety. New students rotate through the categories.
 * - Difficulty: their recommended level in that category.
 * - Topic: leans towards topics they score lowest on (or haven't tried).
 * - The same all day for that person (seeded by who they are and the date).
 */
import { categories } from './categories';
import { dailyReference, dayOf } from './daily';
import { recommendedTier, suggestTopic, type Attempt } from './progress';
import { references, tierOf, type Reference } from './references';
import { topicsFor } from './topics';

export interface PersonalChallenge {
	day: string;
	ref: Reference;
	category: string;
	tier: number;
	topic: string | null;
	/** why this one, in plain words */
	reason: string;
}

/** Chance of a category other than the favourite on any day. */
const VARIETY = 0.2;

function hash(s: string) {
	let h = 2166136261;
	for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
	return h >>> 0;
}

function seeded(seed: number) {
	return () => {
		seed = (seed + 0x6d2b79f5) | 0;
		let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
		t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
}

const KEY = 'pp.personalKey';

/** Who the challenge is for: the account, or a random id kept in this browser for guests. */
export function personalKey(userId?: string | null): string {
	if (userId) return `u:${userId}`;
	try {
		let k = localStorage.getItem(KEY);
		if (!k) {
			k = `g:${Math.random().toString(36).slice(2)}${Date.now().toString(36)}`;
			localStorage.setItem(KEY, k);
		}
		return k;
	} catch {
		return 'g:anon';
	}
}

const nameOf = (id: string) => categories.find((c) => c.id === id)?.name ?? id;

export function personalChallenge(
	day: string,
	key: string,
	history: Attempt[],
	all: readonly Reference[] = references,
	now = Date.parse(`${day}T12:00:00Z`)
): PersonalChallenge | null {
	const rand = seeded(hash(`${key}|${day}`));
	const drawable = [...new Set(all.map((r) => r.category))].sort();
	if (!drawable.length) return null;

	// What they've been practising (last 60 days of scored drawings).
	const recent = history.filter(
		(a) => now - a.at < 60 * 86_400_000 && drawable.includes(a.category)
	);
	const counts = new Map<string, number>();
	for (const a of recent) counts.set(a.category, (counts.get(a.category) ?? 0) + 1);
	const ranked = [...counts.entries()].sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1));

	let category: string;
	let reason: string;
	if (!ranked.length) {
		const dayIndex = Math.round(Date.parse(`${day}T00:00:00Z`) / 86_400_000);
		category = drawable[dayIndex % drawable.length];
		reason = `Something to start you off: ${nameOf(category)}. Draw a few and this gets more personal.`;
	} else if (ranked.length === 1 || rand() >= VARIETY) {
		category = ranked[0][0];
		reason = `You draw a lot of ${nameOf(category)}.`;
	} else {
		// variety: another category, weighted by how much they draw it (untried ones count a little)
		const others = drawable.filter((c) => c !== ranked[0][0]);
		const weights = others.map((c) => (counts.get(c) ?? 0) + 0.5);
		let r = rand() * weights.reduce((s, w) => s + w, 0);
		category = others[others.length - 1];
		for (let i = 0; i < others.length; i++)
			if ((r -= weights[i]) <= 0) {
				category = others[i];
				break;
			}
		reason = `A change from ${nameOf(ranked[0][0])} today: ${nameOf(category)}.`;
	}

	const tier = recommendedTier(category, history);
	const inCat = all.filter((r) => r.category === category);
	const topicIds = topicsFor(category)
		.map((t) => t.id)
		.filter((t) => inCat.some((r) => r.topic === t && tierOf(r) === tier));
	const topic = suggestTopic(category, topicIds, history, rand) ?? null;
	const tried = history.filter((a) => a.topic === topic && a.category === category);
	if (topic && tried.length && ranked.length) {
		const avg = tried.reduce((s, a) => s + a.score, 0) / tried.length;
		if (avg < 6.5) reason += ` Today’s topic is one you find tricky.`;
	}

	// The picture: that topic and level if possible, then relax; skip ones drawn
	// lately and the shared daily picture.
	const avoid = new Set([
		...history.filter((a) => now - a.at < 30 * 86_400_000).map((a) => a.refId),
		dailyReference(day, all)?.id
	]);
	const byId = (a: Reference, b: Reference) => (a.id < b.id ? -1 : 1);
	const pools = [
		inCat.filter((r) => tierOf(r) === tier && (!topic || r.topic === topic)),
		inCat.filter((r) => tierOf(r) === tier),
		inCat.slice()
	];
	// a fresh picture from the closest pool first; repeats only as a last resort
	for (const fresh of [true, false])
		for (const pool of pools) {
			const list = pool.filter((r) => !fresh || !avoid.has(r.id)).sort(byId);
			if (list.length)
				return { day, ref: list[Math.floor(rand() * list.length)], category, tier, topic, reason };
		}
	return null;
}

/** Their earlier average in a category (before `day`), for "how did today go". */
export function categoryAverage(history: Attempt[], category: string, before: number) {
	const xs = history.filter((a) => a.category === category && a.at < before).map((a) => a.score);
	return xs.length ? xs.reduce((s, x) => s + x, 0) / xs.length : null;
}

/** Days in a row with a personal challenge done, up to today (or yesterday if today's still open). */
export function personalStreak(history: Attempt[], today = dayOf()): number {
	const days = new Set(history.map((a) => a.personal).filter(Boolean));
	const prev = (d: string) => dayOf(new Date(Date.parse(`${d}T00:00:00Z`) - 86_400_000));
	let d = days.has(today) ? today : prev(today);
	let n = 0;
	while (days.has(d)) {
		n++;
		d = prev(d);
	}
	return n;
}

/** Today's personal challenge attempt, if done. */
export const personalDone = (history: Attempt[], day: string) =>
	history.find((a) => a.personal === day) ?? null;

/** Link that draws someone's personal challenge. */
export const personalUrl = (c: PersonalChallenge) =>
	`/draw?category=${c.category}&ref=${c.ref.id}&personal=${c.day}${c.topic ? `&topic=${c.topic}` : ''}&tier=${c.tier}`;

const PICK = 'pp.personalPick';

/**
 * Today's challenge for this person, remembered on this device once picked
 * (so it can't change during the day when synced history arrives).
 */
export function todaysPersonal(
	key: string,
	history: Attempt[],
	day = dayOf(),
	all: readonly Reference[] = references
): PersonalChallenge | null {
	try {
		const saved = JSON.parse(localStorage.getItem(PICK) ?? 'null');
		if (saved?.day === day && saved?.key === key) {
			const ref = all.find((r) => r.id === saved.refId);
			if (ref) return { ...saved.c, ref };
		}
	} catch {
		/* storage blocked or bad value: pick afresh */
	}
	const c = personalChallenge(day, key, history, all);
	if (c)
		try {
			const { ref, ...rest } = c;
			localStorage.setItem(PICK, JSON.stringify({ day, key, refId: ref.id, c: rest }));
		} catch {
			/* storage blocked: picked again next time (same result for the same history) */
		}
	return c;
}
