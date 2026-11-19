import { error, fail } from '@sveltejs/kit';
import { getDb } from './db.js';
import {
	cleanRubric,
	communityEnabled,
	judgementsByUser,
	nextPair,
	nextToRate,
	recordCloser,
	recordRubric,
	reportDrawing,
	RUBRIC
} from './community.js';
import type { RequestEvent } from '@sveltejs/kit';

/** Shared by /community ("which is closer?") and /community/rate. */
export function loadView(view: 'closer' | 'rate', locals: App.Locals) {
	if (!communityEnabled()) error(404, 'Not found');
	if (!locals.user) return { view, signedIn: false as const, rubric: RUBRIC };
	const db = getDb();
	return {
		view,
		signedIn: true as const,
		rubric: RUBRIC,
		pair: view === 'closer' ? nextPair(db, locals.user.id) : null,
		toRate: view === 'rate' ? nextToRate(db, locals.user.id) : null
	};
}

const MESSAGES = {
	not_found: 'That drawing is gone. Here’s another.',
	own_drawing: 'That’s your own drawing.',
	already_judged: 'You already judged that one.',
	mismatch: 'Something didn’t match up. Here’s another.'
};

function judge(locals: App.Locals) {
	if (!communityEnabled()) error(404, 'Not found');
	if (!locals.user) error(401, 'Sign in first');
	if (!judgementsByUser.take(locals.user.id)) error(429, 'Take a breather and come back soon');
	return locals.user.id;
}

type Ev = RequestEvent;

export const communityActions = {
	closer: async ({ request, locals }: Ev) => {
		const userId = judge(locals);
		const f = await request.formData();
		const err = recordCloser(
			getDb(),
			userId,
			String(f.get('a')),
			String(f.get('b')),
			String(f.get('winner'))
		);
		return err ? fail(400, { message: MESSAGES[err] }) : { ok: true };
	},
	rate: async ({ request, locals }: Ev) => {
		const userId = judge(locals);
		const f = await request.formData();
		const scores = cleanRubric(Object.fromEntries(f));
		if (!scores) return fail(400, { message: 'Pick a score for each row.' });
		const err = recordRubric(getDb(), userId, String(f.get('drawing')), scores);
		return err ? fail(400, { message: MESSAGES[err] }) : { ok: true };
	},
	report: async ({ request, locals }: Ev) => {
		const userId = judge(locals);
		const f = await request.formData();
		const reason = String(f.get('reason') ?? 'unsafe');
		const r = reportDrawing(getDb(), userId, String(f.get('drawing')), reason);
		if (r === 'not_found') return fail(400, { message: MESSAGES.not_found });
		return { reported: true };
	}
};
