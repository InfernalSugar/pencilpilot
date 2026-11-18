/**
 * Keeps this browser's practice history (localStorage) in sync with the
 * signed-in account. Guests: no-ops. Signing up/in merges guest history in.
 */
/** Fired on `window` after synced history lands in localStorage. */
export const SYNCED = 'pp:synced';

let signedIn = false;
let timer: ReturnType<typeof setTimeout> | undefined;

export function setSignedIn(v: boolean) {
	signedIn = v;
}

const read = <T>(key: string, empty: T): T => {
	try {
		return JSON.parse(localStorage.getItem(key) ?? '') as T;
	} catch {
		return empty;
	}
};

/** Push local history, pull the merged copy back. Returns false on failure. */
export async function syncNow(fetcher: typeof fetch = fetch): Promise<boolean> {
	try {
		const res = await fetcher('/api/progress', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				attempts: read('pp.attempts', []),
				drills: read('pp.drills', []),
				thumbs: read('pp.thumbs', {})
			})
		});
		if (!res.ok) return false;
		const s = await res.json();
		localStorage.setItem('pp.attempts', JSON.stringify(s.attempts));
		localStorage.setItem('pp.drills', JSON.stringify(s.drills));
		localStorage.setItem('pp.thumbs', JSON.stringify(s.thumbs));
		window.dispatchEvent(new Event(SYNCED));
		return true;
	} catch {
		return false;
	}
}

/** Sync soon (debounced), if signed in. Call after saving anything new. */
export function queueSync() {
	if (!signedIn || typeof window === 'undefined') return;
	clearTimeout(timer);
	timer = setTimeout(() => void syncNow(), 1500);
}
