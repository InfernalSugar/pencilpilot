/**
 * Timelapse (Procreate's Time-lapse Replay): a small picture of the canvas
 * after every change, played back as a film of the drawing being made, and
 * saved as a video.
 */

/** frames are at most this many px on the long side */
export const FRAME_MAX = 720;
/** past this, every other frame is dropped (the film keeps its whole length) */
export const FRAME_CAP = 1200;

export class Timelapse {
	frames: ImageBitmap[] = [];
	#busy: Promise<void> = Promise.resolve();
	#listeners = new Set<() => void>();

	get length() {
		return this.frames.length;
	}

	/** Add a frame of `src` (taken in order, after any frame still being made). */
	capture(src: CanvasImageSource & { width: number; height: number }) {
		const k = Math.min(1, FRAME_MAX / Math.max(src.width, src.height));
		this.#busy = this.#busy
			.then(() =>
				createImageBitmap(src, {
					resizeWidth: Math.max(1, Math.round(src.width * k)),
					resizeHeight: Math.max(1, Math.round(src.height * k)),
					resizeQuality: 'medium'
				})
			)
			.then((bmp) => {
				this.frames.push(bmp);
				if (this.frames.length > FRAME_CAP) this.frames = thin(this.frames);
				for (const l of this.#listeners) l();
			})
			.catch(() => {
				/* a frame that couldn't be made is just skipped */
			});
	}

	/** Wait for frames still being made. */
	settled() {
		return this.#busy;
	}

	clear() {
		for (const f of this.frames) f.close();
		this.frames = [];
		for (const l of this.#listeners) l();
	}

	onchange(f: () => void) {
		this.#listeners.add(f);
		return () => this.#listeners.delete(f);
	}
}

/** Every other frame (closing the dropped ones), keeping the last. */
export function thin<T extends { close(): void }>(frames: T[]): T[] {
	const keep = frames.filter((f, i) => i % 2 === 0 || i === frames.length - 1);
	for (const f of frames) if (!keep.includes(f)) f.close();
	return keep;
}

/** Frames per second so the film lasts about `seconds` (between 4 and 30 fps). */
export function filmFps(frames: number, seconds = 15): number {
	return Math.min(30, Math.max(4, Math.round(frames / seconds)));
}

/** The best video type this browser can record: MP4 if it can, else WebM. */
export function videoType(): { mime: string; ext: string } | null {
	if (typeof MediaRecorder === 'undefined') return null;
	for (const [mime, ext] of [
		['video/mp4;codecs=avc1', 'mp4'],
		['video/mp4', 'mp4'],
		['video/webm;codecs=vp9', 'webm'],
		['video/webm', 'webm']
	])
		if (MediaRecorder.isTypeSupported(mime)) return { mime, ext };
	return null;
}

/**
 * Record the frames to a video (in real time, so it takes as long as the film).
 * The last frame is held for a second at the end.
 */
export async function recordFilm(
	frames: ImageBitmap[],
	fps: number,
	onprogress?: (done: number) => void
): Promise<{ blob: Blob; ext: string }> {
	const type = videoType();
	if (!type) throw new Error('This browser can’t record video.');
	if (!frames.length) throw new Error('Nothing to record yet.');
	const [w, h] = [frames[0].width & ~1, frames[0].height & ~1];
	const c = document.createElement('canvas');
	c.width = w;
	c.height = h;
	const g = c.getContext('2d')!;
	const stream = c.captureStream(fps);
	const rec = new MediaRecorder(stream, { mimeType: type.mime, videoBitsPerSecond: 4_000_000 });
	const chunks: Blob[] = [];
	rec.ondataavailable = (e) => e.data.size && chunks.push(e.data);
	const done = new Promise<void>((res) => (rec.onstop = () => res()));
	const draw = (f: ImageBitmap) => {
		g.fillStyle = '#fff';
		g.fillRect(0, 0, w, h);
		g.drawImage(f, 0, 0, w, h);
	};
	draw(frames[0]);
	rec.start();
	const step = 1000 / fps;
	const t0 = performance.now();
	for (let i = 0; i < frames.length; i++) {
		draw(frames[i]);
		onprogress?.(i / frames.length);
		const wait = t0 + (i + 1) * step - performance.now();
		await new Promise((r) => setTimeout(r, Math.max(0, wait)));
	}
	await new Promise((r) => setTimeout(r, 1000));
	rec.stop();
	stream.getTracks().forEach((t) => t.stop());
	await done;
	onprogress?.(1);
	return { blob: new Blob(chunks, { type: type.mime.split(';')[0] }), ext: type.ext };
}
