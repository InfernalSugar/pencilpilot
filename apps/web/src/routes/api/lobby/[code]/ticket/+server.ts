import { error, json } from '@sveltejs/kit';
import { getHub } from '#lib/server/lobby/hub.js';
import { isGuestId, memberPlayerId } from '#lib/server/lobby/ids.js';
import { cleanName, issueTicket } from '#lib/server/lobby/ticket.js';
import type { RequestHandler } from './$types';

/** A ticket to join a lobby's live connection, as a member or a named guest. */
export const POST: RequestHandler = async ({ params, request, locals }) => {
	const code = params.code.toUpperCase();
	if (!getHub().exists(code)) error(404, 'That lobby doesn’t exist (or has closed).');
	const body = (await request.json().catch(() => ({}))) as { name?: unknown; playerId?: unknown };
	if (locals.user) {
		const name =
			cleanName(body.name) ??
			cleanName(locals.user.name) ??
			cleanName(locals.user.email?.split('@')[0]) ??
			'Player';
		const who = { playerId: memberPlayerId(locals.user.id, code), name, userId: locals.user.id };
		return json({ ticket: issueTicket(code, who), playerId: who.playerId, name });
	}
	const name = cleanName(body.name);
	if (!name) error(400, 'Pick a name to join');
	if (!isGuestId(body.playerId)) error(400, 'Bad player id');
	const who = { playerId: body.playerId, name, userId: null };
	return json({ ticket: issueTicket(code, who), playerId: who.playerId, name });
};
