import { describe, expect, it } from 'vitest';
import {
	arcsPath,
	compositionFile,
	compositionFileName,
	defShapes,
	elementShapes,
	MAX_ELEMENTS,
	newElement,
	PHI_CUT,
	PRESET_NAMES,
	presetElements,
	readCompositionDef,
	readCompositionFile,
	readElement,
	type BuiltIn,
	type CompElement,
	type CompositionDef,
	type Seg
} from './composition-elements';
import {
	compositionFrame,
	compositionParts,
	compositionPlaced,
	compositionShapes,
	defaultGuides,
	readGuides
} from './guides';

const W = 1200;
const H = 800;
const round = (l: Seg) => l.map((v) => Math.round(v * 100) / 100).join();
const def = (elements: CompElement[]): CompositionDef => ({
	id: 'mc-test',
	name: 'Test',
	color: '#3d6bff',
	opacity: 0.5,
	elements
});

describe('composition elements: the shapes they make', () => {
	it('an even grid, a ratio line from either side, a line between two points', () => {
		const grid = elementShapes({ ...newElement('grid'), cols: 4, rows: 2 } as CompElement, W, H);
		expect(grid.lines.map(round)).toEqual([
			'300,0,300,800',
			'600,0,600,800',
			'900,0,900,800',
			'0,400,1200,400'
		]);
		const golden = newElement('ratio');
		expect(elementShapes(golden, W, H).lines[0][0]).toBeCloseTo(W * PHI_CUT);
		const fromEnd = { ...golden, from: 'end', dir: 'horizontal' } as CompElement;
		expect(elementShapes(fromEnd, W, H).lines[0][1]).toBeCloseTo(H * (1 - PHI_CUT));
		const third = { ...golden, ratio: 'third' } as CompElement;
		expect(elementShapes(third, W, H).lines[0][0]).toBeCloseTo(400);
		const root2 = { ...golden, ratio: 'root2' } as CompElement;
		expect(elementShapes(root2, W, H).lines[0][0]).toBeCloseTo(W / Math.SQRT2);
		const custom = { ...golden, ratio: 'custom', at: 0.25, from: 'end' } as CompElement;
		expect(elementShapes(custom, W, H).lines[0][0]).toBeCloseTo(900);
		const line = { ...newElement('line'), a: [0, 0.5], b: [1, 0.25] } as CompElement;
		expect(elementShapes(line, W, H).lines).toEqual([[0, 400, 1200, 200]]);
	});

	it('diagonals, a frame inset, circles and ellipses; off draws nothing', () => {
		const both = { ...newElement('diagonals'), angled: true } as CompElement;
		expect(elementShapes(both, W, H).lines).toHaveLength(6);
		const frame = { ...newElement('frame'), inset: 10 } as CompElement;
		expect(elementShapes(frame, W, H).lines[0]).toEqual([120, 80, 1080, 80]);
		const circle = { ...newElement('ellipse'), rx: 0.25 } as CompElement;
		expect(elementShapes(circle, W, H).ellipses).toEqual([{ c: [600, 400], rx: 200, ry: 200 }]);
		const ellipse = { ...newElement('ellipse'), circle: false, rx: 0.25, ry: 0.5 } as CompElement;
		expect(elementShapes(ellipse, W, H).ellipses[0]).toEqual({ c: [600, 400], rx: 300, ry: 400 });
		expect(elementShapes({ ...both, on: false }, W, H).lines).toEqual([]);
	});

	it('a spiral at any ratio, in four ways round', () => {
		const s = newElement('spiral');
		const golden = elementShapes(s, W, H);
		expect(golden.arcs[0].to[0]).toBeCloseTo(W * PHI_CUT);
		const half = elementShapes({ ...s, ratio: 0.5 } as CompElement, W, H);
		expect(half.arcs[0].to[0]).toBeCloseTo(W / 2);
		const ends = [
			[false, false],
			[true, false],
			[false, true],
			[true, true]
		].map(([flipX, flipY]) => {
			const e = elementShapes({ ...s, flipX, flipY } as CompElement, W, H).arcs.at(-1)!.to;
			return [Math.sign(e[0] - W / 2), Math.sign(e[1] - H / 2)].join();
		});
		expect(new Set(ends).size).toBe(4);
	});

	it('solid and dashed apart; two spirals draw as two paths', () => {
		const d = def([
			{ ...newElement('grid'), style: 'dashed' } as CompElement,
			newElement('spiral'),
			{ ...newElement('spiral'), flipX: true } as CompElement
		]);
		const { solid, dashed } = defShapes(d, W, H);
		expect(dashed.lines).toHaveLength(4);
		expect(solid.arcs.length).toBeGreaterThan(10);
		expect(arcsPath(solid.arcs).match(/M/g)).toHaveLength(2);
	});

	it('every built-in guide, made of elements, draws the same lines', () => {
		for (const p of Object.keys(PRESET_NAMES) as BuiltIn[]) {
			const built = compositionShapes({ preset: p, flipX: false, flipY: false }, W, H);
			const made = compositionShapes(
				{ preset: 'custom', flipX: false, flipY: false, custom: def(presetElements(p, W / H)) },
				W,
				H
			);
			expect(new Set(made.lines.map(round)), p).toEqual(new Set(built.lines.map(round)));
			expect(made.arcs.length, p).toBe(built.arcs.length);
		}
		// flipped the same way too
		const built = compositionShapes({ preset: 'triangles', flipX: true, flipY: false }, W, H);
		const made = defShapes(def(presetElements('triangles', W / H, true)), W, H).solid;
		expect(new Set(made.lines.map(round))).toEqual(new Set(built.lines.map(round)));
	});
});

