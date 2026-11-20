import { defineConfig, searchForWorkspaceRoot, type Plugin } from 'vite';
import { sveltekit } from '@sveltejs/kit/vite';
import adapter from '@sveltejs/adapter-static';
import { cpSync } from 'node:fs';
import { resolve } from 'node:path';

/**
 * The brush pack (manifest, .myb files, previews) is served at /brushes, as on the
 * website. It lives in apps/web/static/brushes for now (S1 gives it its own home), and
 * is copied into this app's static/brushes (git-ignored) whenever Vite starts.
 */
const BRUSHES = resolve(import.meta.dirname, '../web/static/brushes');
const STATIC_BRUSHES = resolve(import.meta.dirname, 'static/brushes');

function brushPack(): Plugin {
	return {
		name: 'pencilpilot-brush-pack',
		config() {
			cpSync(BRUSHES, STATIC_BRUSHES, { recursive: true });
		}
	};
}

// Tauri's dev window loads this fixed address (src-tauri/tauri.conf.json → build.devUrl).
export default defineConfig({
	plugins: [
		brushPack(),
		sveltekit({
			compilerOptions: { runes: true },
			// a single-page app inside Tauri: no server, every route falls back to index.html
			adapter: adapter({ fallback: 'index.html' })
		})
	],
	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true,
		host: process.env.TAURI_DEV_HOST || false,
		// the WASM engine lives in crates/brush-wasm/pkg, outside this app
		fs: { allow: [searchForWorkspaceRoot(process.cwd())] },
		watch: { ignored: ['**/src-tauri/**'] }
	},
	envPrefix: ['VITE_', 'TAURI_ENV_*'],
	build: {
		target: 'safari16',
		sourcemap: !!process.env.TAURI_ENV_DEBUG
	}
});
