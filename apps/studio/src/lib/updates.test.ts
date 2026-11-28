import { beforeEach, describe, expect, it, vi } from 'vitest';

const calls: string[] = [];
let next: { version: string; body?: string } | null = null;

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true }));
vi.mock('@tauri-apps/plugin-process', () => ({
	relaunch: async () => void calls.push('relaunch')
}));
vi.mock('@tauri-apps/plugin-updater', () => ({
	check: async () =>
		next && {
			...next,
			close: async () => {},
			download: async (on: (e: unknown) => void) => {
				calls.push('download');
				on({ event: 'Started', data: { contentLength: 10 } });
				on({ event: 'Progress', data: { chunkLength: 10 } });
				on({ event: 'Finished' });
			},
			install: async () => void calls.push('install')
		}
}));

const { checkForUpdate, guardUpdate, install, later, progressText, shouldOffer, skip, updates } =
	await import('./updates.svelte');
const { settings } = await import('./settings.svelte');

beforeEach(() => {
	calls.length = 0;
	next = null;
	updates.now = { stage: 'none' };
	updates.hidden = false;
	settings.skipVersion = '';
});

describe('updates', () => {
	it('offer a version unless it was skipped or one is already under way', () => {
		expect(shouldOffer('0.2.0', '', { stage: 'none' })).toBe(true);
		expect(shouldOffer('0.2.0', '0.2.0', { stage: 'none' })).toBe(false);
		expect(shouldOffer('0.3.0', '0.2.0', { stage: 'none' })).toBe(true);
		const ready = { stage: 'ready', version: '0.2.0', notes: '' } as const;
		expect(shouldOffer('0.3.0', '', ready)).toBe(false);
	});

	it('show how much has come', () => {
		expect(progressText(0, null)).toBe('0.0 MB');
		expect(progressText(5 * 1048576, 120 * 1048576)).toBe('5.0 of 120 MB');
	});

	it('find a new version and offer it with its notes', async () => {
		expect(await checkForUpdate()).toBe('latest');
		next = { version: '0.2.0', body: ' Smoother pens \n' };
		expect(await checkForUpdate()).toBe('new');
		expect(updates.now).toEqual({ stage: 'offered', version: '0.2.0', notes: 'Smoother pens' });
	});

	it('a skipped version stays away, until Check now', async () => {
		next = { version: '0.2.0' };
		await checkForUpdate();
		skip();
		expect(settings.skipVersion).toBe('0.2.0');
		expect(await checkForUpdate()).toBe('latest');
		expect(updates.now.stage).toBe('none');
		expect(await checkForUpdate(true)).toBe('new');
		expect(settings.skipVersion).toBe('');
	});

	it('Later puts the card away; the next look brings it back', async () => {
		next = { version: '0.2.0' };
		await checkForUpdate();
		later();
		expect(updates.hidden).toBe(true);
		await checkForUpdate();
		expect(updates.hidden).toBe(false);
	});

	it('install only after the project says yes, then restart', async () => {
		next = { version: '0.2.0' };
		await checkForUpdate();
		let answer = false;
		const unguard = guardUpdate(async () => {
			calls.push('asked');
			return answer;
		});
		await install();
		// cancelled: downloaded and waiting, nothing installed
		expect(calls).toEqual(['download', 'asked']);
		expect(updates.now.stage).toBe('ready');
		answer = true;
		await install();
		expect(calls).toEqual(['download', 'asked', 'asked', 'install', 'relaunch']);
		unguard();
	});
});
