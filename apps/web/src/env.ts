import { defineEnvVars } from '@sveltejs/kit/env';

const optional = (value: string | undefined) => value || undefined;

/** Server-side configuration (read from the environment / .env when the app starts). */
export const variables = defineEnvVars({
	DATABASE_PATH: {
		schema: optional,
		description: 'SQLite file for accounts + synced progress. Default: <repo>/data/pencilpilot.db'
	},
	GOOGLE_CLIENT_ID: {
		schema: optional,
		description: 'Google OAuth client id. Google sign-in is hidden unless both Google vars are set.'
	},
	GOOGLE_CLIENT_SECRET: {
		schema: optional,
		description: 'Google OAuth client secret.'
	},
	COMMUNITY: {
		schema: optional,
		description:
			'Set to "on" to enable community critique (sharing + peer judging). Off by default.'
	},
	REF_SERVICE_URL: {
		schema: optional,
		description:
			'Where the ref-service runs (NSFW guard for shared drawings). Default: http://127.0.0.1:8787'
	}
});
