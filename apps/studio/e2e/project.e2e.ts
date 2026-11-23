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
			/** export frames: how many pixels are dark */
			ink?: number;
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
					repeat: options?.headers?.repeat,
					ink:
						cmd === 'export_frame' && bytes
							? bytes.filter((v, i) => i % 4 === 0 && v < 230).length
							: undefined
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
type Call = {
	cmd: string;
	path?: string;
	bytes?: number;
	head?: number[];
	json?: string;
	repeat?: string;
	ink?: number;
};
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
	// "Save before opening another project?" No: open without saving
	const ask = page.getByRole('dialog', { name: 'Save before opening another project?' });
	await expect(ask).toBeVisible();
	await ask.getByRole('button', { name: 'No' }).click();
	await expect(page.getByLabel('not saved')).toHaveCount(0);
	await expectSavedDrawing(page, drawn);
	expect((await calls(page)).map((c) => c.cmd)).toEqual(
		expect.arrayContaining([
			'plugin:dialog|save',
			'project_save',
			'recovery_discard',
			'plugin:dialog|open',
			'project_open'
		])
	);
});

test('cancel keeps your changes and stops the open', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	await scribble(page, 0.5);
	await page.keyboard.press('Control+o');
	const ask = page.getByRole('dialog', { name: 'Save before opening another project?' });
	await ask.getByRole('button', { name: 'Cancel' }).click();
	await expect(ask).toHaveCount(0);
	expect((await calls(page)).map((c) => c.cmd)).not.toContain('plugin:dialog|open');
	await expect(page.getByLabel('not saved')).toBeVisible();
});

test('yes saves first, then goes ahead', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	await scribble(page, 0.5);
	await page.keyboard.press('Control+o');
	const ask = page.getByRole('dialog', { name: 'Save before opening another project?' });
	await ask.getByRole('button', { name: 'Yes' }).click();
	// saved (asking where: it had no file yet), then the open goes on
	await expect
		.poll(async () => (await calls(page)).map((c) => c.cmd))
		.toEqual(expect.arrayContaining(['plugin:dialog|save', 'project_save', 'plugin:dialog|open']));
	const cmds = (await calls(page)).map((c) => c.cmd);
	expect(cmds.indexOf('project_save')).toBeLessThan(cmds.indexOf('plugin:dialog|open'));
});

test('the File menu saves, saves as, and exports', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	await scribble(page, 0.5);
	const file = page.getByRole('button', { name: 'File', exact: true });
	await file.click();
	const menu = page.getByRole('group', { name: 'File' });
	for (const item of ['Open…', 'Save', 'Save as…', 'PNG picture…', 'Photoshop file (.psd)…'])
		await expect(menu.getByText(item, { exact: true })).toBeVisible();
	await menu.getByRole('button', { name: /^Save as…/ }).click();
	await expect.poll(async () => (await calls(page)).map((c) => c.cmd)).toContain('project_save');
	await expect(page.getByLabel('not saved')).toHaveCount(0);
	await file.click();
	await menu.getByRole('button', { name: /^PNG picture…/ }).click();
	await expect.poll(async () => (await calls(page)).map((c) => c.cmd)).toContain('export_write');
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
	// opening a version over unsaved work asks first (No: don't save); it opens as unsaved work
	await page
		.getByRole('dialog', { name: 'Save before opening an earlier version?' })
		.getByRole('button', { name: 'No' })
		.click();
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
	await stubTauri(page);
	await ready(page);
	await scribble(page, 0.5);
	await page.getByRole('link', { name: 'Back' }).click();
	const ask = page.getByRole('dialog', { name: 'Save before leaving?' });
	await ask.getByRole('button', { name: 'Cancel' }).click();
	await expect(page).toHaveURL(/\/studio/);
	await expect(page.getByLabel('not saved')).toBeVisible();
});

test('settings: change a key (a taken key swaps), and it works in the studio', async ({ page }) => {
	await stubTauri(page);
	await page.setViewportSize({ width: 1440, height: 960 });
	await page.goto('/');
	await page.getByRole('link', { name: 'Settings' }).click();
	// (in sections: the keys have their own)
	await page.getByRole('tab', { name: 'Keyboard shortcuts' }).click();
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
	await page.getByRole('tab', { name: 'Pen and touch' }).click();
	await page.getByLabel('Always moves the view (pen only for drawing)').check();
	await page.getByLabel('Two-finger tap undoes, three-finger tap redoes').uncheck();
	// the sections follow up and down
	await page.getByRole('tab', { name: 'Pen and touch' }).press('ArrowDown');
	await expect(page.getByRole('tab', { name: 'Keyboard shortcuts' })).toBeFocused();
	await page.getByRole('tab', { name: 'Files' }).click();
	await page.getByLabel('Keep a recovery copy of unsaved work').selectOption('60');
	await page.getByLabel('Earlier versions kept per project').fill('5');
	await expect
		.poll(async () => {
			const last = (await calls(page)).filter((c) => c.cmd === 'settings_set').at(-1);
			return last ? JSON.parse(last.json!).settings : null;
		})
		.toMatchObject({ fingers: 'move', tapGestures: false, autosaveSeconds: 60, versionsKept: 5 });
	await page.getByRole('button', { name: 'Reset all' }).click();
	await page.getByRole('tab', { name: 'Pen and touch' }).click();
	await expect(page.getByLabel('Draws until you use a pen, then moves the view')).toBeChecked();
});

/** what the app last kept of its settings */
async function keptSettings(page: Page) {
	const last = (await calls(page)).filter((c) => c.cmd === 'settings_set').at(-1);
	return last ? JSON.parse(last.json!).settings : null;
}

test('settings open over the canvas (no leaving, no question) and apply as they change', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	await scribble(page, 0.5);
	const drawn = await ink(page);
	await expect(page.getByLabel('not saved')).toBeVisible();
	// the menu sits on the left, after the back button; the project's name on the right
	const back = (await page.getByRole('link', { name: 'Back' }).boundingBox())!;
	const file = (await page.getByRole('button', { name: 'File', exact: true }).boundingBox())!;
	const name = (await page.getByTestId('project-name').boundingBox())!;
	expect(back.x).toBeLessThan(file.x);
	expect(file.x).toBeLessThan(name.x);

	await page.getByRole('button', { name: 'Settings', exact: true }).click();
	const dialog = page.getByRole('dialog', { name: 'Settings' });
	await expect(dialog).toBeVisible();
	await expect(page.getByRole('dialog', { name: 'Save before leaving?' })).toHaveCount(0);
	await expect(page).toHaveURL(/\/studio/);
	// a new key, in use as soon as the window closes
	await dialog.getByRole('tab', { name: 'Keyboard shortcuts' }).click();
	await dialog.getByRole('button', { name: 'Change the key for Eraser' }).click();
	await page.keyboard.press('Shift+E');
	await expect(dialog.locator('li[data-action="eraser"] kbd')).toHaveText('Shift+E');
	// (Esc while choosing a key only stops choosing; now it closes)
	await page.keyboard.press('Escape');
	await expect(dialog).toHaveCount(0);
	await expect(page.getByLabel('not saved')).toBeVisible();
	expect(await ink(page)).toBeGreaterThan(drawn * 0.9);
	await page.keyboard.press('Shift+E');
	await expect(page.getByRole('button', { name: 'Eraser (Shift+E)' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await expect.poll(async () => (await keptSettings(page))?.keys?.eraser).toBe('Shift+E');
	// the close button closes it too
	await page.getByRole('button', { name: 'Settings', exact: true }).click();
	await dialog.getByRole('button', { name: 'Close the settings' }).click();
	await expect(dialog).toHaveCount(0);
});

test('dark mode: the interface goes dark at once, the paper stays white', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const html = page.locator('html');
	await expect(html).toHaveAttribute('data-theme', 'light');
	const ground = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
	const light = await ground();
	await page.getByRole('button', { name: 'Settings', exact: true }).click();
	const dialog = page.getByRole('dialog', { name: 'Settings' });
	await dialog.getByRole('tab', { name: 'Appearance' }).click();
	await dialog.getByLabel('Dark').check();
	await expect(html).toHaveAttribute('data-theme', 'dark');
	expect(await ground()).not.toBe(light);
	await page.keyboard.press('Escape');
	// the paper is still white
	await expect
		.poll(() =>
			canvas(page).evaluate((el: HTMLCanvasElement) =>
				Array.from(el.getContext('2d')!.getImageData(el.width >> 1, 4, 1, 1).data)
			)
		)
		.toEqual([255, 255, 255, 255]);
	await expect.poll(async () => (await keptSettings(page))?.theme).toBe('dark');
});

test('bars fold away and come back; the timeline and the side panel resize', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	const room = () =>
		canvas(page).evaluate((el) => el.closest('.stage')!.getBoundingClientRect().height);
	const tall = await room();
	await page.getByRole('button', { name: 'Hide the timeline' }).click();
	await expect(tl).toBeHidden();
	// the canvas takes the room
	await expect.poll(room).toBeGreaterThan(tall + 100);
	await expect.poll(async () => (await keptSettings(page))?.layout?.bottom).toBe(false);
	await page.getByRole('button', { name: 'Show the timeline' }).click();
	await expect(tl).toBeVisible();

	// the brush options bar and the side panel fold the same way
	await page.getByRole('button', { name: 'Hide the brush options' }).click();
	await expect(page.getByRole('group', { name: 'Brush options' })).toHaveCount(0);
	await page.getByRole('button', { name: 'Show the brush options' }).click();
	await expect(page.getByRole('group', { name: 'Brush options' })).toBeVisible();
	const dock = page.getByRole('complementary', { name: 'Side panel' });
	await page.getByRole('button', { name: 'Hide the side panel' }).click();
	await expect(dock).toBeHidden();
	await page.getByRole('button', { name: 'Show the side panel' }).click();
	await expect(dock).toBeVisible();
	// and the tools on the left
	const stageWidth = async () =>
		canvas(page).evaluate((el) => el.closest('.stage')!.getBoundingClientRect().width);
	const was = await stageWidth();
	await page.getByRole('button', { name: 'Hide the tools' }).click();
	await expect(page.getByRole('button', { name: 'Hide the tools' })).toBeHidden();
	await expect.poll(stageWidth).toBeGreaterThan(was + 20);
	await expect.poll(async () => (await keptSettings(page))?.layout?.rail).toBe(false);
	await page.getByRole('button', { name: 'Show the tools' }).click();
	await expect(page.getByRole('button', { name: 'Hide the tools' })).toBeVisible();

	// resizing: drag the timeline's edge up, or use the arrows on the side panel's edge
	const before = (await tl.boundingBox())!.height;
	const grip = (await page.getByRole('separator', { name: 'Resize the timeline' }).boundingBox())!;
	await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
	await page.mouse.down();
	await page.mouse.move(grip.x + grip.width / 2, grip.y - 120, { steps: 5 });
	await page.mouse.up();
	await expect.poll(async () => (await tl.boundingBox())!.height).toBeGreaterThan(before + 100);
	const wide = (await dock.boundingBox())!.width;
	await page.getByRole('separator', { name: 'Resize the side panel' }).focus();
	await page.keyboard.press('Shift+ArrowLeft');
	await expect.poll(async () => (await dock.boundingBox())!.width).toBe(wide + 64);
	await expect
		.poll(async () => (await keptSettings(page))?.layout?.dockWidth)
		.toBe(Math.round(wide + 64));
});

