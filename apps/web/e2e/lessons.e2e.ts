import { expect, test } from '@playwright/test';

test('lessons index → lesson → step through → drill link', async ({ page }) => {
	await page.goto('/lessons');
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Lessons');
	await expect(page.getByRole('heading', { name: 'Proportions & angles' })).toBeVisible();

	await page.getByRole('link', { name: /^Measuring with units/ }).click();
	await expect(page).toHaveURL(/\/lessons\/measuring-with-units$/);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Measuring with units');
	await expect(page.getByRole('img', { name: /head units/ })).toBeVisible();
	// No "Why you're here" without ?from=.
	await expect(page.getByText('Why you’re here')).toHaveCount(0);

	const bar = page.getByRole('progressbar', { name: 'Lesson progress' });
	await expect(bar).toHaveAttribute('aria-valuenow', '1');
	await page.getByRole('button', { name: 'Step →' }).click();
	await expect(bar).toHaveAttribute('aria-valuenow', '2');
	await page.keyboard.press('ArrowRight');
	await expect(bar).toHaveAttribute('aria-valuenow', '3');
	await page.keyboard.press('ArrowLeft');
	await expect(bar).toHaveAttribute('aria-valuenow', '2');
	while (await page.getByRole('button', { name: 'Step →' }).isVisible())
		await page.getByRole('button', { name: 'Step →' }).click();
	await expect(page.getByRole('link', { name: 'Try the drill →' })).toHaveAttribute(
		'href',
		'/drills/head-units'
	);
	await expect(page.getByRole('link', { name: 'Start drill' })).toHaveAttribute(
		'href',
		'/drills/head-units'
	);

	await page.getByRole('button', { name: 'Show all steps' }).click();
	await expect(page.getByRole('heading', { name: 'Pick a unit' })).toBeVisible();
	await expect(page.getByRole('heading', { name: 'Put it on the page' })).toBeVisible();

	await page.getByRole('link', { name: /^Up next\s*Finding angles/ }).click();
	await expect(page).toHaveURL(/\/lessons\/finding-angles$/);
});

test('?from= shows why you are here', async ({ page }) => {
	await page.goto('/lessons/measuring-with-units?from=too-big');
	await expect(page.getByText('Why you’re here')).toBeVisible();
	await expect(page.getByText('Your drawing is too big')).toBeVisible();
});

test('unknown lesson is a 404', async ({ page }) => {
	const res = await page.goto('/lessons/nope');
	expect(res?.status()).toBe(404);
});
