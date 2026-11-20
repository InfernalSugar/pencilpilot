import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
	drillById,
	drills,
	drillsBySkill,
	loadDrillResults,
	saveDrillResult,
	seeded,
	DRILLS_KEY
} from './drills';
import {
	angleError,
	endpointError,
	lineMetrics,
	roundScore,
	straightness,
	strokeAngle,
	taperMetrics
} from './drill-metrics';
import type { Stroke } from '@pencilpilot/paint/strokes';

const rulesToml = readFileSync(
	new URL('../../../../content/feedback/rules.toml', import.meta.url),
	'utf8'
);
const plan = readFileSync(
	new URL('../../../../docs/planning/M8_CONTENT_PLAN.md', import.meta.url),
	'utf8'
);
/** Drill ids from the plan's "New drills" table. */
const planDrills = [
	...plan
		.split('## New drills')[1]
		.split('## Rules')[0]
		.matchAll(/^\| ([a-z0-9-]+) \|/gm)
]
	.map((m) => m[1])
	.filter((id) => id !== 'id');

/**
 * Minimal XML well-formedness check for the generated SVG: every tag closes in order,
 * attributes are quoted, no stray `<` or `&`. Returns the first problem, or null.
 */
function xmlProblem(svg: string): string | null {
	const stack: string[] = [];
	const tag = /<(\/?)([a-zA-Z][\w:-]*)((?:\s+[\w:-]+="[^"<]*")*)\s*(\/?)>/y;
	let i = 0;
	while (i < svg.length) {
		const lt = svg.indexOf('<', i);
		const text = svg.slice(i, lt < 0 ? svg.length : lt);
		if (/&(?!(amp|lt|gt|quot|apos|#\d+);)/.test(text)) return `bad entity in "${text}"`;
		if (lt < 0) break;
		tag.lastIndex = lt;
		const m = tag.exec(svg);
		if (!m) return `bad tag at ${lt}: ${svg.slice(lt, lt + 60)}`;
		const [, close, name, , self] = m;
		if (close) {
			if (stack.pop() !== name) return `unexpected </${name}>`;
		} else if (!self) stack.push(name);
		i = tag.lastIndex;
	}
	return stack.length ? `unclosed <${stack.at(-1)}>` : null;
}

const ruleDrills = [
	...new Set([...rulesToml.matchAll(/^drill\s*=\s*"([^"]+)"/gm)].map((m) => m[1]))
];

describe('drill list', () => {
	it('has every drill id that rules.toml links to', () => {
		expect(ruleDrills.length).toBeGreaterThanOrEqual(13);
		for (const id of ruleDrills) expect(drillById(id), id).toBeDefined();
	});

	it('has unique ids, how-to steps and at least one round', () => {
		expect(new Set(drills.map((d) => d.id)).size).toBe(drills.length);
		for (const d of drills) {
			expect(d.howTo.length).toBeGreaterThan(0);
			expect(d.rounds).toBeGreaterThan(0);
		}
	});

	it('has the 30 drills of the M8 plan (13 original + 17 new)', () => {
		expect(planDrills).toHaveLength(17);
		for (const id of planDrills) expect(drillById(id), id).toBeDefined();
		expect(drills).toHaveLength(30);
	});

	it('weight overrides are complete and add up to 1', () => {
		for (const d of drills) {
			if (!d.weights) continue;
			const v = Object.values(d.weights);
			expect(v, d.id).toHaveLength(8);
			expect(
				v.reduce((s, x) => s + x, 0),
				d.id
			).toBeCloseTo(1, 5);
		}
	});

	it('the creative drill is labelled and graded on a looser curve', () => {
		const creative = drills.filter((d) => d.creative).map((d) => d.id);
		expect(creative).toEqual(['exaggerate-shapes']);
		expect(drillById('exaggerate-shapes')!.curve!.pass).toBeLessThan(0.86);
	});

	it('groups every drill under a skill', () => {
		expect(drillsBySkill().flatMap((g) => g.drills)).toHaveLength(drills.length);
	});
});

describe('generators', () => {
	for (const d of drills) {
		it(`${d.id}: deterministic, valid SVG with the right viewBox`, () => {
			const a = d.generate(seeded(42));
			expect(d.generate(seeded(42))).toBe(a);
			expect(d.generate(seeded(43))).not.toBe(a);
			expect(a).toMatch(/^<svg xmlns="http:\/\/www\.w3\.org\/2000\/svg" viewBox="0 0 800 600"/);
			expect(a.endsWith('</svg>')).toBe(true);
			expect(a).not.toMatch(/NaN|undefined|Infinity/);
			expect(d.generate(seeded(7), 400, 300)).toContain('viewBox="0 0 400 300"');

			const t = d.target(seeded(42));
			for (const svg of [t.answer, t.guide].filter(Boolean) as string[]) {
				expect(svg).toMatch(/^<svg [^>]*viewBox="0 0 800 600"/);
				expect(svg).not.toMatch(/NaN|undefined|Infinity/);
			}
			// Tags balance (cheap well-formedness check).
			const open = (a.match(/<(?!\/)[a-zA-Z]+[^>]*[^/]>|<[a-zA-Z]+>/g) ?? []).length;
			const close = (a.match(/<\/[a-zA-Z]+>/g) ?? []).length;
			expect(open).toBe(close);
		});

		it(`${d.id}: well-formed and varied over many seeds`, () => {
			const seen = new Set<string>();
			for (let s = 1; s <= 25; s++) {
				const t = d.target(seeded(s * 7919));
				for (const svg of [t.prompt, t.answer, t.guide].filter(Boolean) as string[]) {
					expect(xmlProblem(svg), `${d.id} seed ${s}`).toBeNull();
					expect(svg).not.toMatch(/NaN|undefined|Infinity/);
					// ids (clip paths, masks, gradients) are namespaced: several previews share a page
					for (const m of svg.matchAll(/ id="([^"]+)"/g)) expect(m[1]).toMatch(/^pp-/);
				}
				seen.add(t.prompt);
			}
			expect(seen.size).toBe(25);
		});
	}

	const numbers = (svg: string, attr: string) =>
		[...svg.matchAll(new RegExp(` ${attr}="(-?[\\d.]+)"`, 'g'))].map((m) => +m[1]);

	it('perspective drills: the canvas guide shows the horizon and vanishing points', () => {
		for (let s = 1; s <= 20; s++) {
			const cube = drillById('cube-in-perspective')!.target(seeded(s));
			expect(cube.guide).toContain('horizon');
			expect(cube.guide!.match(/<circle/g)).toHaveLength(1);
			const box = drillById('two-point-boxes')!.target(seeded(s));
			expect(box.guide!.match(/<circle/g)).toHaveLength(2);
			// visible edges stay on the canvas
			for (const svg of [cube.answer, box.answer]) {
				for (const x of [...numbers(svg, 'x1'), ...numbers(svg, 'x2')]) {
					expect(x).toBeGreaterThanOrEqual(0);
					expect(x).toBeLessThanOrEqual(800);
				}
				for (const y of [...numbers(svg, 'y1'), ...numbers(svg, 'y2')]) {
					expect(y).toBeGreaterThanOrEqual(0);
					expect(y).toBeLessThanOrEqual(600);
				}
			}
		}
	});

	it('cube: 9 visible edges, or 7 when the vanishing point is behind a face', () => {
		for (let s = 1; s <= 30; s++) {
			const edges = drillById('cube-in-perspective')!
				.target(seeded(s))
				.answer.match(/<line/g)!.length;
			expect([7, 9]).toContain(edges);
		}
	});

	const greys = (svg: string) => [...svg.matchAll(/fill="rgb\((\d+),\1,\1\)"/g)].map((m) => +m[1]);

	it('sphere: light side, halftone, reflected light, core shadow and a darker cast shadow', () => {
		for (let s = 1; s <= 10; s++) {
			const v = greys(drillById('form-shading-spheres')!.target(seeded(s)).answer);
			const [cast, light, half, reflect, core] = v;
			expect(new Set(v).size).toBe(5);
			expect(light).toBeGreaterThan(half);
			expect(half).toBeGreaterThan(reflect);
			expect(reflect).toBeGreaterThan(core);
			expect(cast).toBeLessThan(core);
		}
	});

	it('cast shadow: the shadow is darker than every face', () => {
		for (let s = 1; s <= 10; s++) {
			const [shadow, ...faces] = greys(drillById('cast-shadow-blocks')!.target(seeded(s)).answer);
			expect(faces.length).toBeGreaterThanOrEqual(2);
			for (const f of faces) expect(f).toBeGreaterThan(shadow);
		}
	});

	it('atmospheric layers: four layers, each nearer one darker', () => {
		for (let s = 1; s <= 10; s++) {
			const v = greys(drillById('atmospheric-layers')!.target(seeded(s)).answer);
			expect(v).toHaveLength(4);
			for (let i = 1; i < 4; i++) expect(v[i]).toBeLessThan(v[i - 1]);
		}
	});

	it('hatching: each patch has more ink than the one before', () => {
		for (let s = 1; s <= 10; s++) {
			const t = drillById('hatching-patches')!.target(seeded(s));
			const boxes = [...t.guide!.matchAll(/<rect x="([\d.]+)" y="[\d.]+" width="([\d.]+)"/g)].map(
				(m) => [+m[1], +m[1] + +m[2]]
			);
			const segments = [
				...t.answer.matchAll(/<line x1="([\d.]+)" y1="([\d.]+)" x2="([\d.]+)" y2="([\d.]+)"/g)
			].map((m) => m.slice(1).map(Number));
			const ink = boxes.map(([x0, x1]) =>
				segments
					.filter(([ax, , bx]) => (ax + bx) / 2 >= x0 && (ax + bx) / 2 <= x1)
					.reduce((sum, [ax, ay, bx, by]) => sum + Math.hypot(bx - ax, by - ay), 0)
			);
			expect(ink.length).toBeGreaterThanOrEqual(4);
			for (let i = 1; i < ink.length; i++) expect(ink[i]).toBeGreaterThan(ink[i - 1]);
		}
	});

	it('line weight: the target has a thick end and a thin end on the canvas', () => {
		for (let s = 0; s < 30; s++) {
			const { taper } = drillById('line-weight-pressure')!.target(seeded(s));
			for (const p of [taper!.a, taper!.b]) {
				expect(p.x).toBeGreaterThan(0);
				expect(p.x).toBeLessThan(800);
				expect(p.y).toBeGreaterThan(0);
				expect(p.y).toBeLessThan(600);
			}
			expect(Math.hypot(taper!.b.x - taper!.a.x, taper!.b.y - taper!.a.y)).toBeGreaterThan(300);
		}
	});

	it('eye placement: the canvas guide has the head, the answer only the eyes', () => {
		const t = drillById('eye-placement')!.target(seeded(5));
		expect(t.guide).toMatch(/<path d="M[^"]+A/); // head arc
		expect(t.answer).not.toMatch(/ A\d/);
		expect(t.prompt.length).toBeGreaterThan(t.answer.length);
	});

	it('ghosted-lines dots stay on the canvas', () => {
		for (let s = 0; s < 50; s++) {
			const { segment } = drillById('ghosted-lines')!.target(seeded(s));
			for (const p of [segment!.a, segment!.b]) {
				expect(p.x).toBeGreaterThan(0);
				expect(p.x).toBeLessThan(800);
				expect(p.y).toBeGreaterThan(0);
				expect(p.y).toBeLessThan(600);
			}
		}
	});
});

const stroke = (pts: [number, number][]): Stroke => ({
	brushId: 'pen',
	eraser: false,
	points: pts.map(([x, y], i) => ({ x, y, pressure: 0.5, t: i * 8 }))
});
const straight = (a: [number, number], b: [number, number], k = 40): [number, number][] =>
	Array.from({ length: k + 1 }, (_, i) => [
		a[0] + ((b[0] - a[0]) * i) / k,
		a[1] + ((b[1] - a[1]) * i) / k
	]);
const wobbly = (a: [number, number], b: [number, number], amp = 18): [number, number][] =>
	straight(a, b).map(([x, y], i) => [x, y + Math.sin(i / 3) * amp]);

describe('line metrics', () => {
	it('straightness: a straight line beats a wobbly one', () => {
		const s = straightness(stroke(straight([100, 300], [700, 300])).points);
		const w = straightness(stroke(wobbly([100, 300], [700, 300])).points);
		expect(s).toBeCloseTo(0, 5);
		expect(w).toBeGreaterThan(0.02);
	});

	it('endpoint error is 0 on the dots, either direction, and grows when you miss', () => {
		const a = { x: 100, y: 100 };
		const b = { x: 600, y: 400 };
		expect(endpointError(stroke(straight([100, 100], [600, 400])).points, a, b)).toBeCloseTo(0);
		expect(endpointError(stroke(straight([600, 400], [100, 100])).points, a, b)).toBeCloseTo(0);
		expect(endpointError(stroke(straight([150, 100], [600, 450])).points, a, b)).toBeGreaterThan(
			0.05
		);
	});

	it('stroke angle and angle error', () => {
		expect(strokeAngle(stroke(straight([0, 0], [100, 100])).points)).toBeCloseTo(45);
		expect(strokeAngle(stroke(straight([100, 100], [0, 0])).points)).toBeCloseTo(45);
		expect(angleError(5, 175)).toBeCloseTo(10);
		expect(angleError(30, 50)).toBeCloseTo(20);
	});

	it('ghosted lines: straight on the dots scores better than wobbly', () => {
		const segment = { a: { x: 100, y: 300 }, b: { x: 700, y: 300 } };
		const good = lineMetrics([stroke(straight([100, 300], [700, 300]))], { segment })!;
		const bad = lineMetrics([stroke(wobbly([100, 300], [700, 300]))], { segment })!;
		expect(good.score).toBeCloseTo(1);
		expect(bad.score).toBeLessThan(good.score);
		expect(roundScore(6, good)).toBeGreaterThan(roundScore(6, bad));
	});

	it('angle matching: the right angle beats one 20° off', () => {
		const target = 30;
		const at = (deg: number) => {
			const r = (deg * Math.PI) / 180;
			return stroke(straight([400, 300], [400 + Math.cos(r) * 300, 300 + Math.sin(r) * 300]));
		};
		const right = lineMetrics([at(30)], { angle: target })!;
		const off = lineMetrics([at(50)], { angle: target })!;
		expect(right.angleError).toBeCloseTo(0);
		expect(off.angleError).toBeCloseTo(20);
		expect(right.score).toBeGreaterThan(off.score + 0.5);
	});

	it('uses the longest stroke and ignores eraser strokes', () => {
		const segment = { a: { x: 100, y: 300 }, b: { x: 700, y: 300 } };
		const tiny = stroke(wobbly([300, 100], [320, 120], 5));
		const eraser = { ...stroke(wobbly([0, 0], [800, 600])), eraser: true };
		const m = lineMetrics([tiny, stroke(straight([100, 300], [700, 300])), eraser], { segment })!;
		expect(m.score).toBeCloseTo(1);
		expect(lineMetrics([], { segment })).toBeNull();
	});

	it('roundScore stays in 1–10 and keeps the grader score without metrics', () => {
		expect(roundScore(7.34, null)).toBe(7.3);
		expect(roundScore(10, { straightness: 0, score: 1 })).toBe(10);
		expect(roundScore(1, { straightness: 1, score: 0 })).toBe(1);
	});
});

const pressured = (pts: [number, number][], pressure: (t: number) => number): Stroke => ({
	brushId: 'pen',
	eraser: false,
	points: pts.map(([x, y], i) => ({ x, y, pressure: pressure(i / (pts.length - 1)), t: i * 8 }))
});

describe('taper metric (line weight)', () => {
	const a = { x: 100, y: 300 };
	const b = { x: 700, y: 300 };
	const path = straight([100, 300], [700, 300]);

	it('a steady thick → thin stroke scores near 1', () => {
		const m = taperMetrics(pressured(path, (t) => 0.9 - 0.8 * t).points, a)!;
		expect(m.drop).toBeGreaterThan(0.6);
		expect(m.steadiness).toBeCloseTo(1);
		expect(m.score).toBeGreaterThan(0.95);
	});

	it('drawn backwards (thin end first) it is oriented by the thick dot', () => {
		const back = straight([700, 300], [100, 300]);
		const m = taperMetrics(pressured(back, (t) => 0.1 + 0.8 * t).points, a)!;
		expect(m.score).toBeGreaterThan(0.95);
	});

	it('the wrong way round, or a wavy pressure, scores low', () => {
		const wrong = taperMetrics(pressured(path, (t) => 0.1 + 0.8 * t).points, a)!;
		expect(wrong.drop).toBeLessThan(0);
		expect(wrong.score).toBeLessThan(0.1);
		const wavy = taperMetrics(pressured(path, (t) => 0.5 + 0.2 * Math.sin(t * 20)).points, a)!;
		expect(wavy.score).toBeLessThan(0.5);
	});

	it('flat pressure (a mouse) gives no taper reading', () => {
		expect(taperMetrics(pressured(path, () => 0.5).points, a)).toBeNull();
	});

	it('lineMetrics: taper + straightness + end points; a mouse falls back to straightness + ends', () => {
		const taper = { a, b };
		const good = lineMetrics([pressured(path, (t) => 0.9 - 0.8 * t)], { taper })!;
		const flat = lineMetrics([pressured(path, () => 0.6)], { taper })!;
		const even = lineMetrics([pressured(path, (t) => (t < 0.5 ? 0.6 : 0.64))], { taper })!;
		const bent = lineMetrics([pressured(wobbly([100, 300], [700, 300]), (t) => 0.9 - 0.8 * t)], {
			taper
		})!;
		expect(good.score).toBeGreaterThan(0.95);
		expect(flat.taper).toBeNull();
		expect(flat.score).toBeCloseTo(1); // straight and on the dots
		expect(even.taper).not.toBeNull();
		expect(even.score).toBeLessThan(0.6);
		expect(bent.score).toBeLessThan(good.score);
		expect(roundScore(6, good)).toBeGreaterThan(roundScore(6, even));
	});
});

describe('drill results storage', () => {
	const mem = () => {
		const m = new Map<string, string>();
		return {
			getItem: (k: string) => m.get(k) ?? null,
			setItem: (k: string, v: string) => void m.set(k, v)
		};
	};

	it('appends results and keeps the last 200', () => {
		const s = mem();
		for (let i = 0; i < 205; i++)
			saveDrillResult({ id: 'ghosted-lines', at: i, avg: 5, rounds: [5] }, s);
		const list = loadDrillResults(s);
		expect(list).toHaveLength(200);
		expect(list[0].at).toBe(5);
		expect(JSON.parse(s.getItem(DRILLS_KEY)!)[199]).toEqual({
			id: 'ghosted-lines',
			at: 204,
			avg: 5,
			rounds: [5]
		});
	});
});
