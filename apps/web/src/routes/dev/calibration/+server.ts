import { dev } from '$app/env';
import { error, json } from '@sveltejs/kit';
import { saveSample, samplePng } from '#lib/server/calibration.js';
import type { RequestHandler } from './$types';

/** Save a graded drawing as a calibration sample (dev server only). */
export const POST: RequestHandler = async ({ request }) => {
	if (!dev) error(404, 'Not found');
	const b = await request.json();
	try {
		const s = saveSample(
			{
				category: String(b.category),
				refId: String(b.refId),
				refUrl: String(b.refUrl),
				similarity: Number(b.similarity),
				score: Number(b.score)
			},
			String(b.png)
		);
		return json(s);
	} catch (e) {
		error(400, String(e));
	}
};

/** Serve a sample's drawing: /dev/calibration?img=<id> */
export const GET: RequestHandler = ({ url }) => {
	if (!dev) error(404, 'Not found');
	const png = samplePng(url.searchParams.get('img') ?? '');
	if (!png) error(404, 'No such sample');
	return new Response(new Uint8Array(png), { headers: { 'content-type': 'image/png' } });
};
