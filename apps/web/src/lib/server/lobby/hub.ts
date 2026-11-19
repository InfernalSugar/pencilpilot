/**
 * Private lobbies: rooms, players and the game flow. Plain logic (no
 * sockets, no SvelteKit), so it can be unit-tested and shared by the dev,
 * preview and production servers. Rooms live in memory and vanish when empty.
 */
import { randomBytes, randomInt } from 'node:crypto';
import {
	cleanSettings,
	DEFAULT_SETTINGS,
	MAX_PLAYERS,
	type ClientMsg,
	type LobbyRef,
	type LobbySettings,
	type LobbyView,
	type Phase,
	type RoundResult,
	type ServerMsg,
	type StrokeStyle,
	type WireStroke
} from '../../lobby/protocol';

/** Seconds after time is up to wait for drawings still on their way. */
export const COLLECT_GRACE_MS = 15_000;
/** Empty rooms are closed after this long. */
export const EMPTY_ROOM_MS = 30 * 60_000;
/** Shared-canvas size limit (numbers stored across all strokes in a room). */
export const MAX_STROKE_NUMBERS = 3_000_000;
const MAX_PNG_CHARS = 2_500_000;

export interface Identity {
	playerId: string;
	name: string;
	/** account id; null for guests */
	userId: string | null;
}

interface Player extends Identity {
	joinedAt: number;
	send: ((m: ServerMsg) => void) | null;
}

interface Submission {
	png: string | null;
	score: number | null;
}

interface Room {
	code: string;
	hostId: string;
	settings: LobbySettings;
	players: Player[];
	kicked: Set<string>;
	phase: Phase;
	round: number;
	refs: LobbyRef[];
	reference: LobbyRef | null;
	endsAt: number | null;
	submissions: Map<string, Submission>;
	order: string[];
	revealIndex: number;
	reactions: Map<string, Map<string, 1 | -1>>;
	results: RoundResult[];
	strokes: Map<string, WireStroke[]>;
	/** strokes being drawn now, per player and stroke key */
	live: Map<string, Map<number, WireStroke>>;
	strokeNumbers: number;
	timer: (() => void) | null;
	emptySince: number | null;
}

export interface Clock {
	now(): number;
	/** run `fn` after `ms`; returns a cancel function */
	later(fn: () => void, ms: number): () => void;
}

const realClock: Clock = {
	now: () => Date.now(),
	later: (fn, ms) => {
		const id = setTimeout(fn, ms);
		return () => clearTimeout(id);
	}
};

const CODE_CHARS = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789';

export class Hub {
	rooms = new Map<string, Room>();
	readonly clock: Clock;

	constructor(clock: Clock = realClock) {
		this.clock = clock;
	}

	newPlayerId() {
		return randomBytes(9).toString('base64url');
	}

	/** Open a room; `host` must be signed in. Returns its invite code. */
	create(host: Identity): string {
		if (!host.userId) throw new Error('hosts must be signed in');
		this.sweep();
		let code: string;
		do code = Array.from({ length: 6 }, () => CODE_CHARS[randomInt(CODE_CHARS.length)]).join('');
		while (this.rooms.has(code));
		this.rooms.set(code, {
			code,
			hostId: host.playerId,
			settings: { ...DEFAULT_SETTINGS },
			players: [],
			kicked: new Set(),
			phase: 'waiting',
			round: 0,
			refs: [],
			reference: null,
			endsAt: null,
			submissions: new Map(),
			order: [],
			revealIndex: 0,
			reactions: new Map(),
			results: [],
			strokes: new Map(),
			live: new Map(),
			strokeNumbers: 0,
			timer: null,
			emptySince: this.clock.now()
		});
		return code;
	}

	exists(code: string) {
		return this.rooms.has(code);
	}

	/** Close rooms that have been empty for a while. */
	sweep() {
		const now = this.clock.now();
		for (const [code, r] of this.rooms)
			if (r.emptySince !== null && now - r.emptySince > EMPTY_ROOM_MS) {
				r.timer?.();
				this.rooms.delete(code);
			}
	}

