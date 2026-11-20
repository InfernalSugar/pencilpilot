import { expect, test, type Locator, type Page } from '@playwright/test';

// Photoshop / Procreate-style tools on the draw page: brush outline, Shift-straight
// strokes, picking colours from the reference, grayscale view, a grid on the
// reference, distort, the history panel, fill from a line-art layer, lasso fill
// and colour harmonies.

const canvas = (page: Page) => page.getByLabel(/Drawing canvas/);

async function ready(page: Page, url = '/draw') {
	await page.goto(url);
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await canvas(page).scrollIntoViewIfNeeded();
}

/** screen point for a fraction of the canvas */
async function at(page: Page, fx: number, fy: number): Promise<[number, number]> {
	const b = (await canvas(page).boundingBox())!;
	return [b.x + b.width * fx, b.y + b.height * fy];
}

async function stroke(page: Page, pts: [number, number][], steps = 4) {
	const first = await at(page, ...pts[0]);
	await page.mouse.move(...first);
	await page.mouse.down();
	for (const p of pts.slice(1)) await page.mouse.move(...(await at(page, ...p)), { steps });
	await page.mouse.up();
}

/** pixel [r, g, b] of the drawing at a fraction of the canvas */
const pixel = (page: Page, fx: number, fy: number) =>
	canvas(page).evaluate(
		(el: HTMLCanvasElement, [fx, fy]) => {
			const d = el
				.getContext('2d')!
				.getImageData(Math.round(el.width * fx), Math.round(el.height * fy), 1, 1).data;
			return [d[0], d[1], d[2]];
		},
		[fx, fy]
	);

/** dark pixels in a 9×9 square around a fraction of the canvas */
const inkNear = (page: Page, fx: number, fy: number) =>
	canvas(page).evaluate(
		(el: HTMLCanvasElement, [fx, fy]) => {
			const d = el
				.getContext('2d')!
				.getImageData(Math.round(el.width * fx) - 4, Math.round(el.height * fy) - 4, 9, 9).data;
			let n = 0;
			for (let i = 0; i < d.length; i += 4) if (d[i] < 200) n++;
			return n;
		},
		[fx, fy]
	);

const totalInk = (page: Page) =>
	canvas(page).evaluate((el: HTMLCanvasElement) => {
		const d = el.getContext('2d')!.getImageData(0, 0, el.width, el.height).data;
		let n = 0;
		for (let i = 0; i < d.length; i += 4) if (d[i] < 200) n++;
		return n;
	});

const BLUE = '#3d6bff';
async function withReference(page: Page) {
	await page.route('**/api/ref?*', (r) =>
		r.fulfill({
			json: {
				id: 'px-616161',
				source: 'pixabay',
				sourceId: 616161,
				category: 'animals',
				pageUrl: 'https://pixabay.com/photos/blue-616161/',
				author: 'Ann',
				authorUrl: '',
				license: 'Pixabay Content License',
				licenseUrl: 'https://pixabay.com/service/license-summary/',
				tags: ['blue'],
				width: 400,
				height: 300,
				title: 'Blue',
				keypoints: [],
				topic: 'pets',
				tier: 2,
				imageUrl: '/e2e-blue.svg',
				live: true
			}
		})
	);
	await page.route('**/e2e-blue.svg', (r) =>
		r.fulfill({
			contentType: 'image/svg+xml',
			body: `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300"><rect width="400" height="300" fill="${BLUE}"/></svg>`
		})
	);
	await page.setViewportSize({ width: 1440, height: 900 });
	await ready(page, '/draw?category=animals&tier=2');
	const pane = page.getByRole('region', { name: 'Reference beside the canvas' });
	await expect(pane.getByRole('img', { name: /Reference: blue/ })).toBeVisible();
	return pane;
}

test('1. the brush shows its real size as an outline, and follows size changes', async ({
	page
}) => {
	await ready(page);
	await page.mouse.move(...(await at(page, 0.5, 0.5)));
	const cur = page.getByTestId('brush-cursor');
	await expect(cur).toBeVisible();
	const r0 = Number(await cur.getAttribute('data-r'));
	expect(r0).toBeGreaterThan(0);
	await page.keyboard.press(']');
	await page.mouse.move(...(await at(page, 0.52, 0.5)));
	await expect.poll(async () => Number(await cur.getAttribute('data-r'))).toBeGreaterThan(r0);
	// leaving the canvas hides it
	await page.mouse.move(2, 2);
	await expect(cur).toHaveCount(0);
});

