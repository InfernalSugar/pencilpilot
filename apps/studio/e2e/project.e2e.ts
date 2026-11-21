import { expect, test, type Page } from '@playwright/test';
import { deflateSync } from 'node:zlib';

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
	// the frame store lives here, so it outlasts a page (as the app's Rust side does)
	const frameStore = new Map<string, Buffer>();
	await page.route('**/__frame/*', async (r) => {
		const id = r.request().url().split('/').pop()!;
		if (r.request().method() === 'POST') {
			frameStore.set(id, r.request().postDataBuffer() ?? Buffer.alloc(0));
			return r.fulfill({ body: 'ok' });
		}
		const b = frameStore.get(id);
		return b ? r.fulfill({ body: b }) : r.fulfill({ status: 404 });
	});
	await page.addInitScript((stub: Stub) => {
		const w = window as unknown as Record<string, unknown>;
		const calls: {
			cmd: string;
			path?: string;
			bytes?: number;
			head?: number[];
			json?: string;
			repeat?: string;
		}[] = [];
		w.__calls = calls;
		w.isTauri = true;
		const packet = async () => (await fetch('/__packet')).arrayBuffer();
		// the frame store: drawing id → its tiles as the engine packs them
		const frames = new Map<number, Uint8Array>();
		w.__frames = frames;
		const TILE = 64 * 64 * 4;
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
					json: bytes ? undefined : JSON.stringify(args ?? null),
					repeat: options?.headers?.repeat
				});
				switch (cmd) {
					case 'plugin:dialog|save': {
						const ext = JSON.stringify(args).match(/"extensions":\["(\w+)"/)?.[1] ?? 'ppstudio';
						return `/home/me/Walk cycle.${ext}`;
					}
					case 'plugin:dialog|open':
						return JSON.stringify(args).includes('"mp4"')
							? '/home/me/dance take 2.mp4'
							: '/home/me/Walk cycle.ppstudio';
					case 'video_in_open': {
						// a made-up video: a red frame, then a blue one
						const a = args as { width: number; height: number };
						const colours = [
							[255, 0, 0],
							[0, 0, 255]
						];
						w.__video = colours.map((c) => {
							const f = new Uint8Array(a.width * a.height * 4);
							for (let i = 0; i < f.length; i += 4) f.set([...c, 255], i);
							return f.buffer;
						});
						return { seconds: 0.08, width: 64, height: 36 };
					}
					case 'video_in_frame':
						return (w.__video as ArrayBuffer[]).shift() ?? new ArrayBuffer(0);
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
					case 'export_tools':
						return { ffmpeg: '/usr/bin/ffmpeg', video: ['mp4', 'prores', 'webm'] };
					case 'export_end':
						return '/home/me/Walk cycle.gif';
					case 'lipsync_tools':
						return '/opt/rhubarb/rhubarb';
					case 'lipsync_run': {
						// Rhubarb's answer for half a second (a frame at 24 fps is 1/24 s): rest,
						// wide open, half a frame of closed lips, open
						const f = 1 / 24;
						return [
							{ start: 0, end: 2 * f, value: 'X' },
							{ start: 2 * f, end: 4 * f, value: 'D' },
							{ start: 4 * f, end: 4.5 * f, value: 'A' },
							{ start: 4.5 * f, end: 0.5, value: 'C' }
						];
					}
					case 'frames_put': {
						const b = bytes!;
						const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
						let at = 4;
						for (let i = 0, n = v.getUint32(0, true); i < n; i++) {
							const id = v.getUint32(at, true);
							const end = at + 8 + v.getUint32(at + 4, true) * (8 + TILE);
							frames.set(id, b.slice(at + 4, end));
							await fetch(`/__frame/${id}`, { method: 'POST', body: b.slice(at + 4, end) });
							at = end;
						}
						return null;
					}
					case 'frames_get': {
						for (const id of (args as { ids: number[] }).ids)
							if (!frames.has(id)) {
								const r = await fetch(`/__frame/${id}`);
								if (r.ok) frames.set(id, new Uint8Array(await r.arrayBuffer()));
							}
						const ids = (args as { ids: number[] }).ids.filter((id) => frames.has(id));
						const size = ids.reduce((n, id) => n + 4 + frames.get(id)!.length, 4);
						const out = new Uint8Array(size);
						const v = new DataView(out.buffer);
						v.setUint32(0, ids.length, true);
						let at = 4;
						for (const id of ids) {
							v.setUint32(at, id, true);
							out.set(frames.get(id)!, at + 4);
							at += 4 + frames.get(id)!.length;
						}
						return out.buffer;
					}
					case 'frames_retain': {
						const keep = (args as { keep: number[] }).keep;
						for (const id of [...frames.keys()]) if (!keep.includes(id)) frames.delete(id);
						return null;
					}
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

test('frame store: drawings leave the window and come back exactly', async ({ page }) => {
	await stubTauri(page);
	// keep just one drawing in the engine besides the ones showing
	await page.addInitScript(() => localStorage.setItem('pp.dev.keepDrawings', '1'));
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	const inks: number[] = [];
	for (let i = 0; i < 4; i++) {
		if (i) await tl.getByRole('button', { name: 'New drawing' }).click();
		await scribble(page, 0.15 + i * 0.2);
		let last = -1;
		await expect
			.poll(async () => {
				const before = last;
				last = await ink(page);
				return last === before && last > 0;
			})
			.toBe(true);
		inks.push(last);
	}
	// every frame again: each drawing back from the store, the same as drawn
	for (let i = 0; i < 4; i++) {
		await tl.getByRole('button', { name: `Frame ${i + 1}`, exact: true }).click();
		await expect.poll(() => ink(page, 0.1 + i * 0.2, 0.3 + i * 0.2)).toBeGreaterThan(50);
		await expect
			.poll(async () => Math.abs((await ink(page)) - inks[i]) / inks[i])
			.toBeLessThan(0.02);
	}
	const sent = (await calls(page)).filter((c) => c.cmd === 'frames_put').length;
	const fetched = (await calls(page)).filter((c) => c.cmd === 'frames_get').length;
	expect(sent).toBeGreaterThan(0);
	expect(fetched).toBeGreaterThan(0);
	expect(
		await page.evaluate(
			() => (window as unknown as { __frames: Map<number, unknown> }).__frames.size
		)
	).toBeGreaterThanOrEqual(3);
	// undo from frame 1: the last stroke was frame 4's, whose drawing left the window;
	// it comes back, frame 4 shows, and the stroke goes
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();
	await page.keyboard.press('Control+z');
	await expect(tl.getByTestId('frame')).toHaveText('4');
	await expect.poll(() => ink(page)).toBe(0);
	await page.keyboard.press('Control+Shift+z');
	await expect.poll(async () => Math.abs((await ink(page)) - inks[3]) / inks[3]).toBeLessThan(0.02);
	// saved: the packet names the drawings without their pixels (the store has them)
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	await page.keyboard.press('Control+o');
	await expect(tl.getByRole('button', { name: /drawing 4, frame 4$/ })).toBeVisible({
		timeout: 20_000
	});
	await tl.getByRole('button', { name: 'Frame 2', exact: true }).click();
	await expect.poll(async () => Math.abs((await ink(page)) - inks[1]) / inks[1]).toBeLessThan(0.02);
});

test('playback: plays the cut at its frame rate, loops a range, ping-pongs, scrubs', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	const frameNo = tl.getByTestId('frame');
	// three drawings, each further down
	for (let i = 0; i < 3; i++) {
		if (i) await tl.getByRole('button', { name: 'New drawing' }).click();
		await scribble(page, 0.2 + i * 0.25);
	}
	await tl.getByRole('combobox', { name: 'Frames a second' }).selectOption('6');
	// play once: the player shows each frame in turn, then the canvas is back
	await tl.getByRole('radio', { name: 'Once' }).click();
	await tl.getByRole('button', { name: 'Play' }).click();
	const player = page.getByTestId('player');
	await expect(player).toBeVisible();
	const seen = new Set<string>();
	await expect
		.poll(
			async () => {
				const t = await frameNo.textContent();
				if (t) seen.add(t);
				return (await player.count()) === 0;
			},
			{ intervals: [50], timeout: 5_000 }
		)
		.toBe(true);
	expect([...seen].sort()).toEqual(['1', '2', '3']);
	await expect(frameNo).toHaveText('3');
	// loop frames 2–3, back and forth, then stop with the key
	await tl.getByRole('button', { name: 'Frame 2', exact: true }).click();
	await tl.getByRole('button', { name: 'In', exact: true }).click();
	await tl.getByRole('button', { name: 'Frame 3', exact: true }).click();
	await tl.getByRole('button', { name: 'Out', exact: true }).click();
	await expect(tl.getByRole('button', { name: 'Frames 2–3 ×' })).toBeVisible();
	await tl.getByRole('radio', { name: 'Ping-pong' }).click();
	await page.keyboard.press('Shift+Space');
	await expect(player).toBeVisible();
	const looped = new Set<string>();
	for (let i = 0; i < 12; i++) {
		looped.add((await frameNo.textContent()) ?? '');
		await page.waitForTimeout(80);
	}
	expect([...looped].sort()).toEqual(['2', '3']);
	await page.keyboard.press('Escape');
	await expect(player).toHaveCount(0);
	// the player's picture: a frame rendered small, the line where it belongs
	await page.keyboard.press('Shift+Space');
	await expect(player).toBeVisible();
	// (once its frames are ready; a light pencil line shrunk for the player is pale grey)
	await expect
		.poll(() =>
			player.evaluate((el: HTMLCanvasElement) => {
				if (!el.width || !el.height) return 0;
				const d = el.getContext('2d')!.getImageData(0, 0, el.width, el.height).data;
				let n = 0;
				for (let i = 0; i < d.length; i += 4) if (d[i] < 200) n++;
				return n;
			})
		)
		.toBeGreaterThan(20);
	await tl.getByRole('button', { name: 'Stop' }).click();
	await expect(player).toHaveCount(0);

	// scrub: drag along the frame numbers, the canvas follows
	const ruler = tl.getByRole('group', { name: 'Frames' });
	const r = (await ruler.boundingBox())!;
	await page.mouse.move(r.x + 5, r.y + r.height / 2);
	await page.mouse.down();
	await page.mouse.move(r.x + 22 * 2 + 5, r.y + r.height / 2, { steps: 5 });
	await expect(frameNo).toHaveText('3');
	await page.mouse.move(r.x + 22 + 5, r.y + r.height / 2, { steps: 3 });
	await page.mouse.up();
	await expect(frameNo).toHaveText('2');
	await expect.poll(() => ink(page, 0.4, 0.6)).toBeGreaterThan(50);
	expect(await ink(page, 0, 0.35)).toBe(0);
});

test('onion skin: red before, green after; the light table pins frames in blue', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	const onion = page.getByTestId('onion');
	/** onion pixels of a colour in rows y0–y1 (fractions) */
	const tinted = (hue: 'red' | 'green' | 'blue', y0: number, y1: number) =>
		onion.evaluate(
			(el: HTMLCanvasElement, [hue, y0, y1]) => {
				const top = Math.round(el.height * (y0 as number));
				const h = Math.max(1, Math.round(el.height * ((y1 as number) - (y0 as number))));
				const d = el.getContext('2d')!.getImageData(0, top, el.width, h).data;
				let n = 0;
				for (let i = 0; i < d.length; i += 4) {
					const [r, g, b, a] = [d[i], d[i + 1], d[i + 2], d[i + 3]];
					if (a < 40) continue;
					if (hue === 'red' && r > g + 60 && r > b + 60) n++;
					if (hue === 'green' && g > r + 40 && g > b + 20) n++;
					if (hue === 'blue' && b > r + 60 && b > g + 30) n++;
				}
				return n;
			},
			[hue, y0, y1] as const
		);
	// three drawings: top, middle, bottom
	for (let i = 0; i < 3; i++) {
		if (i) await tl.getByRole('button', { name: 'New drawing' }).click();
		await scribble(page, 0.15 + i * 0.3);
	}
	await expect(onion).toHaveCount(0);
	// on the middle frame, onion on: the top drawing in red, the bottom one in green
	await tl.getByRole('button', { name: 'Frame 2', exact: true }).click();
	await page.keyboard.press('o');
	await expect(tl.getByRole('button', { name: 'Onion', exact: true })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await expect.poll(() => tinted('red', 0.05, 0.3)).toBeGreaterThan(50);
	await expect.poll(() => tinted('green', 0.65, 0.9)).toBeGreaterThan(50);
	expect(await tinted('red', 0.65, 0.9)).toBe(0);
	// it's over the canvas, not in the drawing
	expect(await ink(page, 0.05, 0.3)).toBe(0);
	// two before, on the last frame: both earlier drawings in red
	await tl.getByRole('button', { name: 'Onion skin settings' }).click();
	await page.getByRole('group', { name: 'Onion skin' }).getByRole('spinbutton').first().fill('2');
	await tl.getByRole('button', { name: 'Frame 3', exact: true }).click();
	await expect.poll(() => tinted('red', 0.05, 0.3)).toBeGreaterThan(50);
	await expect.poll(() => tinted('red', 0.35, 0.6)).toBeGreaterThan(50);
	// onion off, frame 1 pinned to the light table: blue, from any frame
	await page.keyboard.press('o');
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();
	await page.keyboard.press('Shift+O');
	await tl.getByRole('button', { name: 'Frame 3', exact: true }).click();
	await expect.poll(() => tinted('blue', 0.05, 0.3)).toBeGreaterThan(50);
	expect(await tinted('red', 0.35, 0.6)).toBe(0);
	await tl.getByRole('button', { name: '1 pinned ×' }).click();
	await expect(onion).toHaveCount(0);
});

test('static layers hold one drawing all cut long; each cut has a camera', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	const frameNo = tl.getByTestId('frame');
	const top = () => ink(page, 0, 0.3);
	const bottom = () => ink(page, 0.6, 1);
	// a background on the first layer, made static
	await scribble(page, 0.15);
	await tl.getByRole('button', { name: /^Static layer:/ }).click();
	await expect(tl.getByRole('button', { name: /^Static layer:/ })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await expect(tl.getByLabel(/: static, all cut long/)).toBeVisible();
	await expect(tl.getByRole('button', { name: 'New drawing' })).toBeDisabled();
	// a character on a new cel layer, two drawings
	await page.getByRole('button', { name: 'New layer' }).click();
	await scribble(page, 0.7);
	await tl.getByRole('button', { name: 'New drawing' }).click();
	await expect(frameNo).toHaveText('2');
	await expect(tl.getByText('of 2')).toBeVisible();
	// frame 2: the background still shows, the character's first drawing doesn't
	await expect.poll(bottom).toBe(0);
	const bg = await top();
	expect(bg).toBeGreaterThan(50);
	// inserting a frame doesn't stretch the background (it has none to stretch)
	await tl.getByRole('button', { name: 'Insert frame' }).click();
	await expect(tl.getByText('of 3')).toBeVisible();
	await tl.getByRole('button', { name: 'Delete frame' }).click();
	await expect(tl.getByText('of 2')).toBeVisible();

	// the camera: a square in the middle, with safe areas
	await tl.getByRole('button', { name: 'Camera settings' }).click();
	const cam = tl.getByRole('group', { name: 'Camera' });
	await cam.getByRole('button', { name: '1:1' }).click();
	const size = await page
		.getByLabel(/Drawing canvas/)
		.evaluate((el: HTMLCanvasElement) => [el.width, el.height]);
	await expect(cam.getByLabel('Camera width')).toHaveValue(String(size[1]));
	await expect(cam.getByLabel('Camera x')).toHaveValue(String((size[0] - size[1]) / 2));
	await expect(page.getByTestId('camera')).toBeVisible();
	await expect(page.getByTestId('camera').locator('rect.safe')).toHaveCount(0);
	await cam.getByLabel(/Safe areas/).check();
	await expect(page.getByTestId('camera').locator('rect.safe')).toHaveCount(2);
	await tl.getByRole('button', { name: 'Camera settings' }).click();
	// the player shows only the camera: square
	await tl.getByRole('radio', { name: 'Once' }).click();
	await tl.getByRole('button', { name: 'Play' }).click();
	const player = page.getByTestId('player');
	await expect(page.getByTestId('camera')).toHaveCount(0);
	await expect(player).toHaveCount(0, { timeout: 5_000 });
	await expect(page.getByTestId('camera')).toBeVisible();

	// saved and opened again: still static, still framed
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	await page.keyboard.press('Control+o');
	await expect(tl.getByLabel(/: static, all cut long/)).toBeVisible({ timeout: 20_000 });
	await expect(page.getByTestId('camera')).toBeVisible();
	await expect.poll(top).toBeGreaterThan(bg * 0.9);
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();
	await expect.poll(bottom).toBeGreaterThan(50);
	await expect.poll(top).toBeGreaterThan(bg * 0.9);
});

test('frame tools: cycle, reverse, duplicate, copy and paste, boil', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	const frameNo = tl.getByTestId('frame');
	const top = () => ink(page, 0, 0.45);
	const bottom = () => ink(page, 0.5, 1);
	const tick = (n: number) => tl.getByRole('button', { name: `Frame ${n}`, exact: true });
	/** where the dark pixels are (to tell two drawings of the same amount of ink apart) */
	const signature = () =>
		canvas(page).evaluate((el: HTMLCanvasElement) => {
			const d = el.getContext('2d')!.getImageData(0, 0, el.width, el.height).data;
			let s = 0;
			for (let i = 0; i < d.length; i += 4) if (d[i] < 200) s = (s * 31 + i) % 1_000_000_007;
			return s;
		});
	// drawing A at the top on frame 1, B at the bottom on frame 2
	await scribble(page, 0.25);
	await tl.getByRole('button', { name: 'New drawing' }).click();
	await scribble(page, 0.75);
	await tl.getByRole('button', { name: 'Frames ▾' }).click();
	const tools = tl.getByRole('group', { name: 'Frame tools' });
	const on = tools.getByTestId('frames-target');

	// cycle frames 1–2 twice more: A B A B A B
	await tick(1).click({ modifiers: ['Shift'] });
	await expect(on).toContainText('On frames 1–2.');
	await tools.getByRole('button', { name: 'Cycle', exact: true }).click();
	await expect(tl.getByText('of 6')).toBeVisible();
	await expect(tl.getByRole('button', { name: /drawing 1, frame 5$/ })).toBeVisible();

	// reverse frames 1–2: B A A B A B (B is now the first drawing)
	await tick(1).click();
	await tick(2).click({ modifiers: ['Shift'] });
	await tools.getByRole('button', { name: 'Reverse' }).click();
	await expect(tl.getByRole('button', { name: /drawing 2, frame 2–3$/ })).toBeVisible();
	await tick(1).click();
	await expect.poll(bottom).toBeGreaterThan(50);
	expect(await top()).toBe(0);

	// duplicate frame 1 (B) as a new drawing, and change the copy: B stays as it was
	await page.keyboard.press('Shift+D');
	await expect(frameNo).toHaveText('2');
	await expect(tl.getByText('of 7')).toBeVisible();
	await expect.poll(bottom).toBeGreaterThan(50);
	await scribble(page, 0.25);
	await expect.poll(top).toBeGreaterThan(50);
	await tick(1).click();
	await expect.poll(top).toBe(0);
	expect(await bottom()).toBeGreaterThan(50);

	// copy frame 2 (the changed copy) and paste it at the end: the same drawing
	await tick(2).click();
	await page.keyboard.press('Alt+C');
	await tick(8).click();
	await page.keyboard.press('Alt+V');
	await expect(tl.getByText('of 8')).toBeVisible();
	await expect(tl.getByRole('button', { name: /drawing 2, frame 8$/ })).toBeVisible();
	await expect.poll(top).toBeGreaterThan(50);

	// boil: hold frame 1 for 4 frames, boil it on twos
	await tick(1).click();
	for (let i = 0; i < 3; i++) await tl.getByRole('button', { name: 'Hold longer' }).click();
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1–4$/ })).toBeVisible();
	await expect(on).toContainText('On frames 1–4 (this drawing)');
	const still = await signature();
	const inkStill = await bottom();
	await tools.getByRole('button', { name: 'Boil', exact: true }).click();
	await expect(tools.getByText(/Boiled: 2 wobbled copies/)).toBeVisible();
	await expect(tl.getByRole('button', { name: /frame 1–2$/ })).toBeVisible();
	await expect(tl.getByRole('button', { name: /frame 3–4$/ })).toBeVisible();
	await tick(3).click();
	await expect.poll(signature).not.toBe(still);
	// the same line, a little moved
	const inkBoiled = await bottom();
	expect(Math.abs(inkBoiled - inkStill) / inkStill).toBeLessThan(0.3);
	expect(await top()).toBe(0);
});

