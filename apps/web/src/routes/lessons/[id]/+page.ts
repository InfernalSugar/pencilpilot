import { error } from '@sveltejs/kit';
import { lessonById } from '#lib/lessons.js';
import type { PageLoad } from './$types';

export const load: PageLoad = ({ params }) => {
	const lesson = lessonById(params.id);
	if (!lesson) error(404, 'No such lesson');
	return { lesson, next: lessonById(lesson.next) };
};
