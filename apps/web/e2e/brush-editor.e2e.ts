import { expect, test, type Page } from '@playwright/test';

const canvas = (page: Page) => page.getByLabel(/Drawing canvas/);

const inked = (page: Page) =>
	canvas(page).evaluate((c: HTMLCanvasElement) => {
		const d = c.getContext('2d')!.getImageData(0, 0, c.width, c.height).data;
		let n = 0;
		for (let i = 0; i < d.length; i += 4) if (d[i] < 120) n++;
		return n;
	});

test('brush editor: adjust a brush, save it to My brushes, draw with it, keep it after reload, delete it', async ({
	page
}) => {
	await page.goto('/draw');
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await page.getByRole('tab', { name: /^Brushes/ }).click();
	await page.getByRole('button', { name: /^Adjust .* and save as mine/ }).click();

	const editor = page.getByRole('dialog', { name: /your own/ });
	const sample = editor.getByRole('figure', { name: /Sample stroke/ }).locator('canvas');
	await expect
		.poll(() =>
			sample.evaluate((c: HTMLCanvasElement) => {
				const d = c.getContext('2d')!.getImageData(0, 0, c.width, c.height).data;
				let n = 0;
				for (let i = 0; i < d.length; i += 4) if (d[i] < 200) n++;
				return n;
			})
		)
		.toBeGreaterThan(200);
	await editor.getByRole('slider', { name: 'Edge' }).fill('1');
	await editor.getByRole('tab', { name: 'Pressure and speed' }).click();
	await editor.getByRole('slider', { name: 'Press harder → thicker' }).fill('0');
	await editor.getByRole('textbox', { name: 'Brush name' }).fill('Crisp pencil');
	await editor.getByRole('button', { name: 'Save to My brushes' }).click();

	await expect(page.getByRole('tab', { name: 'My brushes (1)' })).toHaveAttribute(
		'aria-selected',
		'true'
	);
	const mine = page.getByRole('list', { name: 'My brushes' });
	await expect(mine.getByRole('button', { name: 'Crisp pencil', exact: true })).toHaveAttribute(
		'aria-pressed',
		'true'
	);

	// draw with it
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.move(b.x + b.width * 0.2, b.y + b.height * 0.5);
	await page.mouse.down();
	for (let i = 1; i <= 20; i++)
		await page.mouse.move(b.x + b.width * (0.2 + i * 0.03), b.y + b.height * 0.5);
	await page.mouse.up();
	await expect.poll(() => inked(page)).toBeGreaterThan(50);

	// pen feel is remembered
	await page.getByRole('radio', { name: 'Heavy hand' }).click();

	// still there after a reload
	await page.reload();
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await page.getByRole('tab', { name: /^Brushes/ }).click();
	await page.getByRole('tab', { name: 'My brushes (1)' }).click();
	await mine.getByRole('button', { name: 'Crisp pencil', exact: true }).click();
	await expect(page.locator('.brush-name')).toHaveText('Crisp pencil');
	await expect(page.getByRole('radio', { name: 'Heavy hand' })).toHaveAttribute(
		'aria-checked',
		'true'
	);

	// adjust again: the saved settings come back; then delete
	await page.getByRole('button', { name: 'Edit “Crisp pencil”' }).click();
	const edit = page.getByRole('dialog', { name: 'Edit “Crisp pencil”' });
	await expect(edit.getByRole('slider', { name: 'Edge' })).toHaveValue('1');
	await edit.getByRole('button', { name: 'Delete brush' }).click();
	await edit.getByRole('button', { name: 'Delete', exact: true }).click();
	await expect(page.getByRole('tab', { name: 'My brushes', exact: true })).toBeVisible();
});
