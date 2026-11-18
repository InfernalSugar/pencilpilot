import { fail, redirect } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import {
	createPasswordUser,
	findUserByEmail,
	normaliseEmail,
	passwordProblem,
	signIn,
	validEmail
} from '#lib/server/auth.js';
import { authByIp } from '#lib/server/ratelimit.js';
import { safeNext } from '#lib/server/util.js';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, url }) => {
	if (locals.user) redirect(303, safeNext(url.searchParams.get('next')));
};

export const actions: Actions = {
	default: async ({ request, cookies, getClientAddress, url }) => {
		const f = await request.formData();
		const email = normaliseEmail(String(f.get('email') ?? ''));
		const password = String(f.get('password') ?? '');
		const name = String(f.get('name') ?? '').trim();
		if (!authByIp.take(getClientAddress()))
			return fail(429, { email, name, message: 'Too many attempts. Please wait a few minutes.' });
		if (!validEmail(email))
			return fail(400, { email, name, message: 'Enter a valid email address.' });
		const problem = passwordProblem(password, email);
		if (problem) return fail(400, { email, name, message: problem });
		const db = getDb();
		if (findUserByEmail(db, email))
			return fail(400, {
				email,
				name,
				message: 'That email already has an account. Sign in instead.'
			});
		const id = await createPasswordUser(db, email, password, name);
		signIn(db, cookies, id);
		redirect(303, safeNext(url.searchParams.get('next'), '/account?welcome=1'));
	}
};
