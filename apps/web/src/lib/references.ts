/** Approved reference pictures (metadata in content/references/<category>.json; images are downloaded, never committed). */
export interface Reference {
	id: string;
	source: string;
	sourceId: number;
	category: string;
	pageUrl: string;
	author: string;
	authorUrl: string;
	license: string;
	licenseUrl: string;
	tags: string[];
	width: number;
	height: number;
	title: string;
	keypoints: unknown[];
	/** search query the importer used */
	query?: string;
	/** topic id inside the category (content/topics.json) */
	topic?: string;
	/** 0–100, higher = harder (grader::difficulty) */
	difficulty?: number;
	/** automatic tier 1–4 */
	tier?: number;
	/** reviewer's tier, wins over `tier` */
	tierOverride?: number;
}

const files = import.meta.glob<{ references: Reference[] }>(
	'../../../../content/references/*.json',
	{
		eager: true,
		import: 'default'
	}
);

/** All approved references (candidates.json and rejected.json are not included). */
export const references: Reference[] = Object.entries(files)
	.filter(([path]) => !/\/(candidates|rejected)\.json$/.test(path))
	.flatMap(([, f]) => f.references ?? []);

export const imageUrl = (r: Pick<Reference, 'id'>) => `/references/${r.id}.jpg`;

export const byId = (id: string | null | undefined) => references.find((r) => r.id === id);

export const inCategory = (category: string) => references.filter((r) => r.category === category);

/** Difficulty tiers, easiest first. */
export const TIERS = [
	{ id: 1, name: 'Beginner', blurb: 'Simple shapes, clear outlines' },
	{ id: 2, name: 'Easy', blurb: 'A few more details' },
	{ id: 3, name: 'Medium', blurb: 'Texture, shading, more parts' },
	{ id: 4, name: 'Hard', blurb: 'Busy, detailed, lots of tone' }
] as const;

export const tierOf = (r: Reference) => r.tierOverride ?? r.tier ?? 2;
export const tierName = (t: number) => TIERS.find((x) => x.id === t)?.name ?? 'Any';

export interface RefFilter {
	topic?: string | null;
	tier?: number | null;
}

/** References in a category matching the optional topic/tier filter. */
export function filtered(category: string, f: RefFilter = {}) {
	return inCategory(category).filter(
		(r) => (!f.topic || r.topic === f.topic) && (!f.tier || tierOf(r) === f.tier)
	);
}

/** A random reference from a category (and filter), avoiding `exclude` when there is a choice. */
export function pickReference(
	category: string,
	exclude?: string,
	rand = Math.random,
	filter: RefFilter = {}
) {
	const pool = filtered(category, filter);
	const choices = pool.length > 1 ? pool.filter((r) => r.id !== exclude) : pool;
	return choices.length ? choices[Math.floor(rand() * choices.length)] : undefined;
}

/** Display title: the curated title, else the first two tags. */
export const titleOf = (r: Reference) =>
	r.title ||
	r.tags
		.slice(0, 2)
		.join(' ')
		.replace(/^./, (c) => c.toUpperCase());

/**
 * Render a reference into a w×h white canvas (contain-fit, centred), the same way the
 * drawing canvas shows it, and return its pixels for the grader.
 */
export async function referencePixels(r: Reference, w: number, h: number): Promise<ImageData> {
	const img = new Image();
	img.src = imageUrl(r);
	await img.decode();
	const c = new OffscreenCanvas(w, h);
	const ctx = c.getContext('2d')!;
	ctx.fillStyle = '#fff';
	ctx.fillRect(0, 0, w, h);
	const s = Math.min(w / img.naturalWidth, h / img.naturalHeight);
	const dw = img.naturalWidth * s;
	const dh = img.naturalHeight * s;
	ctx.drawImage(img, (w - dw) / 2, (h - dh) / 2, dw, dh);
	return ctx.getImageData(0, 0, w, h);
}
