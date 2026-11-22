/**
 * What home asked Draw together to do as the studio opens: host, join with a code, or
 * rejoin a shared project's session. Kept in memory between the two screens, so an
 * invite code never goes into an address (or anything that logs one).
 */
export type TogetherStart = { what: 'host' } | { what: 'join'; code: string } | { what: 'rejoin' };

let pending: TogetherStart | null = null;

/** Home: the studio should do this as it opens. */
export function askTogether(start: TogetherStart) {
	pending = start;
}

/** The studio: what home asked (once; it's forgotten as it's taken). */
export function takeTogether(): TogetherStart | null {
	const p = pending;
	pending = null;
	return p;
}
