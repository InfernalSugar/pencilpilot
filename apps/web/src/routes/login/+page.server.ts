import { fail, redirect } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import {
	findUserByEmail,
	hashPassword,
	normaliseEmail,
	signIn,
	verifyPassword
} from '#lib/server/auth.js';
import { authByIp, loginByEmail } from '#lib/server/ratelimit.js';
import { safeNext } from '#lib/server/util.js';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, url }) => {
	if (locals.user) redirect(303, safeNext(url.searchParams.get('next')));
	return { error: url.searchParams.get('error') };
};

// Unknown emails still run a hash check, so they take as long as known ones.
const DUMMY_HASH = hashPassword(crypto.randomUUID());

export const actions: Actions = {
	default: async ({ request, cookies, getClientAddress, url }) => {
		const f = await request.formData();
		const email = normaliseEmail(String(f.get('email') ?? ''));
		const password = String(f.get('password') ?? '');
		if (!authByIp.take(getClientAddress()) || !loginByEmail.take(email))
			return fail(429, {
				email,
				message: 'Too many attempts. Please wait a few minutes and try again.'
			});
		const db = getDb();
		const user = findUserByEmail(db, email);
		const ok = await verifyPassword(user?.password_hash ?? (await DUMMY_HASH), password).catch(
			() => false
		);
		if (!user || !user.password_hash || !ok)
			return fail(400, {
				email,
				message:
					user && !user.password_hash
						? 'This account uses Google sign-in.'
						: 'Wrong email or password.'
			});
		loginByEmail.reset(email);
		signIn(db, cookies, user.id);
		redirect(303, safeNext(url.searchParams.get('next')));
	}
};
