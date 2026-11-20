import type { Stroke } from './paint/strokes';
import type { Reference, TraceTransform } from './references';

/** The last submitted drawing, handed from /draw to /results for this browser tab. */
export interface Submission {
	/** when it was submitted (ms epoch); identifies the attempt */
	at?: number;
	png: string;
	width: number;
	height: number;
	seconds: number;
	category: string | null;
	referenceId: string | null;
	/** the full reference (needed for Endless references, which aren't in the curated lists) */
	reference?: Reference | null;
	/** topic filter the student chose (null = any) */
	topic?: string | null;
	/** tier of the reference drawn */
	tier?: number | null;
	strokes: number;
	points: number;
	brushes: string[];
	/** raw pen input, for the grader's line-confidence metrics (and replays) */
	strokeData: Stroke[];
	/** layer settings the strokes were painted with, bottom → top (for replays) */
	layers?: { id: number; opacity: number; blend: string; clip?: boolean }[];
	/** a rough sketch (graded on the big picture, ADR-0007) */
	mode?: 'finished' | 'sketch';
	/** the daily challenge day (YYYY-MM-DD) this drawing was for */
	daily?: string;
	/** the day of the student's own personal challenge this drawing was for */
	personal?: string;
	/** seconds, for timed drawings */
	timeLimit?: number;
	/** drawn with a tracing picture showing: tips only, never scored or counted */
	traced?: boolean;
	/** its entry in the drawing history (lib/history.ts) */
	historyId?: string;
	/** an imported picture was showing when it was handed in (so it's treated like tracing) */
	imported?: boolean;
	/**
	 * The proportions check (lib/keypoints.ts): per reference keypoint, where the student
	 * tapped it on the canvas (or null for "I didn't draw this one"). null = skipped;
	 * missing = not asked yet.
	 */
	keypointTaps?: ({ x: number; y: number } | null)[] | null;
	/** how the tracing picture was placed, so the comparison matches what was traced */
	trace?: { source: 'reference' | 'own'; transform: TraceTransform; ownUrl?: string };
}

const KEY = 'pp.lastSubmission';

export function saveSubmission(s: Submission) {
	memory = s;
	try {
		sessionStorage.setItem(KEY, JSON.stringify(s));
	} catch {
		/* storage blocked or full: the in-memory copy still works for this page load */
	}
}

let memory: Submission | null = null;

export function loadSubmission(): Submission | null {
	try {
		const raw = sessionStorage.getItem(KEY);
		if (raw) return JSON.parse(raw);
	} catch {
		/* storage blocked: fall back to this page load's copy */
	}
	return memory;
}
