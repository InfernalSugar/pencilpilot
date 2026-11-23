import { describe, expect, it } from 'vitest';
import { brushTexture, textureById } from './brush-textures';
import { customBrush, customBrushes, deleteCustomBrush } from './custom-brushes';
import { importBrushFiles, importNote, keepImported } from './my-brush-import';

const MYB = JSON.stringify({
	version: 3,
	settings: { radius_logarithmic: { base_value: 1.5, inputs: {} } }
});

describe('importing into My brushes', () => {
	it('a MyPaint brush keeps its own settings', async () => {
		const r = await importBrushFiles([new File([MYB], 'Wet ink.myb')]);
		expect(r.errors).toEqual([]);
		expect(r.brushes).toHaveLength(1);
		const b = customBrush(r.brushes[0].id)!;
		expect(b.name).toBe('Wet ink');
		expect(JSON.parse(b.source!).settings.radius_logarithmic.base_value).toBe(1.5);
		expect(importNote(r)).toBe('Added 1 brush to My brushes.');
		deleteCustomBrush(b.id);
	});

	it('pictures become the brush’s tip and grain', () => {
		const pic = { w: 2, h: 2, px: new Uint8Array([0, 255, 255, 0]) };
		const [b] = keepImported([{ name: 'Chalky', tip: pic, grain: pic }]);
		expect(b.tweaks).toEqual({ hardness: 1 });
		const t = brushTexture(b.id);
		expect(textureById(t.tip)?.kind).toBe('tip');
		expect(textureById(t.grain)?.kind).toBe('grain');
		expect(t.grainStrength).toBeCloseTo(0.8);
		deleteCustomBrush(b.id);
	});

	it('a set makes one brush each, with ids of their own', () => {
		const list = keepImported(
			Array.from({ length: 5 }, (_, i) => ({ name: `Set ${i + 1}`, myb: MYB }))
		);
		expect(new Set(list.map((b) => b.id)).size).toBe(5);
		expect(customBrushes()).toHaveLength(5);
		for (const b of list) deleteCustomBrush(b.id);
	});

	it('a file it can’t read is named, the others still come in', async () => {
		const r = await importBrushFiles([
			new File(['not json'], 'broken.myb'),
			new File(['x'], 'notes.txt'),
			new File([MYB], 'Good.myb')
		]);
		expect(r.brushes.map((b) => b.name)).toEqual(['Good']);
		expect(r.errors).toHaveLength(2);
		expect(r.errors[1]).toMatch(/^notes\.txt: /);
		expect(importNote(r)).toMatch(/^Added 1 brush to My brushes\. broken\.myb: /);
		for (const b of r.brushes) deleteCustomBrush(b.id);
	});
});
