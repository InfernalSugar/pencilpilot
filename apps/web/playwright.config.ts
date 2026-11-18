import { defineConfig } from '@playwright/test';

export default defineConfig({
	testMatch: '**/*.e2e.{ts,js}',
	webServer: [
		{
			command: 'npm run build && npm run preview',
			port: 4173,
			// throwaway database for e2e runs
			env: { DATABASE_PATH: `${process.env.TMPDIR ?? '/tmp'}/pp-e2e-${Date.now()}.db` }
		},
		// `pnpm dev` serves files differently (e.g. WASM from outside apps/web), so smoke-test it too.
		{
			command: 'npx vite dev --port 5179 --strictPort',
			port: 5179,
			env: { DATABASE_PATH: `${process.env.TMPDIR ?? '/tmp'}/pp-e2e-dev-${Date.now()}.db` }
		}
	],
	projects: [
		{ name: 'build', use: { baseURL: 'http://localhost:4173' } },
		{ name: 'dev', use: { baseURL: 'http://localhost:5179' }, testMatch: '**/smoke.e2e.ts' }
	]
});
