import { expect, test, type Page } from '@playwright/test';

async function line(page: Page, y: number) {
	const canvas = page.getByLabel(/Drawing canvas/);
	const box = (await canvas.boundingBox())!;
	await page.mouse.move(box.x + box.width * 0.2, box.y + box.height * y);
	await page.mouse.down();
	for (let i = 1; i <= 20; i++)
		await page.mouse.move(box.x + box.width * (0.2 + i * 0.03), box.y + box.height * y);
	await page.mouse.up();
}

test('sketch mode: construction layer in blue, left out of the graded drawing', async ({
	page
}) => {
	await page.goto('/draw?mode=sketch');
	await expect(page.getByLabel(/Drawing canvas/)).toHaveAttribute('data-ready', 'true', {
		timeout: 15_000
	});
	await expect(page.getByText('Sketch mode:')).toBeVisible();
	const layers = page.getByRole('list', { name: 'Layers, top to bottom' });
	await expect(layers.getByRole('listitem').nth(0)).toContainText('Sketch');
	await expect(layers.getByRole('listitem').nth(1)).toContainText('Construction');
	// starts on the construction layer, in blue, and the setup can't be undone
	await expect(
		layers.getByRole('button', { name: /Construction/, exact: false }).first()
	).toBeVisible();
	await expect(page.getByRole('radio', { name: 'Colour #3d6bff' })).toHaveAttribute(
		'aria-checked',
		'true'
	);
	await expect(page.getByRole('button', { name: /Undo/ })).toBeDisabled();

	await line(page, 0.3); // a guide line
	await layers.getByText('Sketch', { exact: true }).click();
	await expect(page.getByRole('radio', { name: 'Colour #16161a' })).toHaveAttribute(
		'aria-checked',
		'true'
	);
	await line(page, 0.6); // the sketch itself

	await page.getByRole('button', { name: /Submit drawing/ }).click();
	await expect(page).toHaveURL(/\/results$/);
	const img = page.getByRole('img', { name: 'Your submitted drawing' });
	await expect(img).toBeVisible();
	const { blue, dark } = await img.evaluate(async (el: HTMLImageElement) => {
		await el.decode();
		const c = document.createElement('canvas');
		[c.width, c.height] = [el.naturalWidth, el.naturalHeight];
		const x = c.getContext('2d')!;
		x.drawImage(el, 0, 0);
		const d = x.getImageData(0, 0, c.width, c.height).data;
		let [blue, dark] = [0, 0];
		for (let i = 0; i < d.length; i += 4) {
			if (d[i + 2] > d[i] + 40) blue++;
			else if (d[i] < 225) dark++; // pencil grey
		}
		return { blue, dark };
	});
	expect(dark).toBeGreaterThan(50);
	expect(blue).toBe(0);
});

test('the practice page offers sketch mode', async ({ page }) => {
	await page.goto('/practice/animals');
	await page.getByRole('radio', { name: 'Sketch' }).click();
	await expect(page.getByText(/Rough and loose/)).toContainText('proportions');
	await expect(page.getByRole('link', { name: /Surprise me/ })).toHaveAttribute(
		'href',
		/&mode=sketch$/
	);
});