test('the timeline’s drop-downs are readable (not white on white)', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const pick = page.getByRole('region', { name: 'Timeline' }).getByLabel('Into the next cut');
	const [fg, bg, look] = await pick.evaluate((el) => {
		const s = getComputedStyle(el);
		return [s.color, s.backgroundColor, s.appearance];
	});
	expect(fg).not.toBe(bg);
	// drawn by the page, not the system (whose own white ignored the bar's colours)
	expect(look).toBe('none');
});

test('home: draw together has its own row, to host, join, or go back to a shared project', async ({
	page
}) => {
	const at = new Date().toISOString();
	const item = (path: string, shared?: { with: string[] }) => ({
		path,
		found: true,
		width: 1920,
		height: 1080,
		fps: 24,
		layers: 1,
		modified: at,
		thumbnail: null,
		shared
	});
	await stubTauri(page, {
		recent: [
			item('/home/me/Solo.ppstudio'),
			item('/home/me/Jam.ppstudio', { with: ['Sam', 'Ana'] })
		]
	});
	await page.setViewportSize({ width: 1440, height: 960 });
	await page.goto('/');
	const row = page.getByRole('region', { name: 'Draw together' });
	const recent = page.getByRole('region', { name: 'Recent' });
	await expect(row).toContainText('With Sam, Ana');
	await expect(row.getByRole('button', { name: 'Rejoin Jam' })).toBeVisible();
	await expect(recent).toContainText('Solo');
	await expect(recent).not.toContainText('Jam');

	// host: a new canvas, with Draw together open
	await row.getByRole('button', { name: /Host a session/ }).click();
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	const share = page.getByRole('dialog', { name: 'Draw together' });
	await expect(share).toBeVisible();
	await share.getByRole('button', { name: 'Close' }).click();

	// join: the code goes to the studio without going into its address
	await page.getByRole('link', { name: 'Back' }).click();
	await expect(row.getByRole('button', { name: 'Join', exact: true })).toBeDisabled();
	await row.getByLabel('Join with a code').fill('here: pp1-abcDEF_123');
	await row.getByRole('button', { name: 'Join', exact: true }).click();
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await expect(share).toBeVisible();
	await expect
		.poll(async () => (await calls(page)).find((c) => c.cmd === 'share_join')?.json)
		.toContain('"code":"pp1-abcDEF_123"');
	expect(page.url()).not.toContain('pp1-');
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
	await page.getByRole('button', { name: 'Guides', exact: true }).click();
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
	// undo: first the timing (a timeline step), then the last stroke, which was drawing
	// 2's, so it goes there first
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();
	await page.keyboard.press('Control+z');
	await expect(tl.getByRole('button', { name: /drawing 2, frame 2$/ })).toBeVisible();
	await page.keyboard.press('Control+z');
	await expect(frameNo).toHaveText('2');
	await expect.poll(bottom).toBe(0);
	await page.keyboard.press('Control+Shift+z');
	await expect.poll(bottom).toBeGreaterThan(50);
	await page.keyboard.press('Control+Shift+z');
	await expect(tl.getByRole('button', { name: /drawing 2, frame 3$/ })).toBeVisible();
	await tl.getByRole('button', { name: 'Frame 3', exact: true }).click();

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

test('films: the timelapse (kept in the file) and this frame drawn live', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	for (const y of [0.2, 0.4, 0.6]) await scribble(page, y);
	const openFilm = async () => {
		await page.getByRole('button', { name: 'Export', exact: true }).click();
		await page.getByRole('button', { name: /^Film of the drawing…/ }).click();
		return page.getByRole('dialog', { name: 'Export a film of the drawing' });
	};
	const pictures = async (dlg: ReturnType<Page['getByRole']>) =>
		Number(
			(await dlg.getByRole('radio', { name: /^Timelapse/ }).textContent())!.match(
				/\((\d+) so far/
			)![1]
		);

	// the timelapse: 10 seconds and the finished picture held 1.5 s, at 24 fps
	let dlg = await openFilm();
	await expect.poll(() => pictures(dlg)).toBeGreaterThanOrEqual(3);
	await dlg.getByRole('radio', { name: /^Timelapse/ }).click();
	await dlg.getByLabel('Format').selectOption('mp4');
	await dlg.getByLabel('Length').selectOption('10');
	await dlg.getByRole('button', { name: 'Export…' }).click();
	await expect(page.getByText(/Exported to/)).toBeVisible({ timeout: 20_000 });
	let all = await calls(page);
	const tl = JSON.parse(all.filter((c) => c.cmd === 'export_begin').at(-1)!.json!).spec;
	expect(tl).toMatchObject({ format: 'mp4', fps: 24, total: 240 + 36, transparent: false });
	let frames = all.filter((c) => c.cmd === 'export_frame');
	expect(frames.length).toBe(tl.frames);
	expect(frames.reduce((n, c) => n + Number(c.repeat), 0)).toBe(tl.total);

	// drawn live, 8× faster: lines draw on from an empty page, then the finished frame
	dlg = await openFilm();
	await dlg.getByRole('radio', { name: /^Drawn live/ }).click();
	await expect(dlg.getByRole('radio', { name: /^Drawn live/ })).toContainText('(3 strokes)');
	await dlg.getByLabel('Format').selectOption('gif');
	await dlg.getByLabel('Speed').selectOption('8');
	await dlg.getByLabel('Size').selectOption('0.5');
	await dlg.getByRole('button', { name: 'Export…' }).click();
	await expect(dlg).toHaveCount(0, { timeout: 30_000 });
	await expect(page.getByText(/Exported to/)).toBeVisible();
	all = await calls(page);
	const live = JSON.parse(all.filter((c) => c.cmd === 'export_begin').at(-1)!.json!).spec;
	expect(live).toMatchObject({ format: 'gif', fps: 24 });
	frames = all.filter((c) => c.cmd === 'export_frame').slice(tl.frames);
	// (12 frames of pause, 14 of dissolve, 36 of the finished frame after the drawing)
	expect(frames.reduce((n, c) => n + Number(c.repeat), 0)).toBe(live.total);
	expect(live.total).toBeGreaterThan(12 + 14 + 36);
	expect(frames.length).toBeGreaterThan(14 + 3);
	// the first frame starts on paper; the lines grow; the finished frame has them all
	expect(frames[0].head).toEqual([255, 255, 255, 255]);
	const ink = frames.map((c) => c.ink!);
	expect(ink[0]).toBeLessThan(ink.at(-1)!);
	const drawing = ink.slice(0, ink.length - 16);
	expect(drawing.every((n, i) => i === 0 || n >= drawing[i - 1] - 5)).toBe(true);
	expect(new Set(drawing).size).toBeGreaterThan(3);
	expect(frames[0].bytes).toBe(live.width * live.height * 4);

	// the timelapse is kept in the file: opened again, it's still there
	const before = await (async () => {
		const d = await openFilm();
		const n = await pictures(d);
		await d.getByRole('button', { name: 'Close' }).click();
		return n;
	})();
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	await page.keyboard.press('Control+o');
	await expect(page.getByRole('button', { name: 'Export', exact: true })).toBeEnabled({
		timeout: 20_000
	});
	dlg = await openFilm();
	await expect.poll(() => pictures(dlg)).toBeGreaterThanOrEqual(before);
});

test('undo: timeline edits are steps in the history, with the strokes', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	const length = () => tl.getByTestId('frame').locator('..');
	await scribble(page, 0.3);
	// hold it a frame longer: one step, undone and redone
	await tl.getByRole('button', { name: 'Hold longer' }).click();
	await expect(length()).toContainText('of 2');
	await page.keyboard.press('Control+z');
	await expect(length()).toContainText('of 1');
	await expect.poll(() => ink(page)).toBeGreaterThan(0);
	await page.keyboard.press('Control+Shift+z');
	await expect(length()).toContainText('of 2');

	// drawing on an empty cel makes a drawing there: one undo takes the stroke and the cel
	await tl.getByRole('button', { name: 'Frame 3', exact: true }).click();
	await expect.poll(() => ink(page)).toBe(0);
	await scribble(page, 0.6);
	await expect(length()).toContainText('of 3');
	await page.keyboard.press('Control+z');
	await expect(length()).toContainText('of 2');
	await expect.poll(() => ink(page)).toBe(0);
	await page.keyboard.press('Control+Shift+z');
	await expect(length()).toContainText('of 3');
	await expect.poll(() => ink(page)).toBeGreaterThan(0);

	// the History panel lists the timeline step, and going back through it works too
	await page.getByRole('tab', { name: /History/ }).click();
	const history = page.getByRole('region', { name: 'History' });
	await expect(history).toContainText('Timeline change');
	// back over the stroke on frame 3 and the hold: one frame again, its drawing kept
	await page.keyboard.press('Control+z');
	await page.keyboard.press('Control+z');
	await expect(length()).toContainText('of 1');
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();
	await expect.poll(() => ink(page)).toBeGreaterThan(0);
	// one more: the first stroke goes too
	await page.keyboard.press('Control+z');
	await expect.poll(() => ink(page)).toBe(0);
});

test('keys, breakdowns and in-betweens marked; flipping rolls and flips the keys', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	const frameNo = tl.getByTestId('frame');
	await scribble(page, 0.2);
	for (const y of [0.4, 0.6, 0.8]) {
		await tl.getByRole('button', { name: 'New drawing' }).click();
		await scribble(page, y);
	}
	// frame 1 a key, frame 3 a breakdown, frame 4 an in-between
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();
	await page.keyboard.press('Shift+K');
	await expect(tl.getByRole('button', { name: 'Mark this drawing: Key' })).toBeVisible();
	await expect(tl.getByRole('button', { name: 'Layer 1: drawing 1 (Key), frame 1' })).toBeVisible();
	await tl.getByRole('button', { name: 'Frame 3', exact: true }).click();
	await page.keyboard.press('Shift+K');
	await page.keyboard.press('Shift+K');
	await tl.getByRole('button', { name: 'Frame 4', exact: true }).click();
	await tl.getByRole('button', { name: /^Mark this drawing/ }).click();
	await tl.getByRole('button', { name: /^Mark this drawing/ }).click();
	await tl.getByRole('button', { name: /^Mark this drawing/ }).click();
	await expect(tl.getByRole('button', { name: 'Mark this drawing: In-between' })).toBeVisible();
	await page.getByRole('tab', { name: /X.sheet/ }).click();
	const xs = page.getByRole('region', { name: 'Exposure sheet' });
	await expect(
		xs.getByRole('gridcell', { name: 'Layer 1, frame 1: drawing 1 (key)' })
	).toBeVisible();
	await expect(
		xs.getByRole('gridcell', { name: 'Layer 1, frame 3: drawing 3 (breakdown)' })
	).toBeVisible();

	// hold Shift+R: the key and the breakdown flip (frames 1 and 3, nothing between)
	await tl.getByRole('button', { name: 'Frame 2', exact: true }).click();
	const seen = new Set<string>();
	await page.keyboard.down('Shift');
	await page.keyboard.down('R');
	for (let i = 0; i < 12; i++) {
		seen.add((await frameNo.textContent())!);
		await page.waitForTimeout(60);
	}
	await page.keyboard.up('R');
	await page.keyboard.up('Shift');
	expect([...seen].sort()).toEqual(['1', '3']);
	await expect(frameNo).toHaveText('2');

	// hold R on frame 4: the drawings around it roll by, then back to 4
	await tl.getByRole('button', { name: 'Frame 4', exact: true }).click();
	seen.clear();
	await page.keyboard.down('R');
	for (let i = 0; i < 14; i++) {
		seen.add((await frameNo.textContent())!);
		await page.waitForTimeout(60);
	}
	await page.keyboard.up('R');
	expect([...seen].sort()).toEqual(['2', '3', '4']);
	await expect(frameNo).toHaveText('4');
	await expect.poll(() => ink(page, 0.75, 1)).toBeGreaterThan(20);

	// the onion skin can keep to keys and breakdowns
	await tl.getByRole('button', { name: 'Onion skin settings' }).click();
	await tl.getByLabel('Keys and breakdowns only').check();
	await tl.getByRole('button', { name: 'Onion skin settings' }).click();

	// undoable, and kept in the file
	await page.keyboard.press('Control+z');
	await expect(tl.getByRole('button', { name: 'Mark this drawing: Breakdown' })).toBeVisible();
	await page.keyboard.press('Control+Shift+z');
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	await page.keyboard.press('Control+o');
	await expect(xs.getByRole('gridcell', { name: 'Layer 1, frame 1: drawing 1 (key)' })).toBeVisible(
		{ timeout: 20_000 }
	);
	await expect(
		xs.getByRole('gridcell', { name: 'Layer 1, frame 4: drawing 4 (in-between)' })
	).toBeVisible();
});

