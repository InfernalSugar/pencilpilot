/**
 * Exporting an animation: which frames go out, at what size. The window renders each
 * distinct frame once and sends it to the app's Rust side (src-tauri/src/export.rs) with
 * how many output frames it's held for, so a drawing on twos is sent once.
 *
 * Output frames follow time: at another frame rate, output frame i shows the cut's
 * frame at the same moment (i × fps / output fps), as a projector would.
 */
import { cameraOf, cutLength, showing, type Camera, type Cut, type Timeline } from './timeline';

export type ExportFormat = 'mp4' | 'prores' | 'webm' | 'gif' | 'apng' | 'png';

export const FORMATS: {
	id: ExportFormat;
	label: string;
	note: string;
	/** keeps transparency when asked */
	alpha: boolean;
	/** made by ffmpeg */
	video: boolean;
	ext: string;
}[] = [
	{
		id: 'mp4',
		label: 'MP4 video',
		note: 'H.264, plays anywhere',
		alpha: false,
		video: true,
		ext: 'mp4'
	},
	{
		id: 'prores',
		label: 'ProRes 4444',
		note: 'For editing; keeps transparency',
		alpha: true,
		video: true,
		ext: 'mov'
	},
	{
		id: 'webm',
		label: 'WebM video',
		note: 'VP9, for the web; keeps transparency',
		alpha: true,
		video: true,
		ext: 'webm'
	},
	{
		id: 'gif',
		label: 'GIF',
		note: 'Loops anywhere; 256 colours',
		alpha: true,
		video: false,
		ext: 'gif'
	},
	{
		id: 'apng',
		label: 'Animated PNG',
		note: 'Full colour, loops; keeps transparency',
		alpha: true,
		video: false,
		ext: 'png'
	},
	{
		id: 'png',
		label: 'PNG sequence',
		note: 'A picture per frame, numbered',
		alpha: true,
		video: false,
		ext: 'png'
	}
];

export type ExportRange = 'cut' | 'loop' | 'scene' | 'all';

export interface ExportSettings {
	format: ExportFormat;
	range: ExportRange;
	/** of the camera's size (1 = full) */
	scale: number;
	/** frames a second out (0: the project's) */
	fps: number;
	transparent: boolean;
	/** GIF / APNG: play once instead of looping */
	once: boolean;
}

export const EXPORT_DEFAULTS: ExportSettings = {
	format: 'mp4',
	range: 'cut',
	scale: 1,
	fps: 0,
	transparent: false,
	once: false
};

/** One frame of a cut to render. */
export interface Source {
	scene: number;
	cut: number;
	frame: number;
}

/** A distinct frame to render, and how many output frames it's held for. */
export interface Run {
	source: Source;
	/** [layer, drawing] pairs it shows */
	pairs: [number, number][];
	repeat: number;
}

/** The cuts a range covers (scene and cut indices), and the frames of each. */
export function cutsIn(
	tl: Timeline,
	range: ExportRange,
	at: { scene: number; cut: number },
	loop: { from: number; to: number } | null
): { scene: number; cut: number; from: number; to: number }[] {
	const whole = (scene: number, cut: number) => ({
		scene,
		cut,
		from: 0,
		to: cutLength(tl.scenes[scene].cuts[cut], tl.statics) - 1
	});
	if (range === 'all') return tl.scenes.flatMap((s, si) => s.cuts.map((_, ci) => whole(si, ci)));
	if (range === 'scene') return tl.scenes[at.scene].cuts.map((_, ci) => whole(at.scene, ci));
	const w = whole(at.scene, at.cut);
	if (range === 'loop' && loop)
		return [{ ...w, from: Math.max(0, loop.from), to: Math.min(w.to, loop.to) }];
	return [w];
}

/**
 * The runs to render: each cut's frames resampled to the output frame rate, frames in
 * a row that show the same drawings joined (within a cut: another cut can have
 * another camera).
 */
export function plan(
	tl: Timeline,
	cuts: { scene: number; cut: number; from: number; to: number }[],
	layers: number[],
	fps: number,
	outFps: number
): Run[] {
	const out: Run[] = [];
	for (const c of cuts) {
		const cut = tl.scenes[c.scene].cuts[c.cut];
		const frames = c.to - c.from + 1;
		const n = Math.max(1, Math.round((frames * outFps) / fps));
		let last: Run | null = null;
		let lastKey = '';
		for (let i = 0; i < n; i++) {
			const frame = c.from + Math.min(frames - 1, Math.floor((i * fps) / outFps));
			const pairs = showing(cut, layers, frame, tl.statics);
			const key = JSON.stringify(pairs);
			if (last && key === lastKey) last.repeat++;
			else {
				last = { source: { scene: c.scene, cut: c.cut, frame }, pairs, repeat: 1 };
				lastKey = key;
				out.push(last);
			}
		}
	}
	return out;
}

/** The output size: the first cut's camera, scaled (video sizes are made even). */
export function outputSize(camera: Camera, scale: number, even: boolean) {
	const round = (v: number) => {
		const r = Math.max(2, Math.round(v));
		return even ? r - (r % 2) : r;
	};
	return { width: round(camera.width * scale), height: round(camera.height * scale) };
}

/**
 * Where a cut's camera lands in the output: fitted inside it, centred (cuts framed
 * at another shape get bars).
 */
export function placeCamera(cam: Camera, out: { width: number; height: number }) {
	const k = Math.min(out.width / cam.width, out.height / cam.height);
	const [w, h] = [cam.width * k, cam.height * k];
	return { x: (out.width - w) / 2, y: (out.height - h) / 2, width: w, height: h, k };
}

/** The camera of each cut a plan uses. */
export const cameraFor = (cut: Cut, canvas: { width: number; height: number }) =>
	cameraOf(cut, canvas);
