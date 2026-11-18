import { expect, test } from '@playwright/test';

test('paint engine and grader load', async ({ page }) => {
	const errors: string[] = [];
	page.on('pageerror', (e) => errors.push(e.message));
	await page.goto('/draw');
	await expect(page.getByLabel(/Drawing canvas/)).toHaveAttribute('data-ready', 'true', {
		timeout: 30_000
	});
	await expect(page.getByRole('alert')).toHaveCount(0);
	expect(errors).toEqual([]);
});
