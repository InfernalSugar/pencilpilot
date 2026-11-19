/**
 * Timed drawing: single rounds with a time limit, and gesture sessions (a queue
 * of references with a fixed time each, like a life-drawing class).
 */
import type { Submission } from './submission';

export const TIME_LIMITS: { seconds: number; label: string }[] = [
	{ seconds: 0, label: 'No limit' },
	{ seconds: 30, label: '30 s' },
	{ seconds: 60, label: '1 min' },
	{ seconds: 120, label: '2 min' },
	{ seconds: 300, label: '5 min' },
	{ seconds: 600, label: '10 min' }
];
export const GESTURE_COUNTS = [5, 10, 20];
export const GESTURE_TIMES = [30, 60, 120];

/** "30-second", "2-minute" */
export function timeLabel(seconds: number) {
	return seconds < 60 ? `${seconds}-second` : `${Math.round(seconds / 60)}-minute`;
}

/** "0:30", "2:00" */
export function clockOf(seconds: number) {
	const s = Math.max(0, Math.ceil(seconds));
	return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

// ---- gesture sessions ----

export interface SessionPlan {
	count: number;
	/** seconds per picture */
	time: number;
	category: string;
	topic: string | null;
	tier: number | null;
	mode?: 'finished' | 'sketch';
	startedAt: number;
}

interface SessionState {
	plan: SessionPlan;
	drawings: Submission[];
	/** pictures whose time ran out with nothing drawn */
	skipped: number;
}

const KEY = 'pp.session';
// The session lives in memory (navigation is client-side); sessionStorage is a
// best-effort copy for reloads (drawings can exceed its quota).
let memory: SessionState | null = null;

function save() {
	try {
		sessionStorage.setItem(KEY, JSON.stringify(memory));
	} catch {
		/* full or blocked: the in-memory copy still works */
	}
}

export function startSession(plan: Omit<SessionPlan, 'startedAt'>): SessionPlan {
	memory = { plan: { ...plan, startedAt: Date.now() }, drawings: [], skipped: 0 };
	save();
	return memory.plan;
}

export function currentSession(): SessionState | null {
	if (memory) return memory;
	try {
		memory = JSON.parse(sessionStorage.getItem(KEY) ?? 'null');
	} catch {
		memory = null;
	}
	return memory;
}

/** Add a finished picture (or a skipped one). Returns how many pictures are done. */
export function addToSession(sub: Submission | null): number {
	const s = currentSession();
	if (!s) return 0;
	if (sub) s.drawings.push(sub);
	else s.skipped++;
	save();
	return s.drawings.length + s.skipped;
}

export function sessionDrawing(at: number): Submission | null {
	return currentSession()?.drawings.find((d) => d.at === at) ?? null;
}
