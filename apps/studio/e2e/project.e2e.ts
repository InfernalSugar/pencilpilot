import { expect, test, type Page } from '@playwright/test';

// Projects through the window: save, open, autosave / recovery, versions, export.
// Tauri's IPC is stubbed: the "file" the window saves is posted to /__keep (the test
// keeps it), and opening, recovering or opening a version hands it back from /__packet.
// The native dialogs answer with fixed paths. (src-tauri tests the files themselves.)

interface Stub {
	/** what recovery_list / versions_list / recent_list answer */
	recovery?: unknown[];
	versions?: unknown[];
	recent?: unknown[];
	/** the answer to "lose unsaved changes?" */
	askAnswer?: boolean;
}

async function stubTauri(page: Page, stub: Stub = {}) {
	let kept: Buffer | null = null;
	await page.route('**/__keep', async (r) => {
		kept = r.request().postDataBuffer();
		await r.fulfill({ body: 'ok' });
	});
	await page.route('**/__packet', (r) =>
		kept ? r.fulfill({ body: kept }) : r.fulfill({ status: 404 })
	);
	await page.addInitScript((stub: Stub) => {
		const w = window as unknown as Record<string, unknown>;
		const calls: { cmd: string; path?: string; bytes?: number; head?: number[]; json?: string }[] =
			[];
		w.__calls = calls;
		w.isTauri = true;
		const packet = async () => (await fetch('/__packet')).arrayBuffer();
		w.__TAURI_INTERNALS__ = {
			transformCallback: () => 0,
			unregisterCallback: () => {},
			metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
			invoke: async (
				cmd: string,
				args: unknown,
				options?: { headers?: Record<string, string> }
			) => {
				const path = decodeURIComponent(options?.headers?.path ?? '');
				const bytes = args instanceof Uint8Array ? args : null;
				calls.push({
					cmd,
					path,
					bytes: bytes?.length,
					head: bytes ? [...bytes.subarray(0, 4)] : undefined,
					json: bytes ? undefined : JSON.stringify(args ?? null)
				});
				switch (cmd) {
					case 'plugin:dialog|save': {
						const json = JSON.stringify(args);
						const ext = json.includes('"png"')
							? 'png'
							: json.includes('"psd"')
								? 'psd'
								: 'ppstudio';
						return `/home/me/Walk cycle.${ext}`;
					}
					case 'plugin:dialog|open':
						return '/home/me/Walk cycle.ppstudio';
					case 'plugin:dialog|message':
						// ask(): the label of the button pressed
						return (stub.askAnswer ?? true) ? 'Lose changes' : 'Keep working';
					case 'project_save':
						await fetch('/__keep', { method: 'POST', body: bytes as BlobPart });
						return path;
					case 'project_open':
					case 'recovery_open':
					case 'version_open':
						return packet();
					case 'recovery_list':
						return stub.recovery ?? [];
					case 'versions_list':
						return stub.versions ?? [];
					case 'recent_list':
						return stub.recent ?? [];
					case 'export_write':
						return path;
					default:
						// recovery writes, events (window close) and anything else
						return null;
				}
			}
		};
	}, stub);
}

const canvas = (page: Page) => page.getByLabel(/Drawing canvas/);
type Call = { cmd: string; path?: string; head?: number[]; json?: string };
const calls = (page: Page) =>
	page.evaluate(() => (window as unknown as { __calls: Call[] }).__calls);

/** dark pixels in rows y0–y1 (fractions of the canvas) */
async function ink(page: Page, y0 = 0, y1 = 1) {
	return canvas(page).evaluate(
		(el: HTMLCanvasElement, [y0, y1]) => {
			const top = Math.round(el.height * y0);
			const h = Math.max(1, Math.round(el.height * (y1 - y0)));
			const d = el.getContext('2d')!.getImageData(0, top, el.width, h).data;
			let n = 0;
			for (let i = 0; i < d.length; i += 4) if (d[i] < 200) n++;
			return n;
		},
		[y0, y1]
	);
}

async function ready(page: Page, url = '/studio') {
	await page.setViewportSize({ width: 1440, height: 960 });
	await page.goto(url);
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
}

async function scribble(page: Page, y: number) {
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.move(b.x + b.width * 0.2, b.y + b.height * y);
	await page.mouse.down();
	await page.mouse.move(b.x + b.width * 0.8, b.y + b.height * (y + 0.05), { steps: 20 });
	await page.mouse.up();
}

