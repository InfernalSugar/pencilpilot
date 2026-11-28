import { defineConfig } from 'vitest/config';
import { searchForWorkspaceRoot } from 'vite';
import adapter from '@sveltejs/adapter-node';
import { sveltekit } from '@sveltejs/kit/vite';
import { attachLobbySockets } from './src/lib/server/lobby/socket';
import { createUpdatesHandler } from './src/lib/server/updates/http';

const REF_SERVICE = process.env.REF_SERVICE_URL ?? 'http://127.0.0.1:8787';
// Endless references: the Rust ref-service (cargo run -p pencilpilot-ref-service).
const refProxy = Object.fromEntries(
	['/api/ref', '/api/report', '/api/health', '/api/pool', '/refimg'].map((p) => [p, REF_SERVICE])
);

export default defineConfig({
	// one .env at the repo root (git-ignored) for every app and tool
	envDir: '../..',
	server: {
		// The WASM packages live in crates/*/pkg, outside apps/web; let the dev server serve them.
		fs: { allow: [searchForWorkspaceRoot(process.cwd())] },
		proxy: {
			...refProxy,
			// The owner's picture review (/dev/pictures), dev server only. xfwd passes the
			// browser's address on, so the ref-service refuses anyone but this computer.
			'/api/admin': { target: REF_SERVICE, xfwd: true }
		}
	},
	// `vite preview` would reuse server.proxy: never the admin API there
	preview: { proxy: refProxy },
	plugins: [
		// Private lobbies: live connections on /lobby-ws (production: server.js does the same).
		{
			name: 'pencilpilot-lobby-sockets',
			configureServer(s) {
				if (s.httpServer) attachLobbySockets(s.httpServer as import('node:http').Server);
			},
			configurePreviewServer(s) {
				attachLobbySockets(s.httpServer as import('node:http').Server);
			}
		},
		// The app's updates (/updates/…; production: server.js does the same).
		{
			name: 'pencilpilot-updates',
			configureServer(s) {
				const serve = createUpdatesHandler();
				s.middlewares.use((req, res, next) => serve(req, res) || next());
			},
			configurePreviewServer(s) {
				const serve = createUpdatesHandler();
				s.middlewares.use((req, res, next) => serve(req, res) || next());
			}
		},
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},

			// Node server: accounts need a database, and the site sits next to the ref-service.
			adapter: adapter()
		})
	],
	test: {
		expect: { requireAssertions: true },
		projects: [
			{
				extends: './vite.config.ts',
				test: {
					name: 'server',
					environment: 'node',
					include: ['src/**/*.{test,spec}.{js,ts}'],
					exclude: ['src/**/*.svelte.{test,spec}.{js,ts}']
				}
			}
		]
	}
});
