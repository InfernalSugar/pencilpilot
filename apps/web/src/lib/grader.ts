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
	/** main line directions (sketch mode leans on it) */
	angles?: number;
}

/** Finished drawing, or a rough sketch (ADR-0007). */
export type DrawingMode = 'finished' | 'sketch';

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
	mode?: DrawingMode;
	/** Overrides the category's default weights (missing fields use the fundamentals defaults). */
	weights?: Partial<GradeWeights>;
	curve?: Partial<ScoreCurve>;
	/** Recorded strokes, for line-confidence metrics. */
	strokes?: readonly Stroke[];
	keypoints?: KeypointPair[];
	/**
	 * Cap the score of drawings that don't depict the reference (flag `unrelated`).
	 * On unless `false`; value drills turn it off (shading is the point there).
	 */
	relevanceGate?: boolean;
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
	| 'angle_error'
	| 'angles'
	| 'passes'
	| 'structure'
	| 'relevance';

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

/** `unrelated`: the drawing doesn't depict the reference (lines don't follow its shapes); score capped near 1. */
export type GradeFlag = 'blank' | 'reference_blank' | 'alignment_at_limit' | 'unrelated';

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

/**
 * Start downloading and compiling the grader in the background (call while the
 * student is still drawing) so the results page can grade straight away.
 */
export function warmGrader() {
	const go = () => void load().catch(() => (ready = undefined));
	if ('requestIdleCallback' in window) requestIdleCallback(go, { timeout: 4000 });
	else setTimeout(go, 1500);
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
	const g = analysisSize(width, height);
	if (g.scale === 1) return grade(width, height, drawing, reference, options) as Grade;
	// Big canvases are graded at the usual size, so scores stay comparable and fast.
	const k = g.scale;
	// Only set what was given: the grader can't read a key whose value is undefined.
	const scaled: GradeOptions = { ...options };
	if (options.strokes)
		scaled.strokes = options.strokes.map((s) => ({
			...s,
			points: s.points.map((p) => ({ ...p, x: p.x * k, y: p.y * k }))
		}));
	if (options.keypoints) scaled.keypoints = options.keypoints.map((kp) => scaleKeypoint(kp, k));
	return grade(
		g.width,
		g.height,
		downscaleGray(drawing, width, height, g.width, g.height),
		downscaleGray(reference, width, height, g.width, g.height),
		scaled
	) as Grade;
}

/** The grader's working area: the classic 800×600 canvas. Larger drawings are scaled down to it. */
export const ANALYSIS_PIXELS = 800 * 600;

export function analysisSize(width: number, height: number) {
	const area = width * height;
	if (area <= ANALYSIS_PIXELS * 1.05) return { width, height, scale: 1 };
	const scale = Math.sqrt(ANALYSIS_PIXELS / area);
	return {
		width: Math.max(1, Math.round(width * scale)),
		height: Math.max(1, Math.round(height * scale)),
		scale
	};
}

/** Area-average downscale of a grayscale image. */
export function downscaleGray(src: Uint8Array, sw: number, sh: number, dw: number, dh: number) {
	const out = new Uint8Array(dw * dh);
	for (let y = 0; y < dh; y++) {
		const y0 = Math.floor((y * sh) / dh);
		const y1 = Math.max(y0 + 1, Math.floor(((y + 1) * sh) / dh));
		for (let x = 0; x < dw; x++) {
			const x0 = Math.floor((x * sw) / dw);
			const x1 = Math.max(x0 + 1, Math.floor(((x + 1) * sw) / dw));
			let sum = 0;
			for (let yy = y0; yy < y1; yy++) for (let xx = x0; xx < x1; xx++) sum += src[yy * sw + xx];
			out[y * dw + x] = Math.round(sum / ((y1 - y0) * (x1 - x0)));
		}
	}
	return out;
}

function scaleKeypoint(kp: KeypointPair, k: number): KeypointPair {
	const p = (q: Point): Point => ({ ...q, x: q.x * k, y: q.y * k });
	return { reference: p(kp.reference), drawing: p(kp.drawing) };
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
	overlay: 'bbox' | 'offset' | 'grid' | 'missing' | 'extra' | 'points' | null;
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
export async function getFeedback(
	g: Grade,
	seed: number,
	category?: string,
	mode: DrawingMode = 'finished'
): Promise<Feedback> {
	await load();
	return wasmFeedback(g.measurements, category ?? g.category, seed >>> 0, mode) as Feedback;
}

/** "measuring-with-units" → "Measuring with units" */
export const humanize = (id: string) => id.replace(/-/g, ' ').replace(/^./, (c) => c.toUpperCase());
