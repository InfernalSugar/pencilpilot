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
/** pictures kept decoded at once (the rest wait as JPEG files) */
const DECODED = 8;

/** decoded pictures, most recently used last (shared by every timelapse) */
const decoded = new Map<FilmFrame, ImageBitmap>();

/**
 * One frame of a timelapse. Kept as a JPEG file (a few dozen KB, not 1.2 MB of pixels:
 * a long session's frames once filled 1.4 GB), decoded when it's shown.
 */
export class FilmFrame {
	/** the picture as a JPEG file (null: the browser can't make one, so `#bmp` is kept) */
	jpeg: Blob | null;
	#bmp: ImageBitmap | null;
	/** the frame as kept in a project file (made once; frames from a file come with theirs) */
	saved: Uint8Array | null = null;
	constructor(
		readonly width: number,
		readonly height: number,
		/** when it was taken (ms since 1970) */
		readonly t: number,
		jpeg: Blob | null,
		bmp: ImageBitmap | null = null
	) {
		this.jpeg = jpeg;
		this.#bmp = bmp;
	}

	/**
	 * The picture, to draw at once. Owned by the frame: don't close it, and don't keep it
	 * past the next frame's picture (only the last few stay decoded).
	 */
	async picture(): Promise<ImageBitmap> {
		if (this.#bmp) return this.#bmp;
		const known = decoded.get(this);
		if (known) {
			decoded.delete(this);
			decoded.set(this, known);
			return known;
		}
		const bmp = await createImageBitmap(this.jpeg!);
		decoded.set(this, bmp);
		while (decoded.size > DECODED) {
			const [f, old] = decoded.entries().next().value!;
			decoded.delete(f);
			old.close();
		}
		return bmp;
	}

	close() {
		this.#bmp?.close();
		this.#bmp = null;
		decoded.get(this)?.close();
		decoded.delete(this);
	}
}

/** a canvas of w × h to draw on (an OffscreenCanvas where there is one) */
function scratch(w: number, h: number): OffscreenCanvas | HTMLCanvasElement {
	if (typeof OffscreenCanvas !== 'undefined') return new OffscreenCanvas(w, h);
	const c = document.createElement('canvas');
	[c.width, c.height] = [w, h];
	return c;
}

function toJpeg(c: OffscreenCanvas | HTMLCanvasElement, quality: number): Promise<Blob | null> {
	if ('convertToBlob' in c) return c.convertToBlob({ type: 'image/jpeg', quality });
	return new Promise((done) => c.toBlob(done, 'image/jpeg', quality));
}

export class Timelapse {
	frames: FilmFrame[] = [];
	#busy: Promise<void> = Promise.resolve();
	#listeners = new Set<() => void>();

	get length() {
		return this.frames.length;
	}

	/** when each frame was taken (ms since 1970) */
	get times() {
		return this.frames.map((f) => f.t);
	}

