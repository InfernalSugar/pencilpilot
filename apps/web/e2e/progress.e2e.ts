import { expect, test } from '@playwright/test';

const DAY = 24 * 3600 * 1000;

test('progress page shows stats, chart, skills and habits from history', async ({ page }) => {
	const now = Date.now();
	const history = [5, 4, 3, 2, 1, 0].map((d, i) => ({
		category: 'animals',
		topic: 'pets',
		tier: 1,
		score: 4 + i,
		at: now - d * DAY,
		title: `Cat ${i}`,
		fixes: i % 2 ? ['too-big', 'shaky-lines'] : ['too-big'],
		skills: {
			outline: 0.5 + i * 0.05,
			placement: 0.6,
			silhouette: 0.7,
			tone: 0.4,
			line_confidence: 0.8
		}
	}));
	await page.addInitScript((h) => localStorage.setItem('pp.attempts', JSON.stringify(h)), history);
	await page.goto('/progress');
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Progress');
	await expect(page.getByText('Personal best').locator('..')).toContainText('9.0');
	await expect(page.getByText('Streak').locator('..')).toContainText('6');
	await expect(page.getByRole('img', { name: /Daily average score, 6 days/ })).toBeVisible();
	await expect(page.getByText('Needs work').locator('..')).toContainText('Light');
	// too-big is in all of the last drawings → a habit with a lesson link
	const habit = page.getByText('Your drawing is too big');
	await expect(habit).toBeVisible();
	await expect(page.getByRole('link', { name: 'Measuring with units' })).toHaveAttribute(
		'href',
		'/lessons/measuring-with-units?from=too-big'
	);
});

test('empty progress page invites a first drawing', async ({ page }) => {
	await page.goto('/progress');
	await expect(page.getByText('No drawings yet')).toBeVisible();
});
