import { error, fail } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import { communityEnabled, deleteDrawing, myDrawings, RUBRIC } from '#lib/server/community.js';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => {
	if (!communityEnabled()) error(404, 'Not found');
	if (!locals.user) return { signedIn: false as const };
	return { signedIn: true as const, drawings: myDrawings(getDb(), locals.user.id), rubric: RUBRIC };
};

export const actions: Actions = {
	delete: async ({ request, locals }) => {
		if (!communityEnabled()) error(404, 'Not found');
		if (!locals.user) error(401, 'Sign in first');
		const id = String((await request.formData()).get('drawing'));
		if (!deleteDrawing(getDb(), locals.user.id, id)) return fail(400, { message: 'Already gone.' });
		return { deleted: true };
	}
};
