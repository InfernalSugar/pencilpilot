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
	}
});
