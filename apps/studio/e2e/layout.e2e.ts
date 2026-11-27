import { expect, test, type Page } from '@playwright/test';
import { canvas, ink, ready, scribble, stubTauri } from './helpers';

// The new layout (Preferences › Appearance › Layout: New): the timeline's icon trays and
// its playback bar, the right-click menus, the timeline's zoom and the workspace tabs.
// (The classic layout's controls are driven by project.e2e.ts.)

const SHOTS = process.env.PP_SHOTS;
const NEW = { settings: { uiLayout: 'new' } };

async function studio(page: Page) {
	await stubTauri(page, NEW);
	await ready(page);
	return page.getByRole('region', { name: 'Timeline' });
}
/** a tray's icon (it opens and closes the tray) */
const trigger = (page: Page, tray: string) =>
	page.getByRole('button', { name: new RegExp(`^${tray}: `) });
/** an option in an open tray */
const option = (page: Page, tray: string, name: string) =>
	page.getByRole('group', { name: tray, exact: true }).getByRole('button', { name, exact: true });

test('trays: one icon slides open sideways, a pick does it and folds back, one open at a time', async ({
	page
}) => {
	const tl = await studio(page);
	const frameNo = tl.getByTestId('frame');
	await scribble(page, 0.3);
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1$/ })).toBeVisible();

	// folded: the options are out of sight
	const create = trigger(page, 'New');
	await expect(create).toHaveAttribute('aria-expanded', 'false');
	await expect(option(page, 'New', 'Duplicate drawing')).toBeHidden();
	// open: every option, and the arrow turns to ‹
	await create.click();
	await expect(create).toHaveAttribute('aria-expanded', 'true');
	await expect(create).toHaveAccessibleName(/Collapse$/);
	for (const o of ['New drawing', 'Duplicate drawing', 'Blank frame', 'New cut', 'New scene'])
		await expect(option(page, 'New', o)).toBeVisible();
	if (SHOTS) await tl.screenshot({ path: `${SHOTS}/tray-new-open.png` });
	// a pick does it, and the tray folds back to the icon picked
	await option(page, 'New', 'New drawing').click();
	await expect(frameNo).toHaveText('2');
	await expect(tl.getByRole('button', { name: /drawing 2, frame 2$/ })).toBeVisible();
	await expect(create).toHaveAttribute('aria-expanded', 'false');
	await expect(option(page, 'New', 'Duplicate drawing')).toBeHidden();
	await expect(create).toHaveAccessibleName('New: New drawing. Show more');
	await create.click();
	await option(page, 'New', 'Duplicate drawing').click();
	await expect(create).toHaveAccessibleName('New: Duplicate drawing. Show more');
	await expect(tl.getByRole('button', { name: /drawing 3, frame 3$/ })).toBeVisible();

	// only one open at a time: opening Frames folds Hold
	await trigger(page, 'Hold').click();
	await expect(option(page, 'Hold', 'Hold on 2s')).toBeVisible();
	await trigger(page, 'Frames').click();
	await expect(trigger(page, 'Hold')).toHaveAttribute('aria-expanded', 'false');
	await expect(option(page, 'Hold', 'Hold on 2s')).toBeHidden();
	await expect(option(page, 'Frames', 'Insert frame')).toBeVisible();
	if (SHOTS) await tl.screenshot({ path: `${SHOTS}/tray-frames-open.png` });
	// tapping the icon again closes it, nothing changed
	await trigger(page, 'Frames').click();
	await expect(option(page, 'Frames', 'Insert frame')).toBeHidden();
	await expect(trigger(page, 'Frames')).toHaveAccessibleName('Frames: Insert frame. Show more');
	// and Esc closes one too
	await trigger(page, 'Scene').click();
	await expect(option(page, 'Scene', 'New scene')).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(option(page, 'Scene', 'New scene')).toBeHidden();
});

