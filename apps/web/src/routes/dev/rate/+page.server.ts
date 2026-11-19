import { dev } from '$app/env';
import { error, fail } from '@sveltejs/kit';
import { addLabel, deleteSample, labels, samples } from '#lib/server/calibration.js';
import type { Actions, PageServerLoad } from './$types';

/** Ratings per category the fit needs before it's worth trusting. */
const TARGET = 25;

export const load: PageServerLoad = () => {
	if (!dev) error(404, 'Not found');
	const all = labels();
	const done = new Set(all.map((l) => l.sample));
	const rated: Record<string, number> = {};
	for (const l of all) {
		const k = l.mode === 'sketch' ? `${l.category} (sketch)` : l.category;
		rated[k] = (rated[k] ?? 0) + 1;
	}
	return {
		todo: samples()
			.filter((s) => !done.has(s.id))
			.map((s) => ({
				id: s.id,
				category: s.category,
				mode: s.mode ?? 'finished',
				refSrc: s.refFile ? `/dev/calibration?ref=${s.id}` : s.refUrl,
				drawingSrc: `/dev/calibration?img=${s.id}`
			})),
		rated,
		total: all.length,
		target: TARGET
	};
};

export const actions: Actions = {
	rate: async ({ request }) => {
		if (!dev) error(404, 'Not found');
		const f = await request.formData();
		try {
			addLabel(String(f.get('sample')), Number(f.get('human')));
		} catch (e) {
			return fail(400, { message: String(e) });
		}
		return { ok: true };
	},
	delete: async ({ request }) => {
		if (!dev) error(404, 'Not found');
		const f = await request.formData();
		try {
			deleteSample(String(f.get('sample')));
		} catch (e) {
			return fail(400, { message: String(e) });
		}
		return { ok: true };
	}
};