describe('a composition placed on the canvas', () => {
	const c = { preset: 'thirds' as const, flipX: false, flipY: false };

	it('fills the canvas until it is moved or scaled', () => {
		expect(compositionFrame(c, W, H)).toEqual([0, 0, W, H]);
		expect(compositionPlaced(c)).toBe(false);
		expect(compositionPlaced({ ...c, offset: [0, 0], scale: 1 })).toBe(false);
	});

	it('moved and scaled about its middle, then flipped inside its own frame', () => {
		const placed = { ...c, offset: [0.25, 0] as [number, number], scale: 0.5 };
		expect(compositionPlaced(placed)).toBe(true);
		expect(compositionFrame(placed, W, H)).toEqual([600, 200, 600, 400]);
		const lines = compositionShapes(placed, W, H).lines.map(round);
		expect(lines).toEqual([
			'800,200,800,600',
			'1000,200,1000,600',
			expect.any(String),
			expect.any(String)
		]);
		const custom = {
			preset: 'custom' as const,
			flipX: true,
			flipY: false,
			offset: [0.25, 0] as [number, number],
			scale: 0.5,
			custom: def([{ ...newElement('line'), a: [0, 0], b: [0.5, 1] } as CompElement])
		};
		// in its frame (600, 200)–(1200, 600), mirrored: from the top right to the bottom middle
		expect(compositionParts(custom, W, H).solid.lines).toEqual([[1200, 200, 900, 600]]);
		const circle = { ...custom, custom: def([newElement('ellipse')]) };
		expect(compositionShapes(circle, W, H).ellipses[0]).toEqual({
			c: [900, 400],
			rx: 120,
			ry: 120
		});
	});
});

describe('reading a composition back (storage, projects, files)', () => {
	it('clamps numbers, caps the elements, leaves unknown things off', () => {
		const d = readCompositionDef({
			id: 'not an id',
			name: '  A very long name that goes on and on and on past forty letters  ',
			color: 'red',
			opacity: 7,
			elements: [
				{ kind: 'grid', on: true, style: 'wavy', cols: 400, rows: -3 },
				{ kind: 'line', on: 'yes', a: [5, -2], b: ['x', 0.5] },
				{ kind: 'teapot', on: true },
				{ kind: 'spiral', on: true, ratio: 99, flipX: 1 },
				{ kind: 'ratio', on: true, ratio: 'silver', at: NaN, from: 'middle', dir: 'up' },
				{ kind: 'ellipse', on: true, c: [0.5], rx: 0, ry: 1e9 },
				null,
				'grid'
			]
		})!;
		expect(d.id).toBe('');
		expect(d.name).toHaveLength(40);
		expect(d.color).toBe('#e8492f');
		expect(d.opacity).toBe(1);
		expect(d.elements.map((e) => e.kind)).toEqual(['grid', 'line', 'spiral', 'ratio', 'ellipse']);
		expect(d.elements[0]).toMatchObject({ on: true, style: 'solid', cols: 24, rows: 1 });
		expect(d.elements[1]).toMatchObject({ on: false, a: [1, 0], b: [0.8, 0.5] });
		expect(d.elements[2]).toMatchObject({ ratio: 0.8, flipX: false });
		expect(d.elements[3]).toMatchObject({
			ratio: 'golden',
			at: 0.5,
			from: 'start',
			dir: 'vertical'
		});
		expect(d.elements[4]).toMatchObject({ c: [0.5, 0.5], rx: 0.01, ry: 1, circle: false });
		const many = readCompositionDef({ elements: Array(500).fill({ kind: 'frame', on: true }) })!;
		expect(many.elements).toHaveLength(MAX_ELEMENTS);
		expect(readCompositionDef({ name: 'no elements' })).toBeNull();
		expect(readCompositionDef('grid')).toBeNull();
		expect(readElement({ kind: 'grid' })).toMatchObject({ on: false });
	});

	it('kept whole in a project’s guides, and opened the same', () => {
		const g = defaultGuides(W, H);
		g.composition = {
			preset: 'custom',
			flipX: false,
			flipY: true,
			offset: [0.1, -0.2],
			scale: 0.75,
			custom: def([newElement('grid'), { ...newElement('spiral'), style: 'dashed' } as CompElement])
		};
		const back = readGuides(JSON.parse(JSON.stringify(g)))!;
		expect(back).toEqual(g);
		// placed too far, or broken: held in, or off
		const far = readGuides({
			...g,
			composition: { ...g.composition, offset: [9, 'x'], scale: 0 }
		})!;
		expect(far.composition.offset).toEqual([1, 0]);
		expect(far.composition.scale).toBe(0.1);
		const broken = readGuides({ ...g, composition: { preset: 'custom', custom: { name: 'x' } } })!;
		expect(broken.composition.preset).toBe('off');
	});

	it('a file of guides round-trips; odd files bring nothing', () => {
		const a = def([newElement('grid'), { ...newElement('line'), style: 'dashed' } as CompElement]);
		const b = { ...def([newElement('ellipse')]), id: 'mc-two', name: 'Two' };
		const text = compositionFile([a, b]);
		const back = readCompositionFile(text);
		expect(back).toEqual([
			{ ...a, id: '' },
			{ ...b, id: '' }
		]);
		expect(text).not.toContain('mc-');
		// a single guide on its own reads too
		expect(readCompositionFile(JSON.stringify(a))).toEqual([{ ...a, id: '' }]);
		expect(readCompositionFile('not json')).toEqual([]);
		expect(readCompositionFile('{"compositions": [1, 2]}')).toEqual([]);
		expect(compositionFileName('Hero / shot?')).toBe('Hero shot.ppcomp.json');
		expect(compositionFileName('***')).toBe('composition.ppcomp.json');
	});
});