test('trays: hold, mark, frames, cut and scene do what they did in the classic bar', async ({
	page
}) => {
	const tl = await studio(page);
	const frameNo = tl.getByTestId('frame');
	await scribble(page, 0.3);
	// Hold on 2s: drawing 1 shows on frames 1–2; the tray shows 2s
	await trigger(page, 'Hold').click();
	await option(page, 'Hold', 'Hold on 2s').click();
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1–2/ })).toBeVisible();
	await expect(trigger(page, 'Hold')).toHaveAccessibleName('Hold: Hold on 2s. Show more');
	// Mark frame: a key, then a breakdown, then clear
	await trigger(page, 'Mark frame').click();
	await option(page, 'Mark frame', 'Key').click();
	await expect(tl.getByRole('button', { name: /drawing 1 \(Key\), frame 1–2/ })).toBeVisible();
	await expect(trigger(page, 'Mark frame')).toHaveAccessibleName('Mark frame: Key. Show more');
	await trigger(page, 'Mark frame').click();
	await option(page, 'Mark frame', 'Breakdown').click();
	await expect(tl.getByRole('button', { name: /drawing 1 \(Breakdown\)/ })).toBeVisible();
	await trigger(page, 'Mark frame').click();
	await option(page, 'Mark frame', 'Clear mark').click();
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1–2/ })).toBeVisible();
	// Frames: extend the hold, insert a frame, remove one
	await trigger(page, 'Frames').click();
	await option(page, 'Frames', 'Extend hold').click();
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1–3/ })).toBeVisible();
	await trigger(page, 'Frames').click();
	await option(page, 'Frames', 'Insert frame').click();
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1–4/ })).toBeVisible();
	await trigger(page, 'Frames').click();
	await option(page, 'Frames', 'Remove frame').click();
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1–3/ })).toBeVisible();
	// Blank frame: an empty frame after the drawing
	await trigger(page, 'New').click();
	await option(page, 'New', 'Blank frame').click();
	await expect(frameNo).toHaveText('4');
	await expect.poll(() => ink(page, 0, 0.45)).toBe(0);
	// More frame tools… opens the frame tools (copy, paste, cycle, boil)
	await trigger(page, 'Frames').click();
	await option(page, 'Frames', 'More frame tools…').click();
	await expect(page.getByRole('group', { name: 'Frame tools' })).toBeVisible();
	await page
		.getByRole('group', { name: 'Frame tools' })
		.getByRole('button', { name: 'Copy' })
		.click();
	await expect(page.getByRole('group', { name: 'Frame tools' })).toContainText('Copied');

	// Cut and Scene: a new cut, then back to the first; a new scene
	await trigger(page, 'Cut').click();
	await option(page, 'Cut', 'New cut').click();
	await expect(trigger(page, 'Cut')).toHaveAccessibleName(/^Cut: Cut 2 · /);
	await expect(tl.getByRole('button', { name: /drawing 1, frame/ })).toHaveCount(0);
	await trigger(page, 'Cut').click();
	await page.getByRole('group', { name: 'Cut', exact: true }).getByRole('button').first().click();
	await expect(trigger(page, 'Cut')).toHaveAccessibleName(/^Cut: Cut 1 · /);
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1–3/ })).toBeVisible();
	await trigger(page, 'Scene').click();
	await option(page, 'Scene', 'New scene').click();
	await expect(trigger(page, 'Scene')).toHaveAccessibleName(/^Scene: Scene 2\./);
	await expect(frameNo).toHaveText('1');
});

