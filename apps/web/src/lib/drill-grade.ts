/** Browser-side scoring for one drill round (see drill-metrics.ts for the line maths). */
import { gradeImages, measurement, toGray } from './grader';
import { referencePixels, type Reference } from './references';
import { lineMetrics, roundScore, type LineMetrics } from './drill-metrics';
import type { Drill, DrillTarget } from './drills';
import type { DrawingExport } from '@pencilpilot/paint/PaintCanvas.svelte';

/** Render an SVG string onto a white w×h canvas and return grayscale bytes for the grader. */
export async function svgGray(svg: string, w: number, h: number): Promise<Uint8Array> {
	const url = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }));
	try {
		const img = new Image(w, h);
		img.src = url;
		await img.decode();
		const c = document.createElement('canvas');
		c.width = w;
		c.height = h;
		const ctx = c.getContext('2d', { willReadFrequently: true })!;
		ctx.fillStyle = '#fff';
		ctx.fillRect(0, 0, w, h);
		ctx.drawImage(img, 0, 0, w, h);
		return toGray(ctx.getImageData(0, 0, w, h).data);
	} finally {
		URL.revokeObjectURL(url);
	}
}

export interface RoundResult {
	/** 1–10 */
	score: number;
	/** grader's 1–10 score before any blending */
	graderScore: number;
	/** 0–1, from the grader's stroke-quality metrics; null when there were no strokes */
	lineConfidence: number | null;
	line: LineMetrics | null;
}

const blank: RoundResult = { score: 1, graderScore: 1, lineConfidence: null, line: null };

/**
 * Score one round:
 * - blind drills: `1 + 9 × line confidence` only (it's a looking exercise);
 * - line drills: grader score blended with straightness / endpoints / angle / taper (`roundScore`);
 * - everything else: the grader's fundamentals score (with the drill's weight overrides).
 */
export async function gradeRound(
	drill: Drill,
	drawing: DrawingExport,
	target: DrillTarget,
	reference: Reference | null
): Promise<RoundResult> {
	const { width: w, height: h } = drawing;
	if (!drawing.strokes.some((s) => !s.eraser)) return blank;
	const ref = reference
		? toGray((await referencePixels(reference, w, h)).data)
		: await svgGray(target.answer, w, h);
	const g = await gradeImages(w, h, drawing.gray, ref, {
		category: 'fundamentals',
		strokes: drawing.strokes,
		weights: drill.weights,
		curve: drill.curve,
		// value drills are shading and hatching, which the line-direction check can't judge
		relevanceGate: drill.kind !== 'values'
	});
	if (g.flags.includes('blank')) return blank;
	const lineConfidence = measurement(g, 'line_confidence') ?? null;
	if (drill.blind) {
		const score = Math.round((1 + 9 * (lineConfidence ?? 0)) * 10) / 10;
		return { score, graderScore: g.score, lineConfidence, line: null };
	}
	const line = drill.kind === 'lines' ? lineMetrics(drawing.strokes, target) : null;
	return { score: roundScore(g.score, line), graderScore: g.score, lineConfidence, line };
}
