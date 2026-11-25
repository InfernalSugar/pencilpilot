/**
 * The reference pictures of a project: pictures the user imports to draw from. They
 * are the user's own files, kept in the project (crates/ppstudio/FORMAT.md,
 * `references/`), never part of the drawing or its exports, and never fetched from
 * anywhere. Where they show (corner, side, a window) is a setting (settings.reference).
 */
import type { ProjectReference } from './project';

/** The most pictures a project keeps, and the most bytes each may have (as the file format). */
export const REF_MAX_COUNT = 200;
export const REF_MAX_BYTES = 64 << 20;

export interface RefPicture extends ProjectReference {
	id: number;
	/** an object URL for showing it */
	url: string;
	/** its size in px, once read */
	width: number;
	height: number;
}

/** What came of adding pictures: how many were added, and the names left out (and why). */
export interface RefAdded {
	added: number;
	skipped: { name: string; why: 'too big' | 'not a picture' | 'too many' | 'already here' }[];
}

const same = (a: Uint8Array, b: Uint8Array) => {
	if (a.length !== b.length) return false;
	for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
	return true;
};

/** The size of a picture, or null when it isn't one (the browser can't read it). */
async function pictureSize(bytes: Uint8Array): Promise<{ width: number; height: number } | null> {
	try {
		const bmp = await createImageBitmap(new Blob([bytes as BlobPart]));
		const size = { width: bmp.width, height: bmp.height };
		bmp.close();
		return size.width && size.height ? size : null;
	} catch {
		return null;
	}
}

export class RefShelf {
	pics = $state<RefPicture[]>([]);
	/** which picture shows */
	current = $state(0);
	private nextId = 1;

	constructor(
		/** how a picture is shown (an object URL in the app; tests pass their own) */
		private makeUrl: (bytes: Uint8Array, file: string) => string = (bytes, file) =>
			URL.createObjectURL(new Blob([bytes as BlobPart], { type: mimeOf(file) })),
		private dropUrl: (url: string) => void = (url) => URL.revokeObjectURL(url),
		private sizeOf: (
			bytes: Uint8Array
		) => Promise<{ width: number; height: number } | null> = pictureSize
	) {}

	get pic(): RefPicture | null {
		return this.pics[this.current] ?? null;
	}

	/** Add pictures after the others (the first one added shows). */
	async add(files: { name: string; bytes: Uint8Array }[]): Promise<RefAdded> {
		const out: RefAdded = { added: 0, skipped: [] };
		let first = -1;
		for (const f of files) {
			if (f.bytes.length > REF_MAX_BYTES) {
				out.skipped.push({ name: f.name, why: 'too big' });
				continue;
			}
			const had = this.pics.findIndex((p) => same(p.bytes, f.bytes));
			if (had >= 0) {
				out.skipped.push({ name: f.name, why: 'already here' });
				if (first < 0) first = had;
				continue;
			}
			if (this.pics.length >= REF_MAX_COUNT) {
				out.skipped.push({ name: f.name, why: 'too many' });
				continue;
			}
			const size = await this.sizeOf(f.bytes);
			if (!size) {
				out.skipped.push({ name: f.name, why: 'not a picture' });
				continue;
			}
			this.pics.push(this.make({ name: f.name, file: f.name, bytes: f.bytes }, size));
			if (first < 0) first = this.pics.length - 1;
			out.added++;
		}
		if (first >= 0) this.current = first;
		return out;
	}

	/** Take a picture away (the one after it shows, or the last). */
	remove(i: number) {
		const p = this.pics[i];
		if (!p) return;
		this.dropUrl(p.url);
		this.pics.splice(i, 1);
		if (this.current > i || this.current >= this.pics.length)
			this.current = Math.max(0, this.current - 1);
	}

	/** Show the next (+1) or previous (−1) picture, round. */
	step(d: number) {
		const n = this.pics.length;
		if (n) this.current = (((this.current + d) % n) + n) % n;
	}