test('2. Shift + drag keeps a wobbly stroke dead straight', async ({ page }) => {
	await ready(page);
	await page.keyboard.down('Shift');
	await stroke(page, [
		[0.2, 0.5],
		[0.3, 0.55],
		[0.4, 0.45],
		[0.5, 0.56],
		[0.6, 0.47]
	]);
	await page.keyboard.up('Shift');
	await expect.poll(() => inkNear(page, 0.4, 0.5)).toBeGreaterThan(0);
	// none of the wobble made it onto the canvas
	expect(await inkNear(page, 0.3, 0.56)).toBe(0);
	expect(await inkNear(page, 0.4, 0.44)).toBe(0);
});

test('3. pick a colour from the reference (button, or Alt+click) and paint with it', async ({
	page
}) => {
	const pane = await withReference(page);
	await pane.getByRole('button', { name: 'Pick a colour from the reference' }).click();
	const box = (await pane.getByTestId('ref-box').boundingBox())!;
	await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
	await stroke(page, [
		[0.3, 0.5],
		[0.6, 0.5]
	]);
	await expect
		.poll(async () => {
			const [r, , b] = await pixel(page, 0.45, 0.5);
			return b - r;
		})
		.toBeGreaterThan(30); // blue (the pencil is see-through), not the default near-black

	// Alt+click works without the button too
	await pane.getByRole('button', { name: 'Pick a colour from the reference' }).click();
	await page
		.getByRole('radio', { name: /^Colour #/ })
		.first()
		.click();
	await page.keyboard.down('Alt');
	await page.mouse.click(box.x + box.width / 2, box.y + box.height / 3);
	await page.keyboard.up('Alt');
	await stroke(page, [
		[0.3, 0.7],
		[0.6, 0.7]
	]);
	await expect
		.poll(async () => {
			const [r, , b] = await pixel(page, 0.45, 0.7);
			return b - r;
		})
		.toBeGreaterThan(30);
});

test('4. grayscale view: the canvas and the reference lose their colour (the drawing keeps it)', async ({
	page
}) => {
	const pane = await withReference(page);
	const filter = (l: Locator) => l.evaluate((e) => getComputedStyle(e).filter);
	await page.getByRole('button', { name: 'Grayscale view' }).click();
	await expect(page.getByRole('button', { name: 'Grayscale view' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	expect(await filter(canvas(page))).toContain('grayscale');
	expect(await filter(pane.getByRole('img', { name: /Reference: blue/ }))).toContain('grayscale');
	await page.getByRole('button', { name: 'Grayscale view' }).click();
	expect(await filter(canvas(page))).toBe('none');
});

test('5. the canvas grid shows on the reference too (the grid method)', async ({ page }) => {
	const pane = await withReference(page);
	await expect(pane.getByTestId('ref-grid')).toHaveCount(0);
	await page.getByRole('button', { name: 'Guides', exact: true }).click();
	await page.getByRole('group', { name: 'Guides' }).getByRole('radio', { name: 'Square' }).click();
	const grid = pane.getByTestId('ref-grid');
	await expect(grid).toBeVisible();
	expect(await grid.locator('line').count()).toBeGreaterThan(4);
	await pane.getByRole('button', { name: 'Grid on the reference' }).click();
	await expect(grid).toHaveCount(0);
});

test('6. distort: drag one corner on its own, then keep it', async ({ page }) => {
	await ready(page);
	await stroke(page, [
		[0.3, 0.5],
		[0.7, 0.5]
	]);
	await expect.poll(() => totalInk(page)).toBeGreaterThan(50);
	await page.keyboard.press('v');
	await page.getByRole('button', { name: 'Distort' }).click();
	// drag the top-right corner handle (handles: tl, tr, br, bl, then the edges) up
	const handle = (await page.locator('svg.xf rect').nth(1).boundingBox())!;
	await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
	await page.mouse.down();
	await page.mouse.move(handle.x + handle.width / 2, (await at(page, 0.5, 0.2))[1], {
		steps: 6
	});
	await page.mouse.up();
	await page.getByRole('button', { name: 'Done' }).click();
	// the right end now slants up towards the moved corner; a plain resize would keep it flat
	const above = () =>
		canvas(page).evaluate((el: HTMLCanvasElement) => {
			const [x0, y0] = [Math.round(el.width * 0.5), Math.round(el.height * 0.25)];
			const d = el
				.getContext('2d')!
				.getImageData(x0, y0, Math.round(el.width * 0.2), Math.round(el.height * 0.17)).data;
			let n = 0;
			for (let i = 0; i < d.length; i += 4) if (d[i] < 235) n++;
			return n;
		});
	await expect.poll(above).toBeGreaterThan(20);
	expect(await inkNear(page, 0.32, 0.5)).toBeGreaterThan(0);
	await page.getByRole('tab', { name: /History/ }).click();
	await expect(
		page.getByRole('region', { name: 'History' }).getByRole('button', { name: 'Distort' })
	).toHaveAttribute('aria-current', 'step');
});

test('7. history panel: jump back to the start and forward again', async ({ page }) => {
	await ready(page);
	for (const y of [0.3, 0.5, 0.7])
		await stroke(page, [
			[0.3, y],
			[0.6, y]
		]);
	await page.getByRole('tab', { name: /History/ }).click();
	const h = page.getByRole('region', { name: 'History' });
	await expect(h.getByRole('button', { name: 'Brush stroke' })).toHaveCount(3);
	await h.getByRole('button', { name: 'Start' }).click();
	await expect.poll(() => totalInk(page)).toBe(0);
	await expect(h.getByRole('button', { name: 'Start' })).toHaveAttribute('aria-current', 'step');
	// the top step is the newest: back to all three strokes
	await h.getByRole('button', { name: 'Brush stroke' }).first().click();
	await expect.poll(() => inkNear(page, 0.45, 0.7)).toBeGreaterThan(0);
	expect(await inkNear(page, 0.45, 0.3)).toBeGreaterThan(0);
	// the middle one: only the first two
	await h.getByRole('button', { name: 'Brush stroke' }).nth(1).click();
	await expect.poll(() => inkNear(page, 0.45, 0.7)).toBe(0);
	expect(await inkNear(page, 0.45, 0.5)).toBeGreaterThan(0);
});

test('8. fill from a line-art layer while colouring on another layer', async ({ page }) => {
	await ready(page);
	// a closed square drawn with Shift+click straight lines
	await stroke(page, [
		[0.3, 0.3],
		[0.305, 0.3]
	]);
	await page.keyboard.down('Shift');
	for (const p of [
		[0.6, 0.3],
		[0.6, 0.7],
		[0.3, 0.7],
		[0.3, 0.3]
	] as [number, number][])
		await page.mouse.click(...(await at(page, ...p)));
	await page.keyboard.up('Shift');
	await page.getByRole('tab', { name: /^Layers/ }).click();
	await page.getByRole('button', { name: 'New layer' }).click();
	await page
		.getByRole('radio', { name: /^Colour #/ })
		.nth(3)
		.click();
	await page.keyboard.press('g');
	const opts = page.getByRole('group', { name: 'Fill options' });
	await opts.getByRole('button', { name: 'Sample: all layers' }).click(); // the new layer alone…
	await opts.getByLabel('Lines from').selectOption({ label: 'Reference: Layer 1' }); // …but the lines from Layer 1
	await page.mouse.click(...(await at(page, 0.45, 0.5)));
	await expect
		.poll(async () => {
			const [r, g, b] = await pixel(page, 0.45, 0.5);
			return Math.max(r, g, b) - Math.min(r, g, b);
		})
		.toBeGreaterThan(60); // coloured inside
	const [r, g, b] = await pixel(page, 0.85, 0.85);
	expect(Math.min(r, g, b)).toBeGreaterThan(240); // still white outside the lines
});

test('9. lasso fill: draw a shape and it fills solid', async ({ page }) => {
	await ready(page);
	await page.keyboard.press('f');
	await expect(page.getByRole('button', { name: 'Lasso fill (F)' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await stroke(page, [
		[0.2, 0.2],
		[0.5, 0.2],
		[0.5, 0.6],
		[0.2, 0.6],
		[0.2, 0.21]
	]);
	await expect.poll(() => inkNear(page, 0.35, 0.4)).toBe(81); // solid inside
	expect(await inkNear(page, 0.75, 0.8)).toBe(0);
	await page.getByRole('tab', { name: /History/ }).click();
	await expect(
		page.getByRole('region', { name: 'History' }).getByRole('button', { name: 'Lasso fill' })
	).toBeVisible();
});

test('10. colour harmonies: pick a colour that goes with the current one', async ({ page }) => {
	await ready(page);
	await page
		.getByRole('radio', { name: /^Colour #/ })
		.nth(3)
		.click();
	await page.getByRole('button', { name: 'More colours' }).click();
	const panel = page.getByRole('dialog', { name: 'Colours' });
	const hex = panel.getByLabel('Hex colour');
	const before = await hex.inputValue();
	const swatches = panel.getByTestId('harmony').getByRole('button');
	await expect(swatches).toHaveCount(2); // the colour + its complement
	await panel.getByLabel('Colour harmony').selectOption('triadic');
	await expect(swatches).toHaveCount(3);
	const pick = (await swatches.nth(1).getAttribute('aria-label'))!.replace('Use ', '');
	await swatches.nth(1).click();
	await expect(hex).toHaveValue(pick);
	expect(pick).not.toBe(before);
});