test('X-sheet: drawings written down the page, empty cels, dialogue kept in the file', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	await scribble(page, 0.25);
	await tl.getByRole('button', { name: 'New drawing' }).click();
	await scribble(page, 0.75);
	await page.getByRole('tab', { name: /X.sheet/ }).click();
	const xs = page.getByRole('region', { name: 'Exposure sheet' });
	const cellAt = (f: number) =>
		xs.getByRole('gridcell', { name: new RegExp(`^Layer 1, frame ${f}:`) });
	await expect(cellAt(1)).toHaveAccessibleName('Layer 1, frame 1: drawing 1');
	await expect(cellAt(2)).toHaveAccessibleName('Layer 1, frame 2: drawing 2');
	// write drawing 1 on frame 3: Enter goes on to frame 4
	await cellAt(3).click();
	await page.keyboard.type('1');
	await page.keyboard.press('Enter');
	await expect(cellAt(3)).toHaveAccessibleName('Layer 1, frame 3: drawing 1');
	await expect(tl.getByTestId('frame')).toHaveText('4');
	await expect(tl.getByRole('button', { name: /drawing 1, frame 3$/ })).toBeVisible();
	await expect.poll(() => ink(page, 0, 0.45)).toBe(0);
	// a number that doesn't exist yet is refused, the next one is a new drawing
	await page.keyboard.type('5');
	await page.keyboard.press('Enter');
	await expect(xs.getByRole('status')).toContainText('Type 3 for a new one');
	await page.keyboard.press('Escape');
	await page.keyboard.type('3');
	await page.keyboard.press('Enter');
	await expect(cellAt(4)).toHaveAccessibleName('Layer 1, frame 4: drawing 3');
	// X: an empty cel on frame 2
	await cellAt(2).click();
	await page.keyboard.press('x');
	await expect(cellAt(2)).toHaveAccessibleName('Layer 1, frame 2: empty cel');
	await cellAt(2).click();
	await expect.poll(() => ink(page)).toBe(0);
	// dialogue, typed straight in (the studio's keys don't fire meanwhile)
	await xs.getByRole('gridcell', { name: 'Dialogue, frame 1', exact: true }).click();
	await page.keyboard.type('hello');
	await page.keyboard.press('Enter');
	await expect(xs.getByRole('gridcell', { name: 'Dialogue, frame 1', exact: true })).toHaveText(
		'hello'
	);
	await expect(page.getByLabel(/Drawing canvas/)).toBeVisible();

	// saved and opened again: the notes and the sheet come back
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	await page.keyboard.press('Control+o');
	await page.getByRole('tab', { name: /X.sheet/ }).click();
	await expect(xs.getByRole('gridcell', { name: 'Dialogue, frame 1', exact: true })).toHaveText(
		'hello',
		{
			timeout: 20_000
		}
	);
	// (numbered in order of appearance: with drawing 2 gone, the new one is now 2)
	await expect(cellAt(4)).toHaveAccessibleName('Layer 1, frame 4: drawing 2');
	await expect(cellAt(2)).toHaveAccessibleName('Layer 1, frame 2: empty cel');
});

