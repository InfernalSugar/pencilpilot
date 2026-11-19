import { error } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import { communityEnabled, drawingPng } from '#lib/server/community.js';
import type { RequestHandler } from './$types';

/** A shared drawing's PNG: signed-in people only, never cached publicly. */
export const GET: RequestHandler = ({ params, locals }) => {
	if (!communityEnabled() || !locals.user) error(404, 'Not found');
	const png = drawingPng(getDb(), params.id, locals.user.id);
	if (!png) error(404, 'Not found');
	return new Response(new Uint8Array(png), {
		headers: {
			'content-type': 'image/png',
			'cache-control': 'private, max-age=3600',
			'x-content-type-options': 'nosniff'
		}
	});
};
