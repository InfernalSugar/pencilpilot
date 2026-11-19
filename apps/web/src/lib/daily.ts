/**
 * The daily challenge: one reference a day, the same for everyone, from the
 * calmer pictures (Beginner and Easy). Days are UTC dates (YYYY-MM-DD).
 * Each year the pool is shuffled once, then walked day by day, so a picture
 * doesn't come back until the pool has gone round.
 */
import { references, tierOf, type Reference } from './references';

export const dayOf = (d: Date = new Date()) => d.toISOString().slice(0, 10);

export function isDay(s: unknown): s is string {
	if (typeof s !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(s)) return false;
	const t = Date.parse(`${s}T00:00:00Z`);
	return !Number.isNaN(t) && dayOf(new Date(t)) === s;
}

/** Calm pictures: Beginner and Easy, in a fixed order. */
export function dailyPool(all: readonly Reference[] = references): Reference[] {
	return all.filter((r) => tierOf(r) <= 2).sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
}

function mulberry32(seed: number) {
	return () => {
		seed = (seed + 0x6d2b79f5) | 0;
		let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
		t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
}

export function dailyReference(day: string, all: readonly Reference[] = references) {
	const pool = dailyPool(all);
	if (!pool.length || !isDay(day)) return undefined;
	const year = Number(day.slice(0, 4));
	const rand = mulberry32(year * 7919);
	for (let i = pool.length - 1; i > 0; i--) {
		const j = Math.floor(rand() * (i + 1));
		[pool[i], pool[j]] = [pool[j], pool[i]];
	}
	const dayOfYear = Math.round(
		(Date.parse(`${day}T00:00:00Z`) - Date.parse(`${year}-01-01T00:00:00Z`)) / 86_400_000
	);
	return pool[dayOfYear % pool.length];
}

export function msToNextDay(now = new Date()) {
	const next = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() + 1);
	return next - now.getTime();
}

/** Share of other players this score beat (ties count half), 0–1. */
export function percentile(mine: number, scores: number[]) {
	const others = [...scores];
	const i = others.indexOf(mine);
	if (i >= 0) others.splice(i, 1);
	if (!others.length) return 1;
	const below = others.filter((s) => s < mine).length;
	const ties = others.filter((s) => s === mine).length;
	return (below + ties / 2) / others.length;
}

/** Link that draws a day's challenge. */
export const dailyUrl = (day: string, r: Reference) =>
	`/draw?category=${r.category}&ref=${r.id}&daily=${day}`;
