import { defineConfig } from '@playwright/test';

export default defineConfig({
	testMatch: '**/*.e2e.{ts,js}',
	webServer: [
		{ command: 'npm run build && npm run preview', port: 4173 },
		// `pnpm dev` serves files differently (e.g. WASM from outside apps/web), so smoke-test it too.
		{ command: 'npx vite dev --port 5179 --strictPort', port: 5179 }
	],
	projects: [
		{ name: 'build', use: { baseURL: 'http://localhost:4173' } },
		{ name: 'dev', use: { baseURL: 'http://localhost:5179' }, testMatch: '**/smoke.e2e.ts' }
	]
});
