/**
 * Production stages: each layer can belong to one (storyboard, rough, tie-down, clean-up,
 * colour, shading, compositing). The timeline keeps them (`Timeline.stages`); layers
 * with none belong to every stage.
 */
import { STAGES, isStage, type Stage } from './timeline';

/** A layer as far as guessing its stage goes. */
export interface StageGuessLayer {
	name: string;
	lines?: boolean;
	adjust?: unknown;
}

/** name patterns, in the order they're tried (a "Shadow (Lines)" layer is shading) */
const BY_NAME: [Stage, RegExp][] = [
	['board', /\b(story ?)?boards?\b/i],
	['rough', /\b(roughs?|sketch(es)?|guides?|pictures?)\b/i],
	['tiedown', /\btie[ -]?(downs?)?\b/i],
	['shade', /\b(shadows?|shades?|shading|highlights?|rim|tones?)\b/i],
	['flats', /\b(colou?rs?|flats?)\b/i],
	['comp', /\b(bg|backgrounds?|comp(ositing)?)\b/i],
	['clean', /\b(lines?|line ?art|inks?|inking|clean(-? ?up)?)\b/i]
];

/** A stage for a layer from its name and kind (null: no good guess). */
export function guessStage(layer: StageGuessLayer, isStatic = false): Stage | null {
	for (const [stage, re] of BY_NAME) if (re.test(layer.name)) return stage;
	if (layer.lines) return 'clean';
	if (layer.adjust || isStatic) return 'comp';
	return null;
}

// ---- viewing by stage: which layers show (a view of the canvas, never part of the document) ----

/** What the canvas shows: every layer, a preset's stages, or the stages picked. */
export type ViewId =
	'all' | 'board' | 'rough' | 'tiedown' | 'clean' | 'colour' | 'shading' | 'final' | 'custom';

export interface StageView {
	id: ViewId;
	/** the stages a custom view shows */
	stages?: Stage[];
}

export const VIEW_ALL: StageView = { id: 'all' };

/** The views, with the stages each shows (null: every layer). */
export const VIEW_PRESETS: {
	id: Exclude<ViewId, 'custom'>;
	label: string;
	stages: Stage[] | null;
}[] = [
	{ id: 'all', label: 'All', stages: null },
	{ id: 'board', label: 'Storyboard', stages: ['board'] },
	{ id: 'rough', label: 'Rough', stages: ['rough'] },
	{ id: 'tiedown', label: 'Tie-down', stages: ['tiedown'] },
	{ id: 'clean', label: 'Clean-up', stages: ['clean'] },
	{ id: 'colour', label: 'Colour', stages: ['clean', 'flats'] },
	{ id: 'shading', label: 'Shading', stages: ['clean', 'flats', 'shade'] },
	{ id: 'final', label: 'Final', stages: ['clean', 'flats', 'shade', 'comp'] }
];

/** The stages a view shows (null: every layer). */
export function viewStages(view: StageView): Stage[] | null {
	if (view.id === 'custom') return STAGES.filter((s) => view.stages?.includes(s));
	return VIEW_PRESETS.find((p) => p.id === view.id)?.stages ?? null;
}

/** A view's name ("Custom" for one made of picked stages). */
export const viewLabel = (view: StageView) =>
	view.id === 'custom' ? 'Custom' : (VIEW_PRESETS.find((p) => p.id === view.id)?.label ?? 'All');

/** A view as kept (in a project's settings): anything unknown is All. */
export function readView(raw: unknown): StageView {
	const v = raw as Partial<StageView> | null;
	if (!v || typeof v !== 'object') return VIEW_ALL;
	if (v.id === 'custom')
		return { id: 'custom', stages: (Array.isArray(v.stages) ? v.stages : []).filter(isStage) };
	return VIEW_PRESETS.some((p) => p.id === v.id) ? { id: v.id! } : VIEW_ALL;
}

/** A layer as far as views go (bottom → top, as the engine keeps them). */
export interface ViewLayer {
	id: number;
	/** clipped to the nearest unclipped layer below */
	clip: boolean;
}

/**
 * Which layers a view shows and leaves out. Layers with no stage always show; a layer
 * shown clipped to one left out brings it in (or what's clipped to it wouldn't show).
 */
export function layersInView(
	layers: ViewLayer[],
	stages: Record<number, Stage> | undefined,
	view: StageView
): { shown: number[]; hidden: number[] } {
	const want = viewStages(view);
	if (!want) return { shown: layers.map((l) => l.id), hidden: [] };
	const show = layers.map((l) => {
		const st = stages?.[l.id];
		return !st || want.includes(st);
	});
	// clip bases: the nearest unclipped layer under each shown clipped one
	for (let i = layers.length - 1; i >= 0; i--) {
		if (!show[i] || !layers[i].clip) continue;
		let b = i - 1;
		while (b >= 0 && layers[b].clip) b--;
		if (b >= 0) show[b] = true;
	}
	return {
		shown: layers.filter((_, i) => show[i]).map((l) => l.id),
		hidden: layers.filter((_, i) => !show[i]).map((l) => l.id)
	};
}

/** The one stage a view shows (new layers made in it go in it), or null. */
export function singleStage(view: StageView): Stage | null {
	const st = viewStages(view);
	return st?.length === 1 ? st[0] : null;
}
