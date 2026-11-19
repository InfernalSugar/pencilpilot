import { expect, test } from '@playwright/test';

test('your own daily challenge follows what you draw', async ({ page }) => {
	const now = Date.now();
	const history = Array.from({ length: 10 }, (_, i) => ({
		category: 'anime',
		topic: null,
		tier: 1,
		score: 6,
		at: now - (i + 1) * 3_600_000
	}));
	await page.addInitScript((h) => localStorage.setItem('pp.attempts', JSON.stringify(h)), history);
	await page.goto('/daily');
	const yours = page.getByRole('heading', { name: 'Your challenge' });
	await expect(yours).toBeVisible();
	// either "You draw a lot of Anime." or, on a variety day, "A change from Anime today: …"
	await expect(page.getByText(/Anime/).first()).toBeVisible();
	await expect(page.getByText(/You draw a lot of Anime|A change from Anime/)).toBeVisible();
	const link = page.getByRole('link', { name: /Draw your challenge/ });
	const href = (await link.getAttribute('href'))!;
	const day = new Date().toISOString().slice(0, 10);
	expect(href).toContain(`personal=${day}`);

	// the same pick after a reload
	await page.reload();
	await expect(page.getByRole('link', { name: /Draw your challenge/ })).toHaveAttribute(
		'href',
		href
	);

	// it's separate from the shared challenge
	const shared = await page
		.getByRole('link', { name: /Draw today’s picture/ })
		.getAttribute('href');
	expect(new URL(shared!, 'http://x').searchParams.get('ref')).not.toBe(
		new URL(href, 'http://x').searchParams.get('ref')
	);

	await page.getByRole('link', { name: /Draw your challenge/ }).click();
	await expect(page.getByText(/^Your challenge ·/)).toBeVisible();
	await expect(page.getByRole('button', { name: /New one/ })).toHaveCount(0);
});