/** draw two layers and save them as "Walk cycle"; returns the ink drawn */
async function drawAndSave(page: Page) {
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	await scribble(page, 0.3);
	await page.getByRole('button', { name: 'New layer' }).click();
	await scribble(page, 0.6);
	await expect(page.getByLabel('not saved')).toBeVisible();
	const drawn = await ink(page);
	expect(drawn).toBeGreaterThan(200);
	await page.keyboard.press('Control+s');
	// (the stub posts the whole ~16 MB packet through Playwright: allow it time)
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	await expect(page.getByLabel('not saved')).toHaveCount(0);
	return drawn;
}

/** the saved drawing is back (8-bit colour in files rounds faint edges a little) */
async function expectSavedDrawing(page: Page, drawn: number) {
	// (a fresh canvas is blank for a moment: wait for the drawing itself)
	await expect.poll(async () => Math.abs((await ink(page)) - drawn) / drawn).toBeLessThan(0.02);
	expect(await ink(page, 0.8, 1)).toBe(0);
	await expect(page.getByRole('tab', { name: /Layers/ })).toContainText('2 layers');
}

test('save, change, open again (asking before losing the change)', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	await expect(page.getByTestId('project-name')).toHaveText('Untitled');
	const drawn = await drawAndSave(page);
	await scribble(page, 0.85);
	await expect(page.getByLabel('not saved')).toBeVisible();
	await page.keyboard.press('Control+o');
	await expect(page.getByLabel('not saved')).toHaveCount(0);
	await expectSavedDrawing(page, drawn);
	expect((await calls(page)).map((c) => c.cmd)).toEqual(
		expect.arrayContaining([
			'plugin:dialog|save',
			'project_save',
			'recovery_discard',
			'plugin:dialog|message',
			'plugin:dialog|open',
			'project_open'
		])
	);
});

test('keeping your changes stops the open', async ({ page }) => {
	await stubTauri(page, { askAnswer: false });
	await ready(page);
	await scribble(page, 0.5);
	await page.keyboard.press('Control+o');
	await expect
		.poll(async () => (await calls(page)).map((c) => c.cmd))
		.toContain('plugin:dialog|message');
	expect((await calls(page)).map((c) => c.cmd)).not.toContain('plugin:dialog|open');
	await expect(page.getByLabel('not saved')).toBeVisible();
});

test('unsaved work is autosaved as a recovery copy every 20 seconds', async ({ page }) => {
	await page.clock.install();
	await stubTauri(page);
	await ready(page);
	await scribble(page, 0.5);
	const writes = async () => (await calls(page)).filter((c) => c.cmd === 'recovery_write');
	await page.clock.runFor(21_000);
	await expect.poll(writes).toHaveLength(1);
	expect((await writes())[0].head).toEqual([0x50, 0x50, 0x53, 0x31]); // a project packet
	// nothing new since: no new copy
	await page.clock.runFor(21_000);
	expect(await writes()).toHaveLength(1);
});

test('work left behind is offered back on the home screen', async ({ page }) => {
	await stubTauri(page, {
		recovery: [
			{
				id: 'doc-1',
				path: '/home/me/Walk cycle.ppstudio',
				savedAt: '2026-11-21T00:00:00Z',
				width: 1920,
				height: 1080,
				thumbnail: null
			}
		]
	});
	// a drawing to "recover": draw it and save it, then go home
	await ready(page);
	const drawn = await drawAndSave(page);
	await page.goto('/');
	const box = page.getByRole('region', { name: 'Unsaved work was found' });
	await expect(box).toContainText('Walk cycle');
	await box.getByRole('button', { name: 'Recover' }).click();
	await expect(page).toHaveURL(/\/studio\?recover=doc-1/);
	await expectSavedDrawing(page, drawn);
	await expect(page.getByLabel('not saved')).toBeVisible();
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle');
	expect((await calls(page)).map((c) => c.cmd)).toContain('recovery_open');
});

test('an earlier version can be opened', async ({ page }) => {
	await stubTauri(page, {
		versions: [{ id: 'v1', savedAt: new Date().toISOString(), bytes: 48_000, thumbnail: null }]
	});
	await ready(page);
	const drawn = await drawAndSave(page);
	await page.getByRole('button', { name: 'Versions' }).click();
	const menu = page.getByRole('group', { name: 'Versions' });
	await expect(menu).toContainText('just now');
	await expect(menu).toContainText('47 KB');
	await scribble(page, 0.85);
	await menu.getByRole('button', { name: 'Open' }).click();
	// opening a version over unsaved work asks first; it opens as unsaved work
	await expect.poll(async () => (await calls(page)).map((c) => c.cmd)).toContain('version_open');
	await expectSavedDrawing(page, drawn);
	await expect(page.getByLabel('not saved')).toBeVisible();
});

