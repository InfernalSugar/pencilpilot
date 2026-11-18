import { expect, test } from '@playwright/test';

test('home page loads and the WASM grader runs', async ({ page }) => {
	await page.goto('/');
	await expect(page.locator('h1')).toContainText('PencilPilot');
	await expect(page.getByTestId('grader-status')).toContainText('ready', { timeout: 15_000 });
});
