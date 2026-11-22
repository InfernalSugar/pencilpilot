import { describe, expect, it } from 'vitest';
import {
	NO_STYLE,
	readStyle,
	sameStyle,
	hasStyle,
	styleNumbers,
	type LayerStyle
} from './layer-style';

const outline: LayerStyle = {
	...NO_STYLE,
	stroke: { on: true, size: 6, colour: '#ff0000', position: 'centre', opacity: 1 }
};

describe('layer styles', () => {
	it('packs into the 21 numbers the engine reads', () => {
		const n = styleNumbers(outline);
		expect(n.length).toBe(21);
		expect([...n.slice(0, 7)]).toEqual([1, 6, 1, 0, 0, 2, 1]);
		// shadow angle, distance, blur
		expect([...n.slice(12, 15)]).toEqual([120, 10, 8]);
		// none, or nothing changed: empty (the engine's own default)
		expect(styleNumbers(null).length).toBe(0);
		expect(styleNumbers(structuredClone(NO_STYLE)).length).toBe(0);
	});

	it('reads what the engine writes', () => {
		// as layersJson gives it (crates/brush/src/style.rs)
		const json =
			'{"stroke":{"on":true,"size":6,"colour":"#ff0000","position":"centre","opacity":1},' +
			'"shadow":{"on":false,"colour":"#000000","opacity":0.5,"angle":120,"distance":10,"blur":8},' +
			'"glow":{"on":false,"colour":"#ffed87","size":12,"opacity":0.75}}';
		const s = readStyle(JSON.parse(json));
		expect(s).toEqual(outline);
		expect(hasStyle(s)).toBe(true);
		expect(hasStyle(NO_STYLE)).toBe(false);
	});

	it('checks and clamps anything from outside', () => {
		expect(readStyle(null)).toBeNull();
		expect(readStyle('outline')).toBeNull();
		// nothing on, nothing changed: no style
		expect(readStyle({})).toBeNull();
		const s = readStyle({
			stroke: { on: true, size: 900, colour: 'red', position: 'around', opacity: -2 },
			shadow: { on: 'yes', angle: -90, distance: Infinity, blur: 5 },
			glow: { on: true, colour: '#00FF00' }
		})!;
		expect(s.stroke).toEqual({
			on: true,
			size: 100,
			colour: '#000000',
			position: 'outside',
			opacity: 0
		});
		expect(s.shadow.on).toBe(false);
		expect(s.shadow.angle).toBe(270);
		expect(s.shadow.distance).toBe(10);
		expect(s.glow).toEqual({ ...NO_STYLE.glow, on: true, colour: '#00ff00' });
	});

	it('compares looks, null as no style', () => {
		expect(sameStyle(null, structuredClone(NO_STYLE))).toBe(true);
		expect(sameStyle(outline, structuredClone(outline))).toBe(true);
		expect(sameStyle(outline, null)).toBe(false);
		const moved = structuredClone(outline);
		moved.shadow.angle = 45;
		expect(sameStyle(outline, moved)).toBe(false);
	});
});
