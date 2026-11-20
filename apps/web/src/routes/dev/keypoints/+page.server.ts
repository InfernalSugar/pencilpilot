import { dev } from '$app/env';
import { error, fail } from '@sveltejs/kit';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import type { Actions, PageServerLoad } from './$types';
import type { Reference } from '#lib/references.js';
import { cleanKeypoints } from '#lib/keypoints.js';

// Dev-only: mark the keypoints students tap after drawing a curated reference (M5-T03).
// Writes straight into content/references/<category>.json, like /dev/references.
const DIR = resolve(process.cwd(), '../../content/references');
const CATEGORIES = ['anime', 'real-life', 'animals', 'environment', 'cartoon', 'fundamentals'];

type List = { references: Reference[] };
const file = (c: string) => join(DIR, `${c}.json`);
const read = (c: string): List =>
	existsSync(file(c)) ? JSON.parse(readFileSync(file(c), 'utf8')) : { references: [] };

function guard() {
	if (!dev) error(404, 'Not found');
}

export const load: PageServerLoad = () => {
	guard();
	const refs = CATEGORIES.flatMap((c) => read(c).references);
	return { refs, categories: CATEGORIES.filter((c) => refs.some((r) => r.category === c)) };
};

export const actions: Actions = {
	save: async ({ request }) => {
		guard();
		const f = await request.formData();
		const id = String(f.get('id'));
		const category = String(f.get('category'));
		if (!CATEGORIES.includes(category)) return fail(400, { message: 'bad category' });
		let raw: unknown;
		try {
			raw = JSON.parse(String(f.get('keypoints')));
		} catch {
			return fail(400, { message: 'keypoints are not JSON' });
		}
		const keypoints = cleanKeypoints(raw);
		if (!keypoints) return fail(400, { message: 'bad keypoints' });
		const list = read(category);
		const ref = list.references.find((r) => r.id === id);
		if (!ref) return fail(404, { message: 'no such reference' });
		ref.keypoints = keypoints;
		writeFileSync(file(category), JSON.stringify(list, null, 2) + '\n');
		return { saved: id, count: keypoints.length };
	}
};
