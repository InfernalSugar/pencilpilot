/**
 * Private lobbies (M10-T08): the messages between browsers and the lobby
 * server, shared by both sides. Strokes travel as stroke data and are redrawn
 * by the same brush engine everywhere.
 */

export type LobbyMode = 'competition' | 'free';

export interface LobbySettings {
	mode: LobbyMode;
	/** competition: rounds per game, 1–10 */
	rounds: number;
	/** competition: seconds per round, 30–900 */
	timeLimit: number;
	category: string;
	tier: number;
	width: number;
	height: number;
}

export const DEFAULT_SETTINGS: LobbySettings = {
	mode: 'competition',
	rounds: 3,
	timeLimit: 120,
	category: 'animals',
	tier: 1,
	width: 800,
	height: 600
};

export const MAX_PLAYERS = 8;

/** What players need to show and grade a reference (curated or Endless). */
export interface LobbyRef {
	id: string;
	category: string;
	title: string;
	imageUrl: string;
	author: string;
	authorUrl: string;
	pageUrl: string;
	license: string;
	licenseUrl: string;
	tier?: number;
	live?: boolean;
	tags?: string[];
}

export type Phase =
	| 'waiting' // in the lobby room, choosing settings
	| 'free' // free draw: everyone on one canvas
	| 'drawing' // competition round in progress
	| 'collecting' // time's up: waiting for drawings to come in
	| 'reveal' // showing drawings one by one
	| 'round-end' // round scores
	| 'final'; // game over

export interface PlayerView {
	id: string;
	name: string;
	host: boolean;
	connected: boolean;
	/** signed-in account (hosts must be) */
	member: boolean;
}

export interface RevealEntry {
	playerId: string;
	name: string;
	/** data URL, or null when time ran out with nothing drawn */
	png: string | null;
	likes: number;
	dislikes: number;
}

export interface RoundResult {
	round: number;
	ref: LobbyRef | null;
	entries: (RevealEntry & { score: number | null })[];
	/** highest grader score */
	winnerId: string | null;
}

export interface LobbyView {
	code: string;
	settings: LobbySettings;
	players: PlayerView[];
	phase: Phase;
	round: number;
	reference: LobbyRef | null;
	/** server time (ms) the drawing time ends, and the server's clock now */
	endsAt: number | null;
	serverNow: number;
	/** ids that have handed in this round */
	submitted: string[];
	reveal: { index: number; total: number; entry: RevealEntry } | null;
	results: RoundResult[];
	totals: { playerId: string; name: string; total: number; wins: number }[];
	winnerId: string | null;
}

export interface StrokeStyle {
	brushId: string;
	color: string;
	size: number;
	opacity: number;
	eraser: boolean;
}

/** A shared-canvas stroke: style + flat points [x, y, pressure, dtMs, …]. */
export interface WireStroke {
	style: StrokeStyle;
	pts: number[];
}

export type ClientMsg =
	| { t: 'join'; code: string; ticket: string }
	| { t: 'settings'; settings: Partial<LobbySettings> }
	| { t: 'start'; refs?: LobbyRef[] }
	| { t: 'submit'; png: string | null; score: number | null }
	| { t: 'react'; target: string; v: 1 | -1 }
	| { t: 'next' }
	| { t: 'again' }
	| { t: 'end' }
	| { t: 'kick'; playerId: string }
	| { t: 'reference'; ref: LobbyRef | null }
	| { t: 'sb'; k: number; style: StrokeStyle }
	| { t: 'sp'; k: number; pts: number[] }
	| { t: 'se'; k: number }
	| { t: 'undo' }
	| { t: 'clear' };

export type ServerMsg =
	| { t: 'state'; lobby: LobbyView; you: string }
	| { t: 'sync'; strokes: Record<string, WireStroke[]> }
	| { t: 'sb'; from: string; k: number; style: StrokeStyle }
	| { t: 'sp'; from: string; k: number; pts: number[] }
	| { t: 'se'; from: string; k: number }
	| { t: 'undo'; from: string }
	| { t: 'clear'; from: string }
	| { t: 'time-up' }
	| { t: 'kicked' }
	| { t: 'error'; message: string };

/** Clamp settings to what's allowed. */
export function cleanSettings(s: Partial<LobbySettings>, base: LobbySettings): LobbySettings {
	const int = (v: unknown, lo: number, hi: number, d: number) =>
		Number.isInteger(v) ? Math.max(lo, Math.min(hi, v as number)) : d;
	return {
		mode: s.mode === 'free' || s.mode === 'competition' ? s.mode : base.mode,
		rounds: int(s.rounds, 1, 10, base.rounds),
		timeLimit: int(s.timeLimit, 30, 900, base.timeLimit),
		category:
			typeof s.category === 'string' && /^[a-z-]{2,20}$/.test(s.category)
				? s.category
				: base.category,
		tier: int(s.tier, 1, 4, base.tier),
		width: int(s.width, 256, 2048, base.width),
		height: int(s.height, 256, 2048, base.height)
	};
}
