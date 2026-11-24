import { describe, expect, it } from 'vitest';
import { guessStage } from './stages';

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
