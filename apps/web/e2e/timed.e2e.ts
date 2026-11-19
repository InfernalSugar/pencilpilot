import { expect, test, type Page } from '@playwright/test';

async function scribble(page: Page) {
	const canvas = page.getByLabel(/Drawing canvas/);
	await expect(canvas).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	const box = (await canvas.boundingBox())!;
	await page.mouse.move(box.x + box.width * 0.3, box.y + box.height * 0.5);
	await page.mouse.down();
	for (let i = 1; i <= 15; i++)
		await page.mouse.move(box.x + box.width * (0.3 + i * 0.03), box.y + box.height * 0.5);
	await page.mouse.up();
}

test('a timed round counts down, locks and hands in the drawing', async ({ page }) => {
	await page.goto('/draw?time=4');
	await scribble(page);
	await expect(page.getByRole('timer')).toHaveAccessibleName(/Time left 0:0[0-3]/);
	await expect(page.getByRole('timer')).toHaveClass(/urgent/);
	await expect(page.getByRole('alert').filter({ hasText: 'Time’s up!' })).toBeVisible({
		timeout: 8000
	});
	await expect(page).toHaveURL(/\/results$/, { timeout: 8000 });
	await expect(page.getByText(/4-second drawing/)).toBeVisible();
});

test('running out with nothing drawn offers another go', async ({ page }) => {
	await page.goto('/draw?time=2');
	await expect(page.getByLabel(/Drawing canvas/)).toHaveAttribute('data-ready', 'true', {
		timeout: 15_000
	});
	await expect(page.getByText('Nothing drawn this time.')).toBeVisible({ timeout: 6000 });
	await page.getByRole('button', { name: 'Try again' }).click();
	await expect(page.getByRole('timer')).toHaveAccessibleName('Time left 0:02');
	await scribble(page);
	expect(page.url()).toContain('/draw');
});

test('the practice page adds the time limit or a gesture session to the links', async ({
	page
}) => {
	await page.goto('/practice/animals');
	await page.getByRole('radio', { name: '2 min' }).click();
	await expect(page.getByRole('link', { name: /Surprise me/ })).toHaveAttribute(
		'href',
		/&time=120$/
	);
	await page.getByRole('radio', { name: 'Gesture session' }).click();
	await page.getByLabel('Pictures').selectOption('5');
	await page.getByLabel('Each').selectOption('30');
	await expect(page.getByRole('link', { name: /Surprise me/ })).toHaveAttribute(
		'href',
		/&session=gesture&count=5&time=30$/
	);
});

test('the session summary lists the drawings', async ({ page }) => {
	await page.goto('/');
	const png = await page.evaluate(() => {
		const c = document.createElement('canvas');
		c.width = 80;
		c.height = 60;
		c.getContext('2d')!.fillRect(10, 10, 20, 20);
		return c.toDataURL('image/png');
	});
	await page.evaluate((png) => {
		const d = (at: number) => ({
			at,
			png,
			width: 80,
			height: 60,
			seconds: 30,
			category: null,
			referenceId: null,
			strokes: 1,
			points: 2,
			brushes: ['pencil'],
			strokeData: [],
			timeLimit: 30
		});
		sessionStorage.setItem(
			'pp.session',
			JSON.stringify({
				plan: { count: 3, time: 30, category: 'animals', topic: null, tier: 1, startedAt: 1 },
				drawings: [d(11), d(12)],
				skipped: 1
			})
		);
	}, png);
	await page.goto('/session');
	await expect(page.getByRole('heading', { name: 'Session done' })).toBeVisible();
	await expect(page.getByText(/Animals · 3 × 30-second/)).toBeVisible();
	await expect(page.getByRole('list', { name: 'Your drawings' }).getByRole('listitem')).toHaveCount(
		2
	);
	await expect(page.getByText('1 ran out with nothing drawn')).toBeVisible();
	await page.getByRole('link', { name: /Drawing 2/ }).click();
	await expect(page).toHaveURL(/\/results\?at=12$/);
	await expect(page.getByRole('img', { name: 'Your submitted drawing' })).toBeVisible();
});
