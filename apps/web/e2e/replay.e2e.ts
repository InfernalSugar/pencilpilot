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

/** dark-ish and red pixel counts of a canvas or image */
const counts = (el: HTMLCanvasElement | HTMLImageElement) => {
	const c = document.createElement('canvas');
	const w = el instanceof HTMLImageElement ? el.naturalWidth : el.width;
	const h = el instanceof HTMLImageElement ? el.naturalHeight : el.height;
	[c.width, c.height] = [w, h];
	const x = c.getContext('2d')!;
	x.drawImage(el, 0, 0);
	const d = x.getImageData(0, 0, w, h).data;
	let [ink, red] = [0, 0];
	for (let i = 0; i < d.length; i += 4) {
		if (d[i] < 235 || d[i + 1] < 235) ink++;
		if (d[i] > d[i + 1] + 30) red++;
	}
	return { ink, red };
};

test('replay redraws the drawing with its colours and layers', async ({ page }) => {
	await page.goto('/draw');
	await expect(page.getByLabel(/Drawing canvas/)).toHaveAttribute('data-ready', 'true', {
		timeout: 15_000
	});
	// a solid marker: a thin pencil drawn fast (the canvas is shown smaller than its pixels) is faint
	await page.getByRole('tab', { name: /^Brushes/ }).click();
	await page.getByLabel('Brush category').selectOption({ label: 'Markers' });
	await page.getByRole('button', { name: /^Marker fat/ }).click();
	await page.getByRole('tab', { name: /^Layers/ }).click();
	await line(page, 0.3);
	await page.getByRole('button', { name: 'New layer' }).click();
	await page.getByRole('radio', { name: 'Colour #e8492f' }).click();
	await line(page, 0.6);
	await page
		.getByLabel(/^Opacity/)
		.last()
		.fill('50'); // layer opacity
	await page.getByRole('button', { name: /Submit drawing/ }).click();
	await expect(page).toHaveURL(/\/results$/);

	const submitted = page.getByRole('img', { name: 'Your submitted drawing' });
	await expect(submitted).toBeVisible();
	await submitted.evaluate((el: HTMLImageElement) => el.decode());
	const want = await submitted.evaluate(counts);
	expect(want.red).toBeGreaterThan(20);

	await page.getByRole('button', { name: /Watch the replay/ }).click();
	const replay = page.getByLabel('Replay of your drawing');
	await expect(replay).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await page.getByLabel('Replay speed').selectOption('8');
	await expect(page.getByRole('button', { name: 'Play' })).toBeVisible({ timeout: 15_000 });
	const got = await replay.evaluate(counts);
	expect(got.red).toBeGreaterThan(20);
	expect(Math.abs(got.ink - want.ink)).toBeLessThan(want.ink * 0.05 + 10);
	expect(Math.abs(got.red - want.red)).toBeLessThan(want.red * 0.05 + 10);

	// scrubbing back to the start shows an empty page
	await page.getByLabel('Replay position').fill('0');
	expect((await replay.evaluate(counts)).ink).toBe(0);
});

test('the replay can be saved as a video', async ({ page }) => {
	await page.goto('/draw');
	await expect(page.getByLabel(/Drawing canvas/)).toHaveAttribute('data-ready', 'true', {
		timeout: 15_000
	});
	await line(page, 0.4);
	await line(page, 0.6);
	await page.getByRole('button', { name: /Submit drawing/ }).click();
	await expect(page).toHaveURL(/\/results$/);
	await page.getByRole('button', { name: /Watch the replay/ }).click();
	await expect(page.getByLabel('Replay of your drawing')).toHaveAttribute('data-ready', 'true', {
		timeout: 15_000
	});
	const [video] = await Promise.all([
		page.waitForEvent('download', { timeout: 30_000 }),
		page.getByRole('button', { name: 'Save video' }).click()
	]);
	expect(video.suggestedFilename()).toMatch(/^pencilpilot-replay-.*\.(mp4|webm)$/);
	const bytes = Buffer.concat(await (await video.createReadStream()).toArray());
	expect(bytes.length).toBeGreaterThan(1000);
	await expect(page.getByRole('button', { name: 'Save video' })).toBeEnabled();
});
