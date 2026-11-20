/**
 * Grade a submitted drawing and record it in the student's history. Shared by
 * the results page and the gesture-session summary.
 */
import { gradeCategory } from './grade-category';
import {
	getFeedback,
	gradeImages,
	measurement,
	toGray,
	type Feedback,
	type Grade,
	type MeasurementName
} from './grader';
import { recordAttempt, saveThumb } from './progress';
import {
	byId,
	imageUrl,
	NO_TRANSFORM,
	placedPixels,
	referencePixels,
	tierOf,
	titleOf,
	type Reference
} from './references';
import { keypointPairs } from './keypoints';
import type { Submission } from './submission';
import type { Stroke } from './paint/strokes';

/** The skills shown in breakdowns (and stored per attempt), in plain words. */
export const SKILLS: { name: MeasurementName; label: string; skill: string }[] = [
	{ name: 'placement', label: 'Size & placement', skill: 'placement' },
	{ name: 'outline', label: 'Outlines', skill: 'outline' },
	{ name: 'silhouette', label: 'Big shapes', skill: 'silhouette' },
	{ name: 'tone', label: 'Light & dark', skill: 'tone' },
	{ name: 'line_confidence', label: 'Line confidence', skill: 'line_confidence' },
	// only measured when the student did the proportions check
	{ name: 'keypoints', label: 'Proportions', skill: 'proportions' }
];

/** What a sketch is judged on (ADR-0007): no shading or line neatness. */
export const SKETCH_SKILLS: { name: MeasurementName; label: string; skill: string }[] = [
	{ name: 'placement', label: 'Size & placement', skill: 'placement' },
	{ name: 'silhouette', label: 'Big shapes', skill: 'silhouette' },
	{ name: 'angles', label: 'Main angles', skill: 'angles' },
	{ name: 'outline', label: 'Outlines (loose)', skill: 'outline' },
	{ name: 'keypoints', label: 'Proportions', skill: 'proportions' }
];

export interface Graded {
	grade: Grade;
	fb: Feedback;
	/** the curated or Endless reference, if any */
	ref: Reference | undefined;
	/** for traced drawings: the picture as it was placed (data URL) */
	compareSrc: string | null;
	/** the reference as graded (grayscale, canvas size), for re-grading partial drawings */
	refGray: Uint8Array;
}

export const referenceOf = (sub: Submission) => sub.reference ?? byId(sub.referenceId);

/** Whether this submission can be graded (a reference, or a traced picture of your own). */
export const gradable = (sub: Submission) =>
	!!referenceOf(sub) || (sub.trace?.source === 'own' && !!sub.trace.ownUrl);

async function decodeGray(png: string, w: number, h: number) {
	const img = new Image();
	img.src = png;
	await img.decode();
	const c = new OffscreenCanvas(w, h);
	const ctx = c.getContext('2d')!;
	ctx.drawImage(img, 0, 0, w, h);
	return toGray(ctx.getImageData(0, 0, w, h).data);
}

export async function gradeSubmission(sub: Submission): Promise<Graded> {
	const ref = referenceOf(sub);
	const own = sub.trace?.source === 'own' ? sub.trace.ownUrl : undefined;
	if (!ref && !own) throw new Error('nothing to compare with');
	const placed = sub.trace
		? placedPixels(
				own ?? imageUrl(ref!),
				sub.width,
				sub.height,
				sub.trace.transform ?? NO_TRANSFORM
			)
		: null;
	const [drawing, refData] = await Promise.all([
		decodeGray(sub.png, sub.width, sub.height),
		placed ? placed.then((p) => p.data) : referencePixels(ref!, sub.width, sub.height)
	]);
	// own pictures are graded like the kind of picture the student picked
	const cat = ref?.gradeAs ?? ref?.category ?? 'fundamentals';
	const mode = sub.mode ?? 'finished';
	const refGray = toGray(refData.data);
	const grade = await gradeImages(sub.width, sub.height, drawing, refGray, {
		// Cartoons are graded like anime, master studies like real life.
		category: gradeCategory(cat),
		mode,
		strokes: sub.strokeData ?? [],
		// the proportions check: only for the reference as it's placed by default (not traced)
		keypoints:
			ref && !sub.trace && sub.keypointTaps
				? keypointPairs(ref, sub.keypointTaps, sub.width, sub.height)
				: []
	});
	const fb = await getFeedback(grade, sub.at ?? 0, ref?.category, mode);
	return { grade, fb, ref, compareSrc: placed ? (await placed).url : null, refGray };
}

/** Tip ids the grader gives a (partial) version of the drawing: for replay moments. */
export async function tipsFor(
	sub: Submission,
	g: Graded,
	gray: Uint8Array,
	strokes: Stroke[]
): Promise<string[]> {
	const cat = g.ref?.gradeAs ?? g.ref?.category ?? 'fundamentals';
	const mode = sub.mode ?? 'finished';
	const grade = await gradeImages(sub.width, sub.height, gray, g.refGray, {
		category: gradeCategory(cat),
		mode,
		strokes
	});
	return (await getFeedback(grade, sub.at ?? 0, g.ref?.category, mode)).fixes.map((f) => f.id);
}

/** Reloading a results page must not count the same drawing twice. Marks `at` as recorded. */
export function alreadyRecorded(at: number | undefined) {
	if (!at) return false;
	try {
		const seen: number[] = JSON.parse(sessionStorage.getItem('pp.recorded') ?? '[]');
		if (seen.includes(at)) return true;
		sessionStorage.setItem('pp.recorded', JSON.stringify([...seen, at].slice(-100)));
	} catch {
		/* storage blocked: may double count on reload, harmless */
	}
	return false;
}

/**
 * Add a graded drawing to the history (scores, skills, mistakes, thumbnail).
 * Traced drawings and drawings without a reference are never recorded.
 * Returns whether it was recorded now.
 */
export function recordGraded(sub: Submission, g: Graded): boolean {
	const { ref, grade, fb } = g;
	// own pictures are scored but don't feed category progress (it's built from our references)
	if (sub.traced || !ref || ref.own || alreadyRecorded(sub.at)) return false;
	recordAttempt({
		category: ref.category,
		topic: sub.topic ?? ref.topic ?? null,
		tier: sub.tier ?? tierOf(ref),
		score: grade.score,
		at: sub.at ?? Date.now(),
		refId: ref.id,
		title: titleOf(ref),
		fixes: fb.fixes.map((f) => f.id),
		// sketches don't feed the skill radar: their lines and shading aren't judged;
		// nor do drawings of something else (their "big shapes" etc. mean nothing)
		skills:
			sub.mode === 'sketch' || grade.flags.includes('unrelated')
				? {}
				: Object.fromEntries(
						SKILLS.map((s) => [s.skill, measurement(grade, s.name)]).filter(
							([, v]) => v !== undefined
						)
					),
		...(sub.timeLimit ? { timeLimit: sub.timeLimit } : {}),
		...(sub.mode === 'sketch' ? { mode: 'sketch' as const } : {}),
		...(sub.personal ? { personal: sub.personal } : {})
	});
	if (sub.at) saveThumb(sub.at, sub.png);
	return true;
}
