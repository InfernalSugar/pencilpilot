import { error } from '@sveltejs/kit';
import { categoryById } from '#lib/categories.js';
import type { PageLoad } from './$types';

export const load: PageLoad = ({ params }) => {
	const category = categoryById(params.category);
	if (!category) error(404, 'No such category');
	return { category };
};
