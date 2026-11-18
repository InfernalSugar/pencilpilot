/** The last submitted drawing, handed from /draw to /results for this browser tab. */
export interface Submission {
	png: string;
	width: number;
	height: number;
	seconds: number;
	category: string | null;
	strokes: number;
	points: number;
	brushes: string[];
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
