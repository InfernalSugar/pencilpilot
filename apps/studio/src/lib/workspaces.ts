/**
 * Workspaces: named layouts of the one studio, each with the panels open (and their
 * sizes), the side panel's tab and the tools on the strip. They all show the same
 * canvas and drawing; switching only rearranges the room. Changes to the layout go to
 * the workspace in use, so each comes back as it was left.
 */
import type { StudioLayout } from '@pencilpilot/paint/Studio.svelte';
import type { ToolId } from '@pencilpilot/paint/ToolRail.svelte';

export interface Workspace {
	id: string;
	name: string;
	layout: StudioLayout;
	/** the side panel's tab */
	tab: string;
	/** the tools on the strip (their keys work either way) */
	tools: ToolId[];
}

/** every tool the strip can show, in its order, with a name for choosing them */
export const STRIP_TOOLS: { id: ToolId; name: string }[] = [
	{ id: 'brush', name: 'Brush' },
	{ id: 'eraser', name: 'Eraser' },
	{ id: 'picker', name: 'Eyedropper' },
	{ id: 'fill', name: 'Fill' },
	{ id: 'lasso-fill', name: 'Lasso fill' },
	{ id: 'select-rect', name: 'Rectangle select' },
	{ id: 'select-lasso', name: 'Lasso select' },
	{ id: 'select-wand', name: 'Magic wand' },
	{ id: 'transform', name: 'Transform' },
	{ id: 'liquify', name: 'Liquify' },
	{ id: 'gradient', name: 'Gradient' },
	{ id: 'smudge', name: 'Smudge' },
	{ id: 'clone', name: 'Clone' },
	{ id: 'pen', name: 'Pen path' },
	{ id: 'measure', name: 'Measure' },
	{ id: 'adjust', name: 'Adjustments' },
	{ id: 'text', name: 'Text' },
	{ id: 'lines', name: 'Select line' }
];
const ALL = STRIP_TOOLS.map((t) => t.id);

/** the side panel's tabs a workspace can open on */
export const PANEL_TABS = [
	{ id: 'layers', name: 'Layers' },
	{ id: 'brushes', name: 'Brushes' },
	{ id: 'colour', name: 'Colour' },
	{ id: 'xsheet', name: 'X-sheet' },
	{ id: 'history', name: 'History' }
] as const;

/** at most this many (the settings stay small) */
export const MAX_WORKSPACES = 12;

const base: StudioLayout = {
	options: true,
	context: true,
	bottom: true,
	dock: true,
	rail: true,
	bottomHeight: 0,
	dockWidth: 330
};

/** The workspaces the app comes with (each can be changed, and reset). */
export const BUILT_IN: readonly Workspace[] = [
	{ id: 'everything', name: 'Everything', layout: base, tab: 'layers', tools: ALL },
	{
		id: 'draw',
		name: 'Draw',
		layout: { ...base, bottom: false },
		tab: 'brushes',
		tools: ALL.filter((t) => t !== 'lines')
	},
	{
		id: 'animate',
		name: 'Animate',
		layout: base,
		tab: 'xsheet',
		tools: ['brush', 'eraser', 'picker', 'fill', 'lasso-fill', 'select-lasso', 'transform', 'lines']
	},
	{
		id: 'colour',
		name: 'Ink & colour',
		layout: { ...base, bottom: false },
		tab: 'colour',
		tools: ['brush', 'eraser', 'picker', 'fill', 'lasso-fill', 'select-wand', 'gradient', 'smudge']
	},
	{
		id: 'review',
		name: 'Review',
		layout: { ...base, options: false, context: false, bottom: false, dock: false, rail: false },
		tab: 'layers',
		tools: ['brush', 'eraser', 'picker']
	}
];
/** the workspaces the app came with before Review: settings saved then have seen only these */
const FIRST_BUILT_IN = ['everything', 'draw', 'animate', 'colour'];

export const isBuiltIn = (id: string) => BUILT_IN.some((w) => w.id === id);

/** a copy that can be changed */
export const copyOf = (w: Workspace): Workspace => ({
	...w,
	layout: { ...w.layout },
	tools: [...w.tools]
});

/** the tools, known ones only, in the strip's order (the brush always) */
export function cleanTools(v: unknown, d: readonly ToolId[]): ToolId[] {
	if (!Array.isArray(v)) return [...d];
	const has = new Set(v);
	return ALL.filter((t) => t === 'brush' || has.has(t));
}

/**
 * Saved workspaces, cleaned (`cleanLayout` cleans a layout as the settings do). With
 * none saved (settings from before workspaces), the layout in use becomes Everything's,
 * so nothing moves.
 */
export function cleanWorkspaces(
	raw: unknown,
	current: unknown,
	layout: StudioLayout,
	cleanLayout: (v: unknown, d: StudioLayout) => StudioLayout,
	builtInsSeen: unknown = undefined
): { workspaces: Workspace[]; workspace: string; builtInsSeen: string[] } {
	const list: Workspace[] = [];
	const seen = new Set<string>();
	for (const v of Array.isArray(raw) ? raw : []) {
		if (!v || typeof v !== 'object' || list.length >= MAX_WORKSPACES) continue;
		const o = v as Record<string, unknown>;
		const id = typeof o.id === 'string' ? o.id.slice(0, 40) : '';
		if (!id || seen.has(id)) continue;
		const d = BUILT_IN.find((w) => w.id === id) ?? BUILT_IN[0];
		const name = typeof o.name === 'string' ? o.name.trim().slice(0, 40) : '';
		seen.add(id);
		list.push({
			id,
			name: name || d.name,
			layout: cleanLayout(o.layout, d.layout),
			tab: PANEL_TABS.some((t) => t.id === o.tab) ? (o.tab as string) : d.tab,
			tools: cleanTools(o.tools, d.tools)
		});
	}
	if (!list.length) {
		list.push(...BUILT_IN.map(copyOf));
		list[0].layout = { ...layout };
	} else {
		// a built-in workspace that came with a later version is added once (deleted, it stays so)
		const had = Array.isArray(builtInsSeen) ? builtInsSeen : FIRST_BUILT_IN;
		for (const w of BUILT_IN)
			if (!had.includes(w.id) && !list.some((x) => x.id === w.id) && list.length < MAX_WORKSPACES)
				list.push(copyOf(w));
	}
	const workspace =
		typeof current === 'string' && list.some((w) => w.id === current) ? current : list[0].id;
	return { workspaces: list, workspace, builtInsSeen: BUILT_IN.map((w) => w.id) };
}

/** a new id that isn't taken */
export function freshId(list: readonly Workspace[]): string {
	for (let n = 1; ; n++) if (!list.some((w) => w.id === `mine-${n}`)) return `mine-${n}`;
}

/** a name not taken yet: "Name", then "Name 2"… */
export function freshName(list: readonly Workspace[], name: string): string {
	if (!list.some((w) => w.name === name)) return name;
	for (let n = 2; ; n++) if (!list.some((w) => w.name === `${name} ${n}`)) return `${name} ${n}`;
}