	/** Connect a player (new or returning). Returns an error message, or null. */
	join(code: string, who: Identity, send: (m: ServerMsg) => void): string | null {
		const r = this.rooms.get(code);
		if (!r) return 'That lobby doesn’t exist (or has closed).';
		if (r.kicked.has(who.playerId) || (who.userId && r.kicked.has(`u:${who.userId}`)))
			return 'The host removed you from this lobby.';
		let p = r.players.find((x) => x.playerId === who.playerId);
		if (!p) {
			if (r.players.length >= MAX_PLAYERS) return 'This lobby is full.';
			p = { ...who, joinedAt: this.clock.now(), send: null };
			r.players.push(p);
		}
		p.send?.({ t: 'error', message: 'You opened this lobby somewhere else.' });
		p.send = send;
		p.name = who.name;
		r.emptySince = null;
		this.broadcastState(r);
		if (r.phase === 'free') send({ t: 'sync', strokes: Object.fromEntries(r.strokes) });
		return null;
	}

	/** A player's connection closed. */
	disconnect(code: string, playerId: string, send: (m: ServerMsg) => void) {
		const r = this.rooms.get(code);
		const p = r?.players.find((x) => x.playerId === playerId);
		if (!r || !p || p.send !== send) return;
		p.send = null;
		r.live.delete(playerId);
		if (r.hostId === playerId) this.passHost(r);
		if (!r.players.some((x) => x.send)) r.emptySince = this.clock.now();
		this.broadcastState(r);
		this.maybeFinishCollecting(r);
	}

	/** The host left: the longest-standing connected member takes over. */
	private passHost(r: Room) {
		const next = r.players.find((x) => x.send && x.userId) ?? r.players.find((x) => x.send) ?? null;
		if (next) r.hostId = next.playerId;
	}

	handle(code: string, playerId: string, msg: ClientMsg) {
		const r = this.rooms.get(code);
		const p = r?.players.find((x) => x.playerId === playerId);
		if (!r || !p) return;
		const host = r.hostId === playerId;
		const fail = (message: string) => p.send?.({ t: 'error', message });
		switch (msg.t) {
			case 'settings':
				if (!host || r.phase !== 'waiting') return fail('Only the host can change settings.');
				r.settings = cleanSettings(msg.settings ?? {}, r.settings);
				return this.broadcastState(r);
			case 'start':
				if (!host) return fail('Only the host can start.');
				if (r.phase !== 'waiting') return;
				return r.settings.mode === 'free' ? this.startFree(r) : this.startGame(r, msg.refs ?? []);
			case 'end':
				if (!host) return;
				return this.toWaiting(r);
			case 'again':
				if (!host || r.phase !== 'final') return;
				return this.toWaiting(r);
			case 'kick': {
				if (!host || msg.playerId === playerId) return;
				const k = r.players.find((x) => x.playerId === msg.playerId);
				if (!k) return;
				r.kicked.add(k.playerId);
				if (k.userId) r.kicked.add(`u:${k.userId}`);
				k.send?.({ t: 'kicked' });
				r.players = r.players.filter((x) => x !== k);
				r.strokes.delete(k.playerId);
				r.live.delete(k.playerId);
				this.broadcastState(r);
				return this.maybeFinishCollecting(r);
			}
			case 'submit':
				return this.submit(r, p, msg.png, msg.score);
			case 'next':
				if (!host) return;
				return this.next(r);
			case 'react':
				return this.react(r, p, msg.target, msg.v);
			case 'reference':
				if (r.phase !== 'free') return;
				r.reference = msg.ref ? cleanRef(msg.ref) : null;
				return this.broadcastState(r);
			case 'sb':
			case 'sp':
			case 'se':
			case 'undo':
			case 'clear':
				if (r.phase !== 'free') return;
				return this.stroke(r, p, msg);
		}
	}

	// ---- free draw ----

