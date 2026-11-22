import { fail, redirect } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import {
	appLabel,
	createLinkCode,
	loopbackRedirect,
	validChallenge,
	validState
} from '#lib/server/applink.js';
import { cleanName } from '#lib/server/lobby/ticket.js';
import type { Actions, PageServerLoad } from './$types';

/** What the app asked for, if it's well formed (else nothing is sent back to it). */
function request(url: URL) {
	const q = url.searchParams;
	const redirectUri = loopbackRedirect(q.get('redirect_uri'));
	const state = q.get('state');
	const challenge = q.get('code_challenge');
	if (!redirectUri || !validState(state) || !validChallenge(challenge)) return null;
	if (q.get('code_challenge_method') !== 'S256') return null;
	return { redirectUri, state, challenge, label: appLabel(q.get('label')) };
}

const back = (r: { redirectUri: string; state: string }, params: Record<string, string>) => {
	const u = new URL(r.redirectUri);
	for (const [k, v] of Object.entries({ ...params, state: r.state })) u.searchParams.set(k, v);
	return u.href;
};

export const load: PageServerLoad = ({ locals, url }) => {
	const r = request(url);
	if (!r) return { ok: false as const };
	if (!locals.user) redirect(303, `/login?next=${encodeURIComponent(url.pathname + url.search)}`);
	return { ok: true as const, label: r.label, name: locals.user.name };
};

export const actions: Actions = {
	/** Link: a one-time code back to the app. */
	link: async ({ locals, request: req, url }) => {
		const r = request(url);
		if (!r) return fail(400, { message: 'This link didn’t come from the PencilPilot app.' });
		if (!locals.user) redirect(303, `/login?next=${encodeURIComponent(url.pathname + url.search)}`);
		const name = cleanName(String((await req.formData()).get('name') ?? ''));
		if (!name) return fail(400, { message: 'Choose the name others see when you draw together.' });
		const db = getDb();
		if (name !== cleanName(locals.user.name))
			db.prepare('UPDATE users SET name = ? WHERE id = ?').run(name, locals.user.id);
		const code = createLinkCode(db, locals.user.id, {
			challenge: r.challenge,
			redirect: r.redirectUri,
			label: r.label
		});
		// only ever the app's loopback address (checked above)
		redirect(303, back(r, { code }), { external: true });
	},
	/** Not now: tell the app, so it stops waiting. */
	cancel: ({ url }) => {
		const r = request(url);
		if (!r) return fail(400, { message: 'This link didn’t come from the PencilPilot app.' });
		redirect(303, back(r, { error: 'access_denied' }), { external: true });
	}
};
