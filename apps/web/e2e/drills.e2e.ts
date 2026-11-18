import { expect, test, type Page } from '@playwright/test';

test('/drills lists every drill, grouped by skill', async ({ page }) => {
	await page.goto('/drills');
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Drills');
	await expect(page.getByRole('heading', { name: 'Line confidence' })).toBeVisible();
	await expect(page.getByRole('link', { name: 'Ghosted lines' })).toHaveAttribute(
		'href',
		'/drills/ghosted-lines'
	);
	await expect(page.locator('a[href^="/drills/"]')).toHaveCount(13);
});

/** Drag the mouse from the first guide dot to the second. */
async function joinDots(page: Page) {
	const dots = page.locator('[data-guide] circle');
	await expect(dots).toHaveCount(2);
	const a = (await dots.nth(0).boundingBox())!;
	const b = (await dots.nth(1).boundingBox())!;
	const [ax, ay] = [a.x + a.width / 2, a.y + a.height / 2];
	const [bx, by] = [b.x + b.width / 2, b.y + b.height / 2];
	await page.mouse.move(ax, ay);
	await page.mouse.down();
	for (let i = 1; i <= 25; i++)
		await page.mouse.move(ax + ((bx - ax) * i) / 25, ay + ((by - ay) * i) / 25);
	await page.mouse.up();
}

test('ghosted lines: draw each round and see the summary', async ({ page }) => {
	await page.goto('/drills/ghosted-lines?seed=1234');
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Ghosted lines');
	const rounds = 6;
	for (let r = 1; r <= rounds; r++) {
		await expect(page.getByLabel(`Round ${r} of ${rounds}`)).toBeVisible();
		await expect(page.getByLabel(/Drawing canvas/)).toHaveAttribute('data-ready', 'true', {
			timeout: 15_000
		});
		await joinDots(page);
		await expect(page.getByRole('button', { name: 'Undo (Ctrl+Z)' })).toBeEnabled();
		await page.getByRole('button', { name: r === rounds ? /Finish drill/ : /Next round/ }).click();
	}
	const avg = page.getByTestId('drill-average');
	await expect(avg).toBeVisible({ timeout: 15_000 });
	const score = parseFloat((await avg.textContent()) ?? '0');
	expect(score).toBeGreaterThanOrEqual(1);
	expect(score).toBeLessThanOrEqual(10);
	await expect(page.getByRole('button', { name: /Again/ })).toBeVisible();
	await expect(page.getByRole('link', { name: 'Back to lessons' })).toHaveAttribute(
		'href',
		'/lessons'
	);

	const saved = await page.evaluate(() => JSON.parse(localStorage.getItem('pp.drills') ?? '[]'));
	expect(saved).toHaveLength(1);
	expect(saved[0]).toMatchObject({ id: 'ghosted-lines', avg: score });
	expect(saved[0].rounds).toHaveLength(rounds);
});
