/**
 * Keyboard shortcuts: what each key does, and the keys people can change.
 *
 * A key is written like "Ctrl+Shift+Z": modifiers in a fixed order (Ctrl, Alt, Shift),
 * then the key itself. "Ctrl" also means ⌘ on a Mac.
 */

export type ActionId =
	| 'brush'
	| 'eraser'
	| 'picker'
	| 'fill'
	| 'lasso-fill'
	| 'measure'
	| 'smudge'
	| 'pen'
	| 'transform'
	| 'liquify'
	| 'select-rect'
	| 'select-lasso'
	| 'select-wand'
	| 'gradient'
	| 'adjust'
	| 'smaller'
	| 'bigger'
	| 'undo'
	| 'redo'
	| 'copy'
	| 'cut'
	| 'copy-to-layer'
	| 'select-all'
	| 'deselect'
	| 'invert-selection'
	| 'clear-selected'
	| 'zoom-in'
	| 'zoom-out'
	| 'fit'
	| 'rotate-left'
	| 'rotate-right'
	| 'rotate-reset'
	| 'mirror'
	| 'prev-frame'
	| 'next-frame'
	| 'new-drawing'
	| 'copy-frames'
	| 'paste-frames'
	| 'duplicate-frames'
	| 'play'
	| 'set-in'
	| 'set-out'
	| 'onion'
	| 'pin-frame'
	| 'save'
	| 'save-as'
	| 'open';

export interface Action {
	id: ActionId;
	label: string;
	group: 'Tools' | 'Brush' | 'Edit' | 'Selection' | 'View' | 'Animation' | 'File';
	/** the key it starts with */
	key: string;
}

export const ACTIONS: Action[] = [
	{ id: 'brush', label: 'Brush', group: 'Tools', key: 'B' },
	{ id: 'eraser', label: 'Eraser', group: 'Tools', key: 'E' },
	{ id: 'picker', label: 'Eyedropper', group: 'Tools', key: 'I' },
	{ id: 'fill', label: 'Fill', group: 'Tools', key: 'G' },
	{ id: 'lasso-fill', label: 'Lasso fill', group: 'Tools', key: 'F' },
	{ id: 'measure', label: 'Measure', group: 'Tools', key: 'U' },
	{ id: 'smudge', label: 'Smudge', group: 'Tools', key: 'S' },
	{ id: 'pen', label: 'Pen path', group: 'Tools', key: 'P' },
	{ id: 'transform', label: 'Transform', group: 'Tools', key: 'V' },
	{ id: 'liquify', label: 'Liquify', group: 'Tools', key: 'K' },
	{ id: 'select-rect', label: 'Rectangle select', group: 'Tools', key: 'M' },
	{ id: 'select-lasso', label: 'Lasso select', group: 'Tools', key: 'L' },
	{ id: 'select-wand', label: 'Magic wand', group: 'Tools', key: 'W' },
	{ id: 'gradient', label: 'Gradient', group: 'Tools', key: 'Shift+G' },
	{ id: 'adjust', label: 'Adjust the layer', group: 'Tools', key: 'Ctrl+U' },
	{ id: 'smaller', label: 'Smaller brush', group: 'Brush', key: '[' },
	{ id: 'bigger', label: 'Bigger brush', group: 'Brush', key: ']' },
	{ id: 'undo', label: 'Undo', group: 'Edit', key: 'Ctrl+Z' },
	{ id: 'redo', label: 'Redo', group: 'Edit', key: 'Ctrl+Shift+Z' },
	{ id: 'copy', label: 'Copy', group: 'Edit', key: 'Ctrl+C' },
	{ id: 'cut', label: 'Cut', group: 'Edit', key: 'Ctrl+X' },
	{ id: 'copy-to-layer', label: 'Copy to a new layer', group: 'Edit', key: 'Ctrl+J' },
	{ id: 'select-all', label: 'Select all', group: 'Selection', key: 'Ctrl+A' },
	{ id: 'deselect', label: 'Deselect', group: 'Selection', key: 'Ctrl+D' },
	{
		id: 'invert-selection',
		label: 'Invert the selection',
		group: 'Selection',
		key: 'Ctrl+Shift+I'
	},
	{ id: 'clear-selected', label: 'Clear what’s selected', group: 'Selection', key: 'Delete' },
	{ id: 'zoom-in', label: 'Zoom in', group: 'View', key: '=' },
	{ id: 'zoom-out', label: 'Zoom out', group: 'View', key: '-' },
	{ id: 'fit', label: 'Fit the canvas', group: 'View', key: '0' },
	{ id: 'rotate-left', label: 'Rotate left', group: 'View', key: '4' },
	{ id: 'rotate-right', label: 'Rotate right', group: 'View', key: '6' },
	{ id: 'rotate-reset', label: 'Straighten the view', group: 'View', key: '5' },
	{ id: 'mirror', label: 'Mirror the view', group: 'View', key: 'H' },
	{ id: 'prev-frame', label: 'Previous frame', group: 'Animation', key: ',' },
	{ id: 'next-frame', label: 'Next frame', group: 'Animation', key: '.' },
	{ id: 'new-drawing', label: 'New drawing after this one', group: 'Animation', key: 'N' },
	{ id: 'copy-frames', label: 'Copy frames', group: 'Animation', key: 'Alt+C' },
	{ id: 'paste-frames', label: 'Paste frames', group: 'Animation', key: 'Alt+V' },
	{
		id: 'duplicate-frames',
		label: 'Duplicate frames (as new drawings)',
		group: 'Animation',
		key: 'Shift+D'
	},
	{ id: 'play', label: 'Play / stop', group: 'Animation', key: 'Shift+Space' },
	{ id: 'set-in', label: 'Loop from this frame', group: 'Animation', key: 'Alt+I' },
	{ id: 'set-out', label: 'Loop to this frame', group: 'Animation', key: 'Alt+O' },
	{ id: 'onion', label: 'Onion skin on / off', group: 'Animation', key: 'O' },
	{
		id: 'pin-frame',
		label: 'Pin this frame to the light table',
		group: 'Animation',
		key: 'Shift+O'
	},
	{ id: 'save', label: 'Save', group: 'File', key: 'Ctrl+S' },
	{ id: 'save-as', label: 'Save as…', group: 'File', key: 'Ctrl+Shift+S' },
	{ id: 'open', label: 'Open…', group: 'File', key: 'Ctrl+O' }
];