	private startFree(r: Room) {
		r.phase = 'free';
		r.reference = null;
		r.strokes = new Map();
		r.live = new Map();
		r.strokeNumbers = 0;
		this.broadcastState(r);
		this.broadcast(r, { t: 'sync', strokes: {} });
	}

	private stroke(r: Room, p: Player, msg: ClientMsg) {
		const id = p.playerId;
		const mine = r.strokes.get(id) ?? [];
		const live = r.live.get(id) ?? new Map<number, WireStroke>();
		r.live.set(id, live);
		const relay = (m: ServerMsg) => this.broadcast(r, m, id);
		if (msg.t === 'sb') {
			if (!Number.isInteger(msg.k)) return;
			live.set(msg.k, { style: cleanStyle(msg.style), pts: [] });
			relay({ t: 'sb', from: id, k: msg.k, style: live.get(msg.k)!.style });
		} else if (msg.t === 'sp') {
			const s = live.get(msg.k);
			if (!s || !Array.isArray(msg.pts) || msg.pts.length % 4 || msg.pts.length > 4000) return;
			if (!msg.pts.every((v) => typeof v === 'number' && Number.isFinite(v))) return;
			if (r.strokeNumbers + msg.pts.length > MAX_STROKE_NUMBERS)
				return p.send?.({
					t: 'error',
					message: 'This canvas is full. Clear your layer to keep drawing.'
				});
			r.strokeNumbers += msg.pts.length;
			s.pts.push(...msg.pts);
			relay({ t: 'sp', from: id, k: msg.k, pts: msg.pts });
		} else if (msg.t === 'se') {
			const s = live.get(msg.k);
			if (!s) return;
			live.delete(msg.k);
			mine.push(s);
			r.strokes.set(id, mine);
			relay({ t: 'se', from: id, k: msg.k });
		} else if (msg.t === 'undo') {
			const s = mine.pop();
			if (!s) return;
			r.strokeNumbers -= s.pts.length;
			relay({ t: 'undo', from: id });
		} else if (msg.t === 'clear') {
			for (const s of mine) r.strokeNumbers -= s.pts.length;
			r.strokes.set(id, []);
			relay({ t: 'clear', from: id });
		}
	}

	// ---- competition ----

	private startGame(r: Room, refs: LobbyRef[]) {
		const clean = refs.map(cleanRef).filter((x): x is LobbyRef => !!x);
		if (clean.length < r.settings.rounds)
			return this.broadcast(r, {
				t: 'error',
				message: 'Couldn’t find enough pictures for every round.'
			});
		r.refs = clean.slice(0, r.settings.rounds);
		r.results = [];
		r.round = 0;
		this.startRound(r);
	}

	private startRound(r: Room) {
		r.round++;
		r.phase = 'drawing';
		r.reference = r.refs[r.round - 1];
		r.submissions = new Map();
		r.reactions = new Map();
		r.endsAt = this.clock.now() + r.settings.timeLimit * 1000;
		this.setTimer(r, () => this.timeUp(r), r.settings.timeLimit * 1000);
		this.broadcastState(r);
	}

	private timeUp(r: Room) {
		if (r.phase !== 'drawing') return;
		r.phase = 'collecting';
		this.broadcast(r, { t: 'time-up' });
		this.setTimer(r, () => this.startReveal(r), COLLECT_GRACE_MS);
		this.broadcastState(r);
		this.maybeFinishCollecting(r);
	}

	private submit(r: Room, p: Player, png: string | null, score: number | null) {
		if (r.phase !== 'drawing' && r.phase !== 'collecting') return;
		if (r.submissions.has(p.playerId)) return;
		const okPng =
			typeof png === 'string' &&
			png.startsWith('data:image/png;base64,') &&
			png.length <= MAX_PNG_CHARS;
		const okScore =
			typeof score === 'number' && Number.isFinite(score) && score >= 1 && score <= 10;
		r.submissions.set(p.playerId, {
			png: okPng ? png : null,
			score: okPng && okScore ? Math.round(score * 100) / 100 : null
		});
		this.broadcastState(r);
		this.maybeFinishCollecting(r);
	}

