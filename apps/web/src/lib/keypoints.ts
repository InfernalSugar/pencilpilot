/**
 * Reference keypoints (M3-T04, M5-T03): spots marked on a reference in /dev/keypoints,
 * which the student taps on their own drawing after handing it in. The grader fits the
 * two sets of points onto each other (Procrustes) and measures what's left: proportions.
 */
import type { KeypointPair, Point } from './grader';
import type { Reference } from './references';

/** A marked spot on a reference. `x`, `y` are fractions (0–1) of the picture's width and height. */
export interface RefKeypoint {
	x: number;
	y: number;
	/** what the student looks for, e.g. "nose tip" */
	label: string;
}

/** Fewer than this and the proportions check isn't worth the student's time. */
export const MIN_KEYPOINTS = 3;
export const MAX_KEYPOINTS = 12;
const MAX_LABEL = 40;

/**
 * Validate keypoints from a manifest or a form. Returns null if anything is off
 * (so a typo in a manifest can't send students a broken step).
 */
export function cleanKeypoints(raw: unknown): RefKeypoint[] | null {
	if (!Array.isArray(raw) || raw.length > MAX_KEYPOINTS) return null;
	const out: RefKeypoint[] = [];
	for (const k of raw) {
		if (!k || typeof k !== 'object') return null;
		const { x, y, label } = k as Record<string, unknown>;
		if (typeof x !== 'number' || typeof y !== 'number' || typeof label !== 'string') return null;
		if (!(x >= 0 && x <= 1 && y >= 0 && y <= 1)) return null;
		const l = label.trim().slice(0, MAX_LABEL);
		if (!l) return null;
		out.push({ x: round(x), y: round(y), label: l });
	}
	return out;
}

const round = (v: number) => Math.round(v * 10000) / 10000;

/** The reference's usable keypoints ([] if it has none, too few, or they're malformed). */
export function refKeypoints(r: Pick<Reference, 'keypoints'> | null | undefined): RefKeypoint[] {
	const k = cleanKeypoints(r?.keypoints ?? []);
	return k && k.length >= MIN_KEYPOINTS ? k : [];
}

/** Where a w×h picture lands when fitted inside a W×H canvas (as the grader places it). */
export function containRect(pw: number, ph: number, W: number, H: number) {
	const s = Math.min(W / pw, H / ph);
	const w = pw * s;
	const h = ph * s;
	return { x: (W - w) / 2, y: (H - h) / 2, w, h };
}

/** A reference keypoint in canvas pixels. */
export function onCanvas(
	k: Pick<RefKeypoint, 'x' | 'y'>,
	ref: Pick<Reference, 'width' | 'height'>,
	W: number,
	H: number
): Point {
	const r = containRect(ref.width || 1, ref.height || 1, W, H);
	return { x: r.x + k.x * r.w, y: r.y + k.y * r.h };
}

/** What the student tapped for each keypoint: a canvas point, or null for "I didn't draw that". */
export type Taps = (Point | null)[];

/**
 * Pairs for the grader: each tapped point with its reference keypoint, both in canvas pixels.
 * Points the student skipped are left out. Returns [] if too few are left to judge.
 */
export function keypointPairs(
	ref: Pick<Reference, 'width' | 'height' | 'keypoints'>,
	taps: Taps,
	W: number,
	H: number
): KeypointPair[] {
	const kps = refKeypoints(ref);
	const pairs = kps.flatMap((k, i) => {
		const t = taps[i];
		return t ? [{ reference: onCanvas(k, ref, W, H), drawing: { x: t.x, y: t.y } }] : [];
	});
	return pairs.length >= MIN_KEYPOINTS ? pairs : [];
}

/** Indexes (into the reference's keypoints) of the points that made it into `keypointPairs`. */
export function tappedIndexes(taps: Taps): number[] {
	const idx = taps.flatMap((t, i) => (t ? [i] : []));
	return idx.length >= MIN_KEYPOINTS ? idx : [];
}

/** Label ideas for the annotator, by category and topic. */
export function labelIdeas(category: string, topic?: string): string[] {
	const face = [
		'top of head',
		'chin',
		'left eye',
		'right eye',
		'nose tip',
		'mouth',
		'left ear',
		'right ear'
	];
	const body = [
		'top of head',
		'chin',
		'left shoulder',
		'right shoulder',
		'left hand',
		'right hand',
		'hips',
		'left foot',
		'right foot'
	];
	if (topic === 'hands')
		return ['wrist', 'thumb tip', 'index tip', 'middle tip', 'ring tip', 'little finger tip'];
	if (['full-body', 'action', 'figures', 'gesture'].includes(topic ?? '')) return body;
	if (['people', 'portraits', 'characters', 'chibi'].includes(topic ?? '')) return face;
	switch (category) {
		case 'anime':
		case 'cartoon':
		case 'real-life':
			return [...face, ...body.slice(2)];
		case 'animals':
			return [
				'nose',
				'eye',
				'ear tip',
				'top of head',
				'chin',
				'shoulder',
				'front paw',
				'back paw',
				'hip',
				'tail tip'
			];
		case 'environment':
			return [
				'horizon left',
				'horizon right',
				'roof peak',
				'building corner',
				'door',
				'tree top',
				'path end'
			];
		default:
			return ['top', 'bottom', 'left-most point', 'right-most point', 'centre'];
	}
}