test('colour: colour layers under the lines, a fill carried through the frames', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	const b = (await canvas(page).boundingBox())!;
	/** a closed box (one stroke), in fractions of the canvas */
	const box = async (x0: number, y0: number, x1: number, y1: number) => {
		const at = (x: number, y: number) => [b.x + b.width * x, b.y + b.height * y] as const;
		await page.mouse.move(...at(x0, y0));
		await page.mouse.down();
		for (const [x, y] of [
			[x1, y0],
			[x1, y1],
			[x0, y1],
			[x0, y0]
		])
			await page.mouse.move(...at(x, y), { steps: 12 });
		await page.mouse.up();
	};
	/** the colour at a point of the canvas (fractions) */
	const px = (x: number, y: number) =>
		canvas(page).evaluate(
			(el: HTMLCanvasElement, [x, y]) => [
				...el
					.getContext('2d')!
					.getImageData(Math.round(el.width * x), Math.round(el.height * y), 1, 1).data
			],
			[x, y]
		);
	const dark = async (x: number, y: number) => (await px(x, y))[0] < 100;
	// the lines: a box moving right over three drawings, on twos
	for (const [i, x] of [0.2, 0.26, 0.32].entries()) {
		if (i) await tl.getByRole('button', { name: 'New drawing' }).click();
		await box(x, 0.3, x + 0.25, 0.6);
		await tl.getByRole('button', { name: '2s' }).click();
	}
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();

	// colour layers for the lines; carry fills on
	await page.getByRole('button', { name: 'Fill (G)' }).click();
	await page.getByRole('button', { name: 'Set up colour layers' }).click();
	await expect(page.getByText('Fill on the Colour layer')).toBeVisible();
	const carry = page.getByRole('button', { name: 'Carry to next frames' });
	await carry.click();
	await expect(carry).toHaveAttribute('aria-pressed', 'true');

	// one fill inside the box on frame 1: every drawing's box gets it
	expect(await dark(0.32, 0.45)).toBe(false);
	await page.mouse.click(b.x + b.width * 0.32, b.y + b.height * 0.45);
	await expect(page.getByText('Filled on 3 drawings.')).toBeVisible({ timeout: 20_000 });
	await expect(tl.getByTestId('frame')).toHaveText('1');
	await expect.poll(() => dark(0.32, 0.45)).toBe(true);
	expect(await dark(0.6, 0.45)).toBe(false);
	for (const [f, x] of [
		[3, 0.38],
		[5, 0.44]
	] as const) {
		await tl.getByRole('button', { name: `Frame ${f}`, exact: true }).click();
		await expect.poll(() => dark(x, 0.45)).toBe(true);
		// (outside the box: not coloured)
		expect(await dark(x + 0.2, 0.45)).toBe(false);
		// held as long as the line drawing
		await tl.getByRole('button', { name: `Frame ${f + 1}`, exact: true }).click();
		await expect.poll(() => dark(x, 0.45)).toBe(true);
	}
	// the colour layer's drawings follow the lines' timing
	await expect(
		tl.getByRole('button', { name: /^Colour \(Layer 1\): drawing 3, frame 5–6$/ })
	).toBeVisible();

	// one undo takes the whole carried fill
	await page.keyboard.press('Control+z');
	await expect.poll(() => dark(0.44, 0.45)).toBe(false);
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();
	await expect.poll(() => dark(0.32, 0.45)).toBe(false);
});

test('transitions: a white flash and a dissolve between cuts, in the export', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	// two cuts of 8 frames, a line high in the first and low in the second
	await scribble(page, 0.3);
	for (let i = 0; i < 7; i++) await tl.getByRole('button', { name: 'Hold longer' }).click();
	await tl.getByRole('button', { name: '+ Cut' }).click();
	await scribble(page, 0.7);
	for (let i = 0; i < 7; i++) await tl.getByRole('button', { name: 'Hold longer' }).click();
	await tl.getByLabel('Cut', { exact: true }).selectOption({ index: 0 });
	await tl.getByLabel('Into the next cut').selectOption('flash');

	const exportAll = async () => {
		await page.getByRole('button', { name: 'Export', exact: true }).click();
		await page.getByRole('button', { name: /^Animation…/ }).click();
		const dlg = page.getByRole('dialog', { name: 'Export the animation' });
		await dlg.getByRole('radio', { name: /^GIF/ }).click();
		await dlg.getByLabel('Frames').selectOption('all');
		await dlg.getByRole('button', { name: 'Export…' }).click();
		await expect(dlg).toHaveCount(0, { timeout: 20_000 });
		const all = await calls(page);
		const begin = all.findLastIndex((c) => c.cmd === 'export_begin');
		const frames = all.slice(begin).filter((c) => c.cmd === 'export_frame');
		// every output frame's ink (frames sent once and held are repeated)
		return frames.flatMap((c) => Array(Number(c.repeat)).fill(c.ink!) as number[]);
	};
	const ink = await exportAll();
	expect(ink.length).toBe(16);
	// cut 1's last 3 frames darken (the paper too), one white frame, then cut 2 lands
	const lines = ink[0];
	expect(ink[4]).toBe(lines);
	for (const i of [5, 6, 7]) expect(ink[i]).toBeGreaterThan(lines * 5);
	expect(ink[8]).toBe(0);
	expect(ink[15]).toBeGreaterThan(0);
	expect(ink[9]).toBeLessThanOrEqual(ink[15]);

	// a dissolve instead: both lines show while it blends, the length stays
	await tl.getByLabel('Into the next cut').selectOption('dissolve');
	const d = await exportAll();
	expect(d.length).toBe(16);
	expect(d[0]).toBe(lines);
	// (mid-dissolve: some of each line)
	expect(d[8]).toBeGreaterThan(0);
	expect(new Set(d.slice(2, 14)).size).toBeGreaterThan(4);
});

test('play the scene: every cut in turn, with the flash between them', async ({ page }) => {
	// (two dozen clicks to set up, then watching playback: slow CI machines need longer)
	test.setTimeout(90_000);
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	await scribble(page, 0.3);
	for (let i = 0; i < 11; i++) await tl.getByRole('button', { name: 'Hold longer' }).click();
	await tl.getByRole('button', { name: '+ Cut' }).click();
	await scribble(page, 0.7);
	for (let i = 0; i < 11; i++) await tl.getByRole('button', { name: 'Hold longer' }).click();
	const cutPick = tl.getByLabel('Cut', { exact: true });
	// each cut's length in seconds (12 frames at 24 fps)
	await expect(cutPick.locator('option').first()).toHaveText('Cut 1 · 0.5 s');
	await cutPick.selectOption({ index: 0 });
	await tl.getByLabel('Into the next cut').selectOption('flash');
	await tl
		.getByRole('radiogroup', { name: 'Play what' })
		.getByRole('radio', { name: 'Scene' })
		.click();
	await tl.getByRole('radio', { name: 'Once' }).click();
	await tl.getByRole('button', { name: 'Play' }).click();
	// watch the player: the paper darkens before the cut (the flash), cut 2 plays, it stops
	const player = page.getByTestId('player');
	await expect(player).toBeVisible();
	const seen = { dark: false, cut2: false };
	// watch until both are seen or the player is gone (at most 20 s)
	const until = Date.now() + 20_000;
	while (Date.now() < until && !(seen.dark && seen.cut2) && (await player.count())) {
		const v = await player
			.evaluate((c: HTMLCanvasElement) => {
				const d = c.getContext('2d')!.getImageData(c.width >> 1, c.height >> 1, 1, 1).data;
				return d[0];
			})
			.catch(() => -1);
		if (v >= 0 && v < 235) seen.dark = true;
		if ((await cutPick.inputValue().catch(() => '')) === '1') seen.cut2 = true;
		await page.waitForTimeout(15);
	}
	expect(seen).toEqual({ dark: true, cut2: true });
	await expect(player).toHaveCount(0, { timeout: 5000 });
});

