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
	PASS_SIGNING_KEY: {
		schema: optional,
		description:
			'The ed25519 key (32-byte seed, 64 hex characters) that signs the app’s Draw together passes. Make one with `node apps/web/scripts/pass-key.mjs`; the app is built with its public half. Development uses a known key when unset; production can’t issue passes without it.'
	},
	REF_SERVICE_URL: {
		schema: optional,
		description:
			'Where the ref-service runs (NSFW guard for shared drawings). Default: http://127.0.0.1:8787'
	}
});