test('right-click menus: a frame, the ruler, a layer name, the cut, the play options', async ({
	page
}) => {
	const tl = await studio(page);
	await scribble(page, 0.3);
	const menu = (name: string) => page.getByRole('menu', { name });
	// a frame: hold it longer
	await tl.getByRole('button', { name: /drawing 1, frame 1$/ }).click({ button: 'right' });
	await expect(menu('Frame')).toBeVisible();
	await expect(menu('Frame')).toContainText('Drawing 1');
	for (const item of [
		'Remove drawing',
		'Hold longer',
		'Copy frames',
		'Paste frames',
		'Cycle…',
		'Boil…'
	])
		await expect(menu('Frame').getByRole('menuitem', { name: item })).toBeVisible();
	if (SHOTS) await page.screenshot({ path: `${SHOTS}/menu-frame.png` });
	await menu('Frame').getByRole('menuitem', { name: 'Hold longer' }).click();
	await expect(menu('Frame')).toBeHidden();
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1–2/ })).toBeVisible();
	// Esc closes a menu without doing anything
	await tl.getByRole('button', { name: /drawing 1, frame 1–2/ }).click({ button: 'right' });
	await page.keyboard.press('Escape');
	await expect(menu('Frame')).toBeHidden();
	await expect(tl.getByRole('button', { name: /drawing 1, frame 1–2/ })).toBeVisible();

	// the ruler: loop from frame 2, then a marker there
	await tl.getByRole('button', { name: 'Frame 2', exact: true }).click({ button: 'right' });
	await expect(menu('Frame ruler')).toContainText('Frame 2');
	await menu('Frame ruler').getByRole('menuitem', { name: 'Loop from here' }).click();
	await tl.getByRole('button', { name: 'Frame 2', exact: true }).click({ button: 'right' });
	await expect(
		menu('Frame ruler').getByRole('menuitem', { name: 'Clear the loop range' })
	).toBeEnabled();
	await menu('Frame ruler').getByRole('menuitem', { name: 'Add a marker' }).click();
	await expect(tl.getByRole('button', { name: 'Frame 2, marker' })).toBeVisible();

	// a layer name: static, then a cel layer again
	const layerName = tl.locator('.name .label').first();
	await layerName.click({ button: 'right' });
	await expect(menu('Layer')).toBeVisible();
	await menu('Layer')
		.getByRole('menuitemcheckbox', { name: /^Static/ })
		.click();
	await expect(tl.getByRole('button', { name: /^Static layer: / })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await layerName.click({ button: 'right' });
	await expect(menu('Layer').getByRole('menuitemcheckbox', { name: /^Static/ })).toHaveAttribute(
		'aria-checked',
		'true'
	);
	await menu('Layer')
		.getByRole('menuitemcheckbox', { name: /^Cel layer/ })
		.click();
	await expect(tl.getByRole('button', { name: /^Static layer: / })).toHaveAttribute(
		'aria-pressed',
		'false'
	);
	// and hide it from the eye on its row
	await tl.getByRole('button', { name: /^Hide / }).click();
	await expect(tl.getByRole('button', { name: /^Show / })).toBeVisible();
	await tl.getByRole('button', { name: /^Show / }).click();

	// the Cut icon: what comes into the next cut
	await trigger(page, 'Cut').click({ button: 'right' });
	await menu('Cut').getByRole('menuitemcheckbox', { name: 'White flash' }).click();
	await trigger(page, 'Cut').click({ button: 'right' });
	await expect(menu('Cut').getByRole('menuitemcheckbox', { name: 'White flash' })).toHaveAttribute(
		'aria-checked',
		'true'
	);
	await page.keyboard.press('Escape');

	// play options (▾, or right-click Play): ping-pong, the scene
	await page.getByRole('button', { name: /^Play options/ }).click();
	await menu('Play options').getByRole('menuitemcheckbox', { name: 'Ping-pong' }).click();
	await expect(page.getByRole('button', { name: /^Play options/ })).toHaveAccessibleName(
		/ping-pong/
	);
	await tl.getByRole('button', { name: 'Play', exact: true }).click({ button: 'right' });
	await expect(
		menu('Play options').getByRole('menuitemcheckbox', { name: 'Ping-pong' })
	).toHaveAttribute('aria-checked', 'true');
	await menu('Play options').getByRole('menuitemcheckbox', { name: 'The whole scene' }).click();
	await expect(page.getByRole('button', { name: /^Play options/ })).toHaveAccessibleName(
		/the scene/
	);
	// Markers… lists the marker
	await page.getByRole('button', { name: /^Play options/ }).click();
	await menu('Play options').getByRole('menuitem', { name: 'Markers…' }).click();
	await expect(page.getByRole('group', { name: 'Markers in this scene' })).toBeVisible();
	await expect(page.getByLabel('Marker 2 label')).toBeVisible();
});

