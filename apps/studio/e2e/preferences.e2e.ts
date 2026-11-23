import { expect, test, type Page } from '@playwright/test';

// Preferences (Settings) at work: Tauri's IPC is stubbed, its settings file answered
// with what each test sets.

const SHOTS = process.env.PP_SHOTS;

async function stub(page: Page, settings: Record<string, unknown> = {}) {
	await page.addInitScript((settings) => {
		const w = window as unknown as Record<string, unknown>;
		const calls: { cmd: string; json: string }[] = [];
		w.__calls = calls;
		w.isTauri = true;
		w.__TAURI_INTERNALS__ = {
			transformCallback: () => 0,
			unregisterCallback: () => {},
			metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
			invoke: async (cmd: string, args: unknown) => {
				calls.push({
					cmd,
					json: args instanceof Uint8Array ? '' : JSON.stringify(args ?? null)
				});
				switch (cmd) {
					case 'settings_get':
						return settings;
					case 'plugin:dialog|open':
						return '/home/me/Walk cycle.ppstudio';
					case 'project_open':
						throw new Error('no such file here');
					case 'recent_list':
					case 'recovery_list':
					case 'versions_list':
						return [];
					default:
						return null;
				}
			}
		};
	}, settings);
}

const canvas = (page: Page) => page.getByLabel(/Drawing canvas/);
const cmds = (page: Page) =>
	page.evaluate(() =>
		(window as unknown as { __calls: { cmd: string }[] }).__calls.map((c) => c.cmd)
	);

async function studio(page: Page, url = '/studio') {
	await page.setViewportSize({ width: 1440, height: 960 });
	await page.goto(url);
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
}

async function scribble(page: Page, y = 0.5) {
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.move(b.x + b.width * 0.2, b.y + b.height * y);
	await page.mouse.down();
	await page.mouse.move(b.x + b.width * 0.8, b.y + b.height * (y + 0.05), { steps: 20 });
	await page.mouse.up();
}

test('settings: the preference groups change and are kept', async ({ page }) => {
	await stub(page);
	await page.setViewportSize({ width: 1280, height: 900 });
	await page.goto('/settings');
	const tab = (name: string) => page.getByRole('tab', { name, exact: true });
	const shot = async (name: string) => {
		if (SHOTS)
			await page.locator('#settings-panel').screenshot({ path: `${SHOTS}/settings-${name}.png` });
	};
	// the interface group is part of Appearance, with the theme
	await tab('Appearance').click();
	await expect(page.getByRole('heading', { name: 'Size, colours and hands' })).toBeVisible();
	await page.getByLabel('Left-handed: tools on the right, panels on the left').check();
	await shot('appearance');
	for (const h of [
		'Cursor',
		'Brushes',
		'Canvas and zoom',
		'Selection',
		'Guides and new projects',
		'History and memory',
		'Exports and start-up',
		'Questions and sounds'
	]) {
		await tab(h).click();
		await expect(page.getByRole('heading', { name: h, exact: true, level: 2 })).toBeVisible();
	}
	await tab('Cursor').click();
	await page.getByLabel('A crosshair', { exact: true }).check();
	await shot('cursor');
	await tab('Canvas and zoom').click();
	await page.getByLabel('Units').selectOption('cm');
	await shot('canvas');
	await tab('History and memory').click();
	await page.getByLabel('Undo steps').fill('200');
	await page.getByLabel('Undo steps').blur();
	await page.getByLabel('Memory for frames, at most').fill('16');
	await shot('performance');
	await tab('Questions and sounds').click();
	await page.getByLabel('Ask “Save before…?” when unsaved work would be left').uncheck();
	await expect
		.poll(async () => {
			const last = await page.evaluate(() =>
				(window as unknown as { __calls: { cmd: string; json: string }[] }).__calls
					.filter((c) => c.cmd === 'settings_set')
					.at(-1)
			);
			return last ? JSON.parse(last.json).settings : null;
		})
		.toMatchObject({
			leftHanded: true,
			cursor: 'crosshair',
			units: 'cm',
			undoSteps: 200,
			frameMemoryMb: 16384,
			askToSave: false
		});
	// a number out of range comes back into it once typed
	await tab('History and memory').click();
	await page.getByLabel('Undo steps').fill('5000');
	await page.getByLabel('Undo steps').blur();
	await expect(page.getByLabel('Undo steps')).toHaveValue('500');
	for (const g of ['Guides and new projects', 'Exports and start-up']) {
		await tab(g).click();
		await shot(g.split(' ')[0].toLowerCase());
	}
	// dark, as the rest of Settings
	await tab('Appearance').click();
	await page.getByLabel('Dark', { exact: true }).check();
	await shot('appearance-dark');
});

