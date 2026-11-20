import { expect, test, type Page } from '@playwright/test';

// The third batch of studio tools: timelapse, smudge, curves / levels / gradient
// map, adjustment layers, canvas resize, colour-range selection, the quick menu,
// the pen tool, brush textures and import, ColorDrop's threshold, and placing an
// imported picture.

const canvas = (page: Page) => page.getByLabel(/Drawing canvas/);

async function ready(page: Page) {
	await page.setViewportSize({ width: 1440, height: 1100 });
	await page.goto('/draw');
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await canvas(page).scrollIntoViewIfNeeded();
}

async function at(page: Page, fx: number, fy: number): Promise<[number, number]> {
	const b = (await canvas(page).boundingBox())!;
	return [b.x + b.width * fx, b.y + b.height * fy];
}

async function stroke(page: Page, pts: [number, number][], steps = 6) {
	await page.mouse.move(...(await at(page, ...pts[0])));
	await page.mouse.down();
	for (const p of pts.slice(1)) await page.mouse.move(...(await at(page, ...p)), { steps });
	await page.mouse.up();
}

/** the canvas pixel at a fraction of it: [r, g, b] */
const pixel = (page: Page, fx: number, fy: number) =>
	canvas(page).evaluate(
		(el: HTMLCanvasElement, [fx, fy]) =>
			[
				...el
					.getContext('2d')!
					.getImageData(Math.round(el.width * fx), Math.round(el.height * fy), 1, 1)
					.data.slice(0, 3)
			] as [number, number, number],
		[fx, fy]
	);

/** pixels darker than `below` in a box (fractions of the canvas) */
const ink = (page: Page, x0: number, y0: number, x1: number, y1: number, below = 200) =>
	canvas(page).evaluate(
		(el: HTMLCanvasElement, [x0, y0, x1, y1, below]) => {
			const [x, y] = [Math.round(el.width * x0), Math.round(el.height * y0)];
			const [w, h] = [Math.round(el.width * (x1 - x0)), Math.round(el.height * (y1 - y0))];
			const d = el.getContext('2d')!.getImageData(x, y, w, h).data;
			let n = 0;
			for (let i = 0; i < d.length; i += 4) if (d[i] < below || d[i + 2] < below) n++;
			return n;
		},
		[x0, y0, x1, y1, below]
	);

/** a solid block of the colour (lasso fill), x0–x1 × y0–y1 */
async function block(page: Page, x0: number, y0: number, x1: number, y1: number) {
	await page.keyboard.press('f');
	await stroke(page, [
		[x0, y0],
		[x1, y0],
		[x1, y1],
		[x0, y1],
		[x0, y0]
	]);
	await page.keyboard.press('f');
}

async function thick(page: Page, times = 6) {
	for (let i = 0; i < times; i++) await page.keyboard.press(']');
}

test('timelapse: every change is a frame, played back and saved as a video', async ({ page }) => {
	await ready(page);
	await stroke(page, [
		[0.2, 0.3],
		[0.8, 0.3]
	]);
	await stroke(page, [
		[0.2, 0.6],
		[0.8, 0.6]
	]);
	await page.getByRole('button', { name: 'Save', exact: true }).click();
	await page.getByRole('button', { name: /Timelapse/ }).click();
	const player = page.getByRole('dialog', { name: 'Timelapse' });
	await expect(player.getByTestId('timelapse-canvas')).toBeVisible();
	await expect(player.getByText(/\d+ \/ 2$/)).toBeVisible();
	await expect(player.getByRole('button', { name: 'Save video' })).toBeEnabled();
	await player.getByRole('button', { name: 'Close' }).click();
	await expect(player).toBeHidden();
});

