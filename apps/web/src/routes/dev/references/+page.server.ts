import { dev } from '$app/env';
import { error, fail } from '@sveltejs/kit';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import type { Actions, PageServerLoad } from './$types';
import type { Reference } from '#lib/references.js';

// Dev-only review queue for reference candidates (ADR-0003: nothing goes live unapproved).
const DIR = resolve(process.cwd(), '../../content/references');
const IMAGES = resolve(process.cwd(), 'static/references');
const CATEGORIES = ['anime', 'real-life', 'animals', 'environment', 'fundamentals'];

type List = { references: Reference[] };
const read = <T>(f: string, empty: T): T => {
	const p = join(DIR, f);
	return existsSync(p) ? JSON.parse(readFileSync(p, 'utf8')) : empty;
};
const write = (f: string, v: unknown) =>
	writeFileSync(join(DIR, f), JSON.stringify(v, null, 2) + '\n');

function guard() {
	if (!dev) error(404, 'Not found');
}

export const load: PageServerLoad = () => {
	guard();
	const candidates = read<List>('candidates.json', { references: [] }).references;
	const approved = Object.fromEntries(
		CATEGORIES.map((c) => [c, read<List>(`${c}.json`, { references: [] }).references.length])
	);
	return { candidates, approved, categories: CATEGORIES };
};

function take(id: string) {
	const cands = read<List>('candidates.json', { references: [] });
	const ref = cands.references.find((r) => r.id === id);
	if (!ref) return null;
	cands.references = cands.references.filter((r) => r.id !== id);
	write('candidates.json', cands);
	return ref;
}

export const actions: Actions = {
	approve: async ({ request }) => {
		guard();
		const f = await request.formData();
		const id = String(f.get('id'));
		const category = String(f.get('category'));
		if (!CATEGORIES.includes(category)) return fail(400, { message: 'bad category' });
		const ref = take(id);
		if (!ref) return fail(404, { message: 'not a candidate' });
		const list = read<List>(`${category}.json`, { references: [] });
		list.references.push({ ...ref, category, title: String(f.get('title') ?? '').trim() });
		write(`${category}.json`, list);
		return { ok: id };
	},
	reject: async ({ request }) => {
		guard();
		const id = String((await request.formData()).get('id'));
		if (!take(id)) return fail(404, { message: 'not a candidate' });
		const rej = read<{ ids: string[] }>('rejected.json', { ids: [] });
		rej.ids.push(id);
		write('rejected.json', rej);
		rmSync(join(IMAGES, `${id}.jpg`), { force: true });
		return { ok: id };
	}
};
