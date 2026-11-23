/**
 * The app's settings: look, layout, keyboard shortcuts, touch, pen, files and the
 * preferences (the cursor, brushes, zoom, guides, memory, exports and questions). Kept in
 * the app's data folder (`settings.json`, via the Rust side, which also reads
 * `versionsKept`, `frameMemoryMb` and `framesFolder`); in a plain browser (tests,
 * previews) in localStorage. The pen's pressure curve keeps its own place
 * (`@pencilpilot/paint/pressure`), shared with the website's studio.
 */
import type { StudioLayout } from '@pencilpilot/paint/Studio.svelte';
import type { CursorStyle, Unit } from '@pencilpilot/paint/view-prefs';
import type { ExportFormat } from './exporting';
import { cleanOnion, ONION_DEFAULTS, type OnionSettings } from './onion';
import { loadKeymap, type Keymap } from './shortcuts';

/** light, dark, or as the system is set */
export type Theme = 'light' | 'dark' | 'system';
export type ExportQuality = 'best' | 'good' | 'small';

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

	// ---- interface ----
	/** how big everything is (1 = as designed) */
	uiScale: number;
	/** the accent colour ('' = the theme's) */
	accent: string;
	/** the tool strip on the right, the panels on the left */
	leftHanded: boolean;
	/** the little notes that show when the pointer rests on a button */
	tooltips: boolean;
	/** the colour around the canvas ('' = the theme's) */
	surround: string;
	/** see-through areas: the checkerboard's square size (px) and its darker colour */
	checkerSize: number;
	checkerColour: string;

	// ---- cursor ----
	cursor: CursorStyle;
	/** keep the cursor showing while a stroke is drawn */
	cursorWhilePainting: boolean;
	/** a brush too small to outline shows a crosshair (off: a dot) */
	crosshairWhenSmall: boolean;

	// ---- brushes ----
	/** the brush keeps its size on screen when you zoom (Procreate's dynamic brush scaling) */
	brushFollowsZoom: boolean;
	/** the largest the size slider goes (a multiple of the brush's own size) */
	maxBrushSize: number;
	/** each brush remembers its own size and opacity */
	perBrushSize: boolean;

	// ---- canvas ----
	/** zoom towards the pointer (off: towards the middle of the view) */
	zoomToPointer: boolean;
	/** the plain scroll wheel moves the view ('scroll') or zooms ('zoom') */
	wheel: 'scroll' | 'zoom';
	/** how far each wheel step zooms (1 = normal) */
	zoomSpeed: number;
	/** zoom glides instead of jumping */
	smoothZoom: boolean;
	rulers: boolean;
	/** for rulers, canvas sizes and the measuring tool */
	units: Unit;
	/** pixels per inch, to turn pixels into centimetres and inches */
	ppi: number;

	// ---- selection ----
	/** how strongly the area outside a selection is tinted (0 = not at all) */
	selectionTint: number;
	/** the selection's outline moves (off: it stays still) */
	marchingAnts: boolean;

	// ---- guides and animation ----
	/** the grid's square size on new canvases (px; 0 = an eighth of the canvas) */
	gridSize: number;
	gridColour: string;
	/** perspective, symmetry and ruler guides */
	guideColour: string;
	onion: OnionSettings;
	/** new projects (the home screen's Custom, and the studio opened on its own) */
	newWidth: number;
	newHeight: number;
	newFps: number;
	/** the brush a new canvas starts with ('' = the built-in default) */
	defaultBrush: string;

	// ---- history and performance ----
	undoSteps: number;
	/** the most memory the animation's frames may take, in MB; the rest goes to disk */
	frameMemoryMb: number;
	/** where frames past that go (null: the app's cache folder) */
	framesFolder: string | null;

	// ---- files ----
	/** what Export → Animation… starts on */
	exportFormat: ExportFormat;
	exportQuality: ExportQuality;
	/** write the software, author and who drew it together into exports */
	exportCredits: boolean;
	/** open the last project when the app starts */
	reopenLast: boolean;
	/** recent projects on the home screen */
	recentCount: number;

	// ---- prompts and sounds ----
	/** ask "Save before…?" when unsaved work would be left (off: a recovery copy is kept) */
	askToSave: boolean;
	/** ask before deleting one of "My brushes" */
	askToDelete: boolean;
	/** a snippet of the sound plays when stepping through frames */
	scrubSound: boolean;
}

