import { expect, test, type Page } from '@playwright/test';

async function scribble(page: Page, y = 0.5) {
	const canvas = page.getByLabel(/Drawing canvas/);
	const box = (await canvas.boundingBox())!;
	await page.mouse.move(box.x + box.width * 0.2, box.y + box.height * y);
	await page.mouse.down();
	for (let i = 1; i <= 20; i++) {
		await page.mouse.move(
			box.x + box.width * (0.2 + i * 0.03),
			box.y + box.height * (y + Math.sin(i / 3) * 0.1)
		);
	}
	await page.mouse.up();
}

async function inkPixels(page: Page) {
	return page.getByLabel(/Drawing canvas/).evaluate((c: HTMLCanvasElement) => {
		const d = c.getContext('2d')!.getImageData(0, 0, c.width, c.height).data;
		let n = 0;
		for (let i = 0; i < d.length; i += 4) if (d[i] < 200) n++;
		return n;
	});
}

/** Read the ink count once two frames in a row agree (the last dabs land a frame after pointer-up). */
async function settledInk(page: Page) {
	let prev = -1;
	for (;;) {
		await page.waitForTimeout(100);
		const n = await inkPixels(page);
		if (n === prev) return n;
		prev = n;
	}
}

test('draw, undo, redo, switch brush, submit', async ({ page }) => {
	await page.goto('/draw');
	await expect(page.getByLabel(/Drawing canvas/)).toHaveAttribute('data-ready', 'true', {
		timeout: 15_000
	});
	await expect(page.getByRole('button', { name: /Submit drawing/ })).toBeDisabled();

	await scribble(page);
	const after = await settledInk(page);
	expect(after).toBeGreaterThan(50);

	await page.getByRole('button', { name: /Undo/ }).click();
	await expect.poll(() => inkPixels(page)).toBe(0);
	await page.getByRole('button', { name: /Redo/ }).click();
	await expect.poll(() => inkPixels(page)).toBe(after);

	await page.getByRole('tab', { name: 'Markers' }).click();
	await page.getByRole('button', { name: /^Marker fat/ }).click();
	await scribble(page, 0.7);
	await expect.poll(() => inkPixels(page)).toBeGreaterThan(after);

	await page.getByRole('button', { name: /Submit drawing/ }).click();
	await expect(page).toHaveURL(/\/results$/);
	await expect(page.getByRole('img', { name: 'Your submitted drawing' })).toBeVisible();
	await expect(page.getByText('Brushes used')).toBeVisible();
});

test('brush search finds brushes across categories', async ({ page }) => {
	await page.goto('/draw');
	await page.getByRole('searchbox').fill('watercolor');
	await expect(page.getByRole('list', { name: 'Brushes' }).getByRole('listitem')).not.toHaveCount(
		0
	);
	await expect(page.getByRole('button', { name: /^Thin watercolor/ })).toBeVisible();
});