test('shift & trace: the onion skin’s drawings move to line up, then go back', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	const onion = page.getByTestId('onion');
	/** red onion pixels in rows y0–y1 (fractions) */
	const red = (y0: number, y1: number) =>
		onion.evaluate(
			(el: HTMLCanvasElement, [y0, y1]) => {
				const top = Math.round(el.height * y0);
				const h = Math.max(1, Math.round(el.height * (y1 - y0)));
				const d = el.getContext('2d')!.getImageData(0, top, el.width, h).data;
				let n = 0;
				for (let i = 0; i < d.length; i += 4)
					if (d[i + 3] >= 40 && d[i] > d[i + 1] + 60 && d[i] > d[i + 2] + 60) n++;
				return n;
			},
			[y0, y1] as const
		);
	await scribble(page, 0.2);
	await tl.getByRole('button', { name: 'New drawing' }).click();
	await page.keyboard.press('o');
	await expect.poll(() => red(0.1, 0.35)).toBeGreaterThan(50);
	// move the drawing before down, with the pad's arrow keys (Shift: 20 px a press)
	await tl.getByRole('button', { name: 'Onion skin settings' }).click();
	const pad = tl.getByRole('slider', { name: 'Drag to move the drawings before' });
	await pad.focus();
	const { height } = await canvas(page).evaluate((el: HTMLCanvasElement) => ({
		height: el.height
	}));
	const presses = Math.ceil((height * 0.4) / 20);
	for (let i = 0; i < presses; i++) await page.keyboard.press('Shift+ArrowDown');
	await expect(pad).toHaveAttribute('aria-valuetext', `0, ${presses * 20} px, 0°`);
	await expect.poll(() => red(0.1, 0.35)).toBe(0);
	await expect.poll(() => red(0.5, 0.8)).toBeGreaterThan(50);
	// the drawing itself didn't move: put back, it's where it was
	await tl.getByRole('button', { name: 'Put back' }).click();
	await expect.poll(() => red(0.1, 0.35)).toBeGreaterThan(50);
	// and shifted views go back on their own on another frame
	await pad.focus();
	await page.keyboard.press('Shift+ArrowDown');
	await expect(pad).not.toHaveAttribute('aria-valuetext', '0, 0 px, 0°');
	await tl.getByRole('button', { name: 'Onion skin settings' }).click();
	await tl.getByRole('button', { name: 'Frame 1', exact: true }).click();
	await tl.getByRole('button', { name: 'Frame 2', exact: true }).click();
	await tl.getByRole('button', { name: 'Onion skin settings' }).click();
	await expect(pad).toHaveAttribute('aria-valuetext', '0, 0 px, 0°');
});

test('clone: Alt+click where to copy from, paint the copy, undo it', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	await scribble(page, 0.2);
	await expect.poll(() => ink(page, 0, 0.4)).toBeGreaterThan(100);
	expect(await ink(page, 0.5, 0.8)).toBe(0);
	await page.keyboard.press('j');
	await expect(page.getByRole('group', { name: 'Clone' })).toBeVisible();
	await expect(page.getByTestId('clone-hint')).toBeVisible();
	// the source: where the scribble starts; then the same stroke 40% lower
	const b = (await canvas(page).boundingBox())!;
	await page.keyboard.down('Alt');
	await page.mouse.click(b.x + b.width * 0.2, b.y + b.height * 0.2);
	await page.keyboard.up('Alt');
	await expect(page.getByTestId('clone-hint')).toHaveCount(0);
	await scribble(page, 0.6);
	await expect.poll(() => ink(page, 0.5, 0.8)).toBeGreaterThan(100);
	await expect(page.getByTestId('clone-source')).toBeVisible();
	await page.keyboard.press('Control+z');
	await expect.poll(() => ink(page, 0.5, 0.8)).toBe(0);
});

test('layer style: an outline, undone in one step, kept in the file', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	await scribble(page, 0.4);
	/** red pixels on the canvas */
	const red = () =>
		canvas(page).evaluate((el: HTMLCanvasElement) => {
			const d = el.getContext('2d')!.getImageData(0, 0, el.width, el.height).data;
			let n = 0;
			for (let i = 0; i < d.length; i += 4) if (d[i] > 200 && d[i + 1] < 60) n++;
			return n;
		});
	expect(await red()).toBe(0);
	await page.getByRole('tab', { name: /Layers/ }).click();
	await page.getByText('Layer style').click();
	await page.getByRole('button', { name: 'Stroke', exact: true }).click();
	await page.getByLabel('Stroke colour').fill('#ff0000');
	await page.getByLabel('Stroke colour').blur();
	await expect.poll(red).toBeGreaterThan(200);
	// the colour change is one step, turning it on another
	await page.keyboard.press('Control+z');
	await expect.poll(red).toBe(0);
	await page.keyboard.press('Control+Shift+z');
	await expect.poll(red).toBeGreaterThan(200);
	const outlined = await red();
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	await scribble(page, 0.8);
	await page.keyboard.press('Control+o');
	await page
		.getByRole('dialog', { name: 'Save before opening another project?' })
		.getByRole('button', { name: 'No' })
		.click();
	await expect.poll(async () => Math.abs((await red()) - outlined)).toBeLessThan(outlined * 0.02);
});

test('text: typed on the canvas, one undo step, changed later, kept in the file, rasterized', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	const layersTab = page.getByRole('tab', { name: /Layers/ });
	await page.keyboard.press('t');
	await expect(page.getByRole('group', { name: 'Text' })).toBeVisible();
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.click(b.x + b.width * 0.2, b.y + b.height * 0.3);
	const typed = page.getByRole('textbox', { name: 'Text', exact: true });
	await expect(typed).toBeFocused();
	await page.keyboard.type('Hello');
	await expect(page.getByTestId('text-frame')).toBeVisible();
	// shown on the canvas as it's typed
	await expect.poll(() => ink(page, 0.2, 0.4)).toBeGreaterThan(100);
	// (the whole word drawn: the count stops changing)
	let drawn = -1;
	await expect
		.poll(async () => {
			const was = drawn;
			drawn = await ink(page);
			return drawn === was;
		})
		.toBe(true);
	await page.getByRole('group', { name: 'Text' }).getByRole('button', { name: 'Done' }).click();
	await expect(layersTab).toContainText('2 layers');
	const badge = page.getByRole('button', { name: /Change the text of/ });
	await expect(badge).toBeVisible();
	await expect.poll(() => ink(page)).toBe(drawn);

	// one undo takes it away (layer and all), redo brings it back
	await page.keyboard.press('Control+z');
	await expect(layersTab).toContainText('1 layer');
	await expect.poll(() => ink(page)).toBe(0);
	await page.keyboard.press('Control+Shift+z');
	await expect(layersTab).toContainText('2 layers');
	await expect.poll(() => ink(page)).toBe(drawn);

	// change it: longer
	await badge.click();
	await expect(typed).toHaveValue('Hello');
	await typed.press('End');
	await page.keyboard.type(' there');
	await page.keyboard.press('Control+Enter');
	await expect(page.getByTestId('text-frame')).toHaveCount(0);
	await expect.poll(() => ink(page)).toBeGreaterThan(drawn);
	const longer = await ink(page);

	// saved and opened again: still a text layer, with its text
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	await page.keyboard.press('Control+o');
	await expect.poll(() => ink(page)).toBe(longer);
	await page.getByRole('button', { name: /Change the text of/ }).click();
	await expect(typed).toHaveValue('Hello there');
	await typed.press('Escape');

	// painting on it asks to rasterize first
	await page.keyboard.press('b');
	await scribble(page, 0.3);
	const ask = page.getByRole('alertdialog', { name: 'Rasterize the text?' });
	await expect(ask).toBeVisible();
	expect(await ink(page)).toBe(longer);
	await ask.getByRole('button', { name: 'Rasterize' }).click();
	await expect(page.getByRole('button', { name: /Change the text of/ })).toHaveCount(0);
	await scribble(page, 0.3);
	await expect.poll(() => ink(page)).toBeGreaterThan(longer);
});

test('the top bar menus close on a press outside them (the canvas too) or Escape', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	const file = page.getByRole('group', { name: 'File' });
	await page.getByRole('button', { name: 'File', exact: true }).click();
	await expect(file).toBeVisible();
	// a press on the canvas: closes it without drawing a dot
	const before = await ink(page);
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.click(b.x + b.width * 0.6, b.y + b.height * 0.6);
	await expect(file).toBeHidden();
	expect(await ink(page)).toBe(before);
	// Escape
	await page.getByRole('button', { name: 'File', exact: true }).click();
	await expect(file).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(file).toBeHidden();
	// another menu: only that one stays open
	await page.getByRole('button', { name: 'File', exact: true }).click();
	await page.getByRole('button', { name: 'Import', exact: true }).click();
	await expect(file).toBeHidden();
	await expect(page.getByRole('group', { name: 'Import' })).toBeVisible();
	// its own button closes it again
	await page.getByRole('button', { name: 'Import', exact: true }).click();
	await expect(page.getByRole('group', { name: 'Import' })).toBeHidden();
});

/** the box around the ink on the canvas, in canvas px (null: none), and the canvas width */
async function inkBox(page: Page) {
	return canvas(page).evaluate((el: HTMLCanvasElement) => {
		const d = el.getContext('2d')!.getImageData(0, 0, el.width, el.height).data;
		let [x0, y0, x1, y1] = [el.width, el.height, -1, -1];
		for (let i = 0; i < d.length; i += 4)
			if (d[i] < 200) {
				const p = i / 4;
				const [x, y] = [p % el.width, Math.floor(p / el.width)];
				[x0, y0, x1, y1] = [Math.min(x0, x), Math.min(y0, y), Math.max(x1, x), Math.max(y1, y)];
			}
		return x1 < 0 ? null : { x0, y0, x1, y1, w: el.width };
	});
}
type Box = { x0: number; y0: number; x1: number; y1: number; w: number };

