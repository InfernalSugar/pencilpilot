/**
 * Drawing together, peer to peer (ADR-0013, S12-T03). The session runs in the app's Rust
 * side (`share_*` commands, "share-event" events); the window itself stays offline.
 * Nothing connects until the person hosts or joins.
 *
 * Drawing together needs a PencilPilot account (`account_*` commands): the shell links the
 * app to the website in the system browser, then gets a signed pass for each session.
 * Names come from the account; solo work never needs one.
 */

export interface Member {
	id: string;
	/** their name, from their PencilPilot account */
	name: string;
	/** their account's id */
	user?: string;
	/** how to reach them (kept for rejoining later) */
	addr?: unknown;
}

export interface ShareInfo {
	/** the code to share (pp1-…) */
	invite: string;
	me: Member;
	members: Member[];
	host: boolean;
}

export type ShareEvent =
	| { kind: 'edit'; seq: number; from: string; body: unknown }
	| { kind: 'presence'; from: string; body: unknown }
	| { kind: 'direct'; from: string; body: unknown }
	| { kind: 'joined'; member: Member }
	| { kind: 'left'; id: string }
	/** the host left and `host` took over (`you`: this app); who's here; the new code */
	| { kind: 'host_changed'; host: string; you: boolean; members: Member[]; invite: string }
	| { kind: 'closed'; reason: string };

/** What a cursor message carries. */
export interface CursorBody {
	x: number;
	y: number;
}

/** A member's colour: picked from their id, so it's the same on every app. */
const COLOURS = [
	'#e4572e',
	'#2e86ab',
	'#7b2cbf',
	'#2a9d8f',
	'#d1495b',
	'#ef8a17',
	'#3a5a40',
	'#c9184a'
];
export function memberColour(id: string): string {
	let h = 0;
	for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
	return COLOURS[h % COLOURS.length];
}

/** The invite code from pasted text (one unbroken token): anything around it is dropped. */
export function cleanInvite(text: string): string | null {
	const m = text.match(/pp1-[A-Za-z0-9_-]+/);
	return m ? m[0] : null;
}

export function isCursor(v: unknown): v is CursorBody {
	const c = v as CursorBody;
	return !!c && typeof c === 'object' && Number.isFinite(c.x) && Number.isFinite(c.y);
}

/**
 * Others' cursors: the newest position of each member, dropped after `staleMs`
 * without news (they stopped moving over the canvas, or left).
 */
export class Cursors {
	#at = new Map<string, { x: number; y: number; t: number }>();
	constructor(readonly staleMs = 4000) {}

	/** returns whether anything changed */
	set(id: string, x: number, y: number, now: number): boolean {
		const old = this.#at.get(id);
		this.#at.set(id, { x, y, t: now });
		return !old || old.x !== x || old.y !== y;
	}

	remove(id: string): boolean {
		return this.#at.delete(id);
	}

	clear() {
		this.#at.clear();
	}

	/** drops stale cursors; returns whether any went */
	prune(now: number): boolean {
		let gone = false;
		for (const [id, c] of this.#at)
			if (now - c.t > this.staleMs) {
				this.#at.delete(id);
				gone = true;
			}
		return gone;
	}

	list(members: Member[]) {
		const name = new Map(members.map((m) => [m.id, m.name]));
		return [...this.#at].map(([id, c]) => ({
			id,
			name: name.get(id) ?? 'Someone',
			color: memberColour(id),
			x: c.x,
			y: c.y
		}));
	}
}

/**
 * Sends at most one cursor every `gapMs` (20 a second by default), always ending with the
 * newest position so others don't see a cursor stop short.
 */
export class Throttle<T> {
	#last = -Infinity;
	#pending: T | undefined;
	#timer: ReturnType<typeof setTimeout> | undefined;
	constructor(
		readonly send: (v: T) => void,
		readonly gapMs = 50,
		readonly now: () => number = () => performance.now()
	) {}

	push(v: T) {
		const t = this.now();
		if (t - this.#last >= this.gapMs) {
			this.#last = t;
			this.send(v);
			return;
		}
		this.#pending = v;
		this.#timer ??= setTimeout(
			() => {
				this.#timer = undefined;
				if (this.#pending === undefined) return;
				const p = this.#pending;
				this.#pending = undefined;
				this.#last = this.now();
				this.send(p);
			},
			this.gapMs - (t - this.#last)
		);
	}

	stop() {
		clearTimeout(this.#timer);
		this.#timer = undefined;
		this.#pending = undefined;
	}
}

// ---- the app's Rust side ----

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
	const { invoke } = await import('@tauri-apps/api/core');
	return invoke<T>(cmd, args);
}

/** Who this app is signed in as. */
export interface Account {
	user: string;
	name: string;
}

/** The PencilPilot account this app is signed in with, or null. */
export const accountStatus = () => call<Account | null>('account_status');
/** Sign in in the system browser; resolves once the person has confirmed on the website. */
export const signIn = () => call<Account>('account_sign_in');
/** Stop waiting for the browser. */
export const cancelSignIn = () => call<void>('account_cancel');
export const signOut = () => call<void>('account_sign_out');

/** What the Draw together dialog can offer: signing in comes first. */
export function shareChoices(account: Account | null, busy: boolean) {
	const ready = !!account && !busy;
	return { signIn: !account, host: ready, join: ready, rejoin: ready };
}

/** `relay`: our own relay's URL (Settings), or empty for the default */
export const hostSession = (relay = '') => call<ShareInfo>('share_host', { relay: relay || null });
export const joinSession = (code: string, relay = '') =>
	call<ShareInfo>('share_join', { code, relay: relay || null });

/** A relay URL as typed: https only, no spaces; '' when empty or not a URL. */
export function cleanRelay(text: string): string {
	const t = text.trim();
	if (!t) return '';
	try {
		const u = new URL(t.includes('://') ? t : `https://${t}`);
		return u.protocol === 'https:' && u.hostname.includes('.') ? u.origin : '';
	} catch {
		return '';
	}
}
/** Back to a shared project: join a member who's online, or host it again. */
export const rejoinSession = (code: string, addrs: unknown[], relay = '') =>
	call<ShareInfo>('share_rejoin', { code, addrs, relay: relay || null });
export const sessionStatus = () => call<ShareInfo | null>('share_status');
export const sendEdit = (body: unknown) => call<void>('share_edit', { body });
export const sendPresence = (body: unknown) => call<void>('share_presence', { body });
export const sendDirect = (to: string, body: unknown) => call<void>('share_direct', { to, body });
export const leaveSession = () => call<void>('share_leave');
/** An invite code as a QR code (SVG markup). */
export const inviteQr = (code: string) => call<string>('share_qr', { code });

/**
 * AI generation is a solo act (ADR-0013): AI tools stay hidden and refuse to run while
 * drawing together. The shell checks again before running any AI job.
 */
export const aiAllowed = (sharing: ShareInfo | null) => sharing === null;

/** Listen to the session's events; returns the function that stops listening. */
export async function onShareEvent(f: (e: ShareEvent) => void): Promise<() => void> {
	const { listen } = await import('@tauri-apps/api/event');
	return listen<ShareEvent>('share-event', (e) => f(e.payload));
}