test('export the animation: held frames sent once, the camera, a smaller size, transparency', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	// A held 2 frames, then B
	await scribble(page, 0.3);
	await tl.getByRole('button', { name: '2s' }).click();
	await tl.getByRole('button', { name: 'New drawing' }).click();
	await scribble(page, 0.6);
	await tl.getByRole('button', { name: '1s' }).click();
	// a square camera
	await tl.getByRole('button', { name: 'Camera settings' }).click();
	await tl.getByRole('group', { name: 'Camera' }).getByRole('button', { name: '1:1' }).click();
	const size = await canvas(page).evaluate((el: HTMLCanvasElement) => el.height);

	await page.getByRole('button', { name: 'Export', exact: true }).click();
	await page.getByRole('button', { name: /^Animation…/ }).click();
	const dlg = page.getByRole('dialog', { name: 'Export the animation' });
	await expect(dlg).toBeVisible();
	await dlg.getByRole('radio', { name: /^GIF/ }).click();
	await dlg.getByLabel('Size').selectOption('0.5');
	await expect(dlg.getByText(`${size / 2} × ${size / 2} px`)).toBeVisible();
	await dlg.getByLabel(/Transparent background/).check();
	await dlg.getByRole('button', { name: 'Export…' }).click();
	await expect(page.getByText(/Exported to/)).toBeVisible({ timeout: 15_000 });
	await expect(dlg).toHaveCount(0);

	const all = await calls(page);
	const begin = JSON.parse(all.find((c) => c.cmd === 'export_begin')!.json!).spec;
	expect(begin).toMatchObject({
		format: 'gif',
		path: '/home/me/Walk cycle.gif',
		width: size / 2,
		height: size / 2,
		frames: 2,
		total: 3,
		transparent: true
	});
	const frames = all.filter((c) => c.cmd === 'export_frame');
	expect(frames.map((c) => c.repeat)).toEqual(['2', '1']);
	expect(frames[0].bytes).toBe((size / 2) * (size / 2) * 4);
	// the corner has no paint: see-through, not paper
	expect(frames[0].head![3]).toBe(0);
	expect(all.some((c) => c.cmd === 'export_end')).toBe(true);
});