test('canvas size: every frame moves with it, one undo step, kept in the file', async ({
	page
}) => {
	await stubTauri(page);
	// one drawing in the window besides the ones showing: the rest come from the store
	await page.addInitScript(() => localStorage.setItem('pp.dev.keepDrawings', '1'));
	await ready(page);
	const tl = page.getByRole('region', { name: 'Timeline' });
	/** the ink's box once it stops changing (a frame coming from the store, a redraw) */
	const settled = async () => {
		let last = '';
		await expect
			.poll(async () => {
				const before = last;
				last = JSON.stringify(await inkBox(page));
				return last === before && last !== 'null';
			})
			.toBe(true);
		return JSON.parse(last) as Box;
	};
	/** the ink is where `want` was, times k, moved by (dx, dy) */
	const lands = async (want: Box, w: number, k = 1, dx = 0, dy = 0, slack = 2) => {
		const at = (b: Box | null) =>
			!!b &&
			b.w === w &&
			[
				[b.x0, want.x0 * k + dx],
				[b.x1, want.x1 * k + dx],
				[b.y0, want.y0 * k + dy],
				[b.y1, want.y1 * k + dy]
			].every(([a, e]) => Math.abs(a - e) <= slack);
		await expect.poll(async () => at(await inkBox(page))).toBe(true);
	};
	// three frames: a line at the top, the middle, the bottom
	const boxes: Box[] = [];
	for (const [i, y] of [0.2, 0.45, 0.7].entries()) {
		if (i) await tl.getByRole('button', { name: 'New drawing' }).click();
		await scribble(page, y);
		boxes.push(await settled());
	}
	expect(boxes[0].w).toBe(1920);
	const frame = (n: number) => tl.getByRole('button', { name: `Frame ${n}`, exact: true }).click();
	const openSize = async () => {
		await page.getByRole('button', { name: 'File', exact: true }).click();
		await page.getByRole('button', { name: /Canvas size…/ }).click();
		return page.getByRole('dialog', { name: 'Canvas size' });
	};

	// more room, anchored in the middle: 2880 × 1620 (the aspect ratio stays locked)
	let dlg = await openSize();
	await dlg.getByLabel('Width').fill('2880');
	await dlg.getByLabel('Width').press('Tab');
	await expect(dlg.getByLabel('Height')).toHaveValue('1620');
	await expect(dlg.getByRole('radio', { name: 'Keep the middle in place' })).toHaveAttribute(
		'aria-checked',
		'true'
	);
	await dlg.getByRole('button', { name: 'Resize canvas' }).click();
	await expect(dlg).toHaveCount(0);
	// frame 3 shows: moved by (480, 270); and so are frames 1 and 2 (from the store)
	await lands(boxes[2], 2880, 1, 480, 270);
	for (const n of [1, 2]) {
		await frame(n);
		await lands(boxes[n - 1], 2880, 1, 480, 270);
	}
	// one undo: every frame as it was
	await page.keyboard.press('Control+z');
	await lands(boxes[1], 1920);
	for (const n of [1, 3]) {
		await frame(n);
		await lands(boxes[n - 1], 1920);
	}
	// redo, and undo again
	await page.keyboard.press('Control+Shift+z');
	await lands(boxes[2], 2880, 1, 480, 270);
	await page.keyboard.press('Control+z');
	await lands(boxes[2], 1920);

	// half the size, the drawing scaled with it
	dlg = await openSize();
	await dlg.getByLabel('Resize the drawing too').check();
	await dlg.getByLabel('Width').fill('960');
	await dlg.getByLabel('Width').press('Tab');
	await expect(dlg.getByLabel('Height')).toHaveValue('540');
	await dlg.getByRole('button', { name: 'Resize canvas' }).click();
	await lands(boxes[2], 960, 0.5, 0, 0, 3);
	await frame(1);
	await lands(boxes[0], 960, 0.5, 0, 0, 3);

	// saved and opened again: the new size, the frames as they are now
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	await page.keyboard.press('Control+o');
	await expect(tl.getByRole('button', { name: /drawing 3, frame 3$/ })).toBeVisible({
		timeout: 20_000
	});
	await frame(2);
	await lands(boxes[1], 960, 0.5, 0, 0, 3);
});

test('a new canvas starts with its guides in the middle; Re-centre puts moved ones back', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	/** the mirror line's x as a share of the canvas width */
	const axisAt = () =>
		page.evaluate(() => {
			const line = document.querySelector('line.axis');
			const c = document.querySelector('canvas[aria-label*="Drawing canvas"]') as HTMLCanvasElement;
			return line ? Number(line.getAttribute('x1')) / c.width : null;
		});
	// last time: a vertical mirror moved far to the right (as kept by the app)
	await page.evaluate(() =>
		localStorage.setItem(
			'pp.app.guides',
			JSON.stringify({
				w: 1000,
				h: 800,
				g: {
					grid: { on: false, kind: 'square', size: 100 },
					perspective: {
						points: 0,
						vps: [
							[500, 320],
							[850, 320],
							[500, 1280]
						]
					},
					ruler: { on: false, a: [200, 400], b: [800, 400] },
					symmetry: { mode: 'vertical', n: 6, c: [900, 100] }
				}
			})
		)
	);
	// reopening the app (a new canvas): the mirror is still on, but in the middle
	await ready(page);
	await expect.poll(axisAt).toBeCloseTo(0.5, 2);

	// moved by hand, then Re-centre
	await page.getByRole('button', { name: 'Guides', exact: true }).click();
	await page.getByRole('button', { name: 'Move guides' }).click();
	const h = (await page.locator('circle.handle.sym').boundingBox())!;
	await page.mouse.move(h.x + h.width / 2, h.y + h.height / 2);
	await page.mouse.down();
	await page.mouse.move(h.x + 150, h.y + 40, { steps: 5 });
	await page.mouse.up();
	await expect.poll(axisAt).toBeGreaterThan(0.55);
	await page.getByRole('button', { name: 'Re-centre' }).click();
	await expect.poll(axisAt).toBeCloseTo(0.5, 2);
});

