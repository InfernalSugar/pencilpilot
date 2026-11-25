/**
 * Production stages: each layer can belong to one (storyboard, rough, tie-down, clean-up,
 * colour, shading, compositing). The timeline keeps them (`Timeline.stages`); layers
 * with none belong to every stage.
 */
import { STAGES, STAGE_LABELS, isStage, type Stage } from './timeline';
import type { ExportShow } from './exporting';

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

// ---- light table: another stage's drawings, faint, under the view (a view too) ----

/** Non-photo blue: the pencil colour a camera (or a scanner) doesn't see. */
export const NON_PHOTO_BLUE = '#a4dded';

export interface LightTable {
	/** the stage shown under the view (null: off) */
	stage: Stage | null;
	/** the colour its lines show in; null: their own colours */
	tint: string | null;
	/** 0.05–1 */
	opacity: number;
}

export const LIGHT_TABLE_OFF: LightTable = { stage: null, tint: NON_PHOTO_BLUE, opacity: 0.5 };

/** A light table as kept (in a project's settings): anything unknown is off. */
export function readLightTable(raw: unknown): LightTable {
	const o = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
	const D = LIGHT_TABLE_OFF;
	const tint =
		o.tint === null
			? null
			: typeof o.tint === 'string' && /^#[0-9a-f]{6}$/i.test(o.tint)
				? o.tint
				: D.tint;
	const opacity =
		typeof o.opacity === 'number' && Number.isFinite(o.opacity)
			? Math.min(1, Math.max(0.05, o.opacity))
			: D.opacity;
	return { stage: isStage(o.stage) ? o.stage : null, tint, opacity };
}

/** A layer as far as the light table goes. */
export interface LightTableLayer {
	id: number;
	/** its eye (and its group's) */
	visible: boolean;
	/** an adjustment layer (no drawings to show) */
	adjust?: unknown;
}

/**
 * The layers the light table shows (bottom → top): the paint layers in its stage whose
 * eye is open, but not ones the view shows already (they'd show twice).
 */
export function lightTableLayers(
	layers: LightTableLayer[],
	stages: Record<number, Stage> | undefined,
	table: Pick<LightTable, 'stage'>,
	shown: number[]
): number[] {
	if (!table.stage) return [];
	const inView = new Set(shown);
	return layers
		.filter((l) => l.visible && !l.adjust && !inView.has(l.id) && stages?.[l.id] === table.stage)
		.map((l) => l.id);
}

// ---- a layer to trace another on (the next stage's) ----

/** the stages drawn over the one before, in order */
const TRACED: Stage[] = ['rough', 'tiedown', 'clean'];

/**
 * The stages a new layer traced over a layer can go in: those after its stage (one with
 * none counts as a rough; a storyboard's are all of them).
 */
export function traceStages(stage: Stage | null): Stage[] {
	const at = STAGES.indexOf(stage ?? 'rough');
	return TRACED.filter((s) => STAGES.indexOf(s) > at);
}

/** stage words a layer's name may be ("Rough", "Tie-down", "Clean-up"…) */
const STAGE_WORD =
	/^(story ?boards?|boards?|roughs?|sketch(es)?|tie[ -]?downs?|clean(-? ?up)?|lines?|inks?)$/i;

/**
 * A traced layer's name: its stage's, with what the layer traced over is of ("Rough
 * (Bob)" → "Clean-up (Bob)"; "Bob" → "Clean-up (Bob)"; "Rough" → "Clean-up").
 */
export function traceName(name: string, stage: Stage): string {
	const label = STAGE_LABELS[stage];
	const n = name.trim();
	const m = /^(.*?)\s*\((.+)\)$/.exec(n);
	if (m && STAGE_WORD.test(m[1])) return `${label} (${m[2]})`;
	if (!n || STAGE_WORD.test(n)) return label;
	return `${label} (${n})`;
}

/**
 * The stage a layer in `stage` is traced from: the nearest earlier one that has layers
 * (`used`), e.g. what to show under a clean-up on a light table. Null: none.
 */
export function traceSource(stage: Stage, used: Iterable<Stage>): Stage | null {
	const have = new Set(used);
	const before = TRACED.filter((s) => STAGES.indexOf(s) < STAGES.indexOf(stage));
	return before.reverse().find((s) => have.has(s)) ?? null;
}

/** The view showing just one stage. */
export function oneStageView(stage: Stage): StageView {
	return VIEW_PRESETS.some((p) => p.id === stage)
		? { id: stage as ViewId }
		: { id: 'custom', stages: [stage] };
}

// ---- exporting by stage ----

/**
 * The passes an export makes: one (the final film, the view, or every layer), or one per
 * stage that has layers ("each", labelled with the stage's name for its file).
 */
export function exportPasses(
	show: ExportShow,
	stages: Record<number, Stage> | undefined,
	view: StageView
): { label: string | null; view: StageView }[] {
	if (show === 'final') return [{ label: null, view: { id: 'final' } }];
	if (show === 'view') return [{ label: null, view }];
	if (show === 'each') {
		const used = new Set(Object.values(stages ?? {}));
		const each = STAGES.filter((s) => used.has(s)).map((s) => ({
			label: STAGE_LABELS[s],
			view: { id: 'custom', stages: [s] } as StageView
		}));
		if (each.length) return each;
	}
	return [{ label: null, view: VIEW_ALL }];
}
