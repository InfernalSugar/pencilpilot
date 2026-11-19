import { dev } from '$app/env';
import { error, json } from '@sveltejs/kit';
import { sampleRef, saveSample, samplePng } from '#lib/server/calibration.js';
import type { RequestHandler } from './$types';

/** Save a graded drawing as a calibration sample (dev server only). */
export const POST: RequestHandler = async ({ request }) => {
	if (!dev) error(404, 'Not found');
	const b = await request.json();
	try {
		const measurements =
			b.measurements && typeof b.measurements === 'object'
				? Object.fromEntries(
						Object.entries(b.measurements as Record<string, unknown>)
							.filter(([, v]) => typeof v === 'number' && Number.isFinite(v))
							.map(([k, v]) => [k.slice(0, 40), v as number])
					)
				: undefined;
		const s = saveSample(
			{
				category: String(b.category),
				refId: String(b.refId),
				refUrl: String(b.refUrl).startsWith('data:') ? '' : String(b.refUrl),
				similarity: Number(b.similarity),
				score: Number(b.score),
				...(measurements ? { measurements } : {}),
				...(b.mode ? { mode: String(b.mode) } : {})
			},
			String(b.png),
			typeof b.refImage === 'string' ? b.refImage : undefined
		);
		return json(s);
	} catch (e) {
		error(400, String(e));
	}
};

/** Serve a sample's drawing (?img=<id>) or its kept reference (?ref=<id>). */
export const GET: RequestHandler = ({ url }) => {
	if (!dev) error(404, 'Not found');
	const refId = url.searchParams.get('ref');
	if (refId) {
		const r = sampleRef(refId);
		if (!r) error(404, 'No kept reference');
		return new Response(new Uint8Array(r.bytes), { headers: { 'content-type': r.type } });
	}
	const png = samplePng(url.searchParams.get('img') ?? '');
	if (!png) error(404, 'No such sample');
	return new Response(new Uint8Array(png), { headers: { 'content-type': 'image/png' } });
};
