import { expect, test } from '@playwright/test';

const unique = () => `e2e-${Date.now()}-${Math.floor(Math.random() * 1e6)}@example.com`;

test('sign up keeps guest progress, sign out, sign back in on a "new device"', async ({
	page,
	browser
}) => {
	const now = Date.now();
	const guest = [1, 2, 3].map((i) => ({
		category: 'animals',
		topic: 'pets',
		tier: 1,
		score: 5 + i,
		at: now - i * 60_000,
		title: `Guest cat ${i}`,
		fixes: ['too-big']
	}));
	await page.addInitScript((h) => {
		if (!localStorage.getItem('pp.attempts'))
			localStorage.setItem('pp.attempts', JSON.stringify(h));
	}, guest);

	const email = unique();
	await page.goto('/signup');
	await page.getByLabel('Email').fill(email);
	await page.getByLabel('Password').fill('a good long password');
	await page.getByRole('button', { name: 'Create account' }).click();
	await expect(page).toHaveURL(/\/account\?welcome=1/);
	await expect(page.getByText('Up to date')).toBeVisible();
	await expect(page.getByRole('link', { name: 'Your account' })).toBeVisible();
	await page.reload();
	await expect(page.getByText('Saved drawings').locator('..')).toContainText('3');

	// a fresh browser (no guest data) signs in and gets the history
	const other = await browser.newPage();
	await other.goto('/login');
	await other.getByLabel('Email').fill(email);
	await other.getByLabel('Password').fill('wrong password');
	await other.getByRole('button', { name: 'Sign in' }).click();
	await expect(other.getByRole('alert')).toContainText('Wrong email or password');
	await other.getByLabel('Password').fill('a good long password');
	await other.getByRole('button', { name: 'Sign in' }).click();
	await expect(other).toHaveURL(/\/progress/);
	await expect(other.getByText('Drawings', { exact: true }).locator('..')).toContainText('3');

	// sign out
	await other.goto('/account');
	await other.getByRole('button', { name: 'Sign out' }).click();
	await expect(other.getByRole('link', { name: 'Sign in' })).toBeVisible();
	await other.close();
});

test('sign-up validation and account deletion', async ({ page }) => {
	await page.goto('/signup');
	await page.getByLabel('Email').fill(unique());
	await page.getByLabel('Password').fill('password1');
	await page.getByRole('button', { name: 'Create account' }).click();
	await expect(page.getByRole('alert')).toContainText('too common');
	await page.getByLabel('Password').fill('another good password');
	await page.getByRole('button', { name: 'Create account' }).click();
	await expect(page).toHaveURL(/\/account/);
	const del = page.getByRole('button', { name: 'Delete forever' });
	await expect(del).toBeDisabled();
	await page.getByLabel('Type DELETE to confirm').fill('DELETE');
	await del.click();
	await expect(page).toHaveURL(/\/\?deleted=1/);
	await page.goto('/account');
	await expect(page).toHaveURL(/\/login/);
});

test('progress API needs a session, and exports require sign-in', async ({ request }) => {
	expect((await request.get('/api/progress')).status()).toBe(401);
	expect((await request.get('/account/export')).status()).toBe(401);
});
