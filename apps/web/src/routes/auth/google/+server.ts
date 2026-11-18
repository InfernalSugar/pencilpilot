import { error, redirect } from '@sveltejs/kit';
import { generateCodeVerifier, generateState } from 'arctic';
import { google } from '#lib/server/google.js';
import { safeNext } from '#lib/server/util.js';
import type { RequestHandler } from './$types';

const cookie = { path: '/', httpOnly: true, sameSite: 'lax' as const, maxAge: 600 };

export const GET: RequestHandler = ({ url, cookies }) => {
	const g = google(url.origin);
	if (!g) error(404, 'Google sign-in is not set up');
	const state = generateState();
	const verifier = generateCodeVerifier();
	cookies.set('g_state', state, cookie);
	cookies.set('g_verifier', verifier, cookie);
	cookies.set('g_next', safeNext(url.searchParams.get('next')), cookie);
	redirect(
		302,
		g.createAuthorizationURL(state, verifier, ['openid', 'profile', 'email']).toString()
	);
};
