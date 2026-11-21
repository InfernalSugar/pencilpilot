import { expect, test, type Page } from '@playwright/test';

// Saving and opening projects, through the window. Tauri's IPC is stubbed: "saving"
// keeps the packet the window sends, "opening" hands it back, and the native dialogs
// answer with a fixed path. (src-tauri/src/project.rs tests the files themselves.)

async function stubTauri(page: Page) {
	await page.addInitScript(() => {
		const w = window as unknown as Record<string, unknown>;
		let saved: Uint8Array | null = null;
		const calls: string[] = [];
		w.__calls = calls;
		w.isTauri = true;
		w.__TAURI_INTERNALS__ = {
			transformCallback: () => 0,
			unregisterCallback: () => {},
			metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
			invoke: async (
				cmd: string,
				args: unknown,
				options?: { headers?: Record<string, string> }
			) => {
				calls.push(cmd);
				if (cmd === 'plugin:dialog|save' || cmd === 'plugin:dialog|open')
					return '/home/me/Walk cycle.ppstudio';
				if (cmd === 'project_save') {
					saved = new Uint8Array(args as Uint8Array);
					return decodeURIComponent(options?.headers?.path ?? '');
				}
				if (cmd === 'project_open') {
					if (!saved) throw new Error('nothing saved');
					return saved.slice().buffer;
				}
				if (cmd === 'app_info') return { version: '0.1.0', os: 'linux', arch: 'x86_64' };
				throw new Error(`unexpected command ${cmd}`);
			}
		};
	});
}

const canvas = (page: Page) => page.getByLabel(/Drawing canvas/);

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

async function scribble(page: Page, y: number) {
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.move(b.x + b.width * 0.2, b.y + b.height * y);
	await page.mouse.down();
	await page.mouse.move(b.x + b.width * 0.8, b.y + b.height * (y + 0.05), { steps: 20 });
	await page.mouse.up();
}

test('save a drawing, change it, open it again', async ({ page }) => {
	await stubTauri(page);
	await page.setViewportSize({ width: 1440, height: 960 });
	await page.goto('/');
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await expect(page.getByTestId('project-name')).toHaveText('Untitled');
	for (let i = 0; i < 4; i++) await page.keyboard.press(']');
	await scribble(page, 0.3);
	// a second layer with its own line
	await page.getByRole('button', { name: 'New layer' }).click();
	await scribble(page, 0.6);
	await expect(page.getByLabel('not saved')).toBeVisible();
	const drawn = await ink(page);
	expect(drawn).toBeGreaterThan(200);

	await page.keyboard.press('Control+s');
	await expect(page.getByTestId('project-name')).toHaveText('Walk cycle');
	await expect(page.getByLabel('not saved')).toHaveCount(0);

	// change it, then open the saved version: the change is gone, both layers are back
	await scribble(page, 0.85);
	await expect(page.getByLabel('not saved')).toBeVisible();
	await page.keyboard.press('Control+o');
	await expect(page.getByLabel('not saved')).toHaveCount(0);
	// the unsaved line is gone; the saved ones are back (8-bit colour in the file rounds
	// faint edges a little, as in Photoshop / Krita 8-bit documents)
	await expect.poll(() => ink(page, 0.8, 1)).toBe(0);
	const back = await ink(page);
	expect(Math.abs(back - drawn) / drawn).toBeLessThan(0.02);
	await expect(page.getByRole('tab', { name: /Layers/ })).toContainText('2 layers');
	const calls = await page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);
	expect(calls).toEqual(
		expect.arrayContaining([
			'plugin:dialog|save',
			'project_save',
			'plugin:dialog|open',
			'project_open'
		])
	);
});
