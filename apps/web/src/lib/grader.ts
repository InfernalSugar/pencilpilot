import init, { feedback as wasmFeedback, grade, defaultWeights } from 'pencilpilot-grader-wasm';
import type { Stroke } from './paint/strokes';

/** Category ids, same as `categories.ts`. */
export type GradeCategory = 'anime' | 'real-life' | 'animals' | 'environment' | 'fundamentals';

/** How much each measurement counts towards the score (normalised over what was measured). */
export interface GradeWeights {
	placement: number;
	outline: number;
	silhouette: number;
	grid: number;
	tone: number;
	ssim: number;
	lineConfidence: number;
	keypoints: number;
}

/** `t = clamp((m - floor) / (pass - floor), 0, 1)`, `score = 1 + 9 * t^gamma`. */
export interface ScoreCurve {
	floor: number;
	pass: number;
	gamma: number;
}

export interface Point {
	x: number;
	y: number;
}

/** A reference keypoint and where the student put the same feature (canvas pixels). */
export interface KeypointPair {
	reference: Point;
	drawing: Point;
}

export interface GradeOptions {
	category?: GradeCategory;
	/** Overrides the category's default weights (missing fields use the fundamentals defaults). */
	weights?: Partial<GradeWeights>;
	curve?: Partial<ScoreCurve>;
	/** Recorded strokes, for line-confidence metrics. */
	strokes?: readonly Stroke[];
	keypoints?: KeypointPair[];
}

/**
 * Stable measurement names. Similarities are 0–1 (1 = matches the reference);
 * the others are raw numbers. See `MEASUREMENT_DOCS` in `crates/grader/src/lib.rs`.
 */
export type MeasurementName =
	| 'placement'
	| 'scale_ratio'
	| 'aspect_error'
	| 'offset_x'
	| 'offset_y'
	| 'outline'
	| 'outline_precision'
	| 'outline_recall'
	| 'outline_distance'
	| 'silhouette'
	| 'grid'
	| 'worst_cell'
	| 'tone'
	| 'contrast_ratio'
	| 'ssim'
	| 'ink_coverage'
	| 'shading'
	| 'line_confidence'
	| 'smoothness'
	| 'curvature_jitter'
	| 'retrace_ratio'
	| 'speed_consistency'
	| 'speed_cv'
	| 'stroke_count'
	| 'keypoints'
	| 'keypoint_error'
	| 'keypoint_max_error'
	| 'angle_error';

export interface Measurement {
	name: MeasurementName;
	value: number;
}

export interface CellRef {
	col: number;
	row: number;
	index: number;
}

/** Outline similarity per region, row-major: `cells[row * cols + col]`. Empty cells score 1. */
export interface GridResult {
	cols: number;
	rows: number;
	cells: number[];
	worst: CellRef | null;
}

/**
 * Drawing → reference transform in canvas pixels:
 * `p_ref = c + scale * R(rotation) * (p_drawing - c) + (dx, dy)`, `c` = canvas centre.
 */
export interface Alignment {
	scale: number;
	dx: number;
	dy: number;
	/** degrees, clockwise on screen */
	rotation: number;
}

/** Fractions of the canvas, 0–1. */
export interface BBox {
	x: number;
	y: number;
	w: number;
	h: number;
}

export interface KeypointResult {
	keypointError: number;
	maxError: number;
	pairs: { error: number; fitted: Point }[];
	/** degrees, per consecutive keypoint pair */
	angleErrors: number[];
	meanAngleError: number;
	scale: number;
	rotation: number;
}

export type GradeFlag = 'blank' | 'reference_blank' | 'alignment_at_limit';

export interface Grade {
	/** 1–10 */
	score: number;
	/** weighted mean similarity before the curve, 0–1 */
	similarity: number;
	passed: boolean;
	category: GradeCategory;
	measurements: Measurement[];
	grid: GridResult;
	alignment: Alignment;
	bbox: { drawing: BBox | null; reference: BBox | null };
	keypoints: KeypointResult | null;
	flags: GradeFlag[];
}

let ready: Promise<unknown> | undefined;

async function load() {
	ready ??= init();
	await ready;
}

/** Grade a grayscale drawing against a grayscale reference (both width*height bytes). */
export async function gradeImages(
	width: number,
	height: number,
	drawing: Uint8Array,
	reference: Uint8Array,
	options: GradeOptions = {}
): Promise<Grade> {
	await load();
	return grade(width, height, drawing, reference, options) as Grade;
}

/** The built-in weights for a category. */
export async function categoryWeights(category: GradeCategory): Promise<GradeWeights> {
	await load();
	return defaultWeights(category) as GradeWeights;
}

/** Look up one measurement by name. */
export function measurement(g: Grade, name: MeasurementName): number | undefined {
	return g.measurements.find((m) => m.name === name)?.value;
}

/** Convert RGBA canvas pixels to grayscale bytes (luma, transparent = white). */
export function toGray(rgba: Uint8ClampedArray): Uint8Array {
	const out = new Uint8Array(rgba.length / 4);
	for (let i = 0; i < out.length; i++) {
		const [r, g, b, a] = [rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2], rgba[i * 4 + 3] / 255];
		const luma = 0.299 * r + 0.587 * g + 0.114 * b;
		out[i] = Math.round(luma * a + 255 * (1 - a));
	}
	return out;
}

/** One tip from the feedback engine (crates/feedback, content/feedback/*.toml). */
export interface FeedbackItem {
	/** rule id, or `praise-<skill>` */
	id: string;
	skill: string;
	title: string;
	text: string;
	/** 0–10 ranking weight (praise: 0) */
	severity: number;
	measurement: string;
	value: number;
	/** what the results overlay should highlight */
	overlay: 'bbox' | 'offset' | 'grid' | 'missing' | 'extra' | null;
	lesson: string | null;
	drill: string | null;
}

export interface Feedback {
	fixes: FeedbackItem[];
	praise: FeedbackItem | null;
}

/**
 * Ranked fixes (max 3, one per skill) + praise for a graded drawing.
 * `category` is the site category (e.g. "cartoon"), which can differ from the
 * grader category used for weights (cartoon is graded like anime); feedback
 * rules are written per site category.
 */
export async function getFeedback(g: Grade, seed: number, category?: string): Promise<Feedback> {
	await load();
	return wasmFeedback(g.measurements, category ?? g.category, seed >>> 0) as Feedback;
}

/** "measuring-with-units" → "Measuring with units" */
export const humanize = (id: string) => id.replace(/-/g, ' ').replace(/^./, (c) => c.toUpperCase());