test('left-handed: the tools are on the right of the canvas', async ({ page }) => {
	await stub(page);
	await studio(page);
	const rail = page.getByRole('button', { name: /^Brush \(/ });
	expect((await rail.boundingBox())!.x).toBeLessThan((await canvas(page).boundingBox())!.x);

	await stub(page, { leftHanded: true });
	await studio(page);
	const c = (await canvas(page).boundingBox())!;
	expect((await rail.boundingBox())!.x).toBeGreaterThan(c.x + c.width);
	if (SHOTS) await page.screenshot({ path: `${SHOTS}/left-handed.png` });
});

test('the cursor: a crosshair, hidden while painting when asked', async ({ page }) => {
	await stub(page, { cursor: 'crosshair', cursorWhilePainting: false });
	await studio(page);
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
	await page.mouse.move(b.x + b.width / 2 + 5, b.y + b.height / 2);
	const cur = page.getByTestId('brush-cursor');
	await expect(cur).toHaveAttribute('data-look', 'crosshair');
	await expect(cur.locator('path')).toHaveCount(2);
	await expect(cur.locator('circle')).toHaveCount(0);
	// while the stroke is drawn it's gone; it comes back after
	await page.mouse.down();
	await page.mouse.move(b.x + b.width / 2 + 60, b.y + b.height / 2 + 10, { steps: 5 });
	await expect(cur).toHaveCount(0);
	await page.mouse.up();
	await page.mouse.move(b.x + b.width / 2 + 70, b.y + b.height / 2 + 10);
	await expect(cur).toHaveAttribute('data-look', 'crosshair');
});

test('the cursor: the brush outline by default, a dot', async ({ page }) => {
	await stub(page, { cursor: 'dot' });
	await studio(page);
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
	await page.mouse.move(b.x + b.width / 2 + 5, b.y + b.height / 2);
	await expect(page.getByTestId('brush-cursor')).toHaveAttribute('data-look', 'dot');
});

test('the scroll wheel zooms when Settings says so (towards the pointer)', async ({ page }) => {
	const zoomLabel = () => page.getByRole('button', { name: /^Zoom \d+%, reset view$/ });
	// as before: the plain wheel doesn't zoom
	await stub(page, { smoothZoom: false });
	await studio(page);
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.move(b.x + b.width * 0.25, b.y + b.height * 0.25);
	await page.mouse.wheel(0, -100);
	await expect(zoomLabel()).toHaveAccessibleName('Zoom 100%, reset view');

	await stub(page, { wheel: 'zoom', smoothZoom: false, zoomSpeed: 2 });
	await studio(page);
	const at = { x: b.x + b.width * 0.25, y: b.y + b.height * 0.25 };
	await page.mouse.move(at.x, at.y);
	await page.mouse.wheel(0, -100);
	// two steps of 10% (the speed doubles it), at once (no glide)
	await expect(zoomLabel()).toHaveAccessibleName('Zoom 121%, reset view');
	// the point under the pointer stayed there: the canvas grew away from it
	const after = (await canvas(page).boundingBox())!;
	expect(after.x).toBeLessThan(b.x);
	const fx = (at.x - after.x) / after.width;
	expect(fx).toBeCloseTo(0.25, 2);
});

test('no "Save before…?" when Settings says not to ask (a recovery copy is kept)', async ({
	page
}) => {
	await stub(page, { askToSave: false });
	await studio(page);
	await scribble(page);
	await expect(page.getByLabel('not saved')).toBeVisible();
	await page.keyboard.press('Control+o');
	await expect.poll(() => cmds(page)).toContain('plugin:dialog|open');
	await expect(page.getByRole('dialog', { name: /Save before/ })).toHaveCount(0);
	const c = await cmds(page);
	expect(c).toContain('recovery_write');
	expect(c).not.toContain('recovery_discard');
});

test('units: a new canvas’s size in centimetres', async ({ page }) => {
	await stub(page, { units: 'cm', ppi: 254, newWidth: 2000, newHeight: 1000, newFps: 12 });
	await page.setViewportSize({ width: 1280, height: 900 });
	await page.goto('/');
	await page.getByRole('button', { name: /Custom/ }).click();
	// 254 ppi: 100 px a centimetre
	const width = page.getByLabel('Width (cm)');
	await expect(width).toHaveValue('20');
	await expect(page.getByLabel('Height (cm)')).toHaveValue('10');
	await expect(page.getByLabel('Frames a second')).toHaveValue('12');
	await width.fill('12.5');
	await expect(page.getByTestId('custom-px')).toHaveText('1250 × 1000 px at 254 ppi');
	if (SHOTS) await page.locator('.custom-form').screenshot({ path: `${SHOTS}/units.png` });
	await page.getByRole('button', { name: 'Create' }).click();
	await expect(page).toHaveURL(/new=1250x1000/);
});

test('rulers in the units Settings names', async ({ page }) => {
	await stub(page, { rulers: true, units: 'in', ppi: 192 });
	await studio(page);
	const rulers = page.getByTestId('rulers');
	await expect(rulers).toBeVisible();
	await expect(rulers.locator('text').filter({ hasText: /^in$/ })).toHaveCount(1);
	// 1920 px at 192 ppi: 10 inches across, numbered
	await expect(rulers.locator('text').filter({ hasText: /^10$/ }).first()).toBeVisible();
	if (SHOTS) await page.screenshot({ path: `${SHOTS}/rulers.png` });
});

test('the brush keeps its size on screen when zooming, if Settings says so', async ({ page }) => {
	const radiusAt = async (zoomIn: boolean) => {
		if (zoomIn) await page.getByRole('button', { name: 'Zoom in' }).click();
		const b = (await canvas(page).boundingBox())!;
		await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
		await page.mouse.move(b.x + b.width / 2 + 4, b.y + b.height / 2);
		return Number(await page.getByTestId('brush-cursor').getAttribute('data-r'));
	};
	await stub(page, { smoothZoom: false });
	await studio(page);
	const r = await radiusAt(false);
	expect(await radiusAt(true)).toBeCloseTo(r, 0);

	await stub(page, { smoothZoom: false, brushFollowsZoom: true });
	await studio(page);
	expect(await radiusAt(false)).toBeCloseTo(r, 0);
	// zoomed in ×1.2: smaller on the canvas, the same on screen
	expect(await radiusAt(true)).toBeCloseTo(r / 1.2, 0);
});

test('interface: accent colour, the colour around the canvas, tooltips off', async ({ page }) => {
	await stub(page, { accent: '#22a05a', surround: '#333344', tooltips: false });
	await studio(page);
	await expect
		.poll(() =>
			page.evaluate(() => {
				const s = getComputedStyle(document.documentElement);
				return [s.getPropertyValue('--pp-accent'), s.getPropertyValue('--pp-canvas-surround')];
			})
		)
		.toEqual(['#22a05a', '#333344']);
	// the brush button's tooltip goes as the pointer comes; it keeps its name
	const brush = page.getByRole('button', { name: /^Brush \(/ });
	const name = (await brush.getAttribute('aria-label')) ?? (await brush.getAttribute('title'));
	expect(await brush.getAttribute('title')).toBeTruthy();
	await brush.hover();
	await expect(brush).not.toHaveAttribute('title');
	await expect(page.getByRole('button', { name: name! })).toBeVisible();
});

test('a selection: the area outside it shaded as strongly as Settings says', async ({ page }) => {
	await stub(page, { selectionTint: 0.5, marchingAnts: false });
	await studio(page);
	await page.getByRole('button', { name: 'Rectangle select (M)' }).click();
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.move(b.x + b.width * 0.3, b.y + b.height * 0.3);
	await page.mouse.down();
	await page.mouse.move(b.x + b.width * 0.7, b.y + b.height * 0.7, { steps: 8 });
	await page.mouse.up();
	// the shade: half strength outside, none inside
	const alpha = (fx: number, fy: number) =>
		page
			.locator('canvas.sel-tint')
			.evaluate(
				(el: HTMLCanvasElement, [fx, fy]) =>
					el
						.getContext('2d')!
						.getImageData(Math.round(el.width * fx), Math.round(el.height * fy), 1, 1).data[3],
				[fx, fy]
			);
	await expect.poll(() => alpha(0.1, 0.1)).toBe(128);
	expect(await alpha(0.5, 0.5)).toBe(0);
	if (SHOTS) await page.screenshot({ path: `${SHOTS}/selection-tint.png` });
});

test('undo goes back as many steps as Settings keeps', async ({ page }) => {
	await stub(page, { undoSteps: 10 });
	await studio(page);
	for (let i = 0; i < 13; i++) await scribble(page, 0.1 + i * 0.06);
	const undo = page.getByRole('button', { name: /^Undo/ });
	let n = 0;
	while (await undo.isEnabled()) {
		await undo.click();
		n++;
		if (n > 20) break;
	}
	expect(n).toBe(10);
});
