import { describe, expect, it } from 'vitest';
import { compositionFile, newElement } from './composition-elements';
import {
	customCompositions,
	deleteCustomComposition,
	importCompositionFiles,
	saveCustomComposition
} from './custom-compositions';

describe('My compositions', () => {
	it('saved, changed in place, saved as new, deleted', () => {
		const a = saveCustomComposition({
			name: ' Hero ',
			color: '#112233',
			opacity: 0.4,
			elements: [newElement('grid')]
		});
		expect(a.id).toMatch(/^mc-/);
		expect(a.name).toBe('Hero');
		const b = saveCustomComposition({ ...a, id: undefined, name: 'Other' });
		const again = saveCustomComposition({ ...a, id: a.id, opacity: 0.9 });
		expect(customCompositions().map((d) => d.id)).toEqual([a.id, b.id]);
		expect(customCompositions()[0].opacity).toBe(again.opacity);
		deleteCustomComposition(a.id);
		deleteCustomComposition(b.id);
		expect(customCompositions()).toEqual([]);
	});

	it('imports files: each guide in them gets a new id; unreadable files are counted', async () => {
		const text = compositionFile([
			{ name: 'One', color: '#000000', opacity: 1, elements: [newElement('spiral')] },
			{ name: 'Two', color: '#ffffff', opacity: 0.5, elements: [newElement('frame')] }
		]);
		const r = await importCompositionFiles([
			new File([text], 'two.ppcomp.json'),
			new File(['nope'], 'bad.json')
		]);
		expect(r.saved.map((d) => d.name)).toEqual(['One', 'Two']);
		expect(new Set(r.saved.map((d) => d.id)).size).toBe(2);
		expect(r.note).toBe(
			'2 compositions added to My compositions. One file couldn’t be read as a composition.'
		);
		for (const d of r.saved) deleteCustomComposition(d.id);
	});
});
