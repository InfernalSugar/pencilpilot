/**
 * Timelapse (Procreate's Time-lapse Replay): a small picture of the canvas
 * after every change, played back as a film of the drawing being made, and
 * saved as a video.
 */

/** frames are at most this many px on the long side */
export const FRAME_MAX = 720;
/** frames kept in a project file are at most this many px on the long side */
export const SAVED_MAX = 540;
/** past this, every other frame is dropped (the film keeps its whole length) */
export const FRAME_CAP = 1200;

export class Timelapse {
	frames: ImageBitmap[] = [];
	/** when each frame was taken (ms since 1970) */
	times: number[] = [];
	/** frames as JPEG files, once made (frames brought back from a file come with theirs) */
	#jpeg = new WeakMap<ImageBitmap, Uint8Array>();
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
				this.times.push(Date.now());
				if (this.frames.length > FRAME_CAP) this.#thin();
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

	#thin() {
		const keep = thinIndices(this.frames.length);
		const kept = new Set(keep);
		this.frames.forEach((f, i) => kept.has(i) || f.close());
		this.frames = keep.map((i) => this.frames[i]);
		this.times = keep.map((i) => this.times[i]);
	}

	clear() {
		for (const f of this.frames) f.close();
		this.frames = [];
		this.times = [];
		for (const l of this.#listeners) l();
	}

	/**
	 * Put back frames kept in a file (JPEG files and when each was taken), before any
	 * taken since. Pictures that can't be read are skipped.
	 */
	async load(saved: { jpeg: Uint8Array; t: number }[]) {
		const back: { bmp: ImageBitmap; t: number }[] = [];
		for (const s of saved) {
			try {
				const bmp = await createImageBitmap(new Blob([s.jpeg as BlobPart], { type: 'image/jpeg' }));
				this.#jpeg.set(bmp, s.jpeg);
				back.push({ bmp, t: s.t });
			} catch {
				/* skipped */
			}
		}
		this.frames = [...back.map((b) => b.bmp), ...this.frames];
		this.times = [...back.map((b) => b.t), ...this.times];
		while (this.frames.length > FRAME_CAP) this.#thin();
		for (const l of this.#listeners) l();
	}

	/** Every frame as a JPEG file (made once each), with when it was taken: to keep in a file. */
	async saved(): Promise<{ jpeg: Uint8Array; t: number }[]> {
		await this.#busy;
		const out: { jpeg: Uint8Array; t: number }[] = [];
		for (const [i, f] of this.frames.entries()) {
			let j = this.#jpeg.get(f);
			if (!j) {
				const k = Math.min(1, SAVED_MAX / Math.max(f.width, f.height));
				const [w, h] = [
					Math.max(1, Math.round(f.width * k)),
					Math.max(1, Math.round(f.height * k))
				];
				const c = new OffscreenCanvas(w, h);
				const g = c.getContext('2d')!;
				g.fillStyle = '#fff';
				g.fillRect(0, 0, w, h);
				g.imageSmoothingQuality = 'high';
				g.drawImage(f, 0, 0, w, h);
				const blob = await c.convertToBlob({ type: 'image/jpeg', quality: 0.75 });
				j = new Uint8Array(await blob.arrayBuffer());
				this.#jpeg.set(f, j);
			}
			out.push({ jpeg: j, t: this.times[i] ?? 0 });
		}
		return out;
	}

	onchange(f: () => void) {
		this.#listeners.add(f);
		return () => this.#listeners.delete(f);
	}
}

/** Which of `n` frames thinning keeps: every other one, and the last. */
export function thinIndices(n: number): number[] {
	const out: number[] = [];
	for (let i = 0; i < n; i++) if (i % 2 === 0 || i === n - 1) out.push(i);
	return out;
}

/**
 * The film's frames at `fps` for about `seconds` (each source frame held for a whole
 * number of output frames, or skipped when there are too many), then the finished
 * picture held for `hold` seconds: which source frame each output frame shows.
 */
export function filmPlan(frames: number, fps: number, seconds: number, hold = 1.5): number[] {
	if (!frames) return [];
	const n = Math.max(1, Math.round(seconds * fps));
	const out: number[] = [];
	for (let i = 0; i < n; i++) out.push(Math.min(frames - 1, Math.floor((i * frames) / n)));
	for (let i = 0; i < Math.round(hold * fps); i++) out.push(frames - 1);
	return out;
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
