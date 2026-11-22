import { json } from '@sveltejs/kit';
import { dev } from '$app/env';
import { PASS_SIGNING_KEY } from '$app/env/private';
import { getDb } from '#lib/server/db.js';
import { appUser, bearer } from '#lib/server/applink.js';
import { issuePass, passKey, validEndpoint } from '#lib/server/pass.js';
import { RateLimiter } from '#lib/server/ratelimit.js';
import { cleanName } from '#lib/server/lobby/ticket.js';
import type { RequestHandler } from './$types';

/** A pass lasts a day: a handful an hour per app is plenty. */
const passes = new RateLimiter(12, 5 * 60_000);

/** A signed pass for the linked app on this computer (for Draw together). */
export const POST: RequestHandler = async ({ request }) => {
	const token = bearer(request.headers.get('authorization'));
	const db = getDb();
	const who = token ? appUser(db, token) : null;
	if (!who)
		return json(
			{ error: 'unlinked', message: 'This app isn’t linked to an account any more.' },
			{ status: 401 }
		);
	if (!passes.take(who.link))
		return json(
			{ error: 'slow_down', message: 'Too many passes asked for: wait a few minutes.' },
			{ status: 429 }
		);
	const key = passKey(PASS_SIGNING_KEY, dev);
	if (!key)
		return json(
			{ error: 'unavailable', message: 'Drawing together isn’t set up on this site yet.' },
			{ status: 503 }
		);
	let endpoint: unknown;
	try {
		endpoint = (await request.json()).endpoint;
	} catch {
		endpoint = null;
	}
	if (!validEndpoint(endpoint)) return json({ error: 'invalid_request' }, { status: 400 });
	const name = cleanName(who.user.name);
	if (!name)
		return json(
			{
				error: 'no_name',
				message: 'Give your account a name on the PencilPilot website first.'
			},
			{ status: 409 }
		);
	const pass = issuePass(key, { user: who.user.id, name, endpoint });
	return json(
		{ pass, user: { id: who.user.id, name } },
		{ headers: { 'Cache-Control': 'no-store' } }
	);
};
