import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Browser, type Page } from '@playwright/test';

const REF = {
	id: 'live-e2e-1',
	live: true,
	category: 'animals',
	imageUrl: '/refimg/live-e2e-1.jpg',
	pageUrl: 'https://pixabay.com/photos/cat-1/',
	authorUrl: 'https://pixabay.com/users/someone-1/',
	author: 'someone',
	tags: ['cat'],
	title: 'Cat'
};
// a real 1×1 PNG; `extra` bytes after it let the fake safety check refuse one
const PNG_1PX =
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR4nGP4DwABAQEAvZQfbQAAAABJRU5ErkJggg==';
const png = (extra = '') =>
	'data:image/png;base64,' +
	Buffer.concat([Buffer.from(PNG_1PX, 'base64'), Buffer.from(extra)]).toString('base64');

async function newUser(browser: Browser): Promise<Page> {
	const page = await (await browser.newContext()).newPage();
	await page.goto('/signup');
	await page
		.getByLabel('Email')
		.fill(`e2e-c-${Date.now()}-${Math.floor(Math.random() * 1e6)}@example.com`);
	await page.getByLabel('Password').fill('a good long password');
	await page.getByRole('button', { name: 'Create account' }).click();
	await expect(page).toHaveURL(/\/account/);
	return page;
}

const shareAs = (page: Page, body: object) =>
	page.request.post('/api/community/share', { data: body });

test('share, compare, rate, see results, delete', async ({ browser }) => {
	const ann = await newUser(browser);
	const bob = await newUser(browser);
	const cat = await newUser(browser);

	// guests can't share; the safety check refuses flagged drawings
	const guest = await (await browser.newContext()).newPage();
	expect((await shareAs(guest, { png: png(), reference: REF })).status()).toBe(401);
	const refused = await shareAs(ann, { png: png('NSFWTEST'), reference: REF });
	expect(refused.status()).toBe(422);
	expect(
		(
			await shareAs(ann, { png: png(), reference: { ...REF, imageUrl: 'https://x.test/a.jpg' } })
		).status()
	).toBe(400);

	expect((await shareAs(ann, { png: png(), reference: REF, score: 6.5, attemptAt: 1 })).ok()).toBe(
		true
	);
	expect((await shareAs(bob, { png: png(), reference: REF, score: 4, attemptAt: 2 })).ok()).toBe(
		true
	);

	// Ann only has Bob's drawing to see, so nothing to compare
	await ann.goto('/community');
	await expect(ann.getByRole('heading', { name: 'You’re all caught up' })).toBeVisible();

	// the signed-in pages with real content pass the accessibility audit too
	const axe = async (p: Page) =>
		(
			await new AxeBuilder({ page: p })
				.withTags(['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'])
				.analyze()
		).violations
			.filter((v) => v.impact === 'serious' || v.impact === 'critical')
			.map((v) => v.id);

	// Cat compares the two, then has nothing left
	await cat.goto('/community');
	await expect(cat.getByRole('link', { name: 'Community', exact: true })).toBeVisible();
	await expect(cat.getByText('Which drawing is closer to the reference?')).toBeVisible();
	await expect(cat.getByRole('img', { name: 'Drawing A' })).toBeVisible();
	expect(await axe(cat)).toEqual([]);
	await cat.getByRole('button', { name: 'A is closer' }).click();
	await expect(cat.getByRole('heading', { name: 'You’re all caught up' })).toBeVisible();

	// …and rates both on the rubric
	await cat.getByRole('link', { name: 'Rate a drawing' }).click();
	await expect(cat.locator('form.rubric')).toBeVisible();
	expect(await axe(cat)).toEqual([]);
	for (let n = 0; n < 2; n++) {
		const form = cat.locator('form.rubric');
		for (const legend of ['Proportions', 'Lines', 'Light & dark', 'Overall'])
			await form
				.getByRole('radiogroup', { name: new RegExp(legend) })
				.getByRole('radio', { name: '4' })
				.check({ timeout: 5000 });
		await cat.getByRole('button', { name: 'Send rating' }).click();
		if (n === 0) await expect(form.getByRole('radio', { checked: true })).toHaveCount(0);
	}
	await expect(cat.getByRole('heading', { name: 'You’re all caught up' })).toBeVisible();

	// Ann sees how hers did, then deletes it
	await ann.goto('/community/mine');
	const card = ann.getByRole('article');
	await expect(card).toHaveCount(1);
	await expect(card).toContainText('6.5');
	expect(await axe(ann)).toEqual([]);
	await expect(card).toContainText('of 1 vote');
	await expect(card.getByText('4.0').first()).toBeVisible();
	await card.getByRole('button', { name: 'Delete' }).click();
	await card.getByRole('button', { name: 'Delete for good' }).click();
	await expect(ann.getByRole('heading', { name: 'Nothing shared yet' })).toBeVisible();

	// shared drawings are signed-in only
	const res = await guest.request.get('/community/img/whatever');
	expect(res.status()).toBe(404);
});
