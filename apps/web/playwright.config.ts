import { defineConfig } from '@playwright/test';

export default defineConfig({
	testMatch: '**/*.e2e.{ts,js}',
	webServer: [
		{
			command: 'npm run build && npm run preview',
			port: 4173,
			env: {
				// throwaway database for e2e runs
				DATABASE_PATH: `${process.env.TMPDIR ?? '/tmp'}/pp-e2e-${Date.now()}.db`,
				// community critique on, with a fake safety check (e2e/fake-ref-service.mjs)
				COMMUNITY: 'on',
				REF_SERVICE_URL: 'http://127.0.0.1:8799'
			}
		},
		{ command: 'node e2e/fake-ref-service.mjs', url: 'http://127.0.0.1:8799/api/health' },
		// `pnpm dev` serves files differently (e.g. WASM from outside apps/web), so smoke-test it too.
		{
			command: 'npx vite dev --port 5179 --strictPort',
			port: 5179,
			env: {
				DATABASE_PATH: `${process.env.TMPDIR ?? '/tmp'}/pp-e2e-dev-${Date.now()}.db`,
				COMMUNITY: 'off'
			}
		}
	],
	projects: [
		{ name: 'build', use: { baseURL: 'http://localhost:4173' } },
		{
			name: 'dev',
			use: { baseURL: 'http://localhost:5179' },
			testMatch: ['**/smoke.e2e.ts', '**/dev-pictures.e2e.ts', '**/keypoints.e2e.ts']
		}
	]
});
