import { expect, test } from '@playwright/test';

test('home page shows the hero and categories', async ({ page }) => {
	await page.goto('/');
	await expect(page.getByRole('heading', { level: 1 })).toContainText('Score it.');
	await expect(page.getByRole('link', { name: /Anime/ })).toHaveAttribute(
		'href',
		'/draw?category=anime'
	);
});

test('main navigation works', async ({ page }) => {
	await page.goto('/');
	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Lessons' })
		.click();
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Lessons');
});
