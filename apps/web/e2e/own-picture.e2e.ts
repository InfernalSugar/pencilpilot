import { expect, test } from '@playwright/test';

/** a PNG made in the page: a dark square on white */
async function squarePng(page: import('@playwright/test').Page): Promise<Buffer> {
	const url = await page.evaluate(() => {
		const c = document.createElement('canvas');
		[c.width, c.height] = [400, 300];
		const g = c.getContext('2d')!;
		g.fillStyle = '#fff';
		g.fillRect(0, 0, 400, 300);
		g.lineWidth = 6;
		g.strokeStyle = '#16161a';
		g.strokeRect(100, 75, 200, 150);
		return c.toDataURL('image/png');
	});
	return Buffer.from(url.split(',')[1], 'base64');
}

test('practise with your own picture: it stays in the browser, is graded, and Try again keeps it', async ({
	page
}) => {
	// the picture must never leave the browser
	const leaks: string[] = [];
	page.on('request', (r) => {
		const body = r.postData() ?? '';
		if (body.includes('data:image/jpeg') || body.length > 200_000) leaks.push(r.url());
	});
	await page.goto('/');
	await page.getByRole('link', { name: /Your own picture/ }).click();
	await expect(page).toHaveURL(/\/practice\/own$/);
	await expect(page.getByText(/stays on this device/)).toBeVisible();
	await page.getByLabel('Picture from your device').setInputFiles({
		name: 'my box.png',
		mimeType: 'image/png',
		buffer: await squarePng(page)
	});
	await expect(page.getByRole('img', { name: /Preview of my box/ })).toBeVisible();
	await page.getByText('Anime / manga').click();
	await expect(page.getByRole('radio', { name: /Anime/ })).toBeChecked();
	await page.getByRole('button', { name: /Start drawing/ }).click();

	await expect(page).toHaveURL(/\/draw\?own=1/);
	await expect(page.getByText('Your own picture · ', { exact: false }).first()).toBeVisible();
	await expect(page.getByRole('heading', { name: 'my box', level: 1 })).toBeVisible();
	await expect(page.getByText(/It stays on this device and isn’t uploaded/)).toBeVisible();
	await expect(page.getByRole('button', { name: 'Report picture' })).toHaveCount(0);

	// draw the box roughly and hand in
	const c = page.getByLabel(/Drawing canvas/);
	await expect(c).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	for (let i = 0; i < 3; i++) await page.keyboard.press(']');
	const b = (await c.boundingBox())!;
	const pts: [number, number][] = [
		[0.25, 0.25],
		[0.75, 0.25],
		[0.75, 0.75],
		[0.25, 0.75],
		[0.25, 0.25]
	];
	await page.mouse.move(b.x + b.width * pts[0][0], b.y + b.height * pts[0][1]);
	await page.mouse.down();
	for (const [x, y] of pts.slice(1))
		await page.mouse.move(b.x + b.width * x, b.y + b.height * y, { steps: 12 });
	await page.mouse.up();
	await page.getByRole('button', { name: /Submit drawing/ }).click();
	await expect(page).toHaveURL(/\/results$/);
	await expect(page.getByText(/your own picture/i).first()).toBeVisible();
	// a score shows ("out of 10", or "Passed!" for a good match)
	await expect(page.getByText(/out of 10|Passed!/).first()).toBeVisible({ timeout: 20_000 });
	await expect(page.getByText(/kept on this device only/)).toBeVisible();

	await page.getByRole('link', { name: 'Try again' }).click();
	await expect(page).toHaveURL(/\/draw\?own=1/);
	await expect(page.getByRole('heading', { name: 'my box', level: 1 })).toBeVisible();
	expect(leaks).toEqual([]);
});