	/**
	 * Add a frame of `src` as it is now (kept in order, after any frame still being made).
	 * Cheapest from a canvas already at most FRAME_MAX on its long side (`TimelapseMirror`).
	 */
	capture(src: CanvasImageSource & { width: number; height: number }) {
		const k = Math.min(1, FRAME_MAX / Math.max(src.width, src.height));
		const [w, h] = [
			Math.max(1, Math.round(src.width * k)),
			Math.max(1, Math.round(src.height * k))
		];
		const t = Date.now();
		// the picture is taken now (the canvas goes on changing); made a file after
		let snap: Promise<ImageBitmap>;
		try {
			snap =
				k === 1
					? createImageBitmap(src)
					: createImageBitmap(src, { resizeWidth: w, resizeHeight: h, resizeQuality: 'medium' });
		} catch {
			return;
		}
		this.#busy = this.#busy
			.then(async () => {
				const bmp = await snap;
				let jpeg: Blob | null = null;
				try {
					const c = scratch(w, h);
					const g = c.getContext('2d') as CanvasRenderingContext2D | null;
					if (g) {
						g.fillStyle = '#fff';
						g.fillRect(0, 0, w, h);
						g.drawImage(bmp, 0, 0);
						jpeg = await toJpeg(c, 0.9);
					}
				} catch {
					/* kept as pixels instead */
				}
				if (jpeg) bmp.close();
				this.frames.push(new FilmFrame(w, h, t, jpeg, jpeg ? null : bmp));
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
		this.frames = thin(this.frames);
	}

	clear() {
		for (const f of this.frames) f.close();
		this.frames = [];
		for (const l of this.#listeners) l();
	}

	/**
	 * Put back frames kept in a file (JPEG files and when each was taken), before any
	 * taken since. Pictures that can't be read are skipped.
	 */
	async load(saved: { jpeg: Uint8Array; t: number }[]) {
		const back: FilmFrame[] = [];
		for (const s of saved) {
			try {
				const blob = new Blob([s.jpeg as BlobPart], { type: 'image/jpeg' });
				// (read once for its size, then kept as the file)
				const bmp = await createImageBitmap(blob);
				const f = new FilmFrame(bmp.width, bmp.height, s.t, blob);
				bmp.close();
				f.saved = s.jpeg;
				back.push(f);
			} catch {
				/* skipped */
			}
		}
		this.frames = [...back, ...this.frames];
		while (this.frames.length > FRAME_CAP) this.#thin();
		for (const l of this.#listeners) l();
	}

	/** Every frame as a JPEG file (made once each), with when it was taken: to keep in a file. */
	async saved(): Promise<{ jpeg: Uint8Array; t: number }[]> {
		await this.#busy;
		const out: { jpeg: Uint8Array; t: number }[] = [];
		for (const f of this.frames.slice()) {
			if (!f.saved) {
				const k = Math.min(1, SAVED_MAX / Math.max(f.width, f.height));
				if (k === 1 && f.jpeg) f.saved = new Uint8Array(await f.jpeg.arrayBuffer());
				else {
					const [w, h] = [
						Math.max(1, Math.round(f.width * k)),
						Math.max(1, Math.round(f.height * k))
					];
					const c = scratch(w, h);
					const g = c.getContext('2d') as CanvasRenderingContext2D;
					g.fillStyle = '#fff';
					g.fillRect(0, 0, w, h);
					g.imageSmoothingQuality = 'high';
					g.drawImage(await f.picture(), 0, 0, w, h);
					const blob = await toJpeg(c, 0.75);
					if (!blob) continue;
					f.saved = new Uint8Array(await blob.arrayBuffer());
				}
			}
			out.push({ jpeg: f.saved, t: f.t });
		}
		return out;
	}

	onchange(f: () => void) {
		this.#listeners.add(f);
		return () => this.#listeners.delete(f);
	}
}

/**
 * Pixels [x0, x1) × [y0, y1) of a picture `W`×`H` (RGBA `src`) shrunk to `w`×`h`, each
 * the average of the pixels it covers.
 */
export function shrinkRect(
	src: ArrayLike<number>,
	W: number,
	H: number,
	w: number,
	h: number,
	x0: number,
	y0: number,
	x1: number,
	y1: number
): Uint8ClampedArray<ArrayBuffer> {
	const out = new Uint8ClampedArray((x1 - x0) * (y1 - y0) * 4);
	const col = (x: number) => Math.floor((x * W) / w);
	const row = (y: number) => Math.floor((y * H) / h);
	let o = 0;
	for (let y = y0; y < y1; y++) {
		const r0 = row(y);
		const r1 = Math.max(r0 + 1, row(y + 1));
		for (let x = x0; x < x1; x++) {
			const c0 = col(x);
			const c1 = Math.max(c0 + 1, col(x + 1));
			let [r, g, b, a] = [0, 0, 0, 0];
			for (let yy = r0; yy < r1; yy++)
				for (let i = (yy * W + c0) * 4, e = (yy * W + c1) * 4; i < e; i += 4) {
					r += src[i];
					g += src[i + 1];
					b += src[i + 2];
					a += src[i + 3];
				}
			const n = (r1 - r0) * (c1 - c0);
			out[o++] = r / n;
			out[o++] = g / n;
			out[o++] = b / n;
			out[o++] = a / n;
		}
	}
	return out;
}

/**
 * The canvas at timelapse size, kept up to date a changed rectangle at a time: a frame
 * is then taken from it, not by shrinking the whole canvas after every stroke (at 4K
 * that held up the next frame by 25 ms on a slower computer). Small changes are shrunk
 * from the engine's pixels (reading the canvas itself would copy all of it each time).
 */
export class TimelapseMirror {
	canvas: OffscreenCanvas | HTMLCanvasElement | null = null;
	#g: CanvasRenderingContext2D | null = null;

	/**
	 * Copy `rect` ([x, y, w, h], canvas pixels) of the canvas `src`, from `pixels` (its
	 * RGBA, as the engine has them) when given; the whole canvas when `rect` isn't.
	 */
	update(src: HTMLCanvasElement, pixels?: ArrayLike<number>, rect?: ArrayLike<number>) {
		const { width: W, height: H } = src;
		const k = Math.min(1, FRAME_MAX / Math.max(W, H));
		const [w, h] = [Math.max(1, Math.round(W * k)), Math.max(1, Math.round(H * k))];
		if (!this.canvas || this.canvas.width !== w || this.canvas.height !== h) {
			try {
				this.canvas = scratch(w, h);
				this.#g = this.canvas.getContext('2d') as CanvasRenderingContext2D | null;
			} catch {
				this.#g = null;
			}
			if (!this.#g) return;
			this.#g.imageSmoothingQuality = 'medium';
			rect = undefined;
		}
		const g = this.#g;
		if (!g) return;
		// a big change (a new frame shown, a fill): the browser shrinks the whole canvas
		if (!rect || !pixels || rect[2] * rect[3] * 4 > W * H) {
			g.drawImage(src, 0, 0, w, h);
			return;
		}
		const [sx, sy] = [W / w, H / h];
		// the pixels of the small picture the rectangle touches (and one more round them)
		const x0 = Math.max(0, Math.floor(rect[0] / sx) - 1);
		const y0 = Math.max(0, Math.floor(rect[1] / sy) - 1);
		const x1 = Math.min(w, Math.ceil((rect[0] + rect[2]) / sx) + 1);
		const y1 = Math.min(h, Math.ceil((rect[1] + rect[3]) / sy) + 1);
		if (x1 <= x0 || y1 <= y0) return;
		const px = shrinkRect(pixels, W, H, w, h, x0, y0, x1, y1);
		g.putImageData(new ImageData(px, x1 - x0, y1 - y0), x0, y0);
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
 * Where a `w`×`h` picture goes in a `W`×`H` film: as big as fits, in the middle (frames
 * from before a canvas size change keep their shape, with paper around them).
 */
export function fitRect(w: number, h: number, W: number, H: number) {
	const k = Math.min(W / Math.max(1, w), H / Math.max(1, h));
	const [width, height] = [w * k, h * k];
	return { x: (W - width) / 2, y: (H - height) / 2, width, height };
}

/**
 * Record the frames to a video (in real time, so it takes as long as the film), the
 * size of the last (the finished picture). The last frame is held for a second at the end.
 */
export async function recordFilm(
	frames: FilmFrame[],
	fps: number,
	onprogress?: (done: number) => void
): Promise<{ blob: Blob; ext: string }> {
	const type = videoType();
	if (!type) throw new Error('This browser can’t record video.');
	if (!frames.length) throw new Error('Nothing to record yet.');
	const last = frames[frames.length - 1];
	const [w, h] = [last.width & ~1, last.height & ~1];
	const c = document.createElement('canvas');
	c.width = w;
	c.height = h;
	const g = c.getContext('2d')!;
	const stream = c.captureStream(fps);
	const rec = new MediaRecorder(stream, { mimeType: type.mime, videoBitsPerSecond: 4_000_000 });
	const chunks: Blob[] = [];
	rec.ondataavailable = (e) => e.data.size && chunks.push(e.data);
	const done = new Promise<void>((res) => (rec.onstop = () => res()));
	const draw = async (f: FilmFrame) => {
		const pic = await f.picture();
		g.fillStyle = '#fff';
		g.fillRect(0, 0, w, h);
		const at = fitRect(f.width, f.height, w, h);
		g.drawImage(pic, at.x, at.y, at.width, at.height);
	};
	await draw(frames[0]);
	rec.start();
	const step = 1000 / fps;
	const t0 = performance.now();
	for (let i = 0; i < frames.length; i++) {
		await draw(frames[i]);
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
