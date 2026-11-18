import { dev } from '$app/env';
import { error } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import { voteStats } from '#lib/server/tip-votes.js';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = () => {
	if (!dev) error(404, 'Not found');
	return { stats: voteStats(getDb()) };
};