test('smudge pushes the paint into the paper around it', async ({ page }) => {
	await ready(page);
	await page.getByRole('radio', { name: 'Colour #e8492f' }).click();
	await block(page, 0.3, 0.3, 0.7, 0.45);
	const before = await pixel(page, 0.5, 0.5);
	expect(before[1]).toBeGreaterThan(245);
	await thick(page);
	await page.keyboard.press('s');
	await expect(page.getByRole('button', { name: 'Smudge (S)' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	for (const x of [0.46, 0.5, 0.54])
		await stroke(
			page,
			[
				[x, 0.4],
				[x, 0.6]
			],
			12
		);
	// red dragged down into the paper: less green there than before
	await expect.poll(async () => (await pixel(page, 0.5, 0.5))[1]).toBeLessThan(220);
});

test('curves, levels and gradient map change the layer', async ({ page }) => {
	await ready(page);
	await thick(page, 4);
	await stroke(page, [
		[0.2, 0.5],
		[0.8, 0.5]
	]);
	const dark = await ink(page, 0.3, 0.45, 0.7, 0.55);
	expect(dark).toBeGreaterThan(100);
	await page
		.getByRole('toolbar', { name: 'Drawing tools' })
		.getByRole('button', { name: 'Adjust' })
		.click();
	const panel = page.getByRole('group', { name: 'Adjust layer' });
	// gradient map: black turns the darkest sepia (brown, more red than blue)
	await panel.getByRole('radio', { name: 'Gradient map' }).click();
	await panel.getByRole('button', { name: 'Sepia' }).click();
	await expect
		.poll(async () => {
			const [r, , b] = await pixel(page, 0.5, 0.5);
			return r - b;
		})
		.toBeGreaterThan(3);
	// curves: pull the black end up to white, and the line fades away
	await panel.getByRole('radio', { name: 'Curves' }).click();
	const graph = panel.getByRole('application', { name: /Curve/ });
	const g = (await graph.boundingBox())!;
	// the graph has a 12 px margin: its corner (0, 0) is in 12 px from the edges
	await page.mouse.move(g.x + 12, g.y + g.height - 12);
	await page.mouse.down();
	await page.mouse.move(g.x + 12, g.y + 14, { steps: 6 });
	await page.mouse.up();
	await expect.poll(() => ink(page, 0.3, 0.45, 0.7, 0.55)).toBeLessThan(dark / 4);
	await panel.getByRole('button', { name: 'Reset curves' }).click();
	// levels: a lower white point keeps the line (it's already dark)
	await panel.getByRole('radio', { name: 'Levels' }).click();
	await panel.getByLabel(/White point/).fill('0.5');
	await panel.getByRole('button', { name: 'Apply' }).click();
	await expect(panel).toBeHidden();
	expect(await ink(page, 0.3, 0.45, 0.7, 0.55)).toBeGreaterThan(dark / 2);
});

test('an adjustment layer changes what is below it, stays editable and undoes in one step', async ({
	page
}) => {
	await ready(page);
	await thick(page, 4);
	await stroke(page, [
		[0.2, 0.5],
		[0.8, 0.5]
	]);
	await page.getByRole('tab', { name: /Layers/ }).click();
	await page.getByRole('button', { name: 'New adjustment layer' }).click();
	const panel = page.getByRole('group', { name: 'Adjust layer' });
	await expect(panel.getByRole('button', { name: 'Keep editable' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await panel.getByRole('radio', { name: 'Gradient map' }).click();
	await panel.getByRole('button', { name: 'Dusk' }).click();
	await panel.getByRole('button', { name: 'Apply' }).click();
	const layer = page.getByRole('button', { name: /Change the adjustment Gradient map/ });
	await expect(layer).toBeVisible();
	// the paper is mapped too: white turns the lightest colour of the gradient
	await expect.poll(async () => (await pixel(page, 0.1, 0.1))[2]).toBeLessThan(200);
	// change it later: back to plain
	await layer.click();
	await expect(panel).toBeVisible();
	await panel.getByRole('button', { name: 'Black & white' }).click();
	await panel.getByRole('button', { name: 'Apply' }).click();
	await expect.poll(async () => (await pixel(page, 0.1, 0.1))[2]).toBeGreaterThan(240);
	// two undo steps: the edit, then the layer
	await page.keyboard.press('Control+z');
	await expect.poll(async () => (await pixel(page, 0.1, 0.1))[2]).toBeLessThan(200);
	await page.keyboard.press('Control+z');
	await expect(layer).toHaveCount(0);
});

test('the canvas can be cropped or scaled and keeps the drawing (undoable)', async ({ page }) => {
	await ready(page);
	await thick(page, 3);
	await stroke(page, [
		[0.4, 0.5],
		[0.6, 0.5]
	]);
	await page.getByRole('button', { name: /800 × 600/ }).click();
	await page.getByRole('radio', { name: /Custom/ }).check();
	await page.getByLabel('Width').fill('400');
	await page.getByLabel('Height').fill('300');
	await expect(page.getByRole('radio', { name: /Keep its size/ })).toBeChecked();
	await page.getByRole('button', { name: 'Resize canvas' }).click();
	await expect(canvas(page)).toHaveAttribute('aria-label', /400 by 300/);
	// the middle stays: the line is now across most of the canvas
	await expect.poll(() => ink(page, 0.3, 0.45, 0.7, 0.55)).toBeGreaterThan(50);
	await page.keyboard.press('Control+z');
	await expect(canvas(page)).toHaveAttribute('aria-label', /800 by 600/);
	await expect.poll(() => ink(page, 0.4, 0.45, 0.6, 0.55)).toBeGreaterThan(50);
	// scaling up: the drawing grows with it
	await page.getByRole('button', { name: /800 × 600/ }).click();
	await page.getByRole('radio', { name: /Custom/ }).check();
	await page.getByLabel('Width').fill('1600');
	await page.getByLabel('Height').fill('1200');
	await page.getByRole('radio', { name: /Resize it/ }).check();
	await page.getByRole('button', { name: 'Resize canvas' }).click();
	await expect(canvas(page)).toHaveAttribute('aria-label', /1600 by 1200/);
	await expect.poll(() => ink(page, 0.42, 0.48, 0.58, 0.52)).toBeGreaterThan(100);
});

test('colour range selects every similar colour, joined up or not', async ({ page }) => {
	await ready(page);
	await page.getByRole('radio', { name: 'Colour #e8492f' }).click();
	await block(page, 0.2, 0.25, 0.8, 0.35);
	await block(page, 0.2, 0.65, 0.8, 0.75);
	await page.keyboard.press('w');
	await page.getByRole('button', { name: 'Colour range' }).click();
	await page.mouse.click(...(await at(page, 0.5, 0.3)));
	await page.getByRole('button', { name: 'Clear selected' }).click();
	await expect.poll(() => ink(page, 0.3, 0.25, 0.7, 0.35, 240)).toBeLessThan(40);
	expect(await ink(page, 0.3, 0.65, 0.7, 0.75, 240)).toBeLessThan(40);
});

test('the quick menu opens on right-click and its actions work', async ({ page }) => {
	await ready(page);
	await stroke(page, [
		[0.3, 0.5],
		[0.7, 0.5]
	]);
	expect(await ink(page, 0.3, 0.45, 0.7, 0.55)).toBeGreaterThan(20);
	await page.mouse.click(...(await at(page, 0.5, 0.3)), { button: 'right' });
	const menu = page.getByRole('menu', { name: 'Quick menu' });
	await expect(menu).toBeVisible();
	await menu.getByRole('menuitem', { name: 'Undo' }).click();
	await expect(menu).toBeHidden();
	await expect.poll(() => ink(page, 0.3, 0.45, 0.7, 0.55)).toBe(0);
	await page.mouse.click(...(await at(page, 0.5, 0.3)), { button: 'right' });
	await page.keyboard.press('Escape');
	await expect(menu).toBeHidden();
});

test('the pen tool draws a path, then the brush follows it', async ({ page }) => {
	await ready(page);
	await page.keyboard.press('p');
	for (const [x, y] of [
		[0.3, 0.3],
		[0.7, 0.3],
		[0.5, 0.7]
	] as [number, number][])
		await page.mouse.click(...(await at(page, x, y)));
	const path = page.getByTestId('pen-path');
	await expect(path).toHaveAttribute('data-points', '3');
	// close it on the first point, then fill
	await page.mouse.click(...(await at(page, 0.3, 0.3)));
	await expect(path).toHaveAttribute('data-closed', 'true');
	await page.getByRole('button', { name: 'Fill', exact: true }).click();
	await expect.poll(() => ink(page, 0.48, 0.38, 0.52, 0.42)).toBeGreaterThan(10);
	await expect(path).toHaveCount(0);
	// an open curve, drawn with the brush
	await page.mouse.move(...(await at(page, 0.1, 0.85)));
	await page.mouse.down();
	await page.mouse.move(...(await at(page, 0.2, 0.75)), { steps: 4 });
	await page.mouse.up();
	await page.mouse.click(...(await at(page, 0.9, 0.85)));
	await page.keyboard.press('Enter');
	await expect.poll(() => ink(page, 0.85, 0.8, 0.95, 0.9)).toBeGreaterThan(3);
});

test('a paper grain thins the brush, and brushes can be imported', async ({ page }) => {
	await ready(page);
	await thick(page);
	await stroke(page, [
		[0.15, 0.3],
		[0.85, 0.3]
	]);
	const plain = await ink(page, 0.2, 0.22, 0.8, 0.38, 250);
	await page.getByRole('tab', { name: /Brushes/ }).click();
	await page.getByRole('button', { name: /Texture & shape/ }).click();
	await page.getByRole('radio', { name: 'Grit' }).click();
	await page.getByLabel(/Strength/).fill('1');
	await stroke(page, [
		[0.15, 0.7],
		[0.85, 0.7]
	]);
	const grainy = await ink(page, 0.2, 0.62, 0.8, 0.78, 250);
	expect(grainy).toBeGreaterThan(0);
	expect(grainy).toBeLessThan(plain * 0.85);
	// import a MyPaint brush: it joins My brushes and is picked
	await page.locator('input[type=file][accept*=".myb"]').setInputFiles({
		name: 'Fat round.myb',
		mimeType: 'application/json',
		buffer: Buffer.from(
			JSON.stringify({
				version: 3,
				settings: {
					radius_logarithmic: { base_value: 2.5, inputs: {} },
					opaque: { base_value: 1, inputs: {} },
					hardness: { base_value: 1, inputs: {} }
				}
			})
		)
	});
	await expect(page.getByText('Added 1 brush to My brushes.')).toBeVisible();
	await expect(page.getByText('Fat round').first()).toBeVisible();
});

test('ColorDrop: a dropped colour fills, and its spread can be changed', async ({ page }) => {
	await ready(page);
	// a closed box to fill
	await thick(page, 2);
	await stroke(page, [
		[0.3, 0.3],
		[0.7, 0.3],
		[0.7, 0.7],
		[0.3, 0.7],
		[0.3, 0.3]
	]);
	const [x, y] = await at(page, 0.5, 0.5);
	const dt = await page.evaluateHandle(() => {
		const d = new DataTransfer();
		d.setData('application/x-pp-color', '#3d6bff');
		return d;
	});
	await canvas(page).dispatchEvent('dragover', { dataTransfer: dt, clientX: x, clientY: y });
	await canvas(page).dispatchEvent('drop', { dataTransfer: dt, clientX: x, clientY: y });
	await expect.poll(async () => (await pixel(page, 0.5, 0.5))[0]).toBeLessThan(120);
	const chip = page.getByTestId('colordrop-threshold');
	await expect(chip).toBeVisible();
	await chip.getByRole('slider').fill('0.5');
	await expect.poll(async () => (await pixel(page, 0.5, 0.5))[0]).toBeLessThan(120);
	await chip.getByRole('button', { name: 'Done' }).click();
	await expect(chip).toBeHidden();
});

test('a dropped picture comes in ready to place', async ({ page }) => {
	await ready(page);
	const [x, y] = await at(page, 0.5, 0.5);
	const dt = await page.evaluateHandle(async () => {
		const c = new OffscreenCanvas(40, 30);
		const g = c.getContext('2d')!;
		g.fillStyle = '#c33';
		g.fillRect(0, 0, 40, 30);
		const blob = await c.convertToBlob({ type: 'image/png' });
		const d = new DataTransfer();
		d.items.add(new File([blob], 'red.png', { type: 'image/png' }));
		return d;
	});
	await canvas(page).dispatchEvent('dragover', { dataTransfer: dt, clientX: x, clientY: y });
	await canvas(page).dispatchEvent('drop', { dataTransfer: dt, clientX: x, clientY: y });
	await expect(page.getByRole('button', { name: 'Transform (V)' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await expect.poll(async () => (await pixel(page, 0.5, 0.5))[1]).toBeLessThan(100);
});
