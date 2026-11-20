import { expect, test, type Locator, type Page } from '@playwright/test';

// The second batch of studio tools: measuring and angles from the reference,
// notan and squint views, the mid-drawing check, copy / cut / paste, resizing the
// brush on the canvas, editable QuickShapes, warp and feathered selections.

const canvas = (page: Page) => page.getByLabel(/Drawing canvas/);

async function ready(page: Page, url = '/draw') {
	// tall enough that the whole canvas is on screen
	await page.setViewportSize({ width: 1440, height: 1100 });
	await page.goto(url);
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await canvas(page).scrollIntoViewIfNeeded();
}

async function at(page: Page, fx: number, fy: number): Promise<[number, number]> {
	const b = (await canvas(page).boundingBox())!;
	return [b.x + b.width * fx, b.y + b.height * fy];
}

async function stroke(page: Page, pts: [number, number][], steps = 4) {
	await page.mouse.move(...(await at(page, ...pts[0])));
	await page.mouse.down();
	for (const p of pts.slice(1)) await page.mouse.move(...(await at(page, ...p)), { steps });
	await page.mouse.up();
}

/** dark pixels in a 9×9 square around a fraction of the canvas */
const inkNear = (page: Page, fx: number, fy: number, below = 200) =>
	canvas(page).evaluate(
		(el: HTMLCanvasElement, [fx, fy, below]) => {
			const d = el
				.getContext('2d')!
				.getImageData(Math.round(el.width * fx) - 4, Math.round(el.height * fy) - 4, 9, 9).data;
			let n = 0;
			for (let i = 0; i < d.length; i += 4) if (d[i] < below) n++;
			return n;
		},
		[fx, fy, below]
	);

const red = (page: Page, fx: number, fy: number) =>
	canvas(page).evaluate(
		(el: HTMLCanvasElement, [fx, fy]) =>
			el.getContext('2d')!.getImageData(Math.round(el.width * fx), Math.round(el.height * fy), 1, 1)
				.data[0],
		[fx, fy]
	);

async function withReference(page: Page) {
	await page.route('**/api/ref?*', (r) =>
		r.fulfill({
			json: {
				id: 'px-717171',
				source: 'pixabay',
				sourceId: 717171,
				category: 'animals',
				pageUrl: 'https://pixabay.com/photos/box-717171/',
				author: 'Ann',
				authorUrl: '',
				license: 'Pixabay Content License',
				licenseUrl: 'https://pixabay.com/service/license-summary/',
				tags: ['box'],
				width: 400,
				height: 300,
				title: 'Box',
				keypoints: [],
				topic: 'pets',
				tier: 2,
				imageUrl: '/e2e-box2.svg',
				live: true
			}
		})
	);
	await page.route('**/e2e-box2.svg', (r) =>
		r.fulfill({
			contentType: 'image/svg+xml',
			body: '<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300"><rect width="400" height="300" fill="#fff"/><rect x="100" y="75" width="200" height="150" fill="#333"/></svg>'
		})
	);
	await ready(page, '/draw?category=animals&tier=2');
	const pane = page.getByRole('region', { name: 'Reference beside the canvas' });
	await expect(pane.getByRole('img', { name: /Reference: box/ })).toBeVisible();
	return pane;
}

/** a screen point at fractions of the picture shown in the reference pane (fitted inside its box) */
async function onPicture(pane: Locator, fx: number, fy: number): Promise<[number, number]> {
	const b = (await pane.getByTestId('ref-box').boundingBox())!;
	const k = Math.min(b.width / 400, b.height / 300);
	const [w, h] = [400 * k, 300 * k];
	return [b.x + (b.width - w) / 2 + w * fx, b.y + (b.height - h) / 2 + h * fy];
}

async function dragOn(page: Page, from: [number, number], to: [number, number]) {
	await page.mouse.move(...from);
	await page.mouse.down();
	await page.mouse.move(...to, { steps: 6 });
	await page.mouse.up();
}

