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
	strokeAngle
} from './drill-metrics';
import type { Stroke } from './paint/strokes';

const rulesToml = readFileSync(
	new URL('../../../../content/feedback/rules.toml', import.meta.url),
	'utf8'
);
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
	}

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