	/** A project's pictures in place of these (none: an empty shelf). */
	async load(refs: ProjectReference[] | undefined) {
		this.clear();
		for (const r of refs ?? []) {
			const size = (await this.sizeOf(r.bytes)) ?? { width: 0, height: 0 };
			this.pics.push(this.make(r, size));
		}
	}

	clear() {
		for (const p of this.pics) this.dropUrl(p.url);
		this.pics = [];
		this.current = 0;
	}

	/** The pictures as the project keeps them. */
	toFile(): ProjectReference[] {
		return this.pics.map((p) => ({ name: p.name, file: p.file, bytes: p.bytes }));
	}

	private make(r: ProjectReference, size: { width: number; height: number }): RefPicture {
		return {
			...r,
			id: this.nextId++,
			url: this.makeUrl(r.bytes, r.file),
			width: size.width,
			height: size.height
		};
	}
}

/** A picture's type from its file name (the browser sniffs it anyway). */
export function mimeOf(file: string): string {
	const ext = file.split('.').pop()?.toLowerCase() ?? '';
	const types: Record<string, string> = {
		png: 'image/png',
		jpg: 'image/jpeg',
		jpeg: 'image/jpeg',
		webp: 'image/webp',
		gif: 'image/gif',
		bmp: 'image/bmp',
		avif: 'image/avif'
	};
	return types[ext] ?? 'application/octet-stream';
}

/** Why pictures were left out, in a sentence ('' when none were). */
export function skippedNote(r: RefAdded): string {
	if (!r.skipped.length) return '';
	const names = (why: RefAdded['skipped'][number]['why']) =>
		r.skipped.filter((s) => s.why === why).map((s) => s.name);
	const parts: string[] = [];
	const list = (n: string[]) =>
		n.length > 2 ? `${n.slice(0, 2).join(', ')} and ${n.length - 2} more` : n.join(' and ');
	const big = names('too big');
	const bad = names('not a picture');
	const many = names('too many');
	if (big.length) parts.push(`${list(big)} ${big.length > 1 ? 'are' : 'is'} over 64 MB`);
	if (bad.length)
		parts.push(
			`${list(bad)} ${bad.length > 1 ? 'aren’t pictures' : 'isn’t a picture'} the app can read`
		);
	if (many.length) parts.push(`a project keeps at most ${REF_MAX_COUNT} pictures`);
	return parts.length ? `Left out: ${parts.join('; ')}.` : '';
}

/**
 * The canvas grid on a reference (the grid method), as the website does it: the same
 * lines, where the picture sits when it's fitted inside the canvas, in fractions (0–1)
 * of the picture.
 */
export function gridOnPicture(
	lines: [number, number, number, number][],
	canvas: { width: number; height: number },
	pic: { width: number; height: number }
): [number, number, number, number][] | null {
	const at = lines.length ? pictureOnCanvas(canvas, pic) : null;
	if (!at) return null;
	const [x0, y0, w, h] = at;
	return lines.map(([a, b, c, d]) => [(a - x0) / w, (b - y0) / h, (c - x0) / w, (d - y0) / h]);
}

/**
 * Where a reference sits on the canvas: fitted inside it and centred, as the grid method
 * lays it. [x, y, width, height] in canvas pixels, or null before its size is known.
 */
export function pictureOnCanvas(
	canvas: { width: number; height: number },
	pic: { width: number; height: number }
): [number, number, number, number] | null {
	if (!pic.width || !pic.height || !canvas.width || !canvas.height) return null;
	const { width: W, height: H } = canvas;
	const k = Math.min(W / pic.width, H / pic.height);
	const [w, h] = [pic.width * k, pic.height * k];
	return [(W - w) / 2, (H - h) / 2, w, h];
}

/** A point on a reference (fractions of it, past 0–1 off it) → canvas pixels. */
export function canvasFromPicture(
	at: [number, number, number, number],
	[fx, fy]: [number, number]
): [number, number] {
	return [at[0] + fx * at[2], at[1] + fy * at[3]];
}
