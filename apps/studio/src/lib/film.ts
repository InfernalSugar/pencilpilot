/**
 * Films of the drawing being made (S3-T05):
 *
 * - **Timelapse:** the pictures the canvas took after every change (kept in the project
 *   across sessions), played over a chosen length, ending on the finished picture.
 * - **Drawn live:** the strokes of the frame showing, drawn again at the speed they were
 *   drawn (long pauses shortened), then an eased dissolve into the finished frame (fills
 *   and everything else that isn't a stroke).
 */
import type { Stroke } from '@pencilpilot/paint/strokes';

export type FilmKind = 'timelapse' | 'live';
export type FilmFormat = 'mp4' | 'webm' | 'gif';

export interface FilmSettings {
	kind: FilmKind;
	format: FilmFormat;
	/** timelapse: about how long (seconds) */
	seconds: number;
	/** drawn live: how many times faster than drawn */
	speed: number;
	/** drawn live: of the camera's size */
	scale: number;
}

export const FILM_DEFAULTS: FilmSettings = {
	kind: 'timelapse',
	format: 'mp4',
	seconds: 15,
	speed: 2,
	scale: 1
};

/** Frames a second of the films. */
export const FILM_FPS = 24;

/** Pauses between strokes are kept between these (seconds, before speeding up). */
const GAP_MIN = 0.1;
const GAP_MAX = 0.5;
/** A pause within a stroke (the pen held still) is kept up to this. */
const HOLD_MAX = 0.25;

/** A point of a stroke drawn by a moment of the film (seconds, sped up). */
export interface LiveEvent {
	stroke: number;
	/** points drawn by then (past the end: the stroke is finished) */
	points: number;
	t: number;
}

/** When each point is drawn in a drawn-live film, and how long the drawing takes. */
export function liveSchedule(
	strokes: Stroke[],
	speed = 1
): { events: LiveEvent[]; duration: number } {
	const k = 1 / Math.max(0.1, speed);
	const events: LiveEvent[] = [];
	let clock = 0;
	let prevEnd: number | null = null;
	strokes.forEach((s, i) => {
		const pts = s.points;
		if (!pts.length) return;
		const gap =
			prevEnd === null
				? GAP_MIN
				: pts[0].t >= prevEnd
					? Math.min(GAP_MAX, Math.max(GAP_MIN, (pts[0].t - prevEnd) / 1000))
					: // (an earlier session: times started again)
						GAP_MAX / 2;
		clock += gap * k;
		for (let j = 0; j < pts.length; j++) {
			if (j > 0) clock += Math.min(HOLD_MAX, Math.max(0, (pts[j].t - pts[j - 1].t) / 1000)) * k;
			events.push({ stroke: i, points: j + 1, t: clock });
		}
		// (finished: one past the last point)
		events.push({ stroke: i, points: pts.length + 1, t: clock });
		prevEnd = pts.at(-1)!.t;
	});
	return { events, duration: clock };
}

/**
 * The drawing part of a drawn-live film as pictures: each shows the first `upTo`
 * events and is held `repeat` frames. A new picture starts only when a stroke moved,
 * so frames where nothing new is drawn are sent once, held. (The exporter counts
 * pictures, so this is also how many the film announces.)
 */
export function liveSteps(
	times: number[],
	frames: number,
	fps: number
): { upTo: number; repeat: number }[] {
	const out: { upTo: number; repeat: number }[] = [];
	let e = 0;
	for (let i = 0; i < frames; i++) {
		const t = (i + 1) / fps;
		const from = e;
		while (e < times.length && times[e] <= t) e++;
		const last = out.at(-1);
		if (last && e === from) last.repeat++;
		else out.push({ upTo: e, repeat: 1 });
	}
	return out;
}

/** Smooth in and out (0 → 1), for the dissolve into the finished frame. */
export const ease = (x: number) => {
	const t = Math.min(1, Math.max(0, x));
	return t * t * (3 - 2 * t);
};

/**
 * The strokes that made the frame showing: on these layers, drawn on the drawing each
 * shows. A stroke from before strokes noted their drawing counts when its layer has
 * only one drawing.
 */
export function strokesOfFrame(
	strokes: Stroke[],
	pairs: [number, number][],
	drawingsOnLayer: (layer: number) => number
): Stroke[] {
	const shown = new Map(pairs);
	return strokes.filter((s) => {
		if (s.layer === undefined || !shown.has(s.layer) || !s.points.length) return false;
		const d = shown.get(s.layer)!;
		if (s.drawing !== undefined) return s.drawing === d && d !== 0;
		return d !== 0 && drawingsOnLayer(s.layer) <= 1;
	});
}

/** Run lengths: equal neighbours joined, as [value, repeat] pairs. */
export function runs<T>(items: T[]): [T, number][] {
	const out: [T, number][] = [];
	for (const it of items) {
		const last = out.at(-1);
		if (last && last[0] === it) last[1]++;
		else out.push([it, 1]);
	}
	return out;
}