test('line layers: strokes stay editable (select, move, recolour, erase, reshape, save, convert)', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	const layersTab = page.getByRole('tab', { name: /Layers/ });
	await layersTab.click();
	await page.getByRole('button', { name: 'New line layer' }).click();
	await expect(layersTab).toContainText('2 layers');
	await expect(page.getByRole('img', { name: 'Line layer' })).toBeVisible();
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	// (the tool options row changes with the tool: the canvas moves, so it's looked up again)
	let b = (await canvas(page).boundingBox())!;
	const fresh = async () => {
		await page.waitForTimeout(100);
		b = (await canvas(page).boundingBox())!;
	};
	const at = (x: number, y: number) => [b.x + b.width * x, b.y + b.height * y] as const;
	/** a straight stroke across, at height y */
	const line = async (y: number) => {
		await page.mouse.move(...at(0.2, y));
		await page.mouse.down();
		await page.mouse.move(...at(0.8, y), { steps: 20 });
		await page.mouse.up();
	};
	await line(0.3);
	await line(0.6);
	await expect.poll(() => ink(page, 0.25, 0.35)).toBeGreaterThan(200);
	// (both drawn to the end: the canvas shows changes on the next frame)
	await page.waitForTimeout(300);
	const lower = await ink(page, 0.55, 0.65);
	expect(lower).toBeGreaterThan(200);
	const upper = await ink(page, 0.25, 0.35);

	// Select line: a tap picks the upper stroke, a drag moves it down; the other stays
	await page.keyboard.press('Shift+V');
	await fresh();
	const sel = page.getByTestId('line-selection');
	await page.mouse.click(...at(0.5, 0.3));
	await expect(sel).toHaveAttribute('data-count', '1');
	await expect(page.getByTestId('lines-picked')).toContainText('1 line selected');
	await page.mouse.move(...at(0.5, 0.3));
	await page.mouse.down();
	await page.mouse.move(...at(0.5, 0.45), { steps: 8 });
	await page.mouse.up();
	await expect.poll(() => ink(page, 0.25, 0.35)).toBe(0);
	await expect.poll(() => ink(page, 0.4, 0.5)).toBeGreaterThan(upper * 0.9);
	expect(await ink(page, 0.55, 0.65)).toBe(lower);
	// one undo step: back, and forward again
	await page.keyboard.press('Control+z');
	await expect.poll(() => ink(page, 0.25, 0.35)).toBe(upper);
	await page.keyboard.press('Control+Shift+z');
	await expect.poll(() => ink(page, 0.25, 0.35)).toBe(0);

	// recolour the selected stroke: only it turns red
	const red = (y0: number, y1: number) =>
		canvas(page).evaluate(
			(el: HTMLCanvasElement, [y0, y1]) => {
				const top = Math.round(el.height * y0);
				const h = Math.round(el.height * (y1 - y0));
				const d = el.getContext('2d')!.getImageData(0, top, el.width, h).data;
				let n = 0;
				for (let i = 0; i < d.length; i += 4) if (d[i] - d[i + 1] > 20) n++;
				return n;
			},
			[y0, y1]
		);
	const moved = await ink(page, 0.4, 0.5);
	await page.getByRole('radio', { name: 'Colour #e8492f' }).click();
	// (all of it: no dark pencil left there)
	await expect.poll(() => red(0.4, 0.5)).toBeGreaterThan(moved * 0.3);
	expect(await ink(page, 0.4, 0.5)).toBeLessThan(moved * 0.05);
	expect(await red(0.55, 0.65)).toBe(0);

	// the eraser, whole lines: one touch takes the lower stroke away
	await page.keyboard.press('Escape');
	await page.keyboard.press('e');
	await fresh();
	await page.getByRole('radio', { name: 'Whole lines' }).click();
	await fresh();
	await page.mouse.move(...at(0.5, 0.58));
	await page.mouse.down();
	await page.mouse.move(...at(0.5, 0.62), { steps: 3 });
	await page.mouse.up();
	await expect.poll(() => ink(page, 0.55, 0.65)).toBe(0);
	expect(await red(0.4, 0.5)).toBeGreaterThan(moved * 0.3);

	// a QuickShape circle, opened again later and made a perfect circle
	await page.keyboard.press('b');
	await fresh();
	const [cx, cy] = at(0.5, 0.75);
	const r = b.height * 0.08;
	// a rough oval, held at the end
	await page.mouse.move(cx + r * 1.4, cy);
	await page.mouse.down();
	for (let i = 1; i <= 48; i++) {
		const t = (i / 46) * Math.PI * 2;
		await page.mouse.move(cx + r * 1.4 * Math.cos(t) * (1 + (i % 3) * 0.015), cy + r * Math.sin(t));
	}
	await page.waitForTimeout(800);
	await expect(page.getByRole('status').filter({ hasText: 'Snapped to an ellipse' })).toBeVisible();
	await page.mouse.up();
	await expect(page.getByRole('toolbar', { name: 'Edit shape' })).toBeVisible();
	await page.keyboard.press('Shift+V');
	await fresh();
	await expect(page.getByRole('toolbar', { name: 'Edit shape' })).toHaveCount(0);
	await page.mouse.click(cx + r * 1.4, cy);
	const bar = page.getByRole('toolbar', { name: 'Edit shape' });
	await expect(bar).toBeVisible();
	await expect(bar.getByRole('button', { name: 'Ellipse' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	const wide = await red(0.6, 0.9);
	await bar.getByRole('button', { name: 'Circle' }).click();
	await expect(bar.getByRole('button', { name: 'Circle' })).toHaveAttribute('aria-pressed', 'true');
	// (a circle, as big on average, reaches further up and down than the flat ellipse)
	await expect.poll(() => red(0.6, 0.9)).not.toBe(wide);
	const circle = await red(0.6, 0.9);
	await page.keyboard.press('Control+z');
	await expect.poll(() => red(0.6, 0.9)).toBe(wide);
	await page.keyboard.press('Control+Shift+z');
	await expect.poll(() => red(0.6, 0.9)).toBe(circle);

	// fill on a line layer: inside its lines, on the plain layer under it (one step);
	// the line layer stays a line layer and stays the one drawn on
	await page.keyboard.press('g');
	await fresh();
	await expect(page.getByRole('note')).toContainText('fills go on the layer under it');
	const inside = await red(0.72, 0.78);
	await page.mouse.click(...at(0.5, 0.75));
	await expect.poll(() => red(0.72, 0.78)).toBeGreaterThan(inside + 2000);
	await expect(layersTab).toContainText('Lines 2 · 2 layers');
	await expect(page.getByRole('img', { name: 'Line layer' })).toBeVisible();
	await page.keyboard.press('Control+z');
	await expect.poll(() => red(0.72, 0.78)).toBe(inside);

	// saved and opened again: the strokes are still lines to select
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	const saved = await red(0, 1);
	expect(saved).toBeGreaterThan(400);
	// (a change since, so opening asks; no: the file as saved comes back)
	await page.keyboard.press('Control+o');
	await expect.poll(() => red(0, 1)).toBe(saved);
	await expect(page.getByRole('img', { name: 'Line layer' })).toBeVisible();
	const selectLine = page.getByRole('button', { name: /Select line/ });
	if ((await selectLine.getAttribute('aria-pressed')) !== 'true') await selectLine.click();
	await fresh();
	await page.mouse.click(...at(0.5, 0.45));
	await expect(page.getByTestId('line-selection')).toHaveAttribute('data-count', '1');
	await page.keyboard.press('Delete');
	await expect.poll(() => red(0.4, 0.5)).toBe(0);

	// convert to pixels: a plain layer (Select line has nothing to pick then); undo: lines again
	await layersTab.click();
	await page.getByRole('button', { name: 'Convert to pixels' }).click();
	await fresh();
	await expect(page.getByRole('img', { name: 'Line layer' })).toHaveCount(0);
	await expect(page.getByRole('group', { name: 'Select line' })).toContainText(
		'No editable strokes on this layer'
	);
	await page.keyboard.press('Control+z');
	await expect(page.getByRole('img', { name: 'Line layer' })).toBeVisible();
});

test('Ctrl + drag moves the view (no ink)', async ({ page }) => {
	await stubTauri(page);
	await ready(page);
	const before = (await canvas(page).boundingBox())!;
	const inkBefore = await ink(page);
	const [x, y] = [before.x + before.width * 0.5, before.y + before.height * 0.5];
	await page.mouse.move(x, y);
	await page.keyboard.down('Control');
	await expect(page.locator('.viewport.grab')).toBeVisible();
	await page.mouse.down();
	await page.mouse.move(x + 120, y + 60, { steps: 6 });
	await page.mouse.up();
	await page.keyboard.up('Control');
	const after = (await canvas(page).boundingBox())!;
	expect(after.x - before.x).toBeGreaterThan(60);
	expect(after.y - before.y).toBeGreaterThan(30);
	expect(await ink(page)).toBe(inkBefore);
	await expect(page.locator('.viewport.grab')).toHaveCount(0);
});

// ---- editable strokes on plain layers ----

/** dark pixels in a box of the canvas (fractions) */
async function inkIn(page: Page, x0: number, y0: number, x1: number, y1: number) {
	return canvas(page).evaluate(
		(el: HTMLCanvasElement, [x0, y0, x1, y1]) => {
			const [l, t] = [Math.round(el.width * x0), Math.round(el.height * y0)];
			const w = Math.max(1, Math.round(el.width * (x1 - x0)));
			const h = Math.max(1, Math.round(el.height * (y1 - y0)));
			const d = el.getContext('2d')!.getImageData(l, t, w, h).data;
			let n = 0;
			for (let i = 0; i < d.length; i += 4) if (d[i] < 200) n++;
			return n;
		},
		[x0, y0, x1, y1]
	);
}

/** reddish pixels in a box of the canvas (fractions) */
async function redIn(page: Page, x0: number, y0: number, x1: number, y1: number) {
	return canvas(page).evaluate(
		(el: HTMLCanvasElement, [x0, y0, x1, y1]) => {
			const [l, t] = [Math.round(el.width * x0), Math.round(el.height * y0)];
			const w = Math.max(1, Math.round(el.width * (x1 - x0)));
			const h = Math.max(1, Math.round(el.height * (y1 - y0)));
			const d = el.getContext('2d')!.getImageData(l, t, w, h).data;
			let n = 0;
			for (let i = 0; i < d.length; i += 4) if (d[i] - d[i + 1] > 20) n++;
			return n;
		},
		[x0, y0, x1, y1]
	);
}

/** helpers to draw at canvas fractions (the canvas is looked up again with `fresh`) */
async function strokeKit(page: Page) {
	let b = (await canvas(page).boundingBox())!;
	const at = (x: number, y: number) => [b.x + b.width * x, b.y + b.height * y] as const;
	return {
		at,
		fresh: async () => {
			await page.waitForTimeout(100);
			b = (await canvas(page).boundingBox())!;
		},
		/** a straight stroke from (x0, y0) to (x1, y1) */
		line: async (x0: number, y0: number, x1: number, y1: number) => {
			await page.mouse.move(...at(x0, y0));
			await page.mouse.down();
			await page.mouse.move(...at(x1, y1), { steps: 20 });
			await page.mouse.up();
			await page.waitForTimeout(50);
		},
		drag: async (x0: number, y0: number, x1: number, y1: number) => {
			await page.mouse.move(...at(x0, y0));
			await page.mouse.down();
			await page.mouse.move(...at(x1, y1), { steps: 8 });
			await page.mouse.up();
		}
	};
}

test('editable strokes on a plain layer: Move picks one, it moves over what is under it, recolour, undo', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	const k = await strokeKit(page);
	// across, then down through it (on top)
	await k.line(0.2, 0.4, 0.8, 0.4);
	await k.line(0.5, 0.2, 0.5, 0.7);
	await page.waitForTimeout(300);
	const left = await inkIn(page, 0.25, 0.35, 0.4, 0.45);
	expect(left).toBeGreaterThan(100);
	const crossing = await inkIn(page, 0.46, 0.37, 0.54, 0.43);
	const column = await inkIn(page, 0.46, 0.25, 0.54, 0.31);

	// Move: a tap on the stroke across picks it (not the layer)
	await page.keyboard.press('v');
	await k.fresh();
	await page.mouse.click(...k.at(0.3, 0.4));
	const sel = page.getByTestId('line-selection');
	await expect(sel).toHaveAttribute('data-count', '1');
	// (its options show instead of Move's: the canvas may move)
	await k.fresh();
	// a drag moves it down; the stroke down stays whole where they crossed
	await k.drag(0.3, 0.4, 0.3, 0.6);
	await expect.poll(() => inkIn(page, 0.25, 0.35, 0.4, 0.45)).toBe(0);
	await expect.poll(() => inkIn(page, 0.25, 0.55, 0.4, 0.65)).toBeGreaterThan(left * 0.9);
	const there = await inkIn(page, 0.46, 0.37, 0.54, 0.43);
	expect(there).toBeGreaterThan(column * 0.8);
	expect(there).toBeLessThan(crossing);
	expect(await inkIn(page, 0.6, 0.37, 0.75, 0.43)).toBe(0);
	// one undo step
	await page.keyboard.press('Control+z');
	await expect.poll(() => inkIn(page, 0.25, 0.35, 0.4, 0.45)).toBe(left);
	expect(await inkIn(page, 0.46, 0.37, 0.54, 0.43)).toBe(crossing);
	await page.keyboard.press('Control+Shift+z');
	await expect.poll(() => inkIn(page, 0.25, 0.35, 0.4, 0.45)).toBe(0);

	// recolour: only it turns red
	await expect(sel).toHaveAttribute('data-count', '1');
	await page.getByRole('radio', { name: 'Colour #e8492f' }).click();
	await expect.poll(() => redIn(page, 0.25, 0.55, 0.4, 0.65)).toBeGreaterThan(left * 0.3);
	expect(await redIn(page, 0.46, 0.2, 0.54, 0.5)).toBe(0);
	// a tap away puts Move back, its strokes showing; a drag off them moves the whole layer
	await page.mouse.click(...k.at(0.9, 0.9));
	await expect(sel).toHaveCount(0);
	await expect(page.getByTestId('stroke-outlines')).toHaveAttribute('data-count', '2');
	await k.fresh();
	const red = await redIn(page, 0, 0, 1, 1);
	await k.drag(0.7, 0.3, 0.7, 0.35);
	await expect(page.getByTestId('transform-box')).toBeVisible();
	await page.keyboard.press('Enter');
	await expect.poll(() => redIn(page, 0.25, 0.6, 0.4, 0.7)).toBeGreaterThan(0);
	expect(await redIn(page, 0, 0, 1, 1)).toBeGreaterThan(red * 0.8);
	// the strokes moved with it and are still editable: Move picks one and drags it
	// (Done puts the brush back)
	await page.keyboard.press('v');
	await expect(page.getByTestId('stroke-outlines')).toHaveAttribute('data-count', '2');
	await k.fresh();
	const movedRed = await redIn(page, 0.2, 0.6, 0.45, 0.7);
	await k.drag(0.3, 0.65, 0.3, 0.85);
	await expect.poll(() => redIn(page, 0.2, 0.6, 0.45, 0.7)).toBe(0);
	await expect.poll(() => redIn(page, 0.2, 0.8, 0.45, 0.9)).toBeGreaterThan(movedRed * 0.8);
	// the stroke down (moved with the layer too) stays whole where they crossed
	expect(await inkIn(page, 0.46, 0.42, 0.54, 0.48)).toBeGreaterThan(column * 0.8);
	// one undo step each: the stroke back, then the layer back
	await page.keyboard.press('Control+z');
	await expect.poll(() => redIn(page, 0.2, 0.6, 0.45, 0.7)).toBe(movedRed);
	await page.keyboard.press('Control+z');
	await expect.poll(() => redIn(page, 0.25, 0.55, 0.4, 0.65)).toBeGreaterThan(0);
});

test('Move shows a layer’s editable strokes to pick; no layer box until a drag off them', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	const k = await strokeKit(page);
	await k.line(0.2, 0.3, 0.8, 0.3);
	await k.line(0.2, 0.6, 0.8, 0.6);
	await page.waitForTimeout(300);
	await page.keyboard.press('v');
	await k.fresh();
	// the strokes show (one lit under the pointer); no box round the layer
	const outlines = page.getByTestId('stroke-outlines');
	await expect(outlines).toHaveAttribute('data-count', '2');
	await expect(page.getByTestId('transform-box')).toHaveCount(0);
	await page.mouse.move(...k.at(0.5, 0.6));
	await expect(outlines).not.toHaveAttribute('data-hover', '');
	await page.mouse.move(...k.at(0.5, 0.45));
	await expect(outlines).toHaveAttribute('data-hover', '');
	// a press on a stroke and a drag: it moves, in one gesture, one undo step
	const upper = await inkIn(page, 0.2, 0.25, 0.8, 0.35);
	const lower = await inkIn(page, 0.2, 0.55, 0.8, 0.65);
	await k.drag(0.5, 0.3, 0.5, 0.45);
	await expect.poll(() => inkIn(page, 0.2, 0.25, 0.8, 0.35)).toBe(0);
	await expect.poll(() => inkIn(page, 0.2, 0.4, 0.8, 0.5)).toBeGreaterThan(upper * 0.9);
	expect(await inkIn(page, 0.2, 0.55, 0.8, 0.65)).toBe(lower);
	await page.keyboard.press('Control+z');
	await expect.poll(() => inkIn(page, 0.2, 0.25, 0.8, 0.35)).toBe(upper);
	// a layer without editable strokes: Move's box at once, as before
	await page.keyboard.press('Escape');
	await page.keyboard.press('b');
	await page.getByRole('tab', { name: /Layers/ }).click();
	await page.getByRole('button', { name: 'New layer', exact: true }).click();
	await k.fresh();
	await k.line(0.3, 0.8, 0.7, 0.8);
	await page.getByRole('button', { name: 'Settings', exact: true }).click();
	const dialog = page.getByRole('dialog', { name: 'Settings' });
	await dialog.getByRole('tab', { name: 'History and memory' }).click();
	await page.getByLabel('Editable strokes per layer').fill('0');
	await page.getByLabel('Editable strokes per layer').blur();
	await page.keyboard.press('Escape');
	await page.getByRole('button', { name: 'New layer', exact: true }).click();
	await k.fresh();
	await k.line(0.3, 0.8, 0.7, 0.8);
	await page.keyboard.press('v');
	await expect(page.getByTestId('transform-box')).toBeVisible();
	await expect(outlines).toHaveCount(0);
});

