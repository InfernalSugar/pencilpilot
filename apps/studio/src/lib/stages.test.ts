import { describe, expect, it } from 'vitest';
import {
	guessStage,
	layersInView,
	lightTableLayers,
	LIGHT_TABLE_OFF,
	NON_PHOTO_BLUE,
	readLightTable,
	oneStageView,
	readView,
	singleStage,
	traceName,
	traceSource,
	traceStages,
	VIEW_PRESETS,
	viewLabel,
	type StageView
} from './stages';
import type { Stage } from './timeline';

describe('guessing a layer’s stage', () => {
	it('goes by the name first', () => {
		const by = (name: string) => guessStage({ name });
		expect(by('Storyboard')).toBe('board');
		expect(by('Board 2')).toBe('board');
		expect(by('Rough')).toBe('rough');
		expect(by('sketch')).toBe('rough');
		expect(by('Guide lines')).toBe('rough');
		expect(by('Picture 1')).toBe('rough');
		expect(by('Tie-down')).toBe('tiedown');
		expect(by('Tiedown')).toBe('tiedown');
		expect(by('Lines')).toBe('clean');
		expect(by('Ink')).toBe('clean');
		expect(by('Line art')).toBe('clean');
		expect(by('Clean-up')).toBe('clean');
		expect(by('Colour (Lines)')).toBe('flats');
		expect(by('Color')).toBe('flats');
		expect(by('Flats')).toBe('flats');
		expect(by('Shadow (Lines)')).toBe('shade');
		expect(by('Shade')).toBe('shade');
		expect(by('Highlight')).toBe('shade');
		expect(by('Rim light')).toBe('shade');
		expect(by('Tone')).toBe('shade');
		expect(by('BG')).toBe('comp');
		expect(by('Background')).toBe('comp');
		expect(by('Layer 1')).toBeNull();
		expect(by('Toby')).toBeNull();
	});

	it('then by the kind of layer', () => {
		expect(guessStage({ name: 'Layer 2', lines: true })).toBe('clean');
		expect(guessStage({ name: 'Curves', adjust: '{}' })).toBe('comp');
		expect(guessStage({ name: 'Layer 3' }, true)).toBe('comp');
		// a name wins
		expect(guessStage({ name: 'Rough', lines: true })).toBe('rough');
	});
});

describe('viewing by stage', () => {
	// bottom → top: a background, a rough, a tie-down, the lines, their colour, a shadow
	// clipped to the colour, a highlight clipped to it too, an untagged note, a board
	const layers = [
		{ id: 1, clip: false },
		{ id: 2, clip: false },
		{ id: 3, clip: false },
		{ id: 4, clip: false },
		{ id: 5, clip: false },
		{ id: 6, clip: true },
		{ id: 7, clip: true },
		{ id: 8, clip: false },
		{ id: 9, clip: false }
	];
	const stages: Record<number, Stage> = {
		1: 'comp',
		2: 'rough',
		3: 'tiedown',
		4: 'clean',
		5: 'flats',
		6: 'shade',
		7: 'shade',
		9: 'board'
	};
	const shown = (view: StageView) => layersInView(layers, stages, view).shown;

	it('shows each preset’s stages, and untagged layers always', () => {
		const by = Object.fromEntries(VIEW_PRESETS.map((p) => [p.id, shown({ id: p.id })]));
		expect(by).toEqual({
			all: [1, 2, 3, 4, 5, 6, 7, 8, 9],
			board: [8, 9],
			rough: [2, 8],
			tiedown: [3, 8],
			clean: [4, 8],
			colour: [4, 5, 8],
			shading: [4, 5, 6, 7, 8],
			// final leaves the board, the rough and the tie-down out
			final: [1, 4, 5, 6, 7, 8]
		});
		expect(layersInView(layers, stages, { id: 'final' }).hidden).toEqual([2, 3, 9]);
		// no stages at all: everything shows in every view
		expect(layersInView(layers, undefined, { id: 'rough' }).hidden).toEqual([]);
	});

	it('pulls in the layer a shown layer is clipped to', () => {
		// shading alone: its layers are clipped to the colour, which comes along
		expect(shown({ id: 'custom', stages: ['shade'] })).toEqual([5, 6, 7, 8]);
		// (the nearest unclipped layer under them, past other clipped ones)
		const only7 = layersInView(
			layers,
			{ ...stages, 6: 'rough' },
			{ id: 'custom', stages: ['shade'] }
		);
		expect(only7.shown).toEqual([5, 7, 8]);
		expect(only7.hidden).toEqual([1, 2, 3, 4, 6, 9]);
	});

	it('makes custom views and solos from the stages picked', () => {
		expect(shown({ id: 'custom', stages: ['comp', 'rough'] })).toEqual([1, 2, 8]);
		expect(shown({ id: 'custom', stages: [] })).toEqual([8]);
		expect(singleStage({ id: 'custom', stages: ['flats'] })).toBe('flats');
		expect(singleStage({ id: 'rough' })).toBe('rough');
		expect(singleStage({ id: 'colour' })).toBeNull();
		expect(singleStage({ id: 'all' })).toBeNull();
		expect(viewLabel({ id: 'custom', stages: ['flats'] })).toBe('Custom');
		expect(viewLabel({ id: 'tiedown' })).toBe('Tie-down');
	});

	it('reads a kept view back (anything unknown is All)', () => {
		expect(readView({ id: 'final' })).toEqual({ id: 'final' });
		expect(readView({ id: 'custom', stages: ['rough', 'wobbly', 'comp'] })).toEqual({
			id: 'custom',
			stages: ['rough', 'comp']
		});
		expect(readView({ id: 'sideways' })).toEqual({ id: 'all' });
		expect(readView(undefined)).toEqual({ id: 'all' });
		expect(readView('final')).toEqual({ id: 'all' });
	});
});

