import { describe, expect, it } from 'vitest';
import { ruleById, rules } from './rules';

describe('rules (UI view of content/feedback/rules.toml)', () => {
	it('parses every rule with its lesson and a readable title', () => {
		expect(rules.length).toBeGreaterThanOrEqual(20);
		const r = ruleById('too-big')!;
		expect(r.lesson).toBe('measuring-with-units');
		expect(r.drill).toBe('block-in-boxes');
		expect(r.title).toBe('Your drawing is too big');
		for (const x of rules) expect(x.title).not.toMatch(/[{}]/);
	});
});
