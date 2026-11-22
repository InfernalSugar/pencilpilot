import { json } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import { exchangeCode } from '#lib/server/applink.js';
import { authByIp } from '#lib/server/ratelimit.js';
import type { RequestHandler } from './$types';

/** The app trades its one-time code (and its secret) for an app token. */
export const POST: RequestHandler = async ({ request, getClientAddress }) => {
	if (!authByIp.take(getClientAddress()))
		return json({ error: 'slow_down', message: 'Too many tries: wait a minute.' }, { status: 429 });
	let body: Record<string, unknown>;
	try {
		body = await request.json();
	} catch {
		return json({ error: 'invalid_request' }, { status: 400 });
	}
	const got = exchangeCode(getDb(), {
		code: body.code,
		verifier: body.code_verifier,
		redirect: body.redirect_uri
	});
	if (!got)
		return json(
			{ error: 'invalid_grant', message: 'That sign-in didn’t work: try again from the app.' },
			{ status: 400 }
		);
	return json(got, { headers: { 'Cache-Control': 'no-store' } });
};
