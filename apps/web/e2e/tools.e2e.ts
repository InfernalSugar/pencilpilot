import { expect, test, type Page } from '@playwright/test';

const canvas = (page: Page) => page.getByLabel(/Drawing canvas/);

async function ready(page: Page) {
	await page.goto('/draw');
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	return (await canvas(page).boundingBox())!;
}

/** the canvas's box, scrolled into view first (toolbar clicks can scroll the page) */
async function box(page: Page) {
	await canvas(page).scrollIntoViewIfNeeded();
	return (await canvas(page).boundingBox())!;
}

async function zigzag(page: Page, y: number, amp = 0.06) {
	const b = await box(page);
	await page.mouse.move(b.x + b.width * 0.15, b.y + b.height * y);
	await page.mouse.down();
	for (let i = 1; i <= 40; i++)
		await page.mouse.move(
			b.x + b.width * (0.15 + i * 0.017),
			b.y + b.height * (y + (i % 2 ? amp : -amp))
		);
	await page.mouse.up();
}

/** ink pixels and the ink's height, inside a band of rows */
const band = (page: Page, y0: number, y1: number) =>
	canvas(page).evaluate(
		(c: HTMLCanvasElement, [y0, y1]) => {
			const top = Math.round(c.height * y0);
			const h = Math.round(c.height * (y1 - y0));
			const d = c.getContext('2d')!.getImageData(0, top, c.width, h).data;
			let [n, lo, hi, red] = [0, h, 0, 0];
			for (let i = 0; i < d.length; i += 4)
				if (d[i] < 225 || d[i + 1] < 225) {
					n++;
					const row = Math.floor(i / 4 / c.width);
					lo = Math.min(lo, row);
					hi = Math.max(hi, row);
					if (d[i] > d[i + 1] + 40) red++;
				}
			return { n, height: n ? hi - lo : 0, red };
		},
		[y0, y1]
	);

test('eyedropper: the tool and Alt+click pick colours from the drawing', async ({ page }) => {
	await ready(page);
	await page.getByRole('radio', { name: 'Colour #e8492f' }).click();
	for (let i = 0; i < 6; i++) await page.keyboard.press(']'); // a thick line to pick from
	await zigzag(page, 0.3, 0);
	await page.getByRole('radio', { name: 'Colour #3d6bff' }).click();
	await page.getByRole('button', { name: 'Eyedropper (I)' }).click();
	let b2 = await box(page);
	await page.mouse.click(b2.x + b2.width * 0.4, b2.y + b2.height * 0.3);
	// back to the brush, now in (roughly) the red that was picked
	await expect(page.getByRole('button', { name: 'Brush (B)' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await zigzag(page, 0.6, 0);
	// pencil reds are pale, so compare: the new line leans red, not blue
	const tint = await canvas(page).evaluate((c: HTMLCanvasElement) => {
		const d = c
			.getContext('2d')!
			.getImageData(0, Math.round(c.height * 0.55), c.width, Math.round(c.height * 0.1)).data;
		let [r, b, n] = [0, 0, 0];
		for (let i = 0; i < d.length; i += 4)
			if (d[i] < 250 || d[i + 1] < 250 || d[i + 2] < 250) {
				r += d[i];
				b += d[i + 2];
				n++;
			}
		return { n, warm: r - b };
	});
	expect(tint.n).toBeGreaterThan(30);
	expect(tint.warm).toBeGreaterThan(0);

	await page.getByRole('radio', { name: 'Colour #3d6bff' }).click();
	b2 = await box(page);
	await page.keyboard.down('Alt');
	await page.mouse.click(b2.x + b2.width * 0.4, b2.y + b2.height * 0.3);
	await page.keyboard.up('Alt');
	await expect(page.getByRole('radio', { name: 'Colour #3d6bff' })).toHaveAttribute(
		'aria-checked',
		'false'
	);
});

test('smoothing steadies a wobbly line and still reaches the end', async ({ page }) => {
	await ready(page);
	await zigzag(page, 0.3);
	const raw = await band(page, 0.15, 0.45);
	await page.getByLabel(/^Smoothing/).fill('0.9');
	await zigzag(page, 0.7);
	const smooth = await band(page, 0.55, 0.85);
	expect(smooth.height).toBeLessThan(raw.height * 0.6);
	// the catch-up carries the line on to where the pointer stopped (x 0.83 of the width)
	// (the last dabs land a frame later)
	await expect
		.poll(() =>
			canvas(page).evaluate((c: HTMLCanvasElement) => {
				const d = c
					.getContext('2d')!
					.getImageData(0, Math.round(c.height * 0.5), c.width, Math.round(c.height * 0.4)).data;
				let right = 0;
				for (let i = 0; i < d.length; i += 4)
					if (d[i] < 225) right = Math.max(right, (i / 4) % c.width);
				return right / c.width;
			})
		)
		.toBeGreaterThan(0.81);
});

test('two-finger tap undoes, three-finger tap redoes', async ({ page }) => {
	const b = await ready(page);
	await zigzag(page, 0.5, 0);
	// wait for the last dabs (they land a frame after pointer-up)
	let before = -1;
	await expect
		.poll(async () => {
			const n = (await band(page, 0.4, 0.6)).n;
			const same = n === before;
			before = n;
			return same;
		})
		.toBe(true);
	expect(before).toBeGreaterThan(30);
	const tapWith = (fingers: number) =>
		canvas(page).evaluate(
			(c: HTMLCanvasElement, [n, x, y]) => {
				const vp = c.closest('.viewport')!;
				const ev = (type: string, id: number) =>
					new PointerEvent(type, {
						pointerId: 100 + id,
						pointerType: 'touch',
						clientX: x + id * 40,
						clientY: y,
						bubbles: true,
						isPrimary: id === 0
					});
				for (let i = 0; i < n; i++) vp.dispatchEvent(ev('pointerdown', i));
				for (let i = 0; i < n; i++) vp.dispatchEvent(ev('pointerup', i));
			},
			[fingers, b.x + b.width * 0.3, b.y + b.height * 0.8]
		);
	await tapWith(2);
	await expect.poll(async () => (await band(page, 0.4, 0.6)).n).toBe(0);
	await tapWith(3);
	await expect.poll(async () => (await band(page, 0.4, 0.6)).n).toBe(before);
});

test('colour panel: wheel, hex, recent colours and my palette', async ({ page }) => {
	await ready(page);
	await page.getByRole('button', { name: 'More colours' }).click();
	const panel = page.getByRole('dialog', { name: 'Colours' });
	await panel.getByLabel('Hex colour').fill('#1f9d74');
	await panel.getByLabel('Hex colour').press('Enter');
	await expect(page.getByRole('radio', { name: 'Colour #1f9d74' })).toHaveAttribute(
		'aria-checked',
		'true'
	);
	await panel.getByRole('slider', { name: 'Saturation and brightness' }).press('ArrowDown');
	await expect(page.getByRole('radio', { name: 'Colour #1f9d74' })).toHaveAttribute(
		'aria-checked',
		'false'
	);
	await panel.getByRole('button', { name: '+ Add this colour' }).click();
	await panel.getByRole('button', { name: 'Done', exact: true }).click();
	await zigzag(page, 0.5, 0);
	await page.getByRole('button', { name: 'More colours' }).click();
	await expect(panel.getByText('Recent')).toBeVisible();
	await panel.getByRole('button', { name: 'Edit' }).click();
	await expect(panel.getByRole('button', { name: /^Remove #/ })).toHaveCount(1);
});
