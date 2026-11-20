import { describe, expect, it } from 'vitest';
import { recognise, type Pt } from './quickshape';

/** hand-drawn feel: a little deterministic wobble */
const wobble = (pts: Pt[], amt = 1.5): Pt[] =>
	pts.map(([x, y], i) => [x + Math.sin(i * 1.7) * amt, y + Math.cos(i * 2.3) * amt]);
const along = (a: Pt, b: Pt, n = 30): Pt[] =>
	Array.from({ length: n + 1 }, (_, i) => [
		a[0] + ((b[0] - a[0]) * i) / n,
		a[1] + ((b[1] - a[1]) * i) / n
	]);
const path = (...corners: Pt[]): Pt[] =>
	corners.slice(1).flatMap((c, i) => along(corners[i], c).slice(i ? 1 : 0));

describe('QuickShape', () => {
	it('lines and polylines', () => {
		expect(recognise(wobble(along([10, 10], [300, 80])))?.kind).toBe('line');
		const zig = recognise(wobble(path([10, 10], [150, 120], [300, 10])));
		expect(zig?.kind).toBe('polyline');
		expect(zig?.points).toHaveLength(3);
	});

	it('triangles, rectangles, circles, ellipses', () => {
		expect(recognise(wobble(path([50, 200], [150, 20], [250, 200], [52, 198])))?.kind).toBe(
			'triangle'
		);
		expect(
			recognise(wobble(path([50, 50], [250, 50], [250, 150], [50, 150], [50, 52])))?.kind
		).toBe('rectangle');
		const ring = (rx: number, ry: number): Pt[] =>
			Array.from({ length: 80 }, (_, i) => [
				200 + rx * Math.cos((i / 76) * Math.PI * 2),
				150 + ry * Math.sin((i / 76) * Math.PI * 2)
			]);
		const c = recognise(wobble(ring(80, 82), 2));
		expect(c?.kind).toBe('circle');
		const e = recognise(wobble(ring(120, 50), 2));
		expect(e?.kind).toBe('ellipse');
		// the fitted ellipse is about the right size
		const xs = e!.points.map((p) => p[0]);
		expect(Math.max(...xs) - Math.min(...xs)).toBeGreaterThan(220);
		expect(Math.max(...xs) - Math.min(...xs)).toBeLessThan(260);
	});

	it('leaves scribbles and tiny marks alone', () => {
		const scribble: Pt[] = Array.from({ length: 60 }, (_, i) => [
			i * 5,
			100 + Math.sin(i) * 40 + (i % 7) * 9
		]);
		expect(recognise(scribble)).toBeNull();
		expect(
			recognise([
				[0, 0],
				[2, 1],
				[3, 3],
				[4, 4]
			])
		).toBeNull();
	});
});
