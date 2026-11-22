import { describe, expect, it } from 'vitest';
import { accentTokens, darken, lookVars } from './appearance';
import { DEFAULTS } from './settings.svelte';

describe('appearance', () => {
	it('makes the accent’s darker shades for buttons and text', () => {
		expect(darken('#ffffff', 0.5)).toBe('#808080');
		expect(darken('#3d6bff', 0)).toBe('#3d6bff');
		const t = accentTokens('#e8492f');
		// close to the theme's own (#d43f24, #c2391f)
		expect(t['--pp-accent']).toBe('#e8492f');
		expect(t['--pp-accent-strong']).toBe('#d3422b');
		expect(t['--pp-accent-ink']).toBe('#c13d27');
	});

	it('leaves the theme alone where nothing was chosen', () => {
		const v = lookVars(DEFAULTS);
		expect(v['--pp-accent']).toBeUndefined();
		expect(v['--pp-canvas-surround']).toBe('');
		expect(v['--pp-checker-size']).toBe('10px');
		const chosen = lookVars({ ...DEFAULTS, accent: '#22a05a', surround: '#333333' });
		expect(chosen['--pp-accent']).toBe('#22a05a');
		expect(chosen['--pp-canvas-surround']).toBe('#333333');
	});
});