/** a PNG file of one colour */
function solidPng(width: number, height: number, rgba: number[]): Buffer {
	const crcTable = Array.from({ length: 256 }, (_, n) => {
		let c = n;
		for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
		return c >>> 0;
	});
	const crc = (b: Buffer) => {
		let c = 0xffffffff;
		for (const x of b) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8);
		return (c ^ 0xffffffff) >>> 0;
	};
	const chunk = (type: string, data: Buffer) => {
		const t = Buffer.concat([Buffer.from(type, 'ascii'), data]);
		const len = Buffer.alloc(4);
		len.writeUInt32BE(data.length);
		const sum = Buffer.alloc(4);
		sum.writeUInt32BE(crc(t));
		return Buffer.concat([len, t, sum]);
	};
	const ihdr = Buffer.alloc(13);
	ihdr.writeUInt32BE(width, 0);
	ihdr.writeUInt32BE(height, 4);
	ihdr.set([8, 6, 0, 0, 0], 8);
	const row = Buffer.concat([Buffer.from([0]), Buffer.from(Array(width).fill(rgba).flat())]);
	const raw = Buffer.concat(Array(height).fill(row));
	return Buffer.concat([
		Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
		chunk('IHDR', ihdr),
		chunk('IDAT', deflateSync(raw)),
		chunk('IEND', Buffer.alloc(0))
	]);
}

