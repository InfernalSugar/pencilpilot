import { error } from '@sveltejs/kit';
import { getDb } from '#lib/server/db.js';
import { snapshot } from '#lib/server/progress-store.js';
import type { RequestHandler } from './$types';

/** "Download my data": everything we store about the user, as JSON. */
export const GET: RequestHandler = ({ locals }) => {
	if (!locals.user) error(401, 'Sign in first');
	const db = getDb();
	const user = db
		.prepare(
			'SELECT id, email, name, created_at, google_id IS NOT NULL AS google FROM users WHERE id = ?'
		)
		.get(locals.user.id);
	const body = JSON.stringify(
		{
			exportedAt: new Date().toISOString(),
			user,
			...snapshot(db, locals.user.id),
			tipVotes: db
				.prepare(
					'SELECT rule_id, vote, category, score, attempt_at, created_at FROM tip_votes WHERE user_id = ?'
				)
				.all(locals.user.id),
			sharedDrawings: (
				db
					.prepare(
						`SELECT id, reference_id, category, score, rating, comparisons, wins, hidden, created_at, png
						 FROM shared_drawings WHERE user_id = ?`
					)
					.all(locals.user.id) as { png: Buffer }[]
			).map((d) => ({ ...d, png: `data:image/png;base64,${d.png.toString('base64')}` })),
			communityJudgements: {
				closer: db
					.prepare('SELECT a_id, b_id, winner_id, created_at FROM closer_votes WHERE voter_id = ?')
					.all(locals.user.id),
				ratings: db
					.prepare(
						'SELECT drawing_id, proportions, lines, shading, overall, created_at FROM rubric_ratings WHERE rater_id = ?'
					)
					.all(locals.user.id)
			}
		},
		null,
		2
	);
	return new Response(body, {
		headers: {
			'content-type': 'application/json',
			'content-disposition': 'attachment; filename="pencilpilot-data.json"'
		}
	});
};
