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
	// The app's updates (lib/server/updates, served by server.js in front of the site)
	UPDATES_DIR: {
		schema: optional,
		description: "Where the app's updates are kept on disk. Default: <repo>/data/updates"
	},
	UPDATES_TOKEN: {
		schema: optional,
		description:
			'The token the Release workflow uploads updates with (its UPDATES_TOKEN secret). Uploads are off without it.'
	},
	UPDATES_ORIGIN: {
		schema: optional,
		description:
			"The site's public address for update download links, e.g. https://pencilpilot.app (default: ORIGIN, else the request's host)"
	},
	UPDATES_KEEP: {
		schema: optional,
		description:
			'How many releases the orphan sweep keeps (the newest, plus the current one). Default: 3'
	},
	REF_SERVICE_URL: {
		schema: optional,
		description:
			'Where the ref-service runs (NSFW guard for shared drawings). Default: http://127.0.0.1:8787'
	}
});
