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
	await expect(page.getByText('Time · strokes')).toBeVisible();
	await expect(page.getByText(/Free drawings aren/)).toBeVisible();
});

test('brush search finds brushes across categories', async ({ page }) => {
	await page.goto('/draw');
	await page.getByRole('searchbox').fill('watercolor');
	await expect(page.getByRole('list', { name: 'Brushes' }).getByRole('listitem')).not.toHaveCount(
		0
	);
	await expect(page.getByRole('button', { name: /^Thin watercolor/ })).toBeVisible();
});

test('layers: add, hide, lock, rename, merge, undo', async ({ page }) => {
	await page.goto('/draw');
	await expect(page.getByLabel(/Drawing canvas/)).toHaveAttribute('data-ready', 'true', {
		timeout: 15_000
	});
	const layers = page.getByRole('list', { name: 'Layers, top to bottom' });
	await expect(layers.getByRole('listitem')).toHaveCount(2); // Layer 1 + paper

	await scribble(page, 0.3);
	const one = await settledInk(page);
	await page.getByRole('button', { name: 'New layer' }).click();
	await expect(layers.getByRole('listitem')).toHaveCount(3);
	await scribble(page, 0.7);
	const two = await settledInk(page);
	expect(two).toBeGreaterThan(one);

	// hiding the new layer hides its ink; undo shows it again
	await page.getByRole('button', { name: 'Hide Layer 2' }).click();
	await expect.poll(() => inkPixels(page)).toBe(one);
	await page.getByRole('button', { name: /Undo/ }).click();
	await expect.poll(() => inkPixels(page)).toBe(two);

	// a locked layer refuses strokes and says why
	await page.getByRole('button', { name: 'Lock', exact: true }).click();
	await scribble(page, 0.5);
	await expect(page.getByRole('status').filter({ hasText: 'is locked' })).toBeVisible();
	expect(await inkPixels(page)).toBe(two);
	await page.getByRole('button', { name: 'Lock', exact: true }).click();

	// rename
	await page.getByRole('button', { name: 'Rename Layer 2' }).click();
	await page.getByLabel('Layer name').fill('Ink');
	await page.getByLabel('Layer name').press('Enter');
	await expect(layers.getByText('Ink', { exact: true })).toBeVisible();

	// merge down keeps the picture and leaves one layer
	await page.getByRole('button', { name: 'Merge down' }).click();
	await expect(layers.getByRole('listitem')).toHaveCount(2);
	expect(await settledInk(page)).toBe(two);
	await page.getByRole('button', { name: /Undo/ }).click();
	await expect(layers.getByRole('listitem')).toHaveCount(3);
});

/** Bounding box of the ink on the canvas, in canvas pixels. */
async function inkBox(page: Page) {
	return page.getByLabel(/Drawing canvas/).evaluate((c: HTMLCanvasElement) => {
		const d = c.getContext('2d')!.getImageData(0, 0, c.width, c.height).data;
		let [x0, y0, x1, y1] = [Infinity, Infinity, -1, -1];
		for (let y = 0; y < c.height; y++)
			for (let x = 0; x < c.width; x++)
				if (d[(y * c.width + x) * 4] < 200) {
					x0 = Math.min(x0, x);
					x1 = Math.max(x1, x);
					y0 = Math.min(y0, y);
					y1 = Math.max(y1, y);
				}
		return { x0, y0, x1, y1, w: x1 - x0, h: y1 - y0 };
	});
}

/** A straight horizontal drag across the middle of the canvas area, in screen space. */
async function hline(page: Page) {
	const vp = (await page
		.getByLabel(/Drawing canvas/)
		.locator('..')
		.locator('..')
		.boundingBox())!;
	const [cx, cy] = [vp.x + vp.width / 2, vp.y + vp.height / 2];
	await page.mouse.move(cx - 60, cy);
	await page.mouse.down();
	for (let i = 1; i <= 12; i++) await page.mouse.move(cx - 60 + i * 10, cy);
	await page.mouse.up();
}

