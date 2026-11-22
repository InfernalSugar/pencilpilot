import { expect, test } from '@playwright/test';
import { createHash } from 'node:crypto';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';

// Linking the desktop app for Draw together (ADR-0013): the app opens /app/link, the person
// signs in and confirms, the browser goes back to the app's loopback address with a code,
// and the app trades it for a token, then asks for passes. The app's side is played here:
// the test listens on the loopback address, as the app does.

const unique = () => `e2e-link-${Date.now()}-${Math.floor(Math.random() * 1e6)}@example.com`;
const VERIFIER = 'the-app-s-own-secret-0123456789-abcdefghijklmnop';
const ENDPOINT = 'cd'.repeat(32);

test('the app links to an account, gets passes, and can be unlinked', async ({ page }) => {
	// the app, waiting for the browser to come back
	let back: URL | null = null;
	const app = createServer((req, res) => {
		back = new URL(req.url ?? '/', 'http://127.0.0.1');
		res.end('the app got it');
	});
	await new Promise<void>((done) => app.listen(0, '127.0.0.1', done));
	const REDIRECT = `http://127.0.0.1:${(app.address() as AddressInfo).port}/linked`;
	const challenge = createHash('sha256').update(VERIFIER).digest('base64url');
	const state = 'state-from-the-app-0123';
	const link = `/app/link?${new URLSearchParams({
		redirect_uri: REDIRECT,
		state,
		code_challenge: challenge,
		code_challenge_method: 'S256',
		label: 'PencilPilot on Linux'
	})}`;

	// signed out: sign up first, then back to the link page
	await page.goto(link);
	await expect(page).toHaveURL(/\/login\?next=/);
	await page.getByRole('link', { name: 'Create a free account' }).click();
	await expect(page.getByRole('heading', { name: 'Save your progress' })).toBeVisible();
	await page.getByLabel('Email').fill(unique());
	await page.getByLabel('Password').fill('a good long password');
	await page.getByRole('button', { name: 'Create account' }).click();
	await expect(page.getByRole('heading', { name: 'Link this app?' })).toBeVisible();
	await expect(page.getByText('PencilPilot on Linux')).toBeVisible();

	// the browser heads back to the app with a one-time code
	await page.getByLabel('Your name when drawing together').fill('Ana');
	await page.getByRole('button', { name: 'Link this PencilPilot app' }).click();
	await expect(page.getByText('the app got it')).toBeVisible();
	const answer = back as URL | null;
	app.close();
	expect(answer?.pathname).toBe('/linked');
	expect(answer?.searchParams.get('state')).toBe(state);
	const code = answer?.searchParams.get('code') ?? '';
	expect(code.length).toBeGreaterThan(20);

	// the app trades it (with its secret) for a token, once
	const req = page.request;
	const wrong = await req.post('/api/app/token', {
		data: { code: 'not-the-code', code_verifier: VERIFIER, redirect_uri: REDIRECT }
	});
	expect(wrong.status()).toBe(400);
	const got = await req.post('/api/app/token', {
		data: { code, code_verifier: VERIFIER, redirect_uri: REDIRECT }
	});
	expect(got.ok()).toBe(true);
	const { token, user } = await got.json();
	expect(user.name).toBe('Ana');
	const again = await req.post('/api/app/token', {
		data: { code, code_verifier: VERIFIER, redirect_uri: REDIRECT }
	});
	expect(again.status()).toBe(400);

	// a pass for this computer: the name, never the email
	const pass = await req.post('/api/app/pass', {
		headers: { Authorization: `Bearer ${token}` },
		data: { endpoint: ENDPOINT }
	});
	expect(pass.ok()).toBe(true);
	const p = await pass.json();
	const body = JSON.parse(p.pass.body);
	expect(body).toMatchObject({ v: 1, user: user.id, name: 'Ana', endpoint: ENDPOINT });
	expect(p.pass.body).not.toContain('@');
	expect(p.pass.sig).toMatch(/^[0-9a-f]{128}$/);
	const none = await req.post('/api/app/pass', { data: { endpoint: ENDPOINT } });
	expect(none.status()).toBe(401);

	// the Account page lists it; unlinking ends its passes
	await page.goto('/account');
	await expect(page.getByText('PencilPilot on Linux')).toBeVisible();
	await page.getByRole('button', { name: 'Unlink' }).click();
	await expect(page.getByRole('status')).toContainText('Unlinked');
	const after = await req.post('/api/app/pass', {
		headers: { Authorization: `Bearer ${token}` },
		data: { endpoint: ENDPOINT }
	});
	expect(after.status()).toBe(401);
});

test('a link page that didn’t come from the app sends nothing anywhere', async ({ page }) => {
	await page.goto('/app/link?redirect_uri=https%3A%2F%2Fevil.example%2Fx&state=x');
	await expect(page.getByRole('alert')).toContainText('didn’t come from the PencilPilot app');
});
