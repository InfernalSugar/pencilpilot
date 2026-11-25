import { expect, test, type Locator, type Page } from '@playwright/test';

// The brush creator: settings that change the sample stroke, a little canvas to try the
// brush on, saving to My brushes; and importing brushes from other apps. Tauri's IPC is
// stubbed (only its settings file is answered).

const SHOTS = process.env.PP_SHOTS;

async function stub(page: Page, settings: Record<string, unknown> = {}) {
	await page.addInitScript((settings) => {
		const w = window as unknown as Record<string, unknown>;
		w.isTauri = true;
		w.__TAURI_INTERNALS__ = {
			transformCallback: () => 0,
			unregisterCallback: () => {},
			metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
			invoke: async (cmd: string) => {
				switch (cmd) {
					case 'settings_get':
						return settings;
					case 'recent_list':
					case 'recovery_list':
					case 'versions_list':
						return [];
					default:
						return null;
				}
			}
		};
	}, settings);
}

async function studio(page: Page, settings: Record<string, unknown> = {}) {
	await stub(page, settings);
	await page.setViewportSize({ width: 1400, height: 900 });
	await page.goto('/studio');
	await expect(page.getByLabel(/Drawing canvas/)).toHaveAttribute('data-ready', 'true', {
		timeout: 15_000
	});
	await page.getByRole('tab', { name: /^Brushes/ }).click();
}

/** pixels with paint on them in a canvas (the paper is white) */
const inked = (c: Locator) =>
	c.evaluate((el: HTMLCanvasElement) => {
		const d = el.getContext('2d')!.getImageData(0, 0, el.width, el.height).data;
		let n = 0;
		for (let i = 0; i < d.length; i += 4)
			if (d[i + 3] > 40 && Math.min(d[i], d[i + 1], d[i + 2]) < 200) n++;
		return n;
	});
/** a canvas's pixels, to tell whether it changed */
const pixels = (c: Locator) =>
	c.evaluate((el: HTMLCanvasElement) =>
		Array.from(el.getContext('2d')!.getImageData(0, 0, el.width, el.height).data).join()
	);

async function drag(page: Page, target: Locator, steps = 25) {
	const b = (await target.boundingBox())!;
	await page.mouse.move(b.x + b.width * 0.15, b.y + b.height * 0.5);
	await page.mouse.down();
	await page.mouse.move(b.x + b.width * 0.85, b.y + b.height * 0.55, { steps });
	await page.mouse.up();
}