test('export a PNG and a layered PSD', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	await scribble(page, 0.5);
	for (const [label, magic, ext] of [
		['PNG picture', [0x89, 0x50, 0x4e, 0x47], 'png'],
		['Photoshop file', [0x38, 0x42, 0x50, 0x53], 'psd']
	] as const) {
		await page.getByRole('button', { name: 'Export' }).click();
		await page.getByRole('button', { name: new RegExp(label) }).click();
		const last = async () => (await calls(page)).filter((c) => c.cmd === 'export_write').at(-1);
		await expect.poll(async () => (await last())?.path).toBe(`/home/me/Walk cycle.${ext}`);
		expect((await last())!.head).toEqual([...magic]);
	}
});

test('home: templates and custom sizes start a new canvas', async ({ page }) => {
	await stubTauri(page);
	await page.setViewportSize({ width: 1440, height: 960 });
	await page.goto('/');
	await page.getByRole('button', { name: /Square/ }).click();
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await expect(canvas(page)).toHaveAttribute('aria-label', /1080 by 1080/);
	await expect(page.getByTestId('project-name')).toHaveText('Untitled');
	// back home (nothing drawn: no question), then a custom size
	await page.getByRole('link', { name: 'Back' }).click();
	await page.getByRole('button', { name: /Custom/ }).click();
	await page.getByLabel('Width').fill('10');
	await expect(page.getByRole('button', { name: 'Create' })).toBeDisabled();
	await expect(page.getByRole('alert')).toContainText('64–8192');
	await page.getByLabel('Width').fill('640');
	await page.getByLabel('Height').fill('480');
	await page.getByRole('button', { name: 'Create' }).click();
	await expect(canvas(page)).toHaveAttribute('aria-label', /640 by 480/);
});

test('home: recent projects open; missing ones are marked and can be removed', async ({ page }) => {
	await stubTauri(page, {
		recent: [
			{
				path: '/home/me/Walk cycle.ppstudio',
				found: true,
				width: 1920,
				height: 1080,
				fps: 24,
				layers: 2,
				modified: new Date().toISOString(),
				thumbnail: null
			},
			{
				path: '/home/me/Old.ppstudio',
				found: false,
				width: 0,
				height: 0,
				fps: 0,
				layers: 0,
				modified: '',
				thumbnail: null
			}
		]
	});
	// something saved, so opening hands it back
	await ready(page);
	const drawn = await drawAndSave(page);
	await page.goto('/');
	const recent = page.getByRole('region', { name: 'Recent' });
	await expect(recent).toContainText('1920 × 1080 · 2 layers · just now');
	await expect(recent).toContainText('Moved or deleted');
	await expect(recent.getByRole('button', { name: /^Old/ })).toBeDisabled();
	await recent.getByRole('button', { name: 'Remove Old from the list' }).click();
	await expect(recent).not.toContainText('Moved or deleted');
	await expect.poll(async () => (await calls(page)).map((c) => c.cmd)).toContain('recent_remove');
	await recent.getByRole('button', { name: /^Walk cycle/ }).click();
	await expect(page).toHaveURL(/\/studio\?open=/);
	await expectSavedDrawing(page, drawn);
	await expect(page.getByLabel('not saved')).toHaveCount(0);
});

test('leaving the studio with unsaved work asks first', async ({ page }) => {
	await stubTauri(page, { askAnswer: false });
	await ready(page);
	await scribble(page, 0.5);
	await page.getByRole('link', { name: 'Back' }).click();
	await expect
		.poll(async () => (await calls(page)).map((c) => c.cmd))
		.toContain('plugin:dialog|message');
	await expect(page).toHaveURL(/\/studio/);
	await expect(page.getByLabel('not saved')).toBeVisible();
});