test('zoomed and rotated views still put the stroke under the pointer', async ({ page }) => {
	await page.goto('/draw');
	const canvas = page.getByLabel(/Drawing canvas/);
	await expect(canvas).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });

	// zoomed in 2× (approx): a 120-px screen line is ~half as long on the canvas
	await page.getByRole('button', { name: 'Zoom in', exact: true }).click();
	await page.getByRole('button', { name: 'Zoom in', exact: true }).click();
	await page.getByRole('button', { name: 'Zoom in', exact: true }).click();
	await expect(page.getByRole('button', { name: /^Zoom 195%/ })).toBeVisible();
	await hline(page);
	await settledInk(page);
	const zoomed = await inkBox(page);
	// the canvas centre stays under the viewport centre
	expect(Math.abs((zoomed.x0 + zoomed.x1) / 2 - 400)).toBeLessThan(12);
	expect(Math.abs((zoomed.y0 + zoomed.y1) / 2 - 300)).toBeLessThan(12);
	await page.getByRole('button', { name: /Undo/ }).click();

	// rotated 90°: a horizontal screen stroke is vertical on the canvas
	await page.getByRole('button', { name: /^Zoom .*reset view/ }).click();
	for (let i = 0; i < 6; i++) await page.getByRole('button', { name: 'Rotate right' }).click();
	await hline(page);
	await settledInk(page);
	const turned = await inkBox(page);
	expect(turned.h).toBeGreaterThan(turned.w * 3);

	// mirroring is only a view: the drawing itself isn't flipped
	await page.keyboard.press('0');
	await page.getByRole('button', { name: 'Mirror view' }).click();
	await expect(page.getByRole('button', { name: 'Mirror view' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	expect(await inkBox(page)).toEqual(turned);
});

test('canvas size: pick a shape, and confirm before clearing a drawing', async ({ page }) => {
	await page.goto('/draw');
	const canvas = page.getByLabel(/Drawing canvas/);
	await expect(canvas).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await page.getByRole('button', { name: '800 × 600' }).click();
	await page.getByRole('radio', { name: /Portrait 3:4/ }).check();
	await page.getByRole('button', { name: 'Use this size' }).click();
	await expect(page.getByLabel('Drawing canvas, 600 by 800 pixels')).toHaveAttribute(
		'data-ready',
		'true'
	);

	await scribble(page);
	await settledInk(page);
	await page.getByRole('button', { name: '600 × 800' }).click();
	await page.getByRole('radio', { name: /Custom/ }).check();
	await page.getByLabel('Width').fill('5000');
	await expect(page.getByRole('alert').filter({ hasText: 'At most 2048' })).toBeVisible();
	await page.getByLabel('Width').fill('1000');
	await page.getByLabel('Height').fill('1000');
	await page.getByRole('button', { name: 'Use this size' }).click();
	await expect(page.getByText('Your drawing will be cleared')).toBeVisible();
	await page.getByRole('button', { name: 'Clear and resize' }).click();
	await expect(page.getByLabel('Drawing canvas, 1000 by 1000 pixels')).toHaveAttribute(
		'data-ready',
		'true'
	);
	expect(await inkPixels(page)).toBe(0);

	// remembered next time
	await page.reload();
	await expect(page.getByLabel('Drawing canvas, 1000 by 1000 pixels')).toBeVisible();
	await scribble(page);
	await page.getByRole('button', { name: /Submit drawing/ }).click();
	await expect(page).toHaveURL(/\/results$/);
	await expect(page.getByRole('img', { name: 'Your submitted drawing' })).toBeVisible();
});

test('tracing your own picture: line it up, draw, get tips but no score', async ({ page }) => {
	await page.goto('/draw');
	await expect(page.getByLabel(/Drawing canvas/)).toHaveAttribute('data-ready', 'true', {
		timeout: 15_000
	});
	await page.getByRole('button', { name: 'Trace' }).click();
	const tracing = page.getByRole('region', { name: 'Tracing' });
	await expect(tracing).toContainText('no score');

	// a picture from "the device": a black ring on white, made in the page
	const png = await page.evaluate(() => {
		const c = document.createElement('canvas');
		c.width = 400;
		c.height = 300;
		const x = c.getContext('2d')!;
		x.fillStyle = '#fff';
		x.fillRect(0, 0, 400, 300);
		x.lineWidth = 12;
		x.beginPath();
		x.arc(200, 150, 90, 0, Math.PI * 2);
		x.stroke();
		return c.toDataURL('image/png').split(',')[1];
	});
	await tracing.getByLabel('Picture from your device').setInputFiles({
		name: 'ring.png',
		mimeType: 'image/png',
		buffer: Buffer.from(png, 'base64')
	});
	const overlay = page.locator('img.overlay');
	await expect(overlay).toBeVisible();

	// move it: in move mode a drag shifts the picture, and paints nothing
	await tracing.getByRole('button', { name: 'Move picture' }).click();
	const box = (await page.getByLabel(/Drawing canvas/).boundingBox())!;
	await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
	await page.mouse.down();
	await page.mouse.move(box.x + box.width / 2 + 40, box.y + box.height / 2, { steps: 5 });
	await page.mouse.up();
	await expect(overlay).toHaveAttribute('style', /translate\([1-9]/);
	expect(await inkPixels(page)).toBe(0);
	await tracing.getByRole('button', { name: 'Done moving' }).click();

	await scribble(page);
	await expect(page.getByRole('button', { name: 'Get tips →' })).toBeEnabled();
	await page.getByRole('button', { name: 'Get tips →' }).click();
	await expect(page).toHaveURL(/\/results$/);
	await expect(page.getByText('Traced', { exact: true })).toBeVisible();
	await expect(page.getByTestId('score')).toHaveCount(0);
	await expect(page.getByTestId('overlay')).toBeVisible({ timeout: 15_000 });
	// not counted
	const history = await page.evaluate(() => localStorage.getItem('pp.attempts'));
	expect(history ?? '[]').toBe('[]');
});
