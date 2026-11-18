import { defineConfig } from 'vitest/config';
import { searchForWorkspaceRoot } from 'vite';
import adapter from '@sveltejs/adapter-auto';
import { sveltekit } from '@sveltejs/kit/vite';

export default defineConfig({
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
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},

			// adapter-auto only supports some environments, see https://svelte.dev/docs/kit/adapter-auto for a list.
			// If your environment is not supported, or you settled on a specific environment, switch out the adapter.
			// See https://svelte.dev/docs/kit/adapters for more information about adapters.
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
