import { expect, test } from '@playwright/test';

test('category → difficulty → topic → draw', async ({ page }) => {
	await page.goto('/');
	await page
		.getByRole('link', { name: /Animals/ })
		.first()
		.click();
	await expect(page).toHaveURL(/\/practice\/animals$/);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Animals');

	const tiers = page.getByRole('radiogroup', { name: 'Difficulty' });
	await expect(tiers.getByRole('radio')).toHaveCount(4);
	// A fresh visitor is recommended Beginner.
	await expect(tiers.getByRole('radio', { name: /Beginner/ })).toContainText('Recommended');

	// Pick an enabled tier, then the first available topic.
	const enabled = tiers.locator('[role=radio]:not([disabled])');
	await enabled.last().click();
	const topic = page.locator('a.topic:not([aria-disabled=true])').nth(1);
	await topic.click();
	await expect(page).toHaveURL(/\/draw\?category=animals&topic=[a-z-]+&tier=\d/);
	await expect(page.getByRole('region', { name: 'Reference', exact: true })).toBeVisible();
});

test('unknown category is a 404', async ({ page }) => {
	const res = await page.goto('/practice/nope');
	expect(res?.status()).toBe(404);
});
