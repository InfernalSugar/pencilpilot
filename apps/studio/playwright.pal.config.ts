import { defineConfig } from '@playwright/test';

// The app's window, in a browser: the built single-page app, with Tauri's IPC stubbed
// in each test (the Rust side has its own tests).
export default defineConfig({
	testDir: 'e2e',
	testMatch: '**/*.e2e.ts',
	use: { baseURL: 'http://localhost:4199' },
	webServer: {
		command: 'pnpm build && pnpm exec vite preview --port 4199 --strictPort',
		url: 'http://localhost:4199',
		reuseExistingServer: false,
		timeout: 180_000
	}
});