test('a snapped shape drawn with symmetry: editing it changes its mirrored copy too', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	await page.getByRole('button', { name: 'Guides', exact: true }).click();
	await page
		.getByRole('radiogroup', { name: 'Symmetry' })
		.getByRole('radio', { name: 'Vertical' })
		.click();
	await page.getByRole('button', { name: 'Guides', exact: true }).click();
	const k = await strokeKit(page);
	await k.fresh();
	// a rough oval on the left, held at the end: an ellipse (and its mirror)
	const [cx, cy] = k.at(0.25, 0.5);
	const b = (await canvas(page).boundingBox())!;
	const r = b.height * 0.1;
	await page.mouse.move(cx + r * 1.5, cy);
	await page.mouse.down();
	for (let i = 1; i <= 48; i++) {
		const t = (i / 46) * Math.PI * 2;
		await page.mouse.move(cx + r * 1.5 * Math.cos(t) * (1 + (i % 3) * 0.015), cy + r * Math.sin(t));
	}
	await page.waitForTimeout(800);
	await expect(page.getByRole('status').filter({ hasText: 'Snapped to an ellipse' })).toBeVisible();
	await page.mouse.up();
	const bar = page.getByRole('toolbar', { name: 'Edit shape' });
	await expect(bar).toBeVisible();
	await page.waitForTimeout(200);
	/** ink in the left half's box and the same box mirrored */
	const halves = async () => [
		await inkIn(page, 0.05, 0.3, 0.45, 0.7),
		await inkIn(page, 0.55, 0.3, 0.95, 0.7)
	];
	const [l0, r0] = await halves();
	expect(Math.abs(l0 - r0)).toBeLessThan(l0 * 0.1);
	// a perfect circle: both change, and still match
	await bar.getByRole('button', { name: 'Circle' }).click();
	await expect.poll(async () => (await halves())[1]).not.toBe(r0);
	const [l1, r1] = await halves();
	expect(l1).not.toBe(l0);
	expect(Math.abs(l1 - r1)).toBeLessThan(l1 * 0.1);
	// a handle dragged: the mirrored copy follows, live and in one step
	const h = page.getByTestId('shape-handles').locator('circle.shape-h').first();
	const hb = (await h.boundingBox())!;
	await page.mouse.move(hb.x + hb.width / 2, hb.y + hb.height / 2);
	await page.mouse.down();
	await page.mouse.move(hb.x + hb.width / 2 - 40, hb.y + hb.height / 2 - 30, { steps: 6 });
	await page.mouse.up();
	await expect.poll(async () => (await halves())[1]).not.toBe(r1);
	const [l2, r2] = await halves();
	expect(Math.abs(l2 - r2)).toBeLessThan(l2 * 0.1);
	await page.keyboard.press('Control+z');
	await expect.poll(async () => (await halves())[1]).toBe(r1);
});

test('editable strokes: hold the brush still on one, then drag in the same press', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	await page.getByRole('button', { name: 'Guides', exact: true }).click();
	await page
		.getByRole('radiogroup', { name: 'Symmetry' })
		.getByRole('radio', { name: 'Vertical' })
		.click();
	await page.getByRole('button', { name: 'Guides', exact: true }).click();
	const k = await strokeKit(page);
	await k.fresh();
	await k.line(0.15, 0.3, 0.35, 0.3);
	await page.waitForTimeout(300);
	const twin = await inkIn(page, 0.6, 0.25, 0.9, 0.35);
	expect(twin).toBeGreaterThan(50);
	// hold, then move on without lifting: the stroke (and its twin) go along
	const [px, py] = k.at(0.25, 0.3);
	await page.mouse.move(px, py);
	await page.mouse.down();
	await page.waitForTimeout(800);
	await expect(page.getByTestId('line-selection')).toHaveAttribute('data-count', '2');
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.move(px, py + b.height * 0.2, { steps: 10 });
	await page.mouse.up();
	await expect.poll(() => inkIn(page, 0.1, 0.25, 0.9, 0.35)).toBe(0);
	await expect.poll(() => inkIn(page, 0.6, 0.45, 0.9, 0.55)).toBeGreaterThan(twin * 0.8);
	// still picked; one undo step
	await expect(page.getByTestId('line-selection')).toHaveAttribute('data-count', '2');
	await page.keyboard.press('Control+z');
	await expect.poll(() => inkIn(page, 0.6, 0.25, 0.9, 0.35)).toBe(twin);
});

test('a smudging brush’s stroke moved and moved back: the pixels are as they were', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	const k = await strokeKit(page);
	// paint of two colours to smudge, then a stroke with a blending brush across them
	await k.line(0.2, 0.45, 0.8, 0.45);
	await page.getByRole('radio', { name: 'Colour #e8492f' }).click();
	await k.line(0.2, 0.55, 0.8, 0.55);
	await page.getByRole('tab', { name: 'Brushes' }).click();
	await page.getByPlaceholder(/Search .* brushes/).fill('Blend+paint');
	await page
		.getByRole('list', { name: 'Brushes' })
		.getByRole('button', { name: 'Blend+paint', exact: true })
		.click();
	await k.fresh();
	await k.line(0.3, 0.35, 0.6, 0.65);
	await page.waitForTimeout(300);
	/** a fingerprint of every pixel on the canvas */
	const pixels = () =>
		canvas(page).evaluate((el: HTMLCanvasElement) => {
			const d = el.getContext('2d')!.getImageData(0, 0, el.width, el.height).data;
			let h = 2166136261;
			for (let i = 0; i < d.length; i++) h = Math.imul(h ^ d[i], 16777619);
			return h >>> 0;
		});
	const before = await pixels();
	await page.keyboard.press('Shift+V');
	await k.fresh();
	await page.mouse.click(...k.at(0.45, 0.5));
	await expect(page.getByTestId('line-selection')).toHaveAttribute('data-count', '1');
	await page.keyboard.press('Shift+ArrowRight');
	await page.waitForTimeout(150);
	await expect.poll(pixels).not.toBe(before);
	await page.keyboard.press('Shift+ArrowLeft');
	await expect.poll(pixels).toBe(before);
});

