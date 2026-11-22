import { describe, expect, it } from 'vitest';
import { cleanSettings, DEFAULTS } from './settings.svelte';

describe('settings', () => {
	it('fall back to the defaults for anything missing or wrong', () => {
		expect(cleanSettings(null)).toEqual(DEFAULTS);
		const s = cleanSettings({
			keys: { brush: 'N' },
			penPressure: 'yes',
			fingers: 'wave',
			tapGestures: false,
			projectFolder: '/home/me/Art',
			autosaveSeconds: 7,
			versionsKept: 9999
		});
		expect(s.keys.brush).toBe('N');
		expect(s.keys.eraser).toBe('E');
		expect(s.penPressure).toBe(true);
		expect(s.fingers).toBe('auto');
		expect(s.tapGestures).toBe(false);
		expect(s.projectFolder).toBe('/home/me/Art');
		expect(s.autosaveSeconds).toBe(20);
		expect(s.versionsKept).toBe(100);
	});

	it('keep the look and the layout within their limits', () => {
		const s = cleanSettings({
			theme: 'purple',
			layout: { bottom: false, dock: 'no', bottomHeight: 5000, dockWidth: 10 }
		});
		expect(s.theme).toBe('light');
		expect(s.layout).toEqual({
			...DEFAULTS.layout,
			bottom: false,
			bottomHeight: 640,
			dockWidth: 260
		});
		expect(cleanSettings({ theme: 'dark' }).theme).toBe('dark');
	});

	it('keep good values as they are', () => {
		const good = {
			...DEFAULTS,
			theme: 'system' as const,
			layout: { ...DEFAULTS.layout, dock: false, bottomHeight: 300 },
			fingers: 'move' as const,
			autosaveSeconds: 60,
			versionsKept: 0
		};
		expect(cleanSettings(JSON.parse(JSON.stringify(good)))).toEqual(good);
	});
});
