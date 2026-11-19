import { error, json } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import {
	cleanReference,
	communityEnabled,
	looksLikePng,
	MAX_PNG_BYTES,
	moderate,
	shareDrawing,
	sharesByUser
} from '#lib/server/community.js';
import type { RequestHandler } from './$types';

const REFUSED: Record<string, string> = {
	nsfw: 'This drawing can’t be shared.',
	unavailable: 'Sharing is paused right now (the safety check isn’t running). Try again later.'
};

/** Share a drawing with the community, after the NSFW guard says it's fine. */
export const POST: RequestHandler = async ({ request, locals }) => {
	if (!communityEnabled()) error(404, 'Not found');
	if (!locals.user) error(401, 'Sign in to share');
	const text = await request.text();
	if (text.length > MAX_PNG_BYTES * 1.4 + 20_000) error(413, 'Drawing too large');
	let body: { png?: unknown; reference?: unknown; score?: unknown; attemptAt?: unknown };
	try {
		body = JSON.parse(text);
	} catch {
		error(400, 'Bad JSON');
	}
	const reference = cleanReference(body.reference);
	if (!reference) error(400, 'Unknown reference');
	const m =
		typeof body.png === 'string' && /^data:image\/png;base64,([A-Za-z0-9+/=]+)$/.exec(body.png);
	if (!m) error(400, 'Expected a PNG');
	const png = new Uint8Array(Buffer.from(m[1], 'base64'));
	if (!looksLikePng(png)) error(400, 'Expected a PNG');
	if (!sharesByUser.take(locals.user.id)) error(429, 'That’s a lot of sharing for one day');

	const verdict = await moderate(png);
	if (!verdict.safe)
		return json(
			{ ok: false, message: REFUSED[verdict.reason] ?? REFUSED.nsfw },
			{ status: verdict.reason === 'unavailable' ? 503 : 422 }
		);

	const score = typeof body.score === 'number' && Number.isFinite(body.score) ? body.score : null;
	const attemptAt =
		typeof body.attemptAt === 'number' && Number.isInteger(body.attemptAt) ? body.attemptAt : null;
	const id = shareDrawing(getDb(), locals.user.id, { png, reference, score, attemptAt });
	return json({ ok: true, id });
};
