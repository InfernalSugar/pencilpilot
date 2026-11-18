import { error, json } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import { cleanVote, recordVote, votesByIp } from '#lib/server/tip-votes.js';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async ({ request, locals, getClientAddress }) => {
	if (!votesByIp.take(getClientAddress())) error(429, 'Slow down');
	const text = await request.text();
	if (text.length > 2000) error(413, 'Too big');
	let body: unknown;
	try {
		body = JSON.parse(text);
	} catch {
		error(400, 'Bad JSON');
	}
	const v = cleanVote(body);
	if (!v) error(400, 'Bad vote');
	recordVote(getDb(), v, locals.user?.id ?? null);
	return json({ ok: true });
};