describe('the light table', () => {
	// bottom → top: two roughs (one with its eye shut), a rough adjustment, the lines, a note
	const layers = [
		{ id: 1, visible: true },
		{ id: 2, visible: false },
		{ id: 3, visible: true, adjust: '{}' },
		{ id: 4, visible: true },
		{ id: 5, visible: true },
		{ id: 6, visible: true }
	];
	const stages: Record<number, Stage> = {
		1: 'rough',
		2: 'rough',
		3: 'rough',
		4: 'clean',
		6: 'rough'
	};

	it('shows its stage’s paint layers with their eye open, the view’s own not again', () => {
		const clean = layersInView(
			layers.map((l) => ({ id: l.id, clip: false })),
			stages,
			{ id: 'clean' }
		).shown;
		expect(lightTableLayers(layers, stages, { stage: 'rough' }, clean)).toEqual([1, 6]);
		// off, or a stage with no layers: nothing
		expect(lightTableLayers(layers, stages, { stage: null }, clean)).toEqual([]);
		expect(lightTableLayers(layers, stages, { stage: 'board' }, clean)).toEqual([]);
		// the view shows the rough already (All): not drawn twice
		expect(lightTableLayers(layers, stages, { stage: 'rough' }, [1, 2, 3, 4, 5, 6])).toEqual([]);
		// untagged layers are never under (they show in every view)
		expect(lightTableLayers(layers, undefined, { stage: 'rough' }, [])).toEqual([]);
	});

	it('reads kept settings back (anything unknown is off, in non-photo blue)', () => {
		expect(readLightTable(undefined)).toEqual(LIGHT_TABLE_OFF);
		expect(LIGHT_TABLE_OFF.tint).toBe(NON_PHOTO_BLUE);
		expect(readLightTable({ stage: 'rough', tint: null, opacity: 0.3 })).toEqual({
			stage: 'rough',
			tint: null,
			opacity: 0.3
		});
		expect(readLightTable({ stage: 'nope', tint: 'blue', opacity: 7 })).toEqual({
			stage: null,
			tint: NON_PHOTO_BLUE,
			opacity: 1
		});
		expect(readLightTable({ stage: 'tiedown', tint: '#FF0000', opacity: 0 }).opacity).toBe(0.05);
	});
});

describe('a new layer to trace another on', () => {
	it('goes in a later stage', () => {
		expect(traceStages('rough')).toEqual(['tiedown', 'clean']);
		expect(traceStages(null)).toEqual(['tiedown', 'clean']);
		expect(traceStages('tiedown')).toEqual(['clean']);
		expect(traceStages('board')).toEqual(['rough', 'tiedown', 'clean']);
		expect(traceStages('clean')).toEqual([]);
		expect(traceStages('flats')).toEqual([]);
	});

	it('is named after the layer it traces', () => {
		expect(traceName('Rough (Bob)', 'clean')).toBe('Clean-up (Bob)');
		expect(traceName('Tie-down (Bob)', 'clean')).toBe('Clean-up (Bob)');
		expect(traceName('Sketch  (Bob walk)', 'tiedown')).toBe('Tie-down (Bob walk)');
		expect(traceName('Bob', 'clean')).toBe('Clean-up (Bob)');
		expect(traceName('Rough', 'tiedown')).toBe('Tie-down');
		expect(traceName('Bob (left)', 'clean')).toBe('Clean-up (Bob (left))');
		expect(traceName('  ', 'clean')).toBe('Clean-up');
	});

	it('knows the stage it is traced from (the nearest earlier one with layers)', () => {
		expect(traceSource('clean', ['rough', 'tiedown', 'clean'])).toBe('tiedown');
		expect(traceSource('clean', ['rough', 'clean'])).toBe('rough');
		expect(traceSource('tiedown', ['clean'])).toBe(null);
		expect(traceSource('rough', ['board', 'rough'])).toBe(null);
	});

	it('a stage’s own view shows it', () => {
		expect(oneStageView('clean')).toEqual({ id: 'clean' });
		expect(oneStageView('tiedown')).toEqual({ id: 'tiedown' });
		expect(oneStageView('flats')).toEqual({ id: 'custom', stages: ['flats'] });
		const v = layersInView([{ id: 1, clip: false }], { 1: 'tiedown' }, oneStageView('tiedown'));
		expect(v.shown).toEqual([1]);
	});
});