export const AUTOSAVE_CHOICES = [0, 10, 20, 60, 300];
export const UI_SCALES = [0.8, 0.9, 1, 1.1, 1.25, 1.5];
export const ZOOM_SPEEDS = [0.5, 0.75, 1, 1.5, 2];
export const EXPORT_FORMATS: ExportFormat[] = ['mp4', 'webm', 'prores', 'gif', 'apng', 'png'];
/** the slider's own top (2^2.6) */
export const BRUSH_SIZE_TOP = 6.06;

export const DEFAULT_LAYOUT: StudioLayout = {
	options: true,
	context: true,
	bottom: true,
	dock: true,
	rail: true,
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
	shareRelay: '',

	uiScale: 1,
	accent: '',
	leftHanded: false,
	tooltips: true,
	surround: '',
	checkerSize: 10,
	checkerColour: '#e9e6de',

	cursor: 'outline',
	cursorWhilePainting: true,
	crosshairWhenSmall: false,

	brushFollowsZoom: false,
	maxBrushSize: BRUSH_SIZE_TOP,
	perBrushSize: false,

	zoomToPointer: true,
	wheel: 'scroll',
	zoomSpeed: 1,
	smoothZoom: true,
	rulers: false,
	units: 'px',
	ppi: 300,

	selectionTint: 0,
	marchingAnts: true,

	gridSize: 0,
	gridColour: '#3d6bff',
	guideColour: '#e8492f',
	onion: { ...ONION_DEFAULTS },
	newWidth: 1920,
	newHeight: 1080,
	newFps: 24,
	defaultBrush: '',

	undoSteps: 60,
	frameMemoryMb: 1024,
	framesFolder: null,

	exportFormat: 'mp4',
	exportQuality: 'best',
	exportCredits: true,
	reopenLast: false,
	recentCount: 12,

	askToSave: true,
	askToDelete: true,
	scrubSound: true
};

const HEX = /^#[0-9a-f]{6}$/i;

/**
 * Saved settings, cleaned: anything missing or wrong falls back to the default.
 * `legacy` holds what older versions kept elsewhere (the onion skin was in the
 * window's own storage): it's used when the settings don't have it yet.
 */
