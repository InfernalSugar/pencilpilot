import { json } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import { bearer, revokeToken } from '#lib/server/applink.js';
import type { RequestHandler } from './$types';

/** The app signs out: its token stops working. */
export const POST: RequestHandler = ({ request }) => {
	const token = bearer(request.headers.get('authorization'));
	if (token) revokeToken(getDb(), token);
	return json({ ok: true });
};
