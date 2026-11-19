import { error, json } from '@sveltejs/kit';
import { getHub } from '#lib/server/lobby/hub.js';
import { memberPlayerId } from '#lib/server/lobby/ids.js';
import type { RequestHandler } from './$types';

/** Open a private lobby (hosts need an account). */
export const POST: RequestHandler = ({ locals }) => {
	if (!locals.user) error(401, 'Sign in to host a lobby');
	const hub = getHub();
	// the host's player id depends on the code, so make the room first, then name the host
	const code = hub.create({ playerId: 'pending', name: '', userId: locals.user.id });
	const room = hub.rooms.get(code)!;
	room.hostId = memberPlayerId(locals.user.id, code);
	return json({ code });
};
