import { expect, test, type Page } from '@playwright/test';

// The reference can sit beside the canvas in its own pane, resized with a divider.

const PIC = {
	id: 'px-515151',
	source: 'pixabay',
	sourceId: 515151,
	category: 'animals',
	pageUrl: 'https://pixabay.com/photos/whale-515151/',
	author: 'Ann',
	authorUrl: '',
	license: 'Pixabay Content License',
	licenseUrl: 'https://pixabay.com/service/license-summary/',
	tags: ['whale'],
	width: 400,
	height: 300,
	title: 'Whale',
	keypoints: [],
	topic: 'sea-life',
	tier: 2,
	imageUrl: '/e2e-whale.svg',
	live: true
};

async function open(page: Page) {
	await page.route('**/api/ref?*', (r) => r.fulfill({ json: PIC }));
	await page.route('**/e2e-whale.svg', (r) =>
		r.fulfill({
			contentType: 'image/svg+xml',
			body: '<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300"><rect width="400" height="300" fill="#3d6bff"/></svg>'
		})
	);
	await page.goto('/draw?category=animals&tier=2');
	await expect(page.getByLabel(/Drawing canvas/)).toHaveAttribute('data-ready', 'true', {
		timeout: 15_000
	});
}

const width = async (page: Page) =>
	(await page.getByRole('region', { name: 'Reference beside the canvas' }).boundingBox())!.width;

test('the reference sits beside the canvas and the divider resizes it', async ({ page }) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await open(page);
	const pane = page.getByRole('region', { name: 'Reference beside the canvas' });
	await expect(pane.getByRole('img', { name: /Reference: whale/ })).toBeVisible();
	const before = await width(page);
	expect(before).toBeGreaterThan(300); // bigger than the old 300 px panel picture

	const divider = page.getByRole('separator', { name: 'Resize the reference' });
	const b = (await divider.boundingBox())!;
	await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
	await page.mouse.down();
	await page.mouse.move(b.x + 200, b.y + b.height / 2, { steps: 6 });
	await page.mouse.up();
	await expect.poll(() => width(page)).toBeGreaterThan(before + 150);
	const dragged = await width(page);

	// arrow keys work too
	await divider.focus();
	await page.keyboard.press('ArrowLeft');
	await expect.poll(() => width(page)).toBeLessThan(dragged);
	const kept = Number(await divider.getAttribute('aria-valuenow'));

	// remembered after a reload
	await page.reload();
	await expect(divider).toHaveAttribute('aria-valuenow', String(kept));
});

test('the reference can go back to the panel, float, and come back beside', async ({ page }) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await open(page);
	const pane = page.getByRole('region', { name: 'Reference beside the canvas' });
	await pane.getByRole('button', { name: 'To the panel' }).click();
	await expect(pane).toHaveCount(0);
	const tile = page.getByRole('region', { name: 'Reference', exact: true });
	await expect(tile.getByRole('img', { name: /Reference: whale/ })).toBeVisible();

	await tile.getByRole('button', { name: /Float/ }).click();
	const win = page.getByRole('region', { name: 'Reference picture' });
	await expect(win).toBeVisible();
	await win.getByRole('button', { name: 'Beside' }).click();
	await expect(win).toHaveCount(0);
	await expect(pane).toBeVisible();
});

test('phones: the small picture opens the big view, stacked over the canvas', async ({ page }) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await open(page);
	const pane = page.getByRole('region', { name: 'Reference beside the canvas' });
	await expect(pane).toHaveCount(0); // phones start with the small picture
	await page.getByRole('button', { name: 'Show the reference bigger, beside the canvas' }).click();
	await expect(pane).toBeVisible();
	const p = (await pane.boundingBox())!;
	const c = (await page.getByLabel(/Drawing canvas/).boundingBox())!;
	expect(p.y + p.height).toBeLessThanOrEqual(c.y + 1); // above the canvas, not beside it
	expect(p.width).toBeGreaterThan(300);
});