/** the colour in the middle of the canvas */
const middle = (page: Page) =>
	canvas(page).evaluate((el: HTMLCanvasElement) => [
		...el.getContext('2d')!.getImageData(el.width >> 1, el.height >> 1, 1, 1).data
	]);

/** a WAV file: a tone for the first `on` seconds, then silence (`total` seconds) */
function toneWav(on: number, total: number): Buffer {
	const rate = 8000;
	const n = Math.round(rate * total);
	const b = Buffer.alloc(44 + n * 2);
	b.write('RIFF', 0);
	b.writeUInt32LE(36 + n * 2, 4);
	b.write('WAVEfmt ', 8);
	b.writeUInt32LE(16, 16);
	b.writeUInt16LE(1, 20);
	b.writeUInt16LE(1, 22);
	b.writeUInt32LE(rate, 24);
	b.writeUInt32LE(rate * 2, 28);
	b.writeUInt16LE(2, 32);
	b.writeUInt16LE(16, 34);
	b.write('data', 36);
	b.writeUInt32LE(n * 2, 40);
	for (let i = 0; i < n; i++)
		b.writeInt16LE(
			i < rate * on ? Math.round(Math.sin((i / rate) * 2 * Math.PI * 440) * 20000) : 0,
			44 + i * 2
		);
	return b;
}

