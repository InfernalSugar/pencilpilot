import { dev } from '$app/env';
import { error, fail } from '@sveltejs/kit';
import { addLabel, labels, samples } from '#lib/server/calibration.js';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = () => {
	if (!dev) error(404, 'Not found');
	const done = new Set(labels().map((l) => l.sample));
	const all = samples();
	const byCat: Record<string, number> = {};
	for (const l of labels()) byCat[l.category] = (byCat[l.category] ?? 0) + 1;
	return { todo: all.filter((s) => !done.has(s.id)), total: all.length, byCat };
};

export const actions: Actions = {
	default: async ({ request }) => {
		if (!dev) error(404, 'Not found');
		const f = await request.formData();
		try {
			addLabel(String(f.get('sample')), Number(f.get('human')));
		} catch (e) {
			return fail(400, { message: String(e) });
		}
		return { ok: true };
	}
};
