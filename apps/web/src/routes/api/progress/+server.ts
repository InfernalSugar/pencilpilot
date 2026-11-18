import { error, json } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import { snapshot, sync } from '#lib/server/progress-store.js';
import type { RequestHandler } from './$types';

const MAX_BODY = 2_000_000;

/** The signed-in user's stored history. */
export const GET: RequestHandler = ({ locals }) => {
	if (!locals.user) error(401, 'Sign in to sync progress');
	return json(snapshot(getDb(), locals.user.id));
};

/** Merge this browser's history in and return the combined copy. */
export const POST: RequestHandler = async ({ locals, request }) => {
	if (!locals.user) error(401, 'Sign in to sync progress');
	const text = await request.text();
	if (text.length > MAX_BODY) error(413, 'Too much data');
	let body: unknown;
	try {
		body = JSON.parse(text);
	} catch {
		error(400, 'Bad JSON');
	}
	return json(sync(getDb(), locals.user.id, (body ?? {}) as never));
};
