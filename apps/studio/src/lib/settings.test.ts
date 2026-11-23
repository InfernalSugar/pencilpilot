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

	it('start every preference as the app always worked', () => {
		const d = cleanSettings({});
		expect(d).toMatchObject({
			uiScale: 1,
			accent: '',
			leftHanded: false,
			tooltips: true,
			cursor: 'outline',
			cursorWhilePainting: true,
			brushFollowsZoom: false,
			zoomToPointer: true,
			wheel: 'scroll',
			smoothZoom: true,
			rulers: false,
			units: 'px',
			selectionTint: 0,
			marchingAnts: true,
			undoSteps: 60,
			frameMemoryMb: 1024,
			framesFolder: null,
			exportFormat: 'mp4',
			exportQuality: 'best',
			askToSave: true,
			askToDelete: true,
			scrubSound: true
		});
	});

	it('clean the preferences: ranges clamped, unknown choices and bad colours dropped', () => {
		const s = cleanSettings({
			uiScale: 9,
			accent: 'red',
			surround: '#123456',
			checkerSize: 1,
			checkerColour: '#zzzzzz',
			cursor: 'arrow',
			maxBrushSize: 100,
			wheel: 'zoom',
			zoomSpeed: -1,
			units: 'mm',
			ppi: 72.4,
			selectionTint: 2,
			gridColour: '#ABCDEF',
			newWidth: 10,
			newFps: 1000,
			undoSteps: 2,
			frameMemoryMb: 16384,
			framesFolder: '',
			exportFormat: 'avi',
			exportQuality: 'small',
			recentCount: -4,
			askToSave: 'no'
		});
		expect(s).toMatchObject({
			uiScale: 1.5,
			accent: '',
			surround: '#123456',
			checkerSize: 4,
			checkerColour: '#e9e6de',
			cursor: 'outline',
			maxBrushSize: 6.06,
			wheel: 'zoom',
			zoomSpeed: 0.25,
			units: 'px',
			ppi: 72,
			selectionTint: 0.8,
			gridColour: '#ABCDEF',
			newWidth: 64,
			newFps: 60,
			undoSteps: 10,
			frameMemoryMb: 16384,
			framesFolder: null,
			exportFormat: 'mp4',
			exportQuality: 'small',
			recentCount: 0,
			askToSave: true
		});
	});

	it('bring the onion skin in from where older versions kept it', () => {
		const legacy = { onion: { on: true, before: 3, beforeColour: '#112233' } };
		// settings without one: the old one is used
		const moved = cleanSettings({ keys: {} }, legacy);
		expect(moved.onion).toMatchObject({ on: true, before: 3, beforeColour: '#112233' });
		// settings with one: theirs wins
		const kept = cleanSettings({ onion: { before: 5 } }, legacy);
		expect(kept.onion.before).toBe(5);
		expect(kept.onion.on).toBe(false);
		// a bad one is cleaned like the rest
		expect(cleanSettings({ onion: { before: 50, afterColour: 'green' } }).onion).toMatchObject({
			before: 5,
			afterColour: DEFAULTS.onion.afterColour
		});
	});
});
