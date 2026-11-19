import { defineConfig } from 'vitest/config';
import { searchForWorkspaceRoot } from 'vite';
import adapter from '@sveltejs/adapter-node';
import { sveltekit } from '@sveltejs/kit/vite';
import { attachLobbySockets } from './src/lib/server/lobby/socket';

export default defineConfig({
	// one .env at the repo root (git-ignored) for every app and tool
	envDir: '../..',
	server: {
		// The WASM packages live in crates/*/pkg, outside apps/web; let the dev server serve them.
		fs: { allow: [searchForWorkspaceRoot(process.cwd())] },
		// Endless references: the Rust ref-service (cargo run -p pencilpilot-ref-service).
		proxy: Object.fromEntries(
			['/api/ref', '/api/report', '/api/health', '/api/pool', '/refimg'].map((p) => [
				p,
				process.env.REF_SERVICE_URL ?? 'http://127.0.0.1:8787'
			])
		)
	},
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
