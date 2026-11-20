import { expect, test, type Page } from '@playwright/test';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// The proportions check (M3-T04): a reference with keypoints asks the student to tap
// the same spots on their drawing before the score. Keypoints are marked in
// /dev/keypoints (M5-T03), which only exists on the dev server.

const BOX =
	'<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300"><rect width="400" height="300" fill="#fff"/><rect x="100" y="75" width="200" height="150" fill="none" stroke="#000" stroke-width="6"/></svg>';

const CORNERS = [
	{ x: 0.25, y: 0.25, label: 'top-left corner' },
	{ x: 0.75, y: 0.25, label: 'top-right corner' },
	{ x: 0.75, y: 0.75, label: 'bottom-right corner' },
	{ x: 0.25, y: 0.75, label: 'bottom-left corner' }
];

async function handIn(page: Page) {
	await page.route('**/e2e-box.svg', (r) => r.fulfill({ contentType: 'image/svg+xml', body: BOX }));
	await page.goto('/');
	await page.evaluate((corners) => {
		const c = document.createElement('canvas');
		c.width = 400;
		c.height = 300;
		const g = c.getContext('2d')!;
		g.fillStyle = '#fff';
		g.fillRect(0, 0, 400, 300);
		g.lineWidth = 6;
		g.strokeRect(100, 75, 200, 150);
		sessionStorage.setItem(
			'pp.lastSubmission',
			JSON.stringify({
				at: Date.now(),
				png: c.toDataURL('image/png'),
				width: 400,
				height: 300,
				seconds: 42,
				category: 'animals',
				referenceId: 'px-777',
				reference: {
					id: 'px-777',
					source: 'pixabay',
					sourceId: 777,
					category: 'animals',
					pageUrl: 'https://pixabay.com/photos/box-777/',
					author: 'Ann',
					authorUrl: '',
					license: 'Pixabay Content License',
					licenseUrl: 'https://pixabay.com/service/license-summary/',
					tags: ['box'],
					width: 400,
					height: 300,
					title: 'A box',
					keypoints: corners,
					imageUrl: '/e2e-box.svg',
					live: true
				},
				strokes: 1,
				points: 4,
				brushes: ['pencil'],
				strokeData: []
			})
		);
	}, CORNERS);
	await page.goto('/results');
}

test('the proportions check: tap the points, then see the score', async ({ page }, info) => {
	test.skip(info.project.name !== 'build', 'production build only');
	await handIn(page);
	const check = page.getByRole('region', { name: 'Tap your points' });
	await expect(check.getByRole('heading')).toHaveText(/Tap the top-left corner/);
	const area = check.getByRole('button', { name: /Your drawing: tap the/ });
	const tap = async (x: number, y: number) => {
		await area.scrollIntoViewIfNeeded();
		const b = (await area.boundingBox())!;
		await page.mouse.click(b.x + b.width * x, b.y + b.height * y);
	};

	await tap(0.25, 0.25);
	await tap(0.9, 0.9); // oops
	await check.getByRole('button', { name: 'Undo' }).click();
	await expect(check.getByRole('heading')).toHaveText(/Tap the top-right corner/);
	await tap(0.75, 0.25);
	await tap(0.75, 0.75);
	await tap(0.25, 0.75);
	await expect(check.getByRole('heading')).toHaveText('All set!');
	await check.getByRole('button', { name: /See my score/ }).click();

	await expect(check).toHaveCount(0);
	const row = page.getByRole('listitem').filter({ hasText: 'Proportions' });
	await expect(row).toBeVisible({ timeout: 15_000 });
	await expect(row).toContainText(/9\d%|100%/); // the same box: proportions match
	await page.getByRole('tab', { name: 'Proportions' }).click();
	await expect(page.getByText(/your points, lined up/)).toBeVisible();

	// a reload keeps the answer instead of asking again
	await page.reload();
	await expect(page.getByRole('listitem').filter({ hasText: 'Proportions' })).toBeVisible({
		timeout: 15_000
	});
	await expect(page.getByRole('region', { name: 'Tap your points' })).toHaveCount(0);
});

test('the proportions check can be skipped, and points can be left out', async ({ page }, info) => {
	test.skip(info.project.name !== 'build', 'production build only');
	await handIn(page);
	const check = page.getByRole('region', { name: 'Tap your points' });
	await check.getByRole('button', { name: 'I didn’t draw this one' }).click();
	await expect(check.getByRole('heading')).toHaveText(/Tap the top-right corner/);
	await check.getByRole('button', { name: 'Skip the check' }).click();
	await expect(check).toHaveCount(0);
	await expect(page.getByText('out of 10').or(page.getByText(/Passed/))).toBeVisible({
		timeout: 15_000
	});
	await expect(page.getByRole('listitem').filter({ hasText: 'Proportions' })).toHaveCount(0);
	await expect(page.getByRole('tab', { name: 'Proportions' })).toHaveCount(0);
});

test('the keypoint page does not exist in production', async ({ page }, info) => {
	test.skip(info.project.name !== 'build', 'production build only');
	expect((await page.goto('/dev/keypoints'))?.status()).toBe(404);
});

test('mark keypoints on a reference and save them', async ({ page }, info) => {
	test.skip(info.project.name !== 'dev', 'dev server only');
	const file = resolve('../../content/references/animals.json');
	const before = readFileSync(file, 'utf8');
	const id: string = JSON.parse(before).references[0].id;
	try {
		await page.goto(`/dev/keypoints?id=${id}`);
		const frame = page.getByTestId('keypoint-frame');
		await expect(frame).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
		await page.getByLabel('Label for the next point').fill('nose');
		await frame.scrollIntoViewIfNeeded();
		const b = (await frame.boundingBox())!;
		await page.mouse.click(b.x + b.width * 0.2, b.y + b.height * 0.3);
		await page.mouse.click(b.x + b.width * 0.6, b.y + b.height * 0.3);
		await page.mouse.click(b.x + b.width * 0.5, b.y + b.height * 0.8);
		const list = page.getByRole('list', { name: 'Points' });
		await expect(list.getByRole('listitem')).toHaveCount(3);
		await expect(page.getByLabel('Point 1 label')).toHaveValue('nose');
		await page.getByRole('button', { name: 'Move point 3 up' }).click();
		await page.getByRole('button', { name: 'Save', exact: true }).click();
		await expect(page.getByText('Saved 3 points.')).toBeVisible({ timeout: 15_000 });

		const saved = JSON.parse(readFileSync(file, 'utf8')).references.find(
			(r: { id: string }) => r.id === id
		).keypoints;
		// the save rewrites the JSON file, so Vite may reload the page: wait it out
		await expect(frame).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
		await expect(page.getByLabel('Point 2 label')).toHaveValue(saved[1].label, {
			timeout: 15_000
		});
		expect(saved).toHaveLength(3);
		expect(saved[0]).toMatchObject({ label: 'nose' });
		expect(saved[0].x).toBeCloseTo(0.2, 1);
		expect(saved[1].y).toBeCloseTo(0.8, 1); // moved up from third place
	} finally {
		writeFileSync(file, before);
	}
});