	/** Everyone connected has handed in: no need to wait. */
	private maybeFinishCollecting(r: Room) {
		if (r.phase !== 'drawing' && r.phase !== 'collecting') return;
		const waiting = r.players.filter((x) => x.send && !r.submissions.has(x.playerId));
		if (waiting.length === 0 && r.submissions.size > 0) this.startReveal(r);
	}

	private startReveal(r: Room) {
		if (r.phase !== 'drawing' && r.phase !== 'collecting') return;
		r.timer?.();
		r.timer = null;
		r.endsAt = null;
		// everyone who was in the round, in a shuffled order
		const ids = r.players.map((x) => x.playerId);
		for (let i = ids.length - 1; i > 0; i--) {
			const j = randomInt(i + 1);
			[ids[i], ids[j]] = [ids[j], ids[i]];
		}
		r.order = ids;
		r.revealIndex = 0;
		r.phase = 'reveal';
		this.broadcastState(r);
	}

	private react(r: Room, p: Player, target: string, v: 1 | -1) {
		if ((r.phase !== 'reveal' && r.phase !== 'round-end') || target === p.playerId) return;
		if (v !== 1 && v !== -1) return;
		if (!r.order.includes(target)) return;
		const m = r.reactions.get(target) ?? new Map<string, 1 | -1>();
		m.set(p.playerId, v);
		r.reactions.set(target, m);
		// after the reveal, keep this round's stored counts up to date too
		const last = r.results.at(-1);
		if (r.phase === 'round-end' && last?.round === r.round) {
			const e = last.entries.find((x) => x.playerId === target);
			if (e)
				Object.assign(e, {
					likes: this.entry(r, target).likes,
					dislikes: this.entry(r, target).dislikes
				});
		}
		this.broadcastState(r);
	}

	private next(r: Room) {
		if (r.phase === 'reveal') {
			if (r.revealIndex < r.order.length - 1) r.revealIndex++;
			else this.endRound(r);
			return this.broadcastState(r);
		}
		if (r.phase === 'round-end') {
			if (r.round < r.settings.rounds) return this.startRound(r);
			r.phase = 'final';
			return this.broadcastState(r);
		}
	}

	private endRound(r: Room) {
		const entries = r.order.map((id) => ({
			...this.entry(r, id),
			score: r.submissions.get(id)?.score ?? null
		}));
		const best = entries
			.filter((e) => e.score !== null)
			.sort((a, b) => (b.score ?? 0) - (a.score ?? 0))[0];
		r.results.push({ round: r.round, ref: r.reference, entries, winnerId: best?.playerId ?? null });
		r.phase = 'round-end';
	}

	private toWaiting(r: Room) {
		r.timer?.();
		r.timer = null;
		r.phase = 'waiting';
		r.round = 0;
		r.reference = null;
		r.endsAt = null;
		r.results = [];
		r.strokes = new Map();
		r.live = new Map();
		r.strokeNumbers = 0;
		this.broadcastState(r);
	}

	private setTimer(r: Room, fn: () => void, ms: number) {
		r.timer?.();
		r.timer = this.clock.later(fn, ms);
	}

	// ---- views ----

	private name(r: Room, id: string) {
		return r.players.find((x) => x.playerId === id)?.name ?? 'Someone who left';
	}

	private entry(r: Room, id: string) {
		const rx = [...(r.reactions.get(id)?.values() ?? [])];
		return {
			playerId: id,
			name: this.name(r, id),
			png: r.submissions.get(id)?.png ?? null,
			likes: rx.filter((v) => v === 1).length,
			dislikes: rx.filter((v) => v === -1).length
		};
	}

