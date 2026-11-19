/**
 * Grade a submitted drawing and record it in the student's history. Shared by
 * the results page and the gesture-session summary.
 */
import {
	getFeedback,
	gradeImages,
	measurement,
	toGray,
	type Feedback,
	type Grade,
	type GradeCategory,
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
import type { Submission } from './submission';

/** The skills shown in breakdowns (and stored per attempt), in plain words. */
export const SKILLS: { name: MeasurementName; label: string; skill: string }[] = [
	{ name: 'placement', label: 'Size & placement', skill: 'placement' },
	{ name: 'outline', label: 'Outlines', skill: 'outline' },
	{ name: 'silhouette', label: 'Big shapes', skill: 'silhouette' },
	{ name: 'tone', label: 'Light & dark', skill: 'tone' },
	{ name: 'line_confidence', label: 'Line confidence', skill: 'line_confidence' }
];

export interface Graded {
	grade: Grade;
	fb: Feedback;
	/** the curated or Endless reference, if any */
	ref: Reference | undefined;
	/** for traced drawings: the picture as it was placed (data URL) */
	compareSrc: string | null;
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
	const cat = ref?.category ?? 'fundamentals';
	const grade = await gradeImages(sub.width, sub.height, drawing, toGray(refData.data), {
		// Cartoons are graded like anime: clean outlines matter most.
		category: (cat === 'cartoon' ? 'anime' : cat) as GradeCategory,
		strokes: sub.strokeData ?? []
	});
	const fb = await getFeedback(grade, sub.at ?? 0, ref?.category);
	return { grade, fb, ref, compareSrc: placed ? (await placed).url : null };
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
	if (sub.traced || !ref || alreadyRecorded(sub.at)) return false;
	recordAttempt({
		category: ref.category,
		topic: sub.topic ?? ref.topic ?? null,
		tier: sub.tier ?? tierOf(ref),
		score: grade.score,
		at: sub.at ?? Date.now(),
		refId: ref.id,
		title: titleOf(ref),
		fixes: fb.fixes.map((f) => f.id),
		skills: Object.fromEntries(
			SKILLS.map((s) => [s.skill, measurement(grade, s.name)]).filter(([, v]) => v !== undefined)
		),
		...(sub.timeLimit ? { timeLimit: sub.timeLimit } : {})
	});
	if (sub.at) saveThumb(sub.at, sub.png);
	return true;
}
