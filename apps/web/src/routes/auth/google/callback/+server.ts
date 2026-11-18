import { redirect } from '@sveltejs/kit';
import { decodeIdToken } from 'arctic';
import { google } from '#lib/server/google.js';
import { getDb } from '#lib/server/db.js';
import { signIn, upsertGoogleUser } from '#lib/server/auth.js';
import { authByIp } from '#lib/server/ratelimit.js';
import { safeNext } from '#lib/server/util.js';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url, cookies, getClientAddress }) => {
	const fail = () => redirect(302, '/login?error=google');
	const g = google(url.origin);
	const state = url.searchParams.get('state');
	const code = url.searchParams.get('code');
	const saved = cookies.get('g_state');
	const verifier = cookies.get('g_verifier');
	const next = safeNext(cookies.get('g_next'));
	for (const c of ['g_state', 'g_verifier', 'g_next']) cookies.delete(c, { path: '/' });
	if (!g || !state || !code || !saved || !verifier || state !== saved) fail();
	if (!authByIp.take(getClientAddress())) fail();

	let claims: { sub: string; email?: string; email_verified?: boolean; name?: string };
	try {
		const tokens = await g!.validateAuthorizationCode(code!, verifier!);
		claims = decodeIdToken(tokens.idToken()) as typeof claims;
	} catch {
		return fail();
	}
	const db = getDb();
	const id = upsertGoogleUser(db, {
		sub: claims.sub,
		email: claims.email,
		emailVerified: claims.email_verified === true,
		name: claims.name
	});
	signIn(db, cookies, id);
	redirect(302, next);
};
