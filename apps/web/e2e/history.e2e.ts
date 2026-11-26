import { expect, test, type Page } from '@playwright/test';

const canvas = (page: Page) => page.getByLabel(/Drawing canvas/);

async function line(page: Page, y: number) {
	const c = canvas(page);
	await c.scrollIntoViewIfNeeded();
	const b = (await c.boundingBox())!;
	await page.mouse.move(b.x + b.width * 0.2, b.y + b.height * y);
	await page.mouse.down();
	for (let i = 1; i <= 20; i++)
		await page.mouse.move(b.x + b.width * (0.2 + i * 0.03), b.y + b.height * y);
	await page.mouse.up();
}

/** red / dark pixels in a band of rows */
const band = (page: Page, y: number) =>
	canvas(page).evaluate((c: HTMLCanvasElement, y) => {
		const top = Math.round(c.height * (y - 0.03));
		const d = c.getContext('2d')!.getImageData(0, top, c.width, Math.round(c.height * 0.06)).data;
		let [red, ink] = [0, 0];
		for (let i = 0; i < d.length; i += 4) {
			if (d[i] > d[i + 1] + 40) red++;
			if (d[i] < 120 && d[i + 1] < 120) ink++;
		}
		return { red, ink };
	}, y);

test('history: hand in, then keep drawing with the same layers, download, delete; drafts too', async ({
	page
}) => {
	await page.goto('/draw');
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await page.getByRole('tab', { name: /^Brushes/ }).click();
	await page.getByLabel('Brush category').selectOption({ label: 'Markers' });
	await page.getByRole('button', { name: /^Marker fat/ }).click();
	await page.getByRole('tab', { name: /^Layers/ }).click();
	await line(page, 0.3);
	await page.getByRole('button', { name: 'New layer' }).click();
	await page.getByRole('button', { name: 'New group' }).click();
	await page.getByRole('radio', { name: 'Colour #e8492f' }).click();
	await line(page, 0.6);
	await page.getByRole('button', { name: 'Add mask' }).click();
	await page.getByRole('button', { name: 'Edit mask' }).click(); // back to the colour
	await expect.poll(async () => (await band(page, 0.6)).red).toBeGreaterThan(50);
	await page.getByRole('button', { name: /Submit drawing/ }).click();
	await expect(page).toHaveURL(/\/results$/);

	await page.goto('/history');
	const cards = page.getByRole('list', { name: 'Your drawings' }).getByRole('listitem');
	await expect(cards).toHaveCount(1);
	await expect(cards.first()).toContainText('handed in');
	await expect(cards.first()).toContainText('2 layers');

	// downloads
	const [png] = await Promise.all([
		page.waitForEvent('download'),
		cards
			.first()
			.getByRole('button', { name: /as a PNG/ })
			.click()
	]);
	expect(png.suggestedFilename()).toMatch(/\.png$/);
	const [psd] = await Promise.all([
		page.waitForEvent('download'),
		cards
			.first()
			.getByRole('button', { name: /Photoshop/ })
			.click()
	]);
	const bytes = Buffer.concat(await (await psd.createReadStream()).toArray());
	expect(bytes.subarray(0, 4).toString()).toBe('8BPS');

	// keep drawing: the same layers come back
	await cards.first().getByRole('link', { name: 'Keep drawing', exact: true }).click();
	await expect(page).toHaveURL(/\/draw\?history=/);
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	const layers = page.getByRole('list', { name: 'Layers, top to bottom' });
	await expect(layers.getByRole('button', { name: 'Layer 2', exact: true })).toBeVisible();
	await expect(layers.getByRole('button', { name: 'Group 1', exact: true })).toBeVisible();
	await expect(layers.getByRole('button', { name: /Edit the mask of Layer 2/ })).toBeVisible();
	await expect.poll(async () => (await band(page, 0.6)).red).toBeGreaterThan(50);
	expect((await band(page, 0.3)).ink).toBeGreaterThan(50);
	// nothing to undo: the restored drawing is the starting point
	await expect(page.getByRole('button', { name: /Undo/ }).first()).toBeDisabled();

	// a draft: draw more and leave without handing in
	page.on('pageerror', (e) => console.log('DBG PAGEERROR', e.message));
	page.on('console', (m) => m.type() === 'error' && console.log('DBG CONSOLE', m.text()));
	await line(page, 0.8);
	await page.waitForTimeout(5000);
	console.log(
		'DBG',
		await page.evaluate(
			() =>
				new Promise((ok) => {
					const r = indexedDB.open('pencilpilot', 1);
					r.onsuccess = () => {
						const q = r.result.transaction('drawings').objectStore('drawings').getAll();
						q.onsuccess = () =>
							ok(
								JSON.stringify(
									q.result.map((d: { id: string; submitted: boolean; savedAt: number }) => [
										d.id,
										d.submitted,
										d.savedAt
									])
								)
							);
					};
				})
		),
		Date.now()
	);
	await page.getByRole('button', { name: 'Save', exact: true }).click();
	await page.getByRole('link', { name: /Your last 10 drawings/ }).click();
	await expect(page).toHaveURL(/\/history$/);
	await expect(cards).toHaveCount(1); // the same drawing, updated
	await expect(cards.first()).toContainText('draft');

	await cards
		.first()
		.getByRole('button', { name: /^Delete/ })
		.click();
	await cards.first().getByRole('button', { name: 'Delete', exact: true }).click();
	await expect(cards).toHaveCount(0);
	await expect(page.getByText('Nothing here yet')).toBeVisible();
});
