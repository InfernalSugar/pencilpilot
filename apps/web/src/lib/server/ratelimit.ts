/**
 * Small in-memory token-bucket rate limiter (one server process). Used to slow
 * down password guessing: e.g. 5 tries per email per 15 minutes, 30 per IP.
 */
interface Bucket {
	tokens: number;
	updated: number;
}

export class RateLimiter {
	#buckets = new Map<string, Bucket>();
	constructor(
		readonly capacity: number,
		readonly refillMs: number
	) {}

	/** Take one token for `key`; false when the bucket is empty. */
	take(key: string, now = Date.now()): boolean {
		const b = this.#buckets.get(key) ?? { tokens: this.capacity, updated: now };
		const refill = Math.floor((now - b.updated) / this.refillMs);
		if (refill > 0) {
			b.tokens = Math.min(this.capacity, b.tokens + refill);
			b.updated = now;
		}
		if (this.#buckets.size > 10_000) this.#buckets.clear(); // crude memory cap
		this.#buckets.set(key, b);
		if (b.tokens <= 0) return false;
		b.tokens--;
		return true;
	}

	reset(key: string) {
		this.#buckets.delete(key);
	}
}

/** 5 attempts per email, refilling one every 3 minutes. */
export const loginByEmail = new RateLimiter(5, 3 * 60 * 1000);
/** 30 auth requests per IP, refilling one every 20 seconds. */
export const authByIp = new RateLimiter(30, 20 * 1000);
