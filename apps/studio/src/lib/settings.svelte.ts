/**
 * The app's settings: look, layout, keyboard shortcuts, touch, pen and files. Kept in
 * the app's data folder (`settings.json`, via the Rust side, which also reads `versionsKept`); in a
 * plain browser (tests, previews) in localStorage. The pen's pressure curve keeps its
 * own place (`@pencilpilot/paint/pressure`), shared with the website's studio.
 */
import type { StudioLayout } from '@pencilpilot/paint/Studio.svelte';
import { loadKeymap, type Keymap } from './shortcuts';

/** light, dark, or as the system is set */
export type Theme = 'light' | 'dark' | 'system';

/** what the timeline and the side panel may be resized to (as the studio allows) */
export const BOTTOM_MIN = 120;
export const BOTTOM_MAX = 640;
export const DOCK_MIN = 260;
export const DOCK_MAX = 640;

export interface Settings {
	/** the interface's colours (the paper stays white) */
	theme: Theme;
	layout: StudioLayout;
	keys: Keymap;
	/** use the pen's pressure and tilt (off: every stroke at a steady medium pressure) */
	penPressure: boolean;
	/** what one finger does on a touch screen */
	fingers: 'auto' | 'draw' | 'move';
	/** two-finger tap = undo, three-finger tap = redo */
	tapGestures: boolean;
	/** where Save and Open start (null: the system's choice) */
	projectFolder: string | null;
	/** how often unsaved work is copied for recovery (0 = never) */
	autosaveSeconds: number;
	/** earlier versions kept per project (0 = none) */
	versionsKept: number;
	/** the name others see when drawing together */
	shareName: string;
	/** our own relay for drawing together ('' = the default) */
	shareRelay: string;
}

export const AUTOSAVE_CHOICES = [0, 10, 20, 60, 300];

export const DEFAULT_LAYOUT: StudioLayout = {
	options: true,
	context: true,
	bottom: true,
	dock: true,
	bottomHeight: 0,
	dockWidth: 330
};

export const DEFAULTS: Settings = {
	theme: 'light',
	layout: DEFAULT_LAYOUT,
	keys: loadKeymap(null),
	penPressure: true,
	fingers: 'auto',
	tapGestures: true,
	projectFolder: null,
	autosaveSeconds: 20,
	versionsKept: 20,
	shareName: '',
	shareRelay: ''
};

/** Saved settings, cleaned: anything missing or wrong falls back to the default. */
export function cleanSettings(raw: unknown): Settings {
	const o = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
	const bool = (v: unknown, d: boolean) => (typeof v === 'boolean' ? v : d);
	const int = (v: unknown, d: number, lo: number, hi: number) =>
		typeof v === 'number' && Number.isFinite(v) ? Math.min(hi, Math.max(lo, Math.round(v))) : d;
	const l = (o.layout && typeof o.layout === 'object' ? o.layout : {}) as Record<string, unknown>;
	const D = DEFAULT_LAYOUT;
	return {
		theme: o.theme === 'dark' || o.theme === 'system' ? o.theme : 'light',
		layout: {
			options: bool(l.options, D.options),
			context: bool(l.context, D.context),
			bottom: bool(l.bottom, D.bottom),
			dock: bool(l.dock, D.dock),
			// 0 (as tall as it needs) or within the limits
			bottomHeight:
				l.bottomHeight === 0 ? 0 : int(l.bottomHeight, D.bottomHeight, BOTTOM_MIN, BOTTOM_MAX),
			dockWidth: int(l.dockWidth, D.dockWidth, DOCK_MIN, DOCK_MAX)
		},
		keys: loadKeymap(o.keys),
		penPressure: bool(o.penPressure, DEFAULTS.penPressure),
		fingers: o.fingers === 'draw' || o.fingers === 'move' ? o.fingers : 'auto',
		tapGestures: bool(o.tapGestures, DEFAULTS.tapGestures),
		projectFolder:
			typeof o.projectFolder === 'string' && o.projectFolder.length < 4096 ? o.projectFolder : null,
		autosaveSeconds: AUTOSAVE_CHOICES.includes(o.autosaveSeconds as number)
			? (o.autosaveSeconds as number)
			: DEFAULTS.autosaveSeconds,
		versionsKept: int(o.versionsKept, DEFAULTS.versionsKept, 0, 100),
		shareName: typeof o.shareName === 'string' ? o.shareName.slice(0, 24) : '',
		shareRelay: typeof o.shareRelay === 'string' ? o.shareRelay.slice(0, 200) : ''
	};
}

const LOCAL = 'pp.appSettings';
const inApp = () => typeof window !== 'undefined' && 'isTauri' in window;

/** The settings every screen reads (and Settings changes). */
export const settings: Settings = $state(structuredClone(DEFAULTS));
/** the last failure to keep the settings ('' when they were kept) */
export const saving = $state({ error: '' });
let loaded: Promise<void> | null = null;

/** Load once (later calls wait for the same load). */
export function loadSettings(): Promise<void> {
	loaded ??= (async () => {
		let raw: unknown;
		try {
			if (inApp()) {
				const { invoke } = await import('@tauri-apps/api/core');
				raw = await invoke('settings_get');
			} else raw = JSON.parse(localStorage.getItem(LOCAL) ?? 'null');
		} catch {
			raw = null;
		}
		Object.assign(settings, cleanSettings(raw));
	})();
	return loaded;
}

/** Keep the current settings. */
export async function saveSettings(): Promise<void> {
	const value = $state.snapshot(settings);
	if (inApp()) {
		const { invoke } = await import('@tauri-apps/api/core');
		await invoke('settings_set', { settings: value });
	} else localStorage.setItem(LOCAL, JSON.stringify(value));
}

let keepTimer: ReturnType<typeof setTimeout> | undefined;

/** Keep the settings a moment after the last change (dragging a panel's edge is many). */
export function keepSoon() {
	clearTimeout(keepTimer);
	keepTimer = setTimeout(() => {
		saveSettings()
			.then(() => (saving.error = ''))
			.catch((e) => (saving.error = `Couldn’t keep the settings: ${e}`));
	}, 250);
}

/** Back to how the app started (pressure curve aside). */
export function resetSettings() {
	Object.assign(settings, structuredClone(DEFAULTS));
}

/** Whether the interface should be dark now (for 'system', as the system is set). */
export function isDark(theme: Theme): boolean {
	if (theme !== 'system') return theme === 'dark';
	return typeof matchMedia !== 'undefined' && matchMedia('(prefers-color-scheme: dark)').matches;
}