test('1. mark a unit on the reference, then measure the drawing in that unit', async ({ page }) => {
	const pane = await withReference(page);
	await pane.getByRole('button', { name: 'Mark a unit on the reference' }).click();
	// half the picture's height: 150 picture px → 300 canvas px (the picture is fitted at 2×)
	await dragOn(page, await onPicture(pane, 0.5, 0.25), await onPicture(pane, 0.5, 0.75));
	await expect(pane.getByText(/Unit marked/)).toBeVisible();
	await expect(page.getByRole('button', { name: 'Measure (U)' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	// 0.75 of the canvas height = 450 px = 1.5 units
	await dragOn(page, await at(page, 0.2, 0.1), await at(page, 0.2, 0.85));
	await expect(page.getByRole('status').filter({ hasText: /units/ })).toHaveText(
		/1\.5 units · 90°/
	);
	expect(await page.getByTestId('measure').locator('line').count()).toBe(3); // the line + 2 ticks
});

test('2. find an angle on the reference; the canvas shows it as a guide', async ({ page }) => {
	const pane = await withReference(page);
	await pane.getByRole('button', { name: 'Find an angle on the reference' }).click();
	// 200 across, 150 up in picture px: atan(150 / 200) ≈ 37°
	await dragOn(page, await onPicture(pane, 0.25, 0.75), await onPicture(pane, 0.75, 0.25));
	await expect(pane.getByText('Angle 37°: shown on the canvas')).toBeVisible();
	await expect(page.getByTestId('angle-guide')).toBeVisible();
	await pane.getByRole('button', { name: 'Clear marks' }).click();
	await expect(page.getByTestId('angle-guide')).toHaveCount(0);
});

test('3 and 4. notan and squint views change the canvas and the reference', async ({ page }) => {
	const pane = await withReference(page);
	const img = pane.getByRole('img', { name: /Reference: box/ });
	const filter = (l: Locator) => l.evaluate((e) => getComputedStyle(e).filter);
	await page.getByRole('button', { name: 'Notan view, off' }).click();
	expect(await filter(canvas(page))).toContain('url(');
	expect(await filter(img)).toContain('url(');
	await page.getByRole('button', { name: 'Notan view, 2 tones' }).click();
	await expect(page.getByRole('button', { name: 'Notan view, 3 tones' })).toBeVisible();
	await page.getByRole('button', { name: 'Notan view, 3 tones' }).click();
	expect(await filter(canvas(page))).toBe('none');
	await page.getByRole('button', { name: 'Squint view' }).click();
	expect(await filter(canvas(page))).toContain('blur');
	expect(await filter(img)).toContain('blur');
});

test('5. "How am I doing?" shows the biggest problem without handing in', async ({ page }) => {
	await withReference(page);
	await stroke(page, [
		[0.3, 0.3],
		[0.7, 0.3]
	]);
	await page.getByRole('button', { name: 'How am I doing?' }).click();
	const pop = page.getByRole('status', { name: 'Quick check' });
	await expect(pop).toBeVisible({ timeout: 15_000 });
	await expect(pop.locator('b')).not.toBeEmpty();
	await expect(page).toHaveURL(/\/draw/); // still drawing
	await pop.getByRole('button', { name: 'Got it' }).click();
	await expect(pop).toHaveCount(0);
});

test('6. copy, paste as a new layer and move it; cut removes the original', async ({ page }) => {
	await ready(page);
	// a solid block (a thin line's box is all handles)
	await page.keyboard.press('f');
	await stroke(page, [
		[0.2, 0.25],
		[0.35, 0.25],
		[0.35, 0.35],
		[0.2, 0.35],
		[0.2, 0.26]
	]);
	await page.keyboard.press('m');
	await dragOn(page, await at(page, 0.15, 0.2), await at(page, 0.4, 0.4));
	await page.keyboard.press('Control+c');
	const sel = page.getByRole('group', { name: 'Selection' });
	await sel.getByRole('button', { name: 'Paste' }).click();
	await expect(page.getByRole('button', { name: /Transform/ }).first()).toBeVisible();
	await page.getByRole('tab', { name: /^Layers/ }).click();
	await expect(page.getByRole('button', { name: 'Pasted', exact: true })).toBeVisible();
	// move the pasted copy right, keep it
	await dragOn(page, await at(page, 0.27, 0.3), await at(page, 0.67, 0.3));
	await page.getByRole('button', { name: 'Done' }).click();
	await expect.poll(() => inkNear(page, 0.67, 0.3)).toBeGreaterThan(0);
	expect(await inkNear(page, 0.27, 0.3)).toBeGreaterThan(0); // the original is still there
	await page.getByRole('tab', { name: /History/ }).click();
	await expect(
		page.getByRole('region', { name: 'History' }).getByRole('button', { name: 'Paste' })
	).toBeVisible();

	// cut: select the copy and Ctrl+X it away
	await page.keyboard.press('m');
	await dragOn(page, await at(page, 0.55, 0.15), await at(page, 0.85, 0.45));
	await page.keyboard.press('Control+x');
	await expect.poll(() => inkNear(page, 0.67, 0.3)).toBe(0);
});

test('7. Ctrl+Alt+drag resizes the brush on the canvas', async ({ page }) => {
	await ready(page);
	await page.mouse.move(...(await at(page, 0.5, 0.5)));
	const cur = page.getByTestId('brush-cursor');
	const r0 = Number(await cur.getAttribute('data-r'));
	await page.keyboard.down('Control');
	await page.keyboard.down('Alt');
	const [x, y] = await at(page, 0.5, 0.5);
	await page.mouse.down();
	await page.mouse.move(x + 120, y, { steps: 6 });
	await page.mouse.up();
	await page.keyboard.up('Alt');
	await page.keyboard.up('Control');
	await expect.poll(async () => Number(await cur.getAttribute('data-r'))).toBeGreaterThan(r0 * 2);
	expect(await inkNear(page, 0.55, 0.5)).toBe(0); // it didn't paint
});

test('8. a snapped QuickShape stays editable: drag its end', async ({ page }) => {
	await ready(page);
	await page.mouse.move(...(await at(page, 0.2, 0.5)));
	await page.mouse.down();
	await page.mouse.move(...(await at(page, 0.6, 0.5)), { steps: 10 });
	await page.waitForTimeout(800); // hold: snaps to a line
	await page.mouse.up();
	const handles = page.getByTestId('shape-handles').locator('circle');
	await expect(handles).toHaveCount(2);
	const end = (await handles.nth(1).boundingBox())!;
	await dragOn(page, [end.x + end.width / 2, end.y + end.height / 2], await at(page, 0.6, 0.8));
	// the line now runs down to (0.6, 0.8): halfway is (0.4, 0.65); the old end is gone
	await expect.poll(() => inkNear(page, 0.4, 0.65)).toBeGreaterThan(0);
	expect(await inkNear(page, 0.55, 0.5)).toBe(0);
	await page.getByRole('tab', { name: /History/ }).click();
	const h = page.getByRole('region', { name: 'History' });
	await expect(h.getByRole('button', { name: 'Shape' })).toHaveAttribute('aria-current', 'step');
	await expect(h.getByRole('button', { name: 'Brush stroke' })).toHaveCount(0); // one step, replaced
	// a new stroke ends editing
	await stroke(page, [
		[0.1, 0.1],
		[0.15, 0.1]
	]);
	await expect(handles).toHaveCount(0);
});

test('9. warp: pull one grid point and the drawing bends', async ({ page }) => {
	await ready(page);
	await stroke(page, [
		[0.25, 0.5],
		[0.75, 0.5]
	]);
	await page.keyboard.press('v');
	await page.getByRole('button', { name: 'Warp' }).click();
	const grid = page.getByTestId('warp-grid');
	await expect(grid.locator('circle')).toHaveCount(16);
	// an inner point of the top row of cells (row 1, column 1): pull it well up
	const n = (await grid.locator('circle').nth(5).boundingBox())!;
	await dragOn(
		page,
		[n.x + n.width / 2, n.y + n.height / 2],
		[n.x + n.width / 2, (await at(page, 0.5, 0.05))[1]]
	);
	await page.getByRole('button', { name: 'Done' }).click();
	const above = () =>
		canvas(page).evaluate((el: HTMLCanvasElement) => {
			const d = el
				.getContext('2d')!
				.getImageData(0, Math.round(el.height * 0.25), el.width, Math.round(el.height * 0.2)).data;
			let k = 0;
			for (let i = 0; i < d.length; i += 4) if (d[i] < 235) k++;
			return k;
		});
	await expect.poll(above).toBeGreaterThan(20);
	await page.getByRole('tab', { name: /History/ }).click();
	await expect(
		page.getByRole('region', { name: 'History' }).getByRole('button', { name: 'Warp' })
	).toBeVisible();
});

test('10. a feathered selection fades paint out across its edge', async ({ page }) => {
	await ready(page);
	await page.keyboard.press('m');
	await dragOn(page, await at(page, 0.1, 0.1), await at(page, 0.5, 0.9));
	const sel = page.getByRole('group', { name: 'Selection' });
	await sel.getByRole('button', { name: 'Feather' }).click();
	await page.keyboard.press('f');
	await stroke(page, [
		[0.02, 0.02],
		[0.98, 0.02],
		[0.98, 0.98],
		[0.02, 0.98],
		[0.02, 0.03]
	]);
	await expect.poll(() => red(page, 0.3, 0.5)).toBeLessThan(40); // inside: solid
	expect(await red(page, 0.8, 0.5)).toBeGreaterThan(240); // outside: untouched
	const edge = await red(page, 0.5, 0.5);
	expect(edge).toBeGreaterThan(40); // the edge: in between
	expect(edge).toBeLessThan(240);
});