export function cleanSettings(raw: unknown, legacy: { onion?: unknown } = {}): Settings {
	const o = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
	const D = DEFAULTS;
	const bool = (v: unknown, d: boolean) => (typeof v === 'boolean' ? v : d);
	const num = (v: unknown, d: number, lo: number, hi: number) =>
		typeof v === 'number' && Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : d;
	const int = (v: unknown, d: number, lo: number, hi: number) => Math.round(num(v, d, lo, hi));
	const colour = (v: unknown, d: string) => (typeof v === 'string' && HEX.test(v) ? v : d);
	/** a colour, or '' for the theme's own */
	const themed = (v: unknown) => (typeof v === 'string' && HEX.test(v) ? v : '');
	const one = <T>(v: unknown, list: readonly T[], d: T) => (list.includes(v as T) ? (v as T) : d);
	const folder = (v: unknown) => (typeof v === 'string' && v && v.length < 4096 ? v : null);
	const l = (o.layout && typeof o.layout === 'object' ? o.layout : {}) as Record<string, unknown>;
	const L = DEFAULT_LAYOUT;
	return {
		theme: o.theme === 'dark' || o.theme === 'system' ? o.theme : 'light',
		layout: {
			options: bool(l.options, L.options),
			context: bool(l.context, L.context),
			bottom: bool(l.bottom, L.bottom),
			dock: bool(l.dock, L.dock),
			rail: bool(l.rail, L.rail ?? true),
			// 0 (as tall as it needs) or within the limits
			bottomHeight:
				l.bottomHeight === 0 ? 0 : int(l.bottomHeight, L.bottomHeight, BOTTOM_MIN, BOTTOM_MAX),
			dockWidth: int(l.dockWidth, L.dockWidth, DOCK_MIN, DOCK_MAX)
		},
		keys: loadKeymap(o.keys),
		penPressure: bool(o.penPressure, D.penPressure),
		fingers: one(o.fingers, ['auto', 'draw', 'move'] as const, 'auto'),
		tapGestures: bool(o.tapGestures, D.tapGestures),
		projectFolder: folder(o.projectFolder),
		autosaveSeconds: one(o.autosaveSeconds, AUTOSAVE_CHOICES, D.autosaveSeconds),
		versionsKept: int(o.versionsKept, D.versionsKept, 0, 100),
		shareName: typeof o.shareName === 'string' ? o.shareName.slice(0, 24) : '',
		shareRelay: typeof o.shareRelay === 'string' ? o.shareRelay.slice(0, 200) : '',

		uiScale: num(o.uiScale, D.uiScale, 0.75, 1.5),
		accent: themed(o.accent),
		leftHanded: bool(o.leftHanded, D.leftHanded),
		tooltips: bool(o.tooltips, D.tooltips),
		surround: themed(o.surround),
		checkerSize: int(o.checkerSize, D.checkerSize, 4, 32),
		checkerColour: colour(o.checkerColour, D.checkerColour),

		cursor: one(o.cursor, ['outline', 'crosshair', 'dot'] as const, D.cursor),
		cursorWhilePainting: bool(o.cursorWhilePainting, D.cursorWhilePainting),
		crosshairWhenSmall: bool(o.crosshairWhenSmall, D.crosshairWhenSmall),

		brushFollowsZoom: bool(o.brushFollowsZoom, D.brushFollowsZoom),
		maxBrushSize: num(o.maxBrushSize, D.maxBrushSize, 0.5, BRUSH_SIZE_TOP),
		perBrushSize: bool(o.perBrushSize, D.perBrushSize),

		zoomToPointer: bool(o.zoomToPointer, D.zoomToPointer),
		wheel: one(o.wheel, ['scroll', 'zoom'] as const, D.wheel),
		zoomSpeed: num(o.zoomSpeed, D.zoomSpeed, 0.25, 3),
		smoothZoom: bool(o.smoothZoom, D.smoothZoom),
		rulers: bool(o.rulers, D.rulers),
		units: one(o.units, ['px', 'cm', 'in'] as const, D.units),
		ppi: int(o.ppi, D.ppi, 36, 1200),

		selectionTint: num(o.selectionTint, D.selectionTint, 0, 0.8),
		marchingAnts: bool(o.marchingAnts, D.marchingAnts),

		gridSize: int(o.gridSize, D.gridSize, 0, 2000),
		gridColour: colour(o.gridColour, D.gridColour),
		guideColour: colour(o.guideColour, D.guideColour),
		onion: cleanOnion(o.onion ?? legacy.onion),
		newWidth: int(o.newWidth, D.newWidth, 64, 8192),
		newHeight: int(o.newHeight, D.newHeight, 64, 8192),
		newFps: int(o.newFps, D.newFps, 1, 60),
		defaultBrush:
			typeof o.defaultBrush === 'string' && o.defaultBrush.length < 200 ? o.defaultBrush : '',

		undoSteps: int(o.undoSteps, D.undoSteps, 10, 500),
		frameMemoryMb: int(o.frameMemoryMb, D.frameMemoryMb, 256, 262_144),
		framesFolder: folder(o.framesFolder),

		exportFormat: one(o.exportFormat, EXPORT_FORMATS, D.exportFormat),
		exportQuality: one(o.exportQuality, ['best', 'good', 'small'] as const, D.exportQuality),
		exportCredits: bool(o.exportCredits, D.exportCredits),
		reopenLast: bool(o.reopenLast, D.reopenLast),
		recentCount: int(o.recentCount, D.recentCount, 0, 50),

		askToSave: bool(o.askToSave, D.askToSave),
		askToDelete: bool(o.askToDelete, D.askToDelete),
		scrubSound: bool(o.scrubSound, D.scrubSound)
	};
}

const LOCAL = 'pp.appSettings';
/** where the onion skin was kept before it joined the settings */
const LEGACY_ONION = 'pp.app.onion';
const inApp = () => typeof window !== 'undefined' && 'isTauri' in window;

function readLocal(key: string): unknown {
	try {
		return JSON.parse(localStorage.getItem(key) ?? 'null');
	} catch {
		return null;
	}
}

/** The settings every screen reads (and Settings changes). */
export const settings: Settings = $state(structuredClone(DEFAULTS));
/** the last failure to keep the settings ('' when they were kept) */
export const saving = $state({ error: '' });
let loaded: Promise<void> | null = null;
let isLoaded = $state(false);

/** The settings have been read (screens that keep changes wait for this). */
export const settingsLoaded = () => isLoaded;

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
		Object.assign(settings, cleanSettings(raw, { onion: readLocal(LEGACY_ONION) }));
		isLoaded = true;
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

/** Clean what's been typed (a number out of range comes back into it): on change, not while typing. */
export function tidySettings() {
	Object.assign(settings, cleanSettings($state.snapshot(settings)));
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
