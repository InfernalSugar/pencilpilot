import type { Handle } from '@sveltejs/kit/hooks';
import { getDb } from '#lib/server/db.js';
import {
	SESSION_COOKIE,
	clearSessionCookie,
	setSessionCookie,
	validateSession
} from '#lib/server/auth.js';

export const handle: Handle = async ({ event, resolve }) => {
	event.locals.user = null;
	event.locals.sessionId = null;
	const token = event.cookies.get(SESSION_COOKIE);
	if (token) {
		const r = validateSession(getDb(), token);
		if (r) {
			event.locals.user = r.user;
			event.locals.sessionId = r.session.id;
			setSessionCookie(event.cookies, token, r.session.expiresAt);
		} else {
			clearSessionCookie(event.cookies);
		}
	}
	const response = await resolve(event);
	response.headers.set('X-Frame-Options', 'DENY');
	response.headers.set('Referrer-Policy', 'strict-origin-when-cross-origin');
	response.headers.set('X-Content-Type-Options', 'nosniff');
	return response;
};
