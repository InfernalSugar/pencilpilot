import { GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET } from '$app/env/private';
import { communityEnabled } from '#lib/server/community.js';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals }) => ({
	user: locals.user
		? { id: locals.user.id, name: locals.user.name, email: locals.user.email }
		: null,
	googleEnabled: !!(GOOGLE_CLIENT_ID && GOOGLE_CLIENT_SECRET),
	community: communityEnabled()
});