test('sound: a clip with its waveform, markers, kept in the file, mixed into a video', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	await scribble(page, 0.3);
	for (let i = 0; i < 2; i++) await tl.getByRole('button', { name: 'Hold longer' }).click();
	// a second of sound: loud for the first half
	await tl.getByLabel('Sound files').setInputFiles({
		name: 'line.wav',
		mimeType: 'audio/wav',
		buffer: toneWav(0.5, 1)
	});
	const clip = tl.getByRole('slider', { name: /^line: starts on scene frame 1/ });
	await expect(clip).toBeVisible();
	await expect(page.getByLabel('not saved')).toBeVisible();
	// its waveform: bars where it's loud (the first 12 frames at 24 fps), none after
	const bars = (x0: number, x1: number) =>
		clip.locator('canvas').evaluate(
			(c: HTMLCanvasElement, [x0, x1]) => {
				const d = c.getContext('2d')!.getImageData(0, 0, c.width, c.height).data;
				let n = 0;
				for (let y = 0; y < c.height; y++)
					for (let x = Math.round(c.width * x0); x < Math.round(c.width * x1); x++)
						if (d[(y * c.width + x) * 4 + 3] > 0) n++;
				return n;
			},
			[x0, x1]
		);
	await expect.poll(() => bars(0, 0.45)).toBeGreaterThan(100);
	expect(await bars(0.55, 1)).toBe(0);
	// mute and unmute
	await tl.getByRole('button', { name: 'Sound on: line' }).click();
	await expect(tl.getByRole('button', { name: 'Sound on: line' })).toHaveAttribute(
		'aria-pressed',
		'false'
	);
	await tl.getByRole('button', { name: 'Sound on: line' }).click();

	// a marker on frame 2 (Shift+M), shown on the frame numbers and in the list
	await tl.getByRole('button', { name: 'Frame 2', exact: true }).click();
	await page.keyboard.press('Shift+M');
	await expect(tl.getByRole('button', { name: 'Frame 2, marker' })).toBeVisible();
	await tl.getByRole('button', { name: 'Markers', exact: true }).click();
	await tl.getByLabel('Marker 2 label').fill('hit');
	await tl.getByLabel('Marker 2 label').press('Enter');
	await tl.getByLabel('Marker 2 label').blur();

	// saved and opened again: the sound and the marker come back
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	const saved = (await calls(page)).filter((c) => c.cmd === 'project_save').at(-1)!;
	expect(saved.bytes!).toBeGreaterThan(16_000);
	await page.keyboard.press('Control+o');
	await expect(tl.getByRole('slider', { name: /^line: starts on scene frame 1/ })).toBeVisible({
		timeout: 20_000
	});
	await expect(tl.getByRole('button', { name: 'Frame 2, marker' })).toBeVisible();
	await expect.poll(() => bars(0, 0.45)).toBeGreaterThan(100);

	// an MP4: the sound of those frames goes with it
	await page.getByRole('button', { name: 'Export', exact: true }).click();
	await page.getByRole('button', { name: /^Animation…/ }).click();
	const dlg = page.getByRole('dialog', { name: 'Export the animation' });
	await dlg.getByRole('radio', { name: /^MP4/ }).click();
	await dlg.getByRole('button', { name: 'Export…' }).click();
	await expect(page.getByText(/Exported to/)).toBeVisible({ timeout: 15_000 });
	const all = await calls(page);
	const sound = all.findIndex((c) => c.cmd === 'export_audio');
	expect(sound).toBeGreaterThan(-1);
	expect(all[sound].head).toEqual([0x52, 0x49, 0x46, 0x46]); // RIFF
	// 3 frames at 24 fps at 48 kHz stereo 16-bit: 6000 samples
	expect(all[sound].bytes).toBe(44 + 6000 * 4);
	expect(sound).toBeLessThan(all.findIndex((c) => c.cmd === 'export_begin'));
});

