import { describe, expect, it } from 'vitest';
import {
	guessStage,
	layersInView,
	readView,
	singleStage,
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