	/** Game totals: grader scores added over rounds; ties go to more round wins. */
	totals(r: Room) {
		const t = new Map<string, { playerId: string; name: string; total: number; wins: number }>();
		for (const res of r.results)
			for (const e of res.entries) {
				const x = t.get(e.playerId) ?? { playerId: e.playerId, name: e.name, total: 0, wins: 0 };
				x.total = Math.round((x.total + (e.score ?? 0)) * 100) / 100;
				if (res.winnerId === e.playerId) x.wins++;
				t.set(e.playerId, x);
			}
		return [...t.values()].sort((a, b) => b.total - a.total || b.wins - a.wins);
	}

	view(code: string): LobbyView | null {
		const r = this.rooms.get(code);
		if (!r) return null;
		const totals = this.totals(r);
		return {
			code: r.code,
			settings: r.settings,
			players: r.players.map((x) => ({
				id: x.playerId,
				name: x.name,
				host: x.playerId === r.hostId,
				connected: !!x.send,
				member: !!x.userId
			})),
			phase: r.phase,
			round: r.round,
			reference: r.reference,
			endsAt: r.endsAt,
			serverNow: this.clock.now(),
			submitted: [...r.submissions.keys()],
			reveal:
				r.phase === 'reveal'
					? {
							index: r.revealIndex,
							total: r.order.length,
							entry: this.entry(r, r.order[r.revealIndex])
						}
					: null,
			// pictures are only sent while revealing; results carry scores and names
			results: r.results.map((res) => ({
				...res,
				entries: res.entries.map((e) => ({ ...e, name: this.name(r, e.playerId), png: null }))
			})),
			totals,
			winnerId: r.phase === 'final' ? (totals[0]?.playerId ?? null) : null
		};
	}

	private broadcastState(r: Room) {
		const lobby = this.view(r.code);
		if (!lobby) return;
		for (const p of r.players) p.send?.({ t: 'state', lobby, you: p.playerId });
	}

	private broadcast(r: Room, m: ServerMsg, except?: string) {
		for (const p of r.players) if (p.playerId !== except) p.send?.(m);
	}
}

function cleanStyle(s: StrokeStyle): StrokeStyle {
	return {
		brushId: typeof s?.brushId === 'string' ? s.brushId.slice(0, 80) : 'deevad-2b-pencil',
		color: typeof s?.color === 'string' && /^#[0-9a-f]{6}$/i.test(s.color) ? s.color : '#16161a',
		size: Number.isFinite(s?.size) ? Math.max(0.05, Math.min(8, s.size)) : 1,
		opacity: Number.isFinite(s?.opacity) ? Math.max(0, Math.min(1, s.opacity)) : 1,
		eraser: s?.eraser === true
	};
}

/** Only well-formed references with our own image paths go to other players. */
export function cleanRef(x: LobbyRef): LobbyRef | null {
	const str = (v: unknown, max: number) => (typeof v === 'string' && v.length <= max ? v : '');
	const img = str(x?.imageUrl, 120);
	if (!/^\/(references|refimg)\/[A-Za-z0-9_.-]{1,60}\.jpg$/.test(img)) return null;
	const page = (v: unknown) => {
		const u = str(v, 300);
		return /^https:\/\/pixabay\.com\//.test(u) ? u : 'https://pixabay.com/';
	};
	return {
		id: str(x.id, 60) || 'ref',
		category: str(x.category, 30),
		title: str(x.title, 80) || 'Reference',
		imageUrl: img,
		author: str(x.author, 80),
		authorUrl: page(x.authorUrl),
		pageUrl: page(x.pageUrl),
		license: str(x.license, 60) || 'Pixabay Content License',
		licenseUrl: 'https://pixabay.com/service/license-summary/',
		tier: Number.isInteger(x.tier) ? x.tier : undefined,
		live: x.live === true,
		tags: Array.isArray(x.tags) ? x.tags.filter((t) => typeof t === 'string').slice(0, 12) : []
	};
}

/** One hub per server process (shared by the socket layer and the HTTP routes). */
export function getHub(): Hub {
	const g = globalThis as unknown as { __ppLobbyHub?: Hub };
	g.__ppLobbyHub ??= new Hub();
	return g.__ppLobbyHub;
}
