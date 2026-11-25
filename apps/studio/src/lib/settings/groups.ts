/**
 * The preference groups, in the order Settings shows them. Each is a component with its
 * own controls (no heading), so a page or a pop-up can lay them out as it likes.
 */
import type { Component } from 'svelte';
import Brushes from './Brushes.svelte';
import Canvas from './Canvas.svelte';
import Cursor from './Cursor.svelte';
import Exports from './Exports.svelte';
import Guides from './Guides.svelte';
import Interface from './Interface.svelte';
import Performance from './Performance.svelte';
import Prompts from './Prompts.svelte';
import Selection from './Selection.svelte';

export const PREFERENCE_GROUPS: { id: string; title: string; body: Component }[] = [
	{ id: 'interface', title: 'Interface', body: Interface },
	{ id: 'cursor', title: 'Cursor', body: Cursor },
	{ id: 'brushes', title: 'Brushes', body: Brushes },
	{ id: 'canvas', title: 'Canvas and zoom', body: Canvas },
	{ id: 'selection', title: 'Selection', body: Selection },
	{ id: 'guides', title: 'Guides and new projects', body: Guides },
	{ id: 'performance', title: 'History and memory', body: Performance },
	{ id: 'exports', title: 'Exports and start-up', body: Exports },
	{ id: 'prompts', title: 'Questions and sounds', body: Prompts }
];