test('brush creator: a setting changes the preview, the test canvas takes ink, save keeps it', async ({
	page
}) => {
	await studio(page);
	await page.getByRole('button', { name: 'New brush' }).click();
	const win = page.getByRole('dialog', { name: 'New brush' });
	await expect(win).toBeVisible();

	// the sample stroke is drawn
	const preview = win.getByRole('figure', { name: /Sample stroke/ }).locator('canvas');
	await expect.poll(() => inked(preview)).toBeGreaterThan(300);
	const before = await pixels(preview);
	const thin = await inked(preview);

	// a bigger brush: the preview redraws with more ink
	await win.getByRole('slider', { name: 'Size', exact: true }).fill(String(Math.log(30)));
	await expect.poll(() => pixels(preview)).not.toBe(before);
	await expect.poll(() => inked(preview)).toBeGreaterThan(thin * 1.5);

	// other groups: a flat tip, a grain
	await win.getByRole('slider', { name: 'Tip', exact: true }).fill('6');
	await win.getByRole('tab', { name: 'Colour' }).click();
	await expect(win.getByRole('slider', { name: 'Opacity' })).toBeVisible();
	await win.getByRole('tab', { name: 'Grain and tip picture' }).click();
	await win.getByRole('radio', { name: 'Grain: Paper' }).click();
	await win.getByRole('tab', { name: 'Shape' }).click();

	// try it on the little canvas
	const pad = win.getByLabel('Try the brush here');
	expect(await inked(pad)).toBe(0);
	await drag(page, pad);
	await expect.poll(() => inked(pad)).toBeGreaterThan(500);
	if (SHOTS) await win.screenshot({ path: `${SHOTS}/brush-creator.png` });
	await win.getByRole('button', { name: 'Clear' }).click();
	await expect.poll(() => inked(pad)).toBe(0);

	// saved: it's in My brushes, in use
	await win.getByLabel('Brush name').fill('Fat flat');
	await win.getByRole('button', { name: 'Save to My brushes' }).click();
	await expect(win).toHaveCount(0);
	const mine = page.getByRole('list', { name: 'My brushes' });
	await expect(mine.getByRole('button', { name: 'Fat flat', exact: true })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	const kept = await page.evaluate(() => JSON.parse(localStorage.getItem('pp.myBrushes')!));
	expect(kept).toHaveLength(1);
	expect(kept[0].tweaks.size).toBeCloseTo(30, 0);
	expect(kept[0].tweaks.roundness).toBe(6);
	const textures = await page.evaluate(() =>
		JSON.parse(localStorage.getItem('pp.brushTextureSettings')!)
	);
	expect(textures[kept[0].id].grain).toBe('grain-paper');

	// edit, then close without saving: nothing changes
	await mine.getByRole('button', { name: 'Edit Fat flat' }).click();
	const edit = page.getByRole('dialog', { name: 'Edit “Fat flat”' });
	await edit.getByRole('slider', { name: 'Edge' }).fill('0.1');
	await edit.getByRole('button', { name: 'Cancel' }).click();
	await expect(edit).toHaveCount(0);
	expect(await page.evaluate(() => localStorage.getItem('pp.myBrushes'))).toBe(
		JSON.stringify(kept)
	);

	// delete it (asks first)
	await mine.getByRole('button', { name: 'Edit Fat flat' }).click();
	await edit.getByRole('button', { name: 'Delete brush' }).click();
	await edit.getByRole('button', { name: 'Delete', exact: true }).click();
	await expect(mine.getByRole('button', { name: 'Fat flat', exact: true })).toHaveCount(0);
});

test('brush creator: every setting changes the sample stroke', async ({ page }) => {
	await studio(page);
	await page.getByRole('button', { name: 'New brush' }).click();
	const win = page.getByRole('dialog', { name: 'New brush' });
	const preview = win.getByRole('figure', { name: /Sample stroke/ });
	const canvas = preview.locator('canvas');
	await expect.poll(() => inked(canvas)).toBeGreaterThan(300);
	// a bright colour, so colour changes show
	await win.getByLabel('Colour to try it with').fill('#d04020');
	// (tilt needs a pen that leans: the sample stroke is upright)
	const skip = new Set(['Lean the pen → wider']);
	const failed: string[] = [];
	/** wait until the preview has redrawn for every change so far */
	const settle = async () => {
		const count = async () => Number(await preview.getAttribute('data-drawn'));
		let last = -1;
		await expect
			.poll(
				async () => {
					const was = last;
					last = await count();
					return last === was;
				},
				{ intervals: [300] }
			)
			.toBe(true);
	};
	for (const group of ['Shape', 'Stroke', 'Pressure and speed', 'Colour']) {
		await win.getByRole('tab', { name: group }).click();
		for (const slider of await win.getByRole('slider').all()) {
			const name = (await slider.getAttribute('aria-label'))!;
			if (skip.has(name)) continue;
			// (these need another setting on first: blending, a flat tip)
			if (name === 'Carry blended paint')
				await win.getByRole('slider', { name: 'Blend with the paint underneath' }).fill('0.8');
			if (name === 'Tip angle')
				await win.getByRole('slider', { name: 'Tip', exact: true }).fill('5');
			await settle();
			const drawn = Number(await preview.getAttribute('data-drawn'));
			const before = await pixels(canvas);
			const [min, max, now] = await slider.evaluate((el: HTMLInputElement) => [
				+el.min,
				+el.max,
				+el.value
			]);
			await slider.fill(String(now - min > max - now ? min : max));
			await expect
				.poll(async () => Number(await preview.getAttribute('data-drawn')))
				.toBeGreaterThan(drawn);
			if ((await pixels(canvas)) === before) failed.push(name);
			// back as it was, for the next one
			await slider.fill(String(now));
			if (name === 'Carry blended paint')
				await win.getByRole('slider', { name: 'Blend with the paint underneath' }).fill('0');
			if (name === 'Tip angle')
				await win.getByRole('slider', { name: 'Tip', exact: true }).fill('1');
			await expect
				.poll(async () => Number(await preview.getAttribute('data-drawn')))
				.toBeGreaterThan(drawn + 1);
		}
	}
	// a flat tip turns as the S curve does
	await win.getByRole('tab', { name: 'Shape' }).click();
	await win.getByRole('slider', { name: 'Tip', exact: true }).fill('8');
	await win.getByRole('slider', { name: 'Size', exact: true }).fill(String(Math.log(12)));
	await expect.poll(() => inked(canvas)).toBeGreaterThan(300);
	const before = await pixels(canvas);
	await win.getByLabel('Tip turns with the stroke').check();
	await expect.poll(() => pixels(canvas)).not.toBe(before);
	expect(failed).toEqual([]);
});

test('brush creator fits a smaller window, in the dark theme too', async ({ page }) => {
	await studio(page, { theme: 'dark' });
	await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
	await page.getByRole('button', { name: /^Adjust “/ }).click();
	const win = page.getByRole('dialog', { name: /your own/ });
	await expect(win.getByRole('figure', { name: /Sample stroke/ })).toBeVisible();
	const box = (await win.boundingBox())!;
	expect(box.x).toBeGreaterThanOrEqual(0);
	expect(box.y + box.height).toBeLessThanOrEqual(900);
	await drag(page, win.getByLabel('Try the brush here'));
	if (SHOTS) await win.screenshot({ path: `${SHOTS}/brush-creator-dark.png` });
	await page.setViewportSize({ width: 1100, height: 760 });
	for (const name of ['Save to My brushes', 'Cancel'])
		await expect(win.getByRole('button', { name })).toBeInViewport();
	await expect(win.getByLabel('Try the brush here')).toBeInViewport();
	if (SHOTS) await win.screenshot({ path: `${SHOTS}/brush-creator-1100.png` });
	await page.keyboard.press('Escape');
	await expect(win).toHaveCount(0);
});

test('import brushes: a MyPaint brush lands in My brushes and opens in the creator', async ({
	page
}) => {
	await studio(page);
	const myb = JSON.stringify({
		version: 3,
		settings: {
			radius_logarithmic: { base_value: 2.2, inputs: {} },
			opaque: { base_value: 1, inputs: {} },
			hardness: { base_value: 0.9, inputs: {} },
			dabs_per_actual_radius: { base_value: 3, inputs: {} }
		}
	});
	await page.getByLabel('Brush files to import').setInputFiles({
		name: 'Soft ink.myb',
		mimeType: 'application/octet-stream',
		buffer: Buffer.from(myb)
	});
	await expect(page.getByText('Added 1 brush to My brushes.')).toBeVisible();
	if (SHOTS)
		await page
			.getByRole('list', { name: 'My brushes' })
			.locator('xpath=..')
			.screenshot({ path: `${SHOTS}/brushes-panel.png` });
	const mine = page.getByRole('list', { name: 'My brushes' });
	await expect(mine.getByRole('button', { name: 'Soft ink', exact: true })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	// tune it in the creator: its own settings come in
	await mine.getByRole('button', { name: 'Edit Soft ink' }).click();
	const win = page.getByRole('dialog', { name: 'Edit “Soft ink”' });
	await expect(win.getByRole('slider', { name: 'Edge' })).toHaveValue('0.9');
	await win.getByRole('tab', { name: 'Stroke' }).click();
	await expect(win.getByRole('slider', { name: 'Spacing' })).toHaveValue('3');
	await win.getByRole('slider', { name: 'Spacing' }).fill('6');
	await win.getByRole('button', { name: 'Save', exact: true }).click();
	await expect(win).toHaveCount(0);
	const kept = await page.evaluate(() => JSON.parse(localStorage.getItem('pp.myBrushes')!));
	expect(kept).toMatchObject([{ name: 'Soft ink', tweaks: { spacing: 6 } }]);
	expect(JSON.parse(kept[0].source).settings.hardness.base_value).toBe(0.9);

	// the creator imports too, and opens on what came in
	await page.getByRole('button', { name: 'New brush' }).click();
	await page
		.getByRole('dialog', { name: 'New brush' })
		.getByLabel('Brush files to import')
		.setInputFiles({ name: 'Second.myb', mimeType: 'text/plain', buffer: Buffer.from(myb) });
	await expect(page.getByRole('dialog', { name: 'Edit “Second”' })).toBeVisible();
});

test('settings: the brush credit is under About', async ({ page }) => {
	await studio(page);
	await expect(page.getByText('MyPaint brush pack')).toHaveCount(0);
	await page.getByRole('button', { name: 'Settings', exact: true }).click();
	const dialog = page.getByRole('dialog', { name: 'Settings' });
	await dialog.getByRole('tab', { name: 'About' }).click();
	await expect(dialog.getByRole('link', { name: 'MyPaint brush pack' })).toHaveAttribute(
		'href',
		'https://github.com/mypaint/mypaint-brushes'
	);
});

/** in column `fx` (a fraction of the width) of a canvas: the dark runs down it, top to bottom */
const bands = (c: Locator, fx: number) =>
	c.evaluate((el: HTMLCanvasElement, fx) => {
		const x = Math.round(el.width * fx);
		const d = el.getContext('2d')!.getImageData(x, 0, 1, el.height).data;
		const out: [number, number][] = [];
		let start = -1;
		for (let y = 0; y <= el.height; y++) {
			const i = y * 4;
			const dark = y < el.height && Math.min(d[i], d[i + 1], d[i + 2]) < 150;
			if (dark && start < 0) start = y;
			if (!dark && start >= 0) {
				out.push([start, y]);
				start = -1;
			}
		}
		return out;
	}, fx);

test('Hair strands: one stroke draws several separate, parallel strands', async ({ page }) => {
	await studio(page);
	await page.getByRole('tab', { name: 'Inks & pens' }).click();
	const hair = page
		.getByRole('list', { name: 'Brushes' })
		.getByRole('button', { name: 'Hair strands', exact: true });
	await hair.click();
	await expect(hair).toHaveAttribute('aria-pressed', 'true');

	// one straight stroke across the middle
	const c = page.getByLabel(/Drawing canvas/);
	const b = (await c.boundingBox())!;
	await page.mouse.move(b.x + b.width * 0.15, b.y + b.height * 0.5);
	await page.mouse.down();
	await page.mouse.move(b.x + b.width * 0.85, b.y + b.height * 0.5, { steps: 60 });
	await page.mouse.up();

	// across it, all along: several thin bands of ink with paper between them
	await expect.poll(async () => (await bands(c, 0.5)).length).toBeGreaterThanOrEqual(4);
	const mid = (await c.evaluate((el: HTMLCanvasElement) => el.height)) / 2;
	for (const fx of [0.35, 0.5, 0.65]) {
		const runs = await bands(c, fx);
		expect(runs.length, `strands at ${fx}`).toBeGreaterThanOrEqual(4);
		// each thin, the bundle together a good deal wider than one
		const widest = Math.max(...runs.map(([a, z]) => z - a));
		const span = runs[runs.length - 1][1] - runs[0][0];
		expect(widest).toBeLessThan(span / 3);
		// around where the stroke went
		expect(Math.abs((runs[0][0] + runs[runs.length - 1][1]) / 2 - mid)).toBeLessThan(span);
	}
	if (SHOTS) {
		await c.screenshot({ path: `${SHOTS}/hair-strands.png` });
		// the stroke's end, close up
		await page.getByRole('button', { name: 'Zoom in' }).click();
		await page.getByRole('button', { name: 'Zoom in' }).click();
		await c.screenshot({ path: `${SHOTS}/hair-strands-close.png` });
	}
});

test('brush creator: strands for any brush, with a live preview, kept with the brush', async ({
	page
}) => {
	await studio(page);
	await page.getByRole('button', { name: 'New brush' }).click();
	const win = page.getByRole('dialog', { name: 'New brush' });
	const preview = win.getByRole('figure', { name: /Sample stroke/ });
	const canvas = preview.locator('canvas');
	await expect.poll(() => inked(canvas)).toBeGreaterThan(300);
	await win.getByRole('tab', { name: 'Strands' }).click();
	const count = win.getByRole('slider', { name: 'Strand strands' });
	await expect(count).toBeDisabled();
	const redrawn = async () => Number(await preview.getAttribute('data-drawn'));

	// on: the sample stroke redraws as a bundle
	let drawn = await redrawn();
	let before = await pixels(canvas);
	await win.getByLabel('Draw in strands').check();
	await expect(count).toBeEnabled();
	await expect.poll(redrawn).toBeGreaterThan(drawn);
	await expect.poll(() => pixels(canvas)).not.toBe(before);

	// every strand setting changes it
	for (const [name, value] of [
		['Strand strands', '3'],
		['Strand spread', '20'],
		['Strand wobble', '1'],
		['Strand taper', '0']
	]) {
		drawn = await redrawn();
		before = await pixels(canvas);
		await win.getByRole('slider', { name }).fill(value);
		await expect.poll(redrawn).toBeGreaterThan(drawn);
		await expect.poll(() => pixels(canvas), name).not.toBe(before);
	}
	if (SHOTS) await win.screenshot({ path: `${SHOTS}/brush-creator-strands.png` });

	// the try-out canvas draws in strands too
	const pad = win.getByLabel('Try the brush here');
	await drag(page, pad);
	await expect.poll(() => inked(pad)).toBeGreaterThan(300);

	// saved with the brush
	await win.getByLabel('Brush name').fill('Fur');
	await win.getByRole('button', { name: 'Save to My brushes' }).click();
	await expect(win).toHaveCount(0);
	const kept = await page.evaluate(() => JSON.parse(localStorage.getItem('pp.myBrushes')!));
	const settings = await page.evaluate(() =>
		JSON.parse(localStorage.getItem('pp.brushTextureSettings')!)
	);
	expect(settings[kept[0].id].strands).toEqual({ count: 3, spread: 20, wobble: 1, taper: 0 });
});
