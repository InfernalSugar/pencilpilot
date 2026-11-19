import { dev } from '$app/env';
import { error } from '@sveltejs/kit';
import { categories } from '#lib/categories.js';
import { topicsFor } from '#lib/topics.js';
import type { PageServerLoad } from './$types';

// Dev-only: the owner reviews what the picture sorter let into the live pool.
// The pictures come from the ref-service's /api/admin/* (on only with REF_ADMIN=1).
export const load: PageServerLoad = () => {
	if (!dev) error(404, 'Not found');
	return {
		categories: categories.map((c) => ({ id: c.id, name: c.name })),
		topics: Object.fromEntries(
			categories.map((c) => [c.id, topicsFor(c.id).map((t) => ({ id: t.id, name: t.name }))])
		)
	};
};
