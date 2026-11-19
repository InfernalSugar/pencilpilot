import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';

/** Every user-facing page, checked at phone, tablet and desktop widths. */
const PAGES = [
	'/',
	'/practice/anime',
	'/draw',
	'/lessons',
	'/lessons/anime-faces',
	'/drills',
	'/drills/ghosted-lines',
	'/progress',
	'/login',
	'/signup',
	'/privacy'
];
const VIEWPORTS = [
	{ name: 'phone', width: 375, height: 740 },
	{ name: 'tablet', width: 820, height: 1180 },
	{ name: 'desktop', width: 1366, height: 860 }
];

async function overflow(page: Page) {
	return page.evaluate(() => {
		const w = document.documentElement.clientWidth;
		if (document.documentElement.scrollWidth <= w + 1) return [];
		return [...document.querySelectorAll<HTMLElement>('body *')]
			.filter((el) => {
				const r = el.getBoundingClientRect();
				return r.right > w + 1 && r.width > 0 && getComputedStyle(el).position !== 'fixed';
			})
			.slice(0, 5)
			.map((el) => `${el.tagName.toLowerCase()}.${[...el.classList].join('.')}`);
	});
}

async function seriousViolations(page: Page) {
	const { violations } = await new AxeBuilder({ page })
		.withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'])
		.analyze();
	return violations
		.filter((v) => v.impact === 'serious' || v.impact === 'critical')
		.map(
			(v) =>
				`${v.id}: ${v.nodes
					.map((n) => n.target.join(' '))
					.slice(0, 4)
					.join(' | ')}`
		);
}

async function drawAndSubmit(page: Page) {
	await page.goto('/draw');
	const canvas = page.getByLabel(/Drawing canvas/);
	await expect(canvas).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await canvas.scrollIntoViewIfNeeded();
	const box = (await canvas.boundingBox())!;
	await page.mouse.move(box.x + box.width * 0.3, box.y + box.height * 0.3);
	await page.mouse.down();
	for (let i = 1; i <= 15; i++)
		await page.mouse.move(
			box.x + box.width * (0.3 + i * 0.03),
			box.y + box.height * (0.3 + i * 0.02)
		);
	await page.mouse.up();
	await page.getByRole('button', { name: /Submit drawing/ }).click();
	await expect(page).toHaveURL(/\/results$/);
	// CI has no reference pictures, so this is the free-draw results page (no score dial).
	await expect(page.getByRole('img', { name: 'Your submitted drawing' })).toBeVisible();
}

for (const scheme of ['light', 'dark'] as const)
	test(`results page (${scheme}) is accessible and fits on a phone`, async ({ page }) => {
		await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
		await page.setViewportSize({ width: 375, height: 740 });
		await drawAndSubmit(page);
		expect(await overflow(page), 'elements wider than the screen').toEqual([]);
		expect(await seriousViolations(page)).toEqual([]);
	});

test.describe('dark mode', () => {
	test.use({ colorScheme: 'dark' });
	for (const path of PAGES)
		test(`${path} is accessible in dark mode`, async ({ page }) => {
			await page.goto(path);
			await page.waitForLoadState('networkidle');
			expect(await seriousViolations(page)).toEqual([]);
		});
});

for (const vp of VIEWPORTS) {
	test.describe(vp.name, () => {
		test.use({ viewport: { width: vp.width, height: vp.height } });
		for (const path of PAGES) {
			test(`${path} is accessible and fits`, async ({ page }) => {
				await page.goto(path);
				await page.waitForLoadState('networkidle');
				expect(await overflow(page), 'elements wider than the screen').toEqual([]);
				expect(await seriousViolations(page)).toEqual([]);
			});
		}
	});
}

test('keyboard users can skip the header', async ({ page }) => {
	await page.goto('/lessons');
	await page.keyboard.press('Tab');
	const skip = page.getByRole('link', { name: 'Skip to content' });
	await expect(skip).toBeFocused();
	await expect(skip).toBeInViewport();
	await page.keyboard.press('Enter');
	await expect(page.locator('main')).toBeFocused();
});

test('phone draw page puts the canvas before the brush list', async ({ page }) => {
	await page.setViewportSize({ width: 375, height: 740 });
	await page.goto('/draw');
	const canvas = page.getByLabel(/Drawing canvas/);
	await expect(canvas).toBeInViewport();
	const toggle = page.getByRole('button', { name: 'Change' });
	await expect(toggle).toHaveAttribute('aria-expanded', 'false');
	await expect(page.getByRole('tab', { name: 'Markers' })).toBeHidden();
	await toggle.click();
	await expect(page.getByRole('tab', { name: 'Markers' })).toBeVisible();
});