test('settings: change a key (a taken key swaps), and it works in the studio', async ({ page }) => {
	await stubTauri(page);
	await page.setViewportSize({ width: 1440, height: 960 });
	await page.goto('/');
	await page.getByRole('link', { name: 'Settings' }).click();
	const brush = page.locator('li[data-action="brush"]');
	const pen = page.locator('li[data-action="pen"]');
	await expect(brush.locator('kbd')).toHaveText('B');
	// Pen takes B: Brush gets Pen's old key, P
	await page.getByRole('button', { name: 'Change the key for Pen path' }).click();
	await expect(pen.locator('kbd')).toHaveText('Press a key…');
	await page.keyboard.press('b');
	await expect(pen.locator('kbd')).toHaveText('B');
	await expect(brush.locator('kbd')).toHaveText('P');
	await expect(page.getByRole('status').filter({ hasText: 'Brush now uses P.' })).toBeVisible();
	// and Undo on its own key
	await page.getByRole('button', { name: 'Change the key for Undo' }).click();
	await page.keyboard.press('Control+Alt+u');
	await expect(page.locator('li[data-action="undo"] kbd')).toHaveText('Ctrl+Alt+U');
	// kept through the app's settings file
	await expect
		.poll(async () => {
			const last = (await calls(page)).filter((c) => c.cmd === 'settings_set').at(-1);
			return last ? JSON.parse(last.json!).settings.keys : null;
		})
		.toMatchObject({ pen: 'B', brush: 'P', undo: 'Ctrl+Alt+U' });

	// in the studio: B is the pen path, P is the brush, Ctrl+Alt+U undoes
	await page.getByRole('link', { name: 'Back to home' }).click();
	await page.getByRole('button', { name: /HD video/ }).click();
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await page.keyboard.press('b');
	// (the tool buttons show the keys chosen)
	await expect(page.getByRole('button', { name: 'Pen path (B)' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await page.keyboard.press('p');
	await expect(page.getByRole('button', { name: 'Brush (P)' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await scribble(page, 0.5);
	await expect.poll(() => ink(page)).toBeGreaterThan(50);
	await page.keyboard.press('Control+Alt+u');
	await expect.poll(() => ink(page)).toBe(0);
	// the old undo key does nothing now
	await scribble(page, 0.5);
	await page.keyboard.press('Control+z');
	await expect.poll(() => ink(page)).toBeGreaterThan(50);
});

test('settings: touch and file choices are kept', async ({ page }) => {
	await stubTauri(page);
	await page.setViewportSize({ width: 1440, height: 960 });
	await page.goto('/settings');
	await page.getByLabel('Always moves the view (pen only for drawing)').check();
	await page.getByLabel('Two-finger tap undoes, three-finger tap redoes').uncheck();
	await page.getByLabel('Keep a recovery copy of unsaved work').selectOption('60');
	await page.getByLabel('Earlier versions kept per project').fill('5');
	await expect
		.poll(async () => {
			const last = (await calls(page)).filter((c) => c.cmd === 'settings_set').at(-1);
			return last ? JSON.parse(last.json!).settings : null;
		})
		.toMatchObject({ fingers: 'move', tapGestures: false, autosaveSeconds: 60, versionsKept: 5 });
	await page.getByRole('button', { name: 'Reset all' }).click();
	await expect(page.getByLabel('Draws until you use a pen, then moves the view')).toBeChecked();
});

test('studio layout: each tool shows its options, as on the website', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const opts = (name: string) => page.getByRole('group', { name });
	// fill: tolerance and where it looks
	await page.keyboard.press('g');
	await expect(opts('Fill options')).toBeVisible();
	await expect(opts('Fill options').getByText(/Tolerance/)).toBeVisible();
	// gradient (its own key), transform, liquify, a selection tool
	await page.keyboard.press('Shift+g');
	await expect(opts('Gradient')).toBeVisible();
	await expect(opts('Fill options')).toHaveCount(0);
	await page.getByRole('button', { name: /^Liquify/ }).click();
	await expect(opts('Liquify')).toBeVisible();
	await page.keyboard.press('m');
	await expect(opts('Selection')).toBeVisible();
	await page.keyboard.press('Control+a');
	await expect(opts('Selection').getByRole('button', { name: 'Invert' })).toBeVisible();
	await page.keyboard.press('Escape');
	await page.keyboard.press('m');
	await expect(opts('Selection')).toHaveCount(0);

	// guides: a grid, kept for next time
	await page.getByRole('button', { name: 'Guides' }).click();
	await opts('Guides').getByRole('radio', { name: 'Square' }).click();
	await expect(page.getByRole('button', { name: 'Guides' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await ready(page);
	await expect(page.getByRole('button', { name: 'Guides' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);

	// adjust the layer: a scribble turned light, then Cancel puts it back
	await scribble(page, 0.5);
	// (wait for the stroke's last bit to show)
	let drawn = -1;
	await expect
		.poll(async () => {
			const before = drawn;
			drawn = await ink(page);
			return drawn === before && drawn > 0;
		})
		.toBe(true);
	await page.keyboard.press('Control+u');
	const adjust = opts('Adjust layer');
	await expect(adjust).toBeVisible();
	await adjust.getByRole('slider', { name: /Brightness/ }).fill('100');
	await expect.poll(() => ink(page)).toBeLessThan(drawn / 2);
	await adjust.getByRole('button', { name: 'Cancel' }).click();
	await expect.poll(() => ink(page)).toBe(drawn);

	// right-click: the quick menu
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2, { button: 'right' });
	await page.getByRole('menuitem', { name: 'Eraser' }).click();
	await expect(page.getByRole('button', { name: 'Eraser (E)' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
});

test('timeline: drawings on frames, timing, undo across frames, saved and opened', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	const frameNo = tl.getByTestId('frame');
	const top = () => ink(page, 0, 0.45);
	const bottom = () => ink(page, 0.5, 1);
	// frame 1: a line at the top
	await scribble(page, 0.3);
	await expect.poll(top).toBeGreaterThan(50);
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1$/ })).toBeVisible();
	// a new drawing: frame 2, empty, then a line at the bottom
	await tl.getByRole('button', { name: 'New drawing' }).click();
	await expect(frameNo).toHaveText('2');
	await expect.poll(top).toBe(0);
	await scribble(page, 0.7);
	await expect.poll(bottom).toBeGreaterThan(50);
	// back to frame 1: its own line
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();
	await expect.poll(top).toBeGreaterThan(50);
	expect(await bottom()).toBe(0);
	// on twos: drawing 1 shows on frames 1–2, drawing 2 moves to frame 3
	await tl.getByRole('button', { name: '2s' }).click();
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1–2/ })).toBeVisible();
	await expect(tl.getByRole('button', { name: /drawing 2, frame 3$/ })).toBeVisible();
	await page.keyboard.press('.');
	await expect(frameNo).toHaveText('2');
	expect(await top()).toBeGreaterThan(50);
	// undo from frame 1's side: the last stroke was drawing 2's, so it goes there first
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();
	await page.keyboard.press('Control+z');
	await expect(frameNo).toHaveText('3');
	await expect.poll(bottom).toBe(0);
	await page.keyboard.press('Control+Shift+z');
	await expect.poll(bottom).toBeGreaterThan(50);

	// saved and opened again: the frames and their timing come back
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	const saved = (await calls(page)).filter((c) => c.cmd === 'project_save').at(-1)!;
	expect(saved.head).toEqual([0x50, 0x50, 0x53, 0x31]);
	await page.keyboard.press('Control+o');
	await expect(tl.getByRole('button', { name: /drawing 2, frame 3$/ })).toBeVisible({
		timeout: 20_000
	});
	await expect(frameNo).toHaveText('3');
	await expect.poll(bottom).toBeGreaterThan(50);
	expect(await top()).toBe(0);
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();
	await expect.poll(top).toBeGreaterThan(50);
});

test('timeline: drag a drawing to move it, or its edge to hold it longer', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	await scribble(page, 0.3);
	await tl.getByRole('button', { name: 'New drawing' }).click();
	await scribble(page, 0.7);
	const second = tl.getByRole('button', { name: /drawing 2, frame 2$/ });
	await expect(second).toBeVisible();
	// its right edge, two frames on: held for 3
	const box = (await second.boundingBox())!;
	await page.mouse.move(box.x + box.width - 3, box.y + box.height / 2);
	await page.mouse.down();
	await page.mouse.move(box.x + box.width - 3 + 44, box.y + box.height / 2, { steps: 6 });
	await page.mouse.up();
	await expect(tl.getByRole('button', { name: /drawing 2, frame 2–4/ })).toBeVisible();
	// drag drawing 1 after it: now drawing 2 comes first
	const first = tl.getByRole('button', { name: /drawing 1, frame 1$/ });
	const b1 = (await first.boundingBox())!;
	await page.mouse.move(b1.x + 8, b1.y + b1.height / 2);
	await page.mouse.down();
	await page.mouse.move(b1.x + 8 + 22 * 4, b1.y + b1.height / 2, { steps: 8 });
	await page.mouse.up();
	// (drawings are numbered in order: the moved one is now "2")
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1–3/ })).toBeVisible();
	await expect(tl.getByRole('button', { name: /drawing 2, frame 4$/ })).toBeVisible();
	await expect(tl.getByTestId('frame')).toHaveText('4');
	await expect.poll(() => ink(page, 0, 0.45)).toBeGreaterThan(50);
});
