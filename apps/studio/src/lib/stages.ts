/**
 * Production stages: each layer can belong to one (storyboard, rough, tie-down, clean-up,
 * colour, shading, compositing). The timeline keeps them (`Timeline.stages`); layers
 * with none belong to every stage.
 */
import type { Stage } from './timeline';

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
