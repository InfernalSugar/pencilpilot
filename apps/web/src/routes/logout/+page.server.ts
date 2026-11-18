import { redirect } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import { clearSessionCookie, invalidateSession } from '#lib/server/auth.js';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = () => redirect(303, '/');

export const actions: Actions = {
	default: ({ locals, cookies }) => {
		if (locals.sessionId) invalidateSession(getDb(), locals.sessionId);
		clearSessionCookie(cookies);
		redirect(303, '/');
	}
};
