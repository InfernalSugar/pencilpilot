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

	it('keep good values as they are', () => {
		const good = { ...DEFAULTS, fingers: 'move' as const, autosaveSeconds: 60, versionsKept: 0 };
		expect(cleanSettings(JSON.parse(JSON.stringify(good)))).toEqual(good);
	});
});
