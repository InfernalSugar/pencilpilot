import { expect, test, type Page, type Route } from '@playwright/test';

// The owner's picture review (/dev/pictures) only exists on the dev server: the
// production build answers 404. On the dev project the ref-service's admin API is
// mocked with page.route, like other tests mock /api/ref.

test('the picture review page does not exist in production', async ({ page }, info) => {
	test.skip(info.project.name !== 'build', 'production build only');
	expect((await page.goto('/dev/pictures'))?.status()).toBe(404);
	expect((await page.goto('/dev'))?.status()).toBe(404);
});

const pic = (id: string, topic: string, extra: Record<string, unknown> = {}) => ({
	id,
	imageUrl: `/api/admin/img/${id}.jpg`,
	category: 'anime',
	topic,
	tier: 2,
	source: 'pixabay',
	provider: 'Pixabay',
	kind: 'illustration',
	title: `Test ${id}`,
	tags: ['anime'],
	credit: {
		author: 'Ann',
		authorUrl: '',
		license: 'Pixabay Content License',
		licenseUrl: '',
		pageUrl: '',
		attribution: ''
	},
	sorted: true,
	blocked: false,
	addedAt: 1,
	reports: { count: 0, reasons: [] },
	review: null,
	moveTo: ['anime', 'cartoon', 'fundamentals'],
	...extra
});

async function mockAdmin(page: Page) {
	const reviews: Record<string, unknown>[] = [];
	const undos: Record<string, unknown>[] = [];
	let seq = 0;
	const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, json: body });
	await page.route('**/api/admin/pictures?*', (r) =>
		json(r, {
			total: 3,
			page: 1,
			per: 48,
			items: [
				pic('px-1', 'characters'),
				pic('px-2', 'characters', {
					reports: { count: 2, reasons: ['not anime', 'scenery'] },
					blocked: true
				}),
				pic('px-3', 'scenery')
			]
		})
	);
	await page.route('**/api/admin/stats', (r) =>
		json(r, {
			topics: [
				{
					category: 'anime',
					topic: 'characters',
					pictures: 2,
					reviewed: reviews.length,
					kept: reviews.filter((x) => x.decision === 'keep').length,
					moved: 0,
					rejected: 0
				}
			]
		})
	);
	await page.route('**/api/admin/review', async (r) => {
		const body = r.request().postDataJSON();
		if (body.id === 'px-3') return json(r, { error: 'disk full' }, 500);
		reviews.push(body);
		seq++;
		return json(r, { ok: true, seq, decision: body.decision, picture: null });
	});
	await page.route('**/api/admin/undo', async (r) => {
		undos.push(r.request().postDataJSON());
		return json(r, { ok: true, undone: { seq }, picture: null });
	});
	await page.route('**/api/admin/img/*', (r) =>
		r.fulfill({
			contentType: 'image/svg+xml',
			body: '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30" fill="#3d6bff"/></svg>'
		})
	);
	return { reviews, undos };
}

test('review pictures with the keyboard: keep, move, reject, undo', async ({ page }, info) => {
	test.skip(info.project.name !== 'dev', 'dev pages 404 in the production build');
	const api = await mockAdmin(page);
	await page.goto('/dev/pictures');
	const cards = page.getByRole('list', { name: 'Pictures' }).getByRole('listitem');
	await expect(cards).toHaveCount(3);
	await expect(page.getByText(/Reported 2×: not anime; scenery/)).toBeVisible();

	// focus mode: one picture at a time, topic shown big
	await page.keyboard.press('f');
	const focus = page.getByRole('region', { name: 'Focus review' });
	await expect(focus.getByRole('heading', { name: /Characters/ })).toBeVisible();
	await expect(focus.getByText('1 of 3')).toBeVisible();

	await page.keyboard.press('k'); // keep px-1, on to px-2
	await expect(focus.getByText(/2 of 3/)).toBeVisible();
	await expect.poll(() => api.reviews.length).toBe(1);
	expect(api.reviews[0]).toEqual({ id: 'px-1', decision: 'keep' });

	// M opens the topic picker; typing filters, Enter picks
	await page.keyboard.press('m');
	const picker = page.getByRole('dialog', { name: 'Move to…' });
	await expect(picker).toBeVisible();
	await picker.getByLabel(/Filter topics/).fill('scen');
	await page.keyboard.press('Enter');
	await expect(picker).toBeHidden();
	await expect.poll(() => api.reviews.length).toBe(2);
	expect(api.reviews[1]).toEqual({
		id: 'px-2',
		decision: 'move',
		category: 'anime',
		topic: 'scenery'
	});

	// a failed save is rolled back and explained
	await expect(focus.getByText(/3 of 3/)).toBeVisible();
	await page.keyboard.press('x');
	await expect(page.getByRole('status')).toContainText('Not saved: reject px-3: disk full');
	await expect(focus.getByText(/Not reviewed/)).toBeVisible();

	// Z takes back the last saved decision (the move)
	await page.keyboard.press('z');
	await expect.poll(() => api.undos.length).toBe(1);
	expect(api.undos[0]).toEqual({ seq: 2 });
	await expect(focus.getByText(/2 of 3 · Reported/)).toBeVisible();

	// back to the grid: buttons work too, and the stats strip shows the kept share
	await page.keyboard.press('Escape');
	await cards.nth(0).getByRole('button', { name: 'Reject' }).click();
	await expect.poll(() => api.reviews.length).toBe(3);
	expect(api.reviews[2]).toEqual({ id: 'px-1', decision: 'reject' });
	await expect(page.getByText(/% kept/).first()).toBeVisible();
});

test('says so when the review API is off', async ({ page }, info) => {
	test.skip(info.project.name !== 'dev', 'dev pages 404 in the production build');
	await page.route('**/api/admin/**', (r) => r.fulfill({ status: 404, body: '' }));
	await page.goto('/dev/pictures');
	await expect(page.getByRole('alert')).toContainText('REF_ADMIN=1');
});