export type Keymap = Record<ActionId, string>;

export const DEFAULT_KEYS: Keymap = Object.fromEntries(ACTIONS.map((a) => [a.id, a.key])) as Keymap;

/** Keys that never become shortcuts on their own (they're modifiers, or the app needs them). */
const RESERVED = new Set(['Control', 'Shift', 'Alt', 'Meta', 'Escape', 'Enter', 'Tab', ' ']);

/** The key a keyboard event stands for ("Ctrl+Shift+Z"), or null for a modifier alone. */
export function comboOf(e: {
	key: string;
	ctrlKey: boolean;
	metaKey: boolean;
	altKey: boolean;
	shiftKey: boolean;
}): string | null {
	const mod = e.ctrlKey || e.metaKey || e.altKey || e.shiftKey;
	// Space on its own moves the view; with a modifier it can be a shortcut
	if (RESERVED.has(e.key) && !(e.key === ' ' && mod)) return null;
	let key = e.key === ' ' ? 'Space' : e.key.length === 1 ? e.key.toUpperCase() : e.key;
	// Shift changes the character on some keys; keep the plain one ("Shift+[", not "{")
	const shifted: Record<string, string> = { '{': '[', '}': ']', '?': '/', '+': '=', _: '-' };
	if (e.shiftKey && shifted[key]) key = shifted[key];
	return [
		e.ctrlKey || e.metaKey ? 'Ctrl' : '',
		e.altKey ? 'Alt' : '',
		e.shiftKey ? 'Shift' : '',
		key
	]
		.filter(Boolean)
		.join('+');
}

/** Which action a key runs, if any. */
export function actionFor(map: Keymap, combo: string): ActionId | null {
	for (const a of ACTIONS) if (map[a.id] === combo) return a.id;
	return null;
}

/** The actions another action's key would clash with. */
export function clashes(map: Keymap, id: ActionId, combo: string): ActionId[] {
	return ACTIONS.filter((a) => a.id !== id && map[a.id] === combo).map((a) => a.id);
}

/** A saved keymap, filled in with defaults for anything missing or broken. */
export function loadKeymap(saved: unknown): Keymap {
	const out = { ...DEFAULT_KEYS };
	if (saved && typeof saved === 'object')
		for (const a of ACTIONS) {
			const v = (saved as Record<string, unknown>)[a.id];
			if (typeof v === 'string' && v.length > 0 && v.length < 40) out[a.id] = v;
		}
	return out;
}

/** How a key shows on screen ("⌘⇧Z" on a Mac). */
export function showKey(combo: string, mac = false): string {
	if (!mac) return combo;
	return combo.replace('Ctrl+', '⌘').replace('Alt+', '⌥').replace('Shift+', '⇧');
}
