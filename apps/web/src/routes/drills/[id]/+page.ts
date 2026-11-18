import { error } from '@sveltejs/kit';
import { drillById } from '#lib/drills.js';
import type { PageLoad } from './$types';

export const load: PageLoad = ({ params }) => {
	if (!drillById(params.id)) error(404, 'No such drill');
	return { id: params.id };
};
