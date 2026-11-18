import { fail, redirect } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import {
	clearSessionCookie,
	deleteUser,
	hashPassword,
	invalidateUserSessions,
	passwordProblem,
	signIn,
	verifyPassword
} from '#lib/server/auth.js';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, url }) => {
	if (!locals.user) redirect(303, '/login?next=/account');
	const db = getDb();
	const count = (t: string) =>
		(
			db.prepare(`SELECT COUNT(*) AS n FROM ${t} WHERE user_id = ?`).get(locals.user!.id) as {
				n: number;
			}
		).n;
	const created = (
		db.prepare('SELECT created_at FROM users WHERE id = ?').get(locals.user.id) as {
			created_at: number;
		}
	).created_at;
	return {
		account: { ...locals.user, created },
		counts: { attempts: count('attempts'), drills: count('drill_results') },
		welcome: url.searchParams.has('welcome')
	};
};

export const actions: Actions = {
	rename: async ({ locals, request }) => {
		if (!locals.user) return fail(401);
		const name = String((await request.formData()).get('name') ?? '')
			.trim()
			.slice(0, 60);
		getDb().prepare('UPDATE users SET name = ? WHERE id = ?').run(name, locals.user.id);
		return { saved: 'name' };
	},
	password: async ({ locals, request, cookies }) => {
		if (!locals.user) return fail(401);
		const f = await request.formData();
		const current = String(f.get('current') ?? '');
		const next = String(f.get('new') ?? '');
		const db = getDb();
		const row = db.prepare('SELECT password_hash FROM users WHERE id = ?').get(locals.user.id) as {
			password_hash: string | null;
		};
		if (row.password_hash && !(await verifyPassword(row.password_hash, current).catch(() => false)))
			return fail(400, { form: 'password', message: 'Your current password is wrong.' });
		const problem = passwordProblem(next, locals.user.email ?? '');
		if (problem) return fail(400, { form: 'password', message: problem });
		db.prepare('UPDATE users SET password_hash = ? WHERE id = ?').run(
			await hashPassword(next),
			locals.user.id
		);
		// sign out everywhere else
		invalidateUserSessions(db, locals.user.id);
		signIn(db, cookies, locals.user.id);
		return { saved: 'password' };
	},
	delete: async ({ locals, request, cookies }) => {
		if (!locals.user) return fail(401);
		const confirm = String((await request.formData()).get('confirm') ?? '');
		if (confirm !== 'DELETE')
			return fail(400, { form: 'delete', message: 'Type DELETE to confirm.' });
		deleteUser(getDb(), locals.user.id);
		clearSessionCookie(cookies);
		redirect(303, '/?deleted=1');
	}
};