test('lip-sync: a mouth chart, Rhubarb’s shapes on twos, kept in the file', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	// four mouths on frames 1–4 (A, B, C, D in order)
	await scribble(page, 0.2);
	for (const x of [0.4, 0.6, 0.8]) {
		await tl.getByRole('button', { name: 'New drawing' }).click();
		await scribble(page, x);
	}
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();
	await tl.getByLabel('Sound files').setInputFiles({
		name: 'line.wav',
		mimeType: 'audio/wav',
		buffer: toneWav(0.5, 0.5)
	});
	await tl.getByRole('button', { name: 'Lip-sync: line' }).click();
	const dlg = page.getByRole('dialog', { name: 'Lip-sync' });
	await expect(dlg.getByRole('heading')).toHaveText('Lip-sync “line”');
	await expect(dlg).toContainText('Frames 1–4 of this cut');
	await expect(dlg.getByRole('button', { name: 'Lip-sync', exact: true })).toBeDisabled();
	await dlg.getByRole('button', { name: 'Fill in order' }).click();
	await expect(dlg.getByLabel('Mouth A')).toHaveValue(/[1-9]/);
	await expect(dlg.getByLabel('Mouth D')).not.toHaveValue('0');
	await expect(dlg.getByLabel('Mouth X')).toHaveValue('0');
	await expect(dlg).toContainText('No drawing for E, F');
	// the line is longer than the cut: make room for all of it
	await dlg.getByLabel('Make the cut long enough for the whole line').check();
	await expect(dlg).toContainText('Frames 1–12 of this cut');
	await dlg.getByLabel('Language').selectOption('other');
	await dlg.getByRole('button', { name: 'Lip-sync', exact: true }).click();
	await expect(dlg).toBeHidden({ timeout: 15_000 });
	await expect(page.getByText('Lip-synced 12 frames of “line”.')).toBeVisible();

	const all = await calls(page);
	const sent = all.find((c) => c.cmd === 'lipsync_audio')!;
	expect(sent.head).toEqual([0x52, 0x49, 0x46, 0x46]); // RIFF
	// half a second at 16 kHz, stereo 16-bit
	expect(sent.bytes).toBe(44 + 8000 * 4);
	const run = JSON.parse(all.find((c) => c.cmd === 'lipsync_run')!.json!);
	expect(run.opts).toEqual({ english: false, dialog: '', extended: '' });

	// on twos: rest (no X drawn: A stands in), D, the closed lips kept, then C
	await page.getByRole('tab', { name: /X.sheet/ }).click();
	const xs = page.getByRole('region', { name: 'Exposure sheet' });
	const cellAt = (f: number) =>
		xs.getByRole('gridcell', { name: new RegExp(`^Layer 1, frame ${f}:`) });
	const want = [1, 1, 2, 2, 1, 1, 3, 3, 3, 3, 3, 3];
	for (const [i, d] of want.entries())
		await expect(cellAt(i + 1)).toHaveAccessibleName(
			`Layer 1, frame ${i + 1}: ${d === want[i - 1] ? 'held' : `drawing ${d}`}`
		);
	await expect(tl.getByTestId('frame').locator('..')).toContainText('of 12');

	// saved and opened: the chart comes back, with drawing B, which no frame shows now
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	await page.keyboard.press('Control+o');
	await expect(cellAt(12)).toBeVisible({ timeout: 20_000 });
	await tl.getByRole('button', { name: 'Lip-sync: line' }).click();
	await expect(dlg.getByLabel('Mouth B')).not.toHaveValue('0');
	await expect(dlg.getByLabel('Mouth B').locator('option:checked')).toHaveText(
		/another cut|Drawing/
	);
	await expect(dlg.locator('.shape.set img')).toHaveCount(4);
});