test('the playback bar: onion skin, camera, pin, play and the frame rate', async ({ page }) => {
	const tl = await studio(page);
	const bar = page.getByRole('toolbar', { name: 'Playback and view' });
	await scribble(page, 0.3);
	await trigger(page, 'New').click();
	await option(page, 'New', 'New drawing').click();
	await scribble(page, 0.7);
	// onion skin on and off, its settings ▾
	const onionBtn = bar.getByRole('button', { name: 'Onion skin', exact: true });
	const was = await onionBtn.getAttribute('aria-pressed');
	await onionBtn.click();
	await expect(onionBtn).not.toHaveAttribute('aria-pressed', was!);
	await bar.getByRole('button', { name: 'Onion skin settings' }).click();
	await expect(page.getByRole('group', { name: 'Onion skin' })).toBeVisible();
	await bar.getByRole('button', { name: 'Onion skin settings' }).click();
	// camera settings ▾
	await bar.getByRole('button', { name: 'Camera settings' }).click();
	await expect(page.getByRole('group', { name: 'Camera', exact: true })).toBeVisible();
	await page
		.getByRole('group', { name: 'Camera shape' })
		.getByRole('button', { name: '4:3' })
		.click();
	await expect(bar.getByRole('button', { name: 'Camera', exact: true })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await bar.getByRole('button', { name: 'Camera settings' }).click();
	// pin this frame (light table), unpin all
	await bar.getByRole('button', { name: 'Pin frame' }).click();
	await expect(bar.getByRole('button', { name: 'Pin frame' })).toHaveAttribute(
		'aria-pressed',
		'true'
	);
	await bar.getByRole('button', { name: '1 pinned ×' }).click();
	await expect(bar.getByRole('button', { name: 'Pin frame' })).toHaveAttribute(
		'aria-pressed',
		'false'
	);
	// previous / next and the counter
	await bar.getByRole('button', { name: 'Previous frame' }).click();
	await expect(tl.getByTestId('frame')).toHaveText('1');
	await bar.getByRole('button', { name: 'Next frame' }).click();
	await expect(tl.getByTestId('frame')).toHaveText('2');
	// the frame rate
	await bar.getByRole('combobox', { name: 'Frames a second' }).selectOption('12');
	await expect(bar).toContainText('/ 2 · 0.08 s');
	// play, then stop
	await bar.getByRole('button', { name: 'Play', exact: true }).click();
	await expect(page.getByTestId('player')).toBeVisible();
	await bar.getByRole('button', { name: 'Stop', exact: true }).click();
	await expect(page.getByTestId('player')).toHaveCount(0);
});

test('the timeline zoom: the slider and the mountains make frames wider or narrower, kept', async ({
	page
}) => {
	const tl = await studio(page);
	await scribble(page, 0.3);
	const cel = tl.getByRole('button', { name: /drawing 1, frame 1$/ });
	const width = async () => (await cel.boundingBox())!.width;
	const zoom = tl.getByRole('slider', { name: 'Timeline zoom' });
	await expect(zoom).toHaveValue('22');
	expect(Math.round(await width())).toBe(22);
	await zoom.fill('44');
	await expect.poll(width).toBeCloseTo(44, 0);
	await tl.getByRole('button', { name: 'Narrower frames' }).click();
	await expect(zoom).toHaveValue('38');
	await expect.poll(width).toBeCloseTo(38, 0);
	for (let i = 0; i < 8; i++) await tl.getByRole('button', { name: 'Narrower frames' }).click();
	await expect(zoom).toHaveValue('8');
	await tl.getByRole('button', { name: 'Wider frames' }).click();
	await expect(zoom).toHaveValue('14');
	// a click on the ruler lands on the frame under it at any zoom
	const tick = tl.getByRole('button', { name: 'Frame 5', exact: true });
	await tick.click();
	await expect(tl.getByTestId('frame')).toHaveText('5');
	// kept for next time
	await page.reload();
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
	await expect(page.getByRole('slider', { name: 'Timeline zoom' })).toHaveValue('14');
});

test('workspace tabs: Animate, Draw, Ink & colour and Review lay out the one canvas', async ({
	page
}) => {
	const tl = await studio(page);
	const tabs = page.getByRole('tablist', { name: 'Workspace' });
	await expect(tabs.getByRole('tab')).toHaveText([
		'Everything',
		'Draw',
		'Animate',
		'Ink & colour',
		'Review'
	]);
	await scribble(page, 0.3);
	const drawn = await ink(page);
	await tabs.getByRole('tab', { name: 'Animate' }).click();
	await expect(tabs.getByRole('tab', { name: 'Animate' })).toHaveAttribute('aria-selected', 'true');
	await expect(tl).toBeVisible();
	await trigger(page, 'New').click();
	await option(page, 'New', 'New drawing').click();
	await scribble(page, 0.7);
	await page.keyboard.press(',');
	if (SHOTS) await page.screenshot({ path: `${SHOTS}/page-animate.png` });

	// Draw: the timeline folds away, a frame strip of the layer's drawings instead
	await tabs.getByRole('tab', { name: 'Draw' }).click();
	await expect(tl).toBeHidden();
	const strip = page.getByRole('region', { name: 'Frame strip' });
	await expect(strip).toBeVisible();
	await expect(strip.getByRole('button', { name: /^Drawing \d/ })).toHaveCount(2);
	await expect(strip.getByRole('button', { name: 'Drawing 1, frame 1' })).toHaveAttribute(
		'aria-current',
		'true'
	);
	// the same canvas, the same drawing (seen a little bigger or smaller, with its onion skin)
	await expect.poll(async () => Math.abs((await ink(page)) - drawn) / drawn).toBeLessThan(0.3);
	await strip.getByRole('button', { name: 'Next drawing' }).click();
	await expect(strip).toContainText('Drawing 2');
	if (SHOTS) await page.screenshot({ path: `${SHOTS}/page-draw.png` });
	await strip.getByRole('button', { name: 'New drawing' }).click();
	await expect(strip.getByRole('button', { name: /^Drawing \d/ })).toHaveCount(3);

	// Ink & colour: the drawings to colour
	await tabs.getByRole('tab', { name: 'Ink & colour' }).click();
	const colour = page.getByRole('region', { name: 'Drawings to colour' });
	await expect(colour).toBeVisible();
	await expect(colour.getByRole('button', { name: /^Drawing \d/ })).toHaveCount(3);
	await colour.getByRole('button', { name: /^Drawing 1,/ }).click({ button: 'right' });
	await expect(page.getByRole('menu', { name: 'Drawing' })).toBeVisible();
	await page
		.getByRole('menu', { name: 'Drawing' })
		.getByRole('menuitem', { name: 'Open in Draw' })
		.click();
	await expect(tabs.getByRole('tab', { name: 'Draw' })).toHaveAttribute('aria-selected', 'true');
	await tabs.getByRole('tab', { name: 'Ink & colour' }).click();
	if (SHOTS) await page.screenshot({ path: `${SHOTS}/page-colour.png` });

	// Review: no tool strip, a player
	await tabs.getByRole('tab', { name: 'Review' }).click();
	const player = page.getByRole('region', { name: 'Review player' });
	await expect(player).toBeVisible();
	await expect(page.getByRole('button', { name: /^Brush \(/ })).toBeHidden();
	await player.getByRole('slider', { name: 'Where in the cut' }).focus();
	await page.keyboard.press('ArrowRight');
	await expect(player).toContainText('frame 2');
	await player.click({ button: 'right', position: { x: 40, y: 70 } });
	await expect(page.getByRole('menu', { name: 'Player' })).toBeVisible();
	await page.keyboard.press('Escape');
	if (SHOTS) await page.screenshot({ path: `${SHOTS}/page-review.png` });
	await player.getByRole('button', { name: 'Play', exact: true }).click();
	await expect(page.getByTestId('player')).toBeVisible();
	await player.getByRole('button', { name: 'Stop', exact: true }).click();

	// back to Animate: the timeline as it was
	await tabs.getByRole('tab', { name: 'Animate' }).click();
	await expect(tl).toBeVisible();
	await expect(tl.getByRole('button', { name: /drawing 3, frame 3$/ })).toBeVisible();
});

test('right-click menus outside the timeline: the canvas, a tool, a brush, a palette colour', async ({
	page
}) => {
	await studio(page);
	const menu = (name: string) => page.getByRole('menu', { name });
	await scribble(page, 0.3);
	// the canvas: undo, select all; with a selection, its own actions first
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2, { button: 'right' });
	await expect(menu('Canvas')).toBeVisible();
	for (const item of ['Undo', 'Select all', 'Eyedropper', 'Rotate left', 'Flip view', 'Reset view'])
		await expect(menu('Canvas').getByRole('menuitem', { name: item })).toBeVisible();
	await expect(menu('Canvas').getByRole('menuitemcheckbox', { name: 'Grid' })).toBeVisible();
	if (SHOTS) await page.screenshot({ path: `${SHOTS}/menu-canvas.png` });
	await menu('Canvas').getByRole('menuitem', { name: 'Select all' }).click();
	await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2, { button: 'right' });
	await expect(menu('Canvas')).toContainText('Selection');
	await menu('Canvas').getByRole('menuitem', { name: 'Deselect' }).click();
	await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2, { button: 'right' });
	await expect(menu('Canvas')).not.toContainText('Selection');
	await menu('Canvas').getByRole('menuitem', { name: 'Undo' }).click();
	await expect.poll(() => ink(page)).toBe(0);

	// a tool on the strip: hide it from the strip
	const fill = page.getByRole('button', { name: /^Fill \(/ });
	await fill.click({ button: 'right' });
	await expect(menu('Tool')).toContainText('Fill');
	await menu('Tool').getByRole('menuitem', { name: 'Hide from the strip' }).click();
	await expect(fill).toHaveCount(0);

	// a brush: favourites
	await page.getByRole('tab', { name: /^Brushes/ }).click();
	const brush = page.getByRole('list', { name: 'Brushes' }).locator('button.brush').first();
	await brush.click({ button: 'right' });
	await expect(menu('Brush').getByRole('menuitem', { name: /favourites/ })).toBeVisible();
	await expect(menu('Brush').getByRole('menuitem', { name: /save as mine/ })).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(menu('Brush')).toBeHidden();

	// a palette colour: delete it
	await page.getByRole('tab', { name: /^Colour/ }).click();
	const palettes = page.getByRole('region', { name: 'Character palettes' });
	await palettes.getByRole('button', { name: '+ New' }).click();
	await palettes.getByRole('button', { name: 'Done' }).click();
	await palettes.getByRole('button', { name: '+ This colour' }).click();
	const swatch = palettes.getByRole('button', { name: /^Character 1: / });
	await expect(swatch).toHaveCount(1);
	await swatch.click({ button: 'right' });
	await expect(
		menu('Palette colour').getByRole('menuitem', { name: 'Copy hex code' })
	).toBeVisible();
	await menu('Palette colour').getByRole('menuitem', { name: 'Delete' }).click();
	await expect(swatch).toHaveCount(0);
});

test('the Layout setting: Classic brings the classic bar back, New the trays', async ({ page }) => {
	const tl = await studio(page);
	await expect(trigger(page, 'Scene')).toBeVisible();
	await expect(tl.getByRole('combobox', { name: 'Scene' })).toHaveCount(0);
	await page.getByRole('button', { name: 'Settings', exact: true }).click();
	await page.getByRole('tab', { name: 'Appearance', exact: true }).click();
	const layout = page.getByRole('combobox', { name: /^Layout/ });
	await expect(layout).toHaveValue('new');
	await layout.selectOption('classic');
	await page.keyboard.press('Escape');
	await expect(tl.getByRole('combobox', { name: 'Scene' })).toBeVisible();
	await expect(trigger(page, 'Scene')).toHaveCount(0);
	await expect(page.getByRole('combobox', { name: 'Workspace' })).toBeVisible();
	// and kept
	await expect
		.poll(
			async () =>
				(await page.evaluate(() =>
					(window as unknown as { __calls: { cmd: string; json?: string }[] }).__calls
						.filter((c) => c.cmd === 'settings_set')
						.map((c) => c.json)
						.at(-1)
				)) ?? ''
		)
		.toContain('"uiLayout":"classic"');
});
