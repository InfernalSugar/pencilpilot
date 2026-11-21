import { describe, expect, it } from 'vitest';
import { actionFor, clashes, comboOf, DEFAULT_KEYS, loadKeymap, showKey } from './shortcuts';

const key = (
	k: string,
	mods: Partial<{ ctrl: boolean; meta: boolean; alt: boolean; shift: boolean }> = {}
) => ({
	key: k,
	ctrlKey: !!mods.ctrl,
	metaKey: !!mods.meta,
	altKey: !!mods.alt,
	shiftKey: !!mods.shift
});

describe('keyboard shortcuts', () => {
	it('write keys the same way however they are pressed', () => {
		expect(comboOf(key('z', { ctrl: true, shift: true }))).toBe('Ctrl+Shift+Z');
		expect(comboOf(key('Z', { meta: true, shift: true }))).toBe('Ctrl+Shift+Z'); // ⌘ on a Mac
		expect(comboOf(key('b'))).toBe('B');
		expect(comboOf(key('{', { shift: true }))).toBe('Shift+[');
		expect(comboOf(key('F5', { alt: true }))).toBe('Alt+F5');
		expect(comboOf(key('Shift', { shift: true }))).toBeNull();
		expect(comboOf(key('Escape'))).toBeNull();
	});

	it('find the action a key runs', () => {
		expect(actionFor(DEFAULT_KEYS, 'Ctrl+Z')).toBe('undo');
		expect(actionFor(DEFAULT_KEYS, 'Ctrl+Shift+Z')).toBe('redo');
		expect(actionFor(DEFAULT_KEYS, 'S')).toBe('smudge');
		expect(actionFor(DEFAULT_KEYS, 'Ctrl+S')).toBe('save');
		expect(actionFor(DEFAULT_KEYS, 'Q')).toBeNull();
	});

	it('report clashes with other actions', () => {
		expect(clashes(DEFAULT_KEYS, 'pen', 'B')).toEqual(['brush']);
		expect(clashes(DEFAULT_KEYS, 'brush', 'B')).toEqual([]);
	});

	it('load saved keys, keeping defaults for anything missing or broken', () => {
		const m = loadKeymap({ brush: 'N', eraser: 42, nope: 'X', undo: '' });
		expect(m.brush).toBe('N');
		expect(m.eraser).toBe('E');
		expect(m.undo).toBe('Ctrl+Z');
		expect(loadKeymap(null)).toEqual(DEFAULT_KEYS);
	});

	it('show Mac symbols on a Mac', () => {
		expect(showKey('Ctrl+Shift+Z', true)).toBe('⌘⇧Z');
		expect(showKey('Ctrl+Shift+Z')).toBe('Ctrl+Shift+Z');
	});
});
