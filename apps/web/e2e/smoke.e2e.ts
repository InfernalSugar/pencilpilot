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

test('community is off unless switched on', async ({ page }, info) => {
	test.skip(info.project.name !== 'dev', 'the build server runs with COMMUNITY=on');
	const res = await page.goto('/community');
	expect(res?.status()).toBe(404);
	await expect(page.getByRole('link', { name: 'Community' })).toHaveCount(0);
});
