import { expect, test } from '@playwright/test';

test('the daily challenge page and scores', async ({ page, browser }) => {
	await page.goto('/daily');
	await expect(page.getByRole('heading', { name: 'Today’s challenge', level: 1 })).toBeVisible();
	const draw = page.getByRole('link', { name: /Draw today’s picture/ });
	const href = (await draw.getAttribute('href'))!;
	const day = new Date().toISOString().slice(0, 10);
	expect(href).toContain(`daily=${day}`);
	const refId = new URL(href, 'http://x').searchParams.get('ref')!;
	await expect(page.getByText('Sign in to keep a daily streak')).toBeVisible();

	// guests can't be counted
	expect((await page.request.post('/api/daily', { data: { day, refId, score: 7 } })).status()).toBe(
		401
	);

	const ctx = await browser.newContext();
	const me = await ctx.newPage();
	await me.goto('/signup');
	await me.getByLabel('Email').fill(`e2e-d-${Date.now()}@example.com`);
	await me.getByLabel('Password').fill('a good long password');
	await me.getByRole('button', { name: 'Create account' }).click();
	await expect(me).toHaveURL(/\/account/);

	const post = (data: object) => me.request.post('/api/daily', { data });
	expect((await post({ day, refId: 'not-it', score: 7 })).status()).toBe(400);
	expect((await post({ day: '2020-01-01', refId, score: 7 })).status()).toBe(400);
	const first = await (await post({ day, refId, score: 7.5, attemptAt: 1 })).json();
	expect(first).toMatchObject({ counted: true, mine: 7.5, streak: 1 });
	const again = await (await post({ day, refId, score: 9.9, attemptAt: 2 })).json();
	expect(again).toMatchObject({ counted: false, mine: 7.5 });

	await me.goto('/daily');
	await expect(me.getByText('Your score today')).toBeVisible();
	await expect(me.getByText('7.5', { exact: true })).toBeVisible();
	await expect(me.getByRole('link', { name: 'Practise it again' })).toBeVisible();

	// the home page points at it
	await page.goto('/');
	await expect(page.getByRole('link', { name: /Today’s challenge →/ })).toHaveAttribute(
		'href',
		'/daily'
	);
});
