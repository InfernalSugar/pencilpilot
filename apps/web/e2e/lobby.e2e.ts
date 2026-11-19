import { expect, test, type Browser, type Page } from '@playwright/test';

async function member(browser: Browser) {
	const page = await (await browser.newContext()).newPage();
	await page.goto('/signup');
	await page.getByLabel('Email').fill(`e2e-l-${Date.now()}-${Math.random()}@example.com`);
	await page.getByLabel('Password').fill('a good long password');
	await page.getByRole('button', { name: 'Create account' }).click();
	await expect(page).toHaveURL(/\/account/);
	return page;
}

async function guest(browser: Browser, url: string, name: string) {
	const page = await (await browser.newContext()).newPage();
	await page.goto(url);
	await page.getByLabel(/Your name/).fill(name);
	await page.getByRole('button', { name: 'Join' }).click();
	return page;
}

async function line(page: Page, y: number) {
	const canvas = page.getByLabel(/Drawing canvas/);
	await expect(canvas).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	const box = (await canvas.boundingBox())!;
	await page.mouse.move(box.x + box.width * 0.2, box.y + box.height * y);
	await page.mouse.down();
	for (let i = 1; i <= 15; i++)
		await page.mouse.move(box.x + box.width * (0.2 + i * 0.04), box.y + box.height * y, {
			steps: 2
		});
	await page.mouse.up();
}

/** Ink in a horizontal band of the canvas (fraction of height). */
const inkAt = (page: Page, y: number) =>
	page.getByLabel(/Drawing canvas/).evaluate((c: HTMLCanvasElement, y) => {
		const row = Math.round(c.height * y);
		const d = c.getContext('2d')!.getImageData(0, row - 6, c.width, 12).data;
		let n = 0;
		for (let i = 0; i < d.length; i += 4) if (d[i] < 220) n++;
		return n;
	}, y);

async function openLobby(browser: Browser) {
	const host = await member(browser);
	await host.goto('/lobby');
	await host.getByRole('button', { name: 'Create a lobby' }).click();
	await expect(host).toHaveURL(/\/lobby\/[A-Z0-9]{6}$/);
	await expect(host.getByRole('heading', { name: 'Lobby', level: 1 })).toBeVisible();
	return { host, url: host.url() };
}

test('free draw: one canvas, everyone sees each other draw live', async ({ browser }) => {
	test.setTimeout(90_000);
	const { host, url } = await openLobby(browser);
	const sam = await guest(browser, url, 'Sam');
	await expect(host.getByRole('listitem').filter({ hasText: 'Sam' })).toBeVisible();
	await host.getByRole('radio', { name: 'Free draw' }).click();
	await expect(sam.getByRole('radio', { name: 'Free draw' })).toHaveAttribute(
		'aria-checked',
		'true'
	);
	await host.getByRole('button', { name: /Start drawing together/ }).click();

	await line(host, 0.3);
	await expect.poll(() => inkAt(sam, 0.3), { timeout: 10_000 }).toBeGreaterThan(30);
	await line(sam, 0.7);
	await expect.poll(() => inkAt(host, 0.7), { timeout: 10_000 }).toBeGreaterThan(30);

	// undo takes back only your own line, everywhere
	await sam.getByRole('button', { name: 'Undo' }).click();
	await expect.poll(() => inkAt(host, 0.7), { timeout: 10_000 }).toBe(0);
	expect(await inkAt(sam, 0.3)).toBeGreaterThan(30);

	// someone joining late gets the canvas so far
	const late = await guest(browser, url, 'Late');
	await expect.poll(() => inkAt(late, 0.3), { timeout: 15_000 }).toBeGreaterThan(30);

	// the host sends everyone back to the lobby
	await host.getByRole('button', { name: 'Back to the lobby' }).click();
	await expect(late.getByRole('heading', { name: 'Lobby', level: 1 })).toBeVisible();
});

test('competition: same picture, hand in, reveal with names, results, play again', async ({
	browser
}) => {
	test.setTimeout(90_000);
	const { host, url } = await openLobby(browser);
	const sam = await guest(browser, url, 'Sam');
	await expect(host.getByRole('listitem').filter({ hasText: 'Sam' })).toBeVisible();
	await host.getByLabel('Rounds').selectOption('1');
	await host.getByLabel('Time per round').selectOption('30');
	await host.getByRole('button', { name: /Start the game/ }).click();

	await expect(host.getByText('Round 1 of 1')).toBeVisible({ timeout: 15_000 });
	await expect(sam.getByRole('timer')).toBeVisible();
	const pic = await host.getByRole('img', { name: /^Reference:/ }).getAttribute('src');
	expect(await sam.getByRole('img', { name: /^Reference:/ }).getAttribute('src')).toBe(pic);

	await line(host, 0.5);
	await line(sam, 0.4);
	await host.getByRole('button', { name: /I’m done/ }).click();
	await expect(host.getByText('Handed in!')).toBeVisible();
	await sam.getByRole('button', { name: /I’m done/ }).click();

	// reveal: names shown, react to the other person's drawing
	await expect(host.getByRole('region', { name: 'Reveal' })).toBeVisible({ timeout: 15_000 });
	for (let i = 0; i < 2; i++) {
		const who = await sam
			.getByRole('region', { name: 'Reveal' })
			.getByRole('heading')
			.textContent();
		if (!who?.includes('(you)')) await sam.getByRole('button', { name: /👍/ }).click();
		await host.getByRole('button', { name: /Next drawing|See the scores/ }).click();
	}
	await expect(sam.getByRole('region', { name: 'Round results' })).toBeVisible();
	await expect(sam.getByRole('region', { name: 'Round results' })).toContainText('👍 1');
	await host.getByRole('button', { name: 'Final results →' }).click();
	await expect(sam.getByRole('region', { name: 'Final results' })).toBeVisible();
	await host.getByRole('button', { name: 'Play again →' }).click();
	await expect(sam.getByRole('heading', { name: 'Lobby', level: 1 })).toBeVisible();
});

test('lobbies: guests can’t host, the host can remove people', async ({ browser, page }) => {
	await page.goto('/lobby');
	await expect(page.getByRole('link', { name: 'Sign in to host' })).toBeVisible();
	const { host, url } = await openLobby(browser);
	const sam = await guest(browser, url, 'Sam');
	await expect(host.getByRole('listitem').filter({ hasText: 'Sam' })).toBeVisible();
	await host.getByRole('button', { name: 'Remove' }).click();
	await expect(sam.getByText('The host removed you from this lobby.')).toBeVisible();
	await expect(host.getByRole('listitem').filter({ hasText: 'Sam' })).toHaveCount(0);
});