test('editable strokes: a fill bakes them, the limit in Preferences, saved and opened again', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	// the limit: 2 strokes a layer
	await page.getByRole('button', { name: 'Settings', exact: true }).click();
	const dialog = page.getByRole('dialog', { name: 'Settings' });
	await dialog.getByRole('tab', { name: 'History and memory' }).click();
	await page.getByLabel('Editable strokes per layer').fill('2');
	await page.getByLabel('Editable strokes per layer').blur();
	await page.keyboard.press('Escape');
	await expect(dialog).toHaveCount(0);
	const k = await strokeKit(page);
	await k.line(0.2, 0.2, 0.8, 0.2);
	await k.line(0.2, 0.5, 0.8, 0.5);
	await k.line(0.2, 0.8, 0.8, 0.8);
	await page.waitForTimeout(300);
	// the first is plain pixels now; the others can be picked
	await page.keyboard.press('Shift+V');
	await k.fresh();
	const sel = page.getByTestId('line-selection');
	await page.mouse.click(...k.at(0.3, 0.2));
	await expect(sel).toHaveCount(0);
	await page.mouse.click(...k.at(0.3, 0.5));
	await expect(sel).toHaveAttribute('data-count', '1');
	await page.keyboard.press('Escape');
	await page.mouse.click(...k.at(0.3, 0.8));
	await expect(sel).toHaveAttribute('data-count', '1');
	await page.keyboard.press('Escape');
	// memory shown in Preferences
	await page.getByRole('button', { name: 'Settings', exact: true }).click();
	await dialog.getByRole('tab', { name: 'History and memory' }).click();
	await expect(page.getByTestId('stroke-memory')).toContainText('2 strokes');
	await page.getByLabel('Editable strokes per layer').fill('200');
	await page.getByLabel('Editable strokes per layer').blur();
	await page.keyboard.press('Escape');

	// saved and opened again: still editable
	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle', { timeout: 20_000 });
	const saved = await ink(page);
	await page.keyboard.press('Control+o');
	await expect.poll(() => ink(page)).toBe(saved);
	const selectLine = page.getByRole('button', { name: /Select line/ });
	if ((await selectLine.getAttribute('aria-pressed')) !== 'true') await selectLine.click();
	await k.fresh();
	await page.mouse.click(...k.at(0.3, 0.5));
	await expect(sel).toHaveAttribute('data-count', '1');
	await page.keyboard.press('Delete');
	await expect.poll(() => inkIn(page, 0.2, 0.45, 0.8, 0.55)).toBe(0);
	expect(await inkIn(page, 0.2, 0.75, 0.8, 0.85)).toBeGreaterThan(100);
	expect(await inkIn(page, 0.2, 0.15, 0.8, 0.25)).toBeGreaterThan(100);
	await page.keyboard.press('Control+z');
	await expect.poll(() => inkIn(page, 0.2, 0.45, 0.8, 0.55)).toBeGreaterThan(100);

	// a fill over the strokes: they become plain pixels (undo: editable again)
	await page.keyboard.press('g');
	await k.fresh();
	await page.mouse.click(...k.at(0.5, 0.65));
	await page.waitForTimeout(200);
	await page.keyboard.press('Shift+V');
	await k.fresh();
	await page.mouse.click(...k.at(0.3, 0.8));
	await expect(sel).toHaveCount(0);
	await page.keyboard.press('Control+z');
	await page.waitForTimeout(200);
	await page.mouse.click(...k.at(0.3, 0.8));
	await expect(sel).toHaveAttribute('data-count', '1');
});

test('mirrored strokes are linked: picking one picks its twin, moving it moves both (plain and line layers)', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	await page.getByRole('button', { name: 'Guides', exact: true }).click();
	await page
		.getByRole('radiogroup', { name: 'Symmetry' })
		.getByRole('radio', { name: 'Vertical' })
		.click();
	await page.getByRole('button', { name: 'Guides', exact: true }).click();
	const k = await strokeKit(page);
	for (const layer of ['plain', 'lines']) {
		if (layer === 'lines') {
			await page.getByRole('tab', { name: /Layers/ }).click();
			await page.getByRole('button', { name: 'New line layer' }).click();
			await page.keyboard.press('b');
		}
		await k.fresh();
		const y = layer === 'plain' ? 0.25 : 0.7;
		await k.line(0.15, y, 0.35, y);
		await page.waitForTimeout(300);
		// the copy on the right is there
		const copy = await inkIn(page, 0.65, y - 0.04, 0.85, y + 0.04);
		expect(copy).toBeGreaterThan(50);
		// a tap on the copy picks both
		await page.keyboard.press('Shift+V');
		await k.fresh();
		await page.mouse.click(...k.at(0.75, y));
		const sel = page.getByTestId('line-selection');
		await expect(sel).toHaveAttribute('data-count', '2');
		await k.fresh();
		// moved right and down: the twin goes left and down
		await k.drag(0.75, y, 0.8, y + 0.1);
		await expect.poll(() => inkIn(page, 0.1, y - 0.03, 0.9, y + 0.03)).toBe(0);
		await expect.poll(() => inkIn(page, 0.7, y + 0.07, 0.9, y + 0.13)).toBeGreaterThan(copy * 0.8);
		expect(await inkIn(page, 0.1, y + 0.07, 0.3, y + 0.13)).toBeGreaterThan(copy * 0.8);
		await page.keyboard.press('Escape');
		await page.keyboard.press('b');
	}
});

test('editable strokes: holding the brush still on one picks it; dots and holds elsewhere still draw', async ({
	page
}) => {
	await stubTauri(page);
	await ready(page);
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	const k = await strokeKit(page);
	await k.line(0.2, 0.4, 0.8, 0.4);
	await page.waitForTimeout(300);
	const drawn = await ink(page);
	/** a press at (x, y) held still `ms`, then drawn `dx` across */
	const press = async (x: number, y: number, ms: number, dx: number) => {
		const [px, py] = k.at(x, y);
		await page.mouse.move(px, py);
		await page.mouse.down();
		await page.waitForTimeout(ms);
		if (dx) await page.mouse.move(px + dx, py + 1, { steps: Math.max(1, Math.round(dx / 3)) });
		await page.mouse.up();
	};
	const sel = page.getByTestId('line-selection');
	// a short stroke on it, not held: it draws, nothing is picked
	await press(0.5, 0.4, 0, 12);
	await expect.poll(() => ink(page)).toBeGreaterThan(drawn);
	await expect(sel).toHaveCount(0);
	const dotted = await ink(page);
	// held still on empty canvas, then drawn: a stroke, nothing picked
	await press(0.5, 0.8, 800, 40);
	await expect(sel).toHaveCount(0);
	await expect.poll(() => ink(page)).toBeGreaterThan(dotted);
	const held = await ink(page);
	// held still on the stroke: it's picked, and the press paints nothing
	await press(0.3, 0.4, 800, 0);
	await expect(sel).toHaveAttribute('data-count', '1');
	expect(await ink(page)).toBe(held);
	await k.fresh();
	await k.drag(0.3, 0.4, 0.3, 0.3);
	await expect.poll(() => inkIn(page, 0.25, 0.37, 0.4, 0.43)).toBe(0);
	// a press away: done, and the brush draws again
	await page.mouse.click(...k.at(0.9, 0.9));
	await expect(sel).toHaveCount(0);
	await k.fresh();
	const before = await ink(page);
	await k.line(0.2, 0.9, 0.4, 0.9);
	await expect.poll(() => ink(page)).toBeGreaterThan(before);
});

test('open a Photoshop file: its layers, groups and masks become a new project', async ({
	page
}) => {
	const { writePsd } = await import('../../../packages/paint/src/psd');
	const W = 400;
	const H = 300;
	const box = (rgba: number[], x0: number, y0: number, x1: number, y1: number) => {
		const px = new Uint8Array(W * H * 4);
		for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) px.set(rgba, (y * W + x) * 4);
		return px;
	};
	const layer = (name: string, rgba: Uint8Array, more = {}) => ({
		kind: 'layer' as const,
		name,
		rgba,
		opacity: 1,
		blend: 'normal',
		visible: true,
		clip: false,
		...more
	});
	const psd = writePsd({
		width: W,
		height: H,
		composite: new Uint8Array(W * H * 4),
		items: [
			layer('Sky', box([120, 170, 230, 255], 0, 0, W, H / 2)),
			{
				kind: 'group',
				name: 'Hero',
				opacity: 1,
				visible: true,
				children: [
					layer('Body', box([20, 20, 20, 255], 150, 100, 250, 280), { blend: 'multiply' }),
					layer('Face', box([240, 200, 170, 255], 170, 60, 230, 120))
				]
			}
		]
	});
	await stubTauri(page);
	await ready(page);
	await page.getByRole('button', { name: 'File', exact: true }).click();
	await page.getByRole('button', { name: /^Open a Photoshop file…/ }).click();
	await page.getByLabel('Photoshop file').setInputFiles({
		name: 'Hero sheet.psd',
		mimeType: 'image/vnd.adobe.photoshop',
		buffer: Buffer.from(psd)
	});
	await expect(page.getByTestId('project-name')).toHaveText('Hero sheet', { timeout: 15_000 });
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true');
	await expect(page.getByRole('tab', { name: /Layers/ })).toContainText('3 layers');
	const list = page.getByRole('list', { name: 'Layers, top to bottom' });
	await expect(list.getByRole('button', { name: 'Hide group Hero' })).toBeVisible();
	for (const name of ['Face', 'Body Multiply', 'Sky'])
		await expect(list.getByRole('button', { name, exact: true })).toBeVisible();
	// the drawing shows, and it's new work: unsaved, offered under the file's name
	await expect.poll(() => ink(page)).toBeGreaterThan(1000);
	await expect(page.getByLabel('not saved')).toBeVisible();
	await page.keyboard.press('Control+s');
	await expect
		.poll(async () => (await calls(page)).some((c) => c.cmd === 'project_save'))
		.toBe(true);

	// a CMYK file: refused, saying why
	await page.getByRole('button', { name: 'File', exact: true }).click();
	await page.getByRole('button', { name: /^Open a Photoshop file…/ }).click();
	const cmyk = Buffer.from(psd);
	cmyk[25] = 4; // colour mode: CMYK
	await page.getByLabel('Photoshop file').setInputFiles({
		name: 'print.psd',
		mimeType: 'image/vnd.adobe.photoshop',
		buffer: cmyk
	});
	await expect(
		page.getByText('Couldn’t open print.psd: it’s in CMYK: save it as RGB first')
	).toBeVisible();
});