test('rotoscoping: an image sequence and a video’s frames come in as drawings on a layer', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	// three pictures, picked out of order: they go in number order
	await page.getByRole('button', { name: 'Import', exact: true }).click();
	const chooser = page.waitForEvent('filechooser');
	await page.getByRole('button', { name: /^Image sequence…/ }).click();
	await expect(page.getByRole('group', { name: 'Import' })).toHaveCount(0);
	await (
		await chooser
	).setFiles(
		[
			['walk_10.png', [0, 0, 255, 255]],
			['walk_2.png', [0, 160, 0, 255]],
			['walk_1.png', [255, 0, 0, 255]]
		].map(([name, c]) => ({
			name: name as string,
			mimeType: 'image/png',
			buffer: solidPng(16, 9, c as number[])
		}))
	);
	await expect(page.getByText('Imported 3 frames onto “walk”.')).toBeVisible({ timeout: 15_000 });
	await expect(tl.getByTestId('frame').locator('..')).toContainText('of 3');
	// half see-through over the paper: red, green, blue
	const tint = async () => {
		const [r, g, b] = await middle(page);
		return r > 200 && g < 160 && b < 160
			? 'red'
			: g > 150 && r < 160 && b < 160
				? 'green'
				: b > 200 && r < 160
					? 'blue'
					: `${r},${g},${b}`;
	};
	await expect.poll(tint).toBe('red');
	await tl.getByRole('button', { name: 'Frame 2', exact: true }).click();
	await expect.poll(tint).toBe('green');
	await tl.getByRole('button', { name: 'Frame 3', exact: true }).click();
	await expect.poll(tint).toBe('blue');
	await page.getByRole('tab', { name: /X.sheet/ }).click();
	const xs = page.getByRole('region', { name: 'Exposure sheet' });
	await expect(xs.getByRole('gridcell', { name: 'walk, frame 3: drawing 3' })).toBeVisible();

	// a video: its frames from this frame on (the 4th), at the project's frame rate
	await page.keyboard.press('.');
	await expect(tl.getByTestId('frame')).toHaveText('4');
	await page.getByRole('button', { name: 'Import', exact: true }).click();
	await page.getByRole('button', { name: /^Video frames…/ }).click();
	await expect(page.getByText('Imported 2 frames onto “dance take”.')).toBeVisible({
		timeout: 15_000
	});
	const open = JSON.parse((await calls(page)).find((c) => c.cmd === 'video_in_open')!.json!);
	expect(open).toMatchObject({ path: '/home/me/dance take 2.mp4', fps: 24 });
	expect(open.width).toBeGreaterThan(100);
	await expect(xs.getByRole('gridcell', { name: 'dance take, frame 4: drawing 1' })).toBeVisible();
	await expect(xs.getByRole('gridcell', { name: 'dance take, frame 5: drawing 2' })).toBeVisible();
	// (the sequence ended on frame 3: the video's red frame shows alone, then its blue one)
	await expect.poll(tint).toBe('red');
	await page.keyboard.press('.');
	await expect.poll(tint).toBe('blue');
	expect((await calls(page)).some((c) => c.cmd === 'video_in_close')).toBe(true);
});

test('export a PSD per frame and a PNG sequence per layer', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	await scribble(page, 0.3);
	await tl.getByRole('button', { name: '2s' }).click();
	await tl.getByRole('button', { name: 'New drawing' }).click();
	await scribble(page, 0.6);
	await tl.getByRole('button', { name: '1s' }).click();
	const exportAs = async (format: RegExp) => {
		await page.getByRole('button', { name: 'Export', exact: true }).click();
		await page.getByRole('button', { name: /^Animation…/ }).click();
		const dlg = page.getByRole('dialog', { name: 'Export the animation' });
		await dlg.getByRole('radio', { name: format }).click();
		await dlg.getByRole('button', { name: 'Export…' }).click();
		await expect(dlg).toHaveCount(0, { timeout: 15_000 });
	};
	const writes = async () => (await calls(page)).filter((c) => c.cmd === 'export_write');

	await exportAs(/^PSD per frame/);
	await expect(page.getByText(/Exported 3 frames/)).toBeVisible();
	let w = await writes();
	expect(w.map((c) => c.path)).toEqual([
		'/home/me/Walk cycle_0001.psd',
		'/home/me/Walk cycle_0002.psd',
		'/home/me/Walk cycle_0003.psd'
	]);
	for (const c of w) expect(c.head).toEqual([0x38, 0x42, 0x50, 0x53]); // 8BPS
	// a held frame is the same file twice; the next frame differs
	expect(w[0].bytes).toBe(w[1].bytes);

	await exportAs(/^PNG per layer/);
	await expect(page.getByText(/Exported 3 frames/)).toBeVisible();
	w = (await writes()).slice(3);
	expect(w.map((c) => c.path)).toEqual([
		'/home/me/Walk cycle_Layer 1_0001.png',
		'/home/me/Walk cycle_Layer 1_0002.png',
		'/home/me/Walk cycle_Layer 1_0003.png'
	]);
	for (const c of w) expect(c.head).toEqual([0x89, 0x50, 0x4e, 0x47]);
});
