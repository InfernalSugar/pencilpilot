/**
 * Bring in brushes from other apps:
 *
 * - **MyPaint** `.myb`: the engine runs these as they are.
 * - **Photoshop** `.abr`: the sampled tips (brush pictures) become picture tips
 *   on a plain round brush. Photoshop's other settings (dynamics, textures)
 *   have no match here and are left out.
 * - **Procreate** `.brush` / `.brushset`: each brush's Shape and Grain pictures
 *   become a picture tip and a grain. Its settings file (an Apple archive) isn't
 *   read, so sizes and dynamics start from defaults.
 */

export interface GrayPic {
	w: number;
	h: number;
	/** 255 = paint */
	px: Uint8Array;
}

export interface ImportedBrush {
	name: string;
	/** a MyPaint brush's own settings */
	myb?: string;
	tip?: GrayPic;
	grain?: GrayPic;
}

/** Turns PNG / JPEG bytes into pixels (the browser's decoder; tests pass their own). */
export type Decode = (bytes: Uint8Array) => Promise<ImageData>;

const MAX_TIP = 256;

/** `g` shrunk (box average) to at most `max` px a side. */
export function shrink(g: GrayPic, max = MAX_TIP): GrayPic {
	const k = Math.min(1, max / Math.max(g.w, g.h));
	if (k >= 1) return g;
	const w = Math.max(1, Math.round(g.w * k));
	const h = Math.max(1, Math.round(g.h * k));
	const px = new Uint8Array(w * h);
	for (let y = 0; y < h; y++)
		for (let x = 0; x < w; x++) {
			const [x0, x1] = [Math.floor((x * g.w) / w), Math.max(Math.floor(((x + 1) * g.w) / w), 1)];
			const [y0, y1] = [Math.floor((y * g.h) / h), Math.max(Math.floor(((y + 1) * g.h) / h), 1)];
			let s = 0;
			let n = 0;
			for (let yy = y0; yy < Math.max(y1, y0 + 1); yy++)
				for (let xx = x0; xx < Math.max(x1, x0 + 1); xx++) {
					s += g.px[Math.min(g.h - 1, yy) * g.w + Math.min(g.w - 1, xx)];
					n++;
				}
			px[y * w + x] = Math.round(s / n);
		}
	return { w, h, px };
}

/** Grey from decoded pixels: white-on-black shapes (Procreate) or dark-on-light pictures. */
export function grayOf(img: ImageData, whiteIsPaint: boolean): GrayPic {
	const px = new Uint8Array(img.width * img.height);
	// a picture with real transparency: its opacity is the shape
	let transparent = false;
	for (let i = 3; i < img.data.length; i += 4)
		if (img.data[i] < 250) {
			transparent = true;
			break;
		}
	for (let i = 0; i < px.length; i++) {
		const [r, g, b, a] = [
			img.data[i * 4],
			img.data[i * 4 + 1],
			img.data[i * 4 + 2],
			img.data[i * 4 + 3]
		];
		const lum = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
		// with transparency the opacity is the shape; otherwise the brightness is
		const v = transparent ? a / 255 : whiteIsPaint ? lum : 1 - lum;
		px[i] = Math.round(Math.min(1, Math.max(0, v)) * 255);
	}
	return { w: img.width, h: img.height, px };
}

// ---- MyPaint ----

export function parseMyb(text: string, name: string): ImportedBrush {
	const doc = JSON.parse(text);
	if (!doc || typeof doc !== 'object' || typeof doc.settings !== 'object')
		throw new Error('not a MyPaint brush (no settings)');
	return { name, myb: JSON.stringify(doc) };
}

// ---- Photoshop .abr ----

class Reader {
	pos = 0;
	constructor(public b: Uint8Array) {}
	get left() {
		return this.b.length - this.pos;
	}
	u8() {
		if (this.pos >= this.b.length) throw new Error('the file ends early');
		return this.b[this.pos++];
	}
	u16() {
		return (this.u8() << 8) | this.u8();
	}
	u32() {
		return ((this.u16() << 16) >>> 0) + this.u16();
	}
	i32() {
		return this.u32() | 0;
	}
	i16() {
		const v = this.u16();
		return v & 0x8000 ? v - 0x10000 : v;
	}
	bytes(n: number) {
		if (n < 0 || this.pos + n > this.b.length) throw new Error('the file ends early');
		const out = this.b.subarray(this.pos, this.pos + n);
		this.pos += n;
		return out;
	}
	ascii(n: number) {
		return String.fromCharCode(...this.bytes(n));
	}
}

/** PackBits rows (Photoshop's RLE) into `w`×`h` bytes. */
function unpackBits(r: Reader, w: number, h: number): Uint8Array {
	const counts = Array.from({ length: h }, () => r.u16());
	const out = new Uint8Array(w * h);
	let o = 0;
	for (const c of counts) {
		const end = r.pos + c;
		const rowEnd = o + w;
		while (r.pos < end && o < rowEnd) {
			let n = r.u8();
			if (n >= 128) n -= 256;
			if (n < 0) {
				if (n === -128) continue;
				const v = r.u8();
				for (let k = 0; k < 1 - n && o < rowEnd; k++) out[o++] = v;
			} else {
				for (let k = 0; k < n + 1 && o < rowEnd; k++) out[o++] = r.u8();
			}
		}
		r.pos = end;
		o = rowEnd;
	}
	return out;
}

/** One sampled tip's picture: bounds, depth, compression, data. */
function readSample(
	r: Reader,
	top: number,
	left: number,
	bottom: number,
	right: number
): GrayPic | null {
	const depth = r.u16();
	const compression = r.u8();
	const w = right - left;
	const h = bottom - top;
	if (w <= 0 || h <= 0 || w > 5000 || h > 5000) return null;
	let px: Uint8Array;
	if (compression === 0) {
		const bpp = depth >> 3 || 1;
		const raw = r.bytes(w * h * bpp);
		px = bpp === 1 ? raw.slice() : Uint8Array.from({ length: w * h }, (_, i) => raw[i * bpp]);
	} else px = unpackBits(r, w, h);
	return { w, h, px };
}

/** Photoshop brushes (.abr, versions 1, 2 and 6–10): their sampled tips. */
export function parseAbr(bytes: Uint8Array, name: string): ImportedBrush[] {
	const r = new Reader(bytes);
	const version = r.u16();
	const out: ImportedBrush[] = [];
	const add = (g: GrayPic | null) => {
		if (g) out.push({ name: `${name} ${out.length + 1}`, tip: shrink(g) });
	};
	if (version === 1 || version === 2) {
		const count = r.u16();
		for (let i = 0; i < count && r.left > 6; i++) {
			const type = r.u16();
			const size = r.u32();
			const end = r.pos + size;
			if (type === 2) {
				r.u32(); // misc
				r.u16(); // spacing
				if (version === 2) r.bytes(r.u32() * 2); // name, UTF-16
				r.u8(); // antialias
				r.bytes(8); // short bounds
				const [top, left, bottom, right] = [r.i32(), r.i32(), r.i32(), r.i32()];
				add(readSample(r, top, left, bottom, right));
			}
			r.pos = end;
		}
		return out;
	}
	if (version < 6 || version > 10)
		throw new Error(`Photoshop brush version ${version} isn't supported`);
	const sub = r.u16();
	while (r.left >= 12) {
		const sig = r.ascii(4);
		const key = r.ascii(4);
		if (sig !== '8BIM') break;
		const size = r.u32();
		const end = r.pos + size;
		if (key === 'samp') {
			while (r.pos < end - 4) {
				let len = r.u32();
				while (len % 4) len++;
				const itemEnd = r.pos + len;
				try {
					r.bytes(sub === 1 ? 47 : 301);
					const [top, left, bottom, right] = [r.i32(), r.i32(), r.i32(), r.i32()];
					add(readSample(r, top, left, bottom, right));
				} catch {
					/* a damaged tip: skip it */
				}
				r.pos = itemEnd;
			}
		}
		r.pos = end + (size % 2);
	}
	return out;
}

// ---- zip (Procreate brushes are zip files) ----

interface ZipEntry {
	name: string;
	method: number;
	size: number;
	offset: number;
}

const le16 = (b: Uint8Array, i: number) => b[i] | (b[i + 1] << 8);
const le32 = (b: Uint8Array, i: number) => (le16(b, i) | (le16(b, i + 2) << 16)) >>> 0;

export function zipEntries(b: Uint8Array): ZipEntry[] {
	let e = -1;
	for (let i = b.length - 22; i >= Math.max(0, b.length - 65_557); i--)
		if (le32(b, i) === 0x06054b50) {
			e = i;
			break;
		}
	if (e < 0) throw new Error('not a zip file');
	const count = le16(b, e + 10);
	let p = le32(b, e + 16);
	const out: ZipEntry[] = [];
	for (let k = 0; k < count && le32(b, p) === 0x02014b50; k++) {
		const method = le16(b, p + 10);
		const size = le32(b, p + 20);
		const nameLen = le16(b, p + 28);
		const extra = le16(b, p + 30);
		const comment = le16(b, p + 32);
		const offset = le32(b, p + 42);
		const name = new TextDecoder().decode(b.subarray(p + 46, p + 46 + nameLen));
		out.push({ name, method, size, offset });
		p += 46 + nameLen + extra + comment;
	}
	return out;
}

export async function unzip(b: Uint8Array, e: ZipEntry): Promise<Uint8Array> {
	if (le32(b, e.offset) !== 0x04034b50) throw new Error('damaged zip');
	const start = e.offset + 30 + le16(b, e.offset + 26) + le16(b, e.offset + 28);
	const data = b.subarray(start, start + e.size);
	if (e.method === 0) return data;
	if (e.method !== 8) throw new Error('unsupported zip compression');
	const stream = new Blob([data as BlobPart])
		.stream()
		.pipeThrough(new DecompressionStream('deflate-raw'));
	return new Uint8Array(await new Response(stream).arrayBuffer());
}

/** Procreate brushes (.brush, or a .brushset of several): their shape and grain pictures. */
export async function parseProcreate(
	bytes: Uint8Array,
	name: string,
	decode: Decode
): Promise<ImportedBrush[]> {
	const entries = zipEntries(bytes).filter((e) => !e.name.startsWith('__MACOSX'));
	// each brush is a folder (a .brushset) or the zip's root (a .brush)
	const folders = new Map<string, { shape?: ZipEntry; grain?: ZipEntry }>();
	for (const e of entries) {
		const m = /^(.*?)(Shape|Grain)\.(png|jpe?g)$/i.exec(e.name);
		if (!m || m[1].includes('QuickLook')) continue;
		const f = folders.get(m[1]) ?? {};
		if (m[2].toLowerCase() === 'shape') f.shape = e;
		else f.grain = e;
		folders.set(m[1], f);
	}
	const out: ImportedBrush[] = [];
	for (const f of folders.values()) {
		const b: ImportedBrush = { name: folders.size > 1 ? `${name} ${out.length + 1}` : name };
		if (f.shape) b.tip = shrink(grayOf(await decode(await unzip(bytes, f.shape)), true));
		if (f.grain) b.grain = shrink(grayOf(await decode(await unzip(bytes, f.grain)), true), 256);
		if (b.tip || b.grain) out.push(b);
	}
	if (!out.length) throw new Error('no brush pictures inside');
	return out;
}

/** Any supported brush file → the brushes in it. */
export async function importBrushFile(file: File, decode: Decode): Promise<ImportedBrush[]> {
	const base = file.name.replace(/\.[^.]+$/, '').slice(0, 30) || 'Brush';
	const ext = file.name.toLowerCase().split('.').pop();
	const bytes = new Uint8Array(await file.arrayBuffer());
	if (ext === 'myb') return [parseMyb(new TextDecoder().decode(bytes), base)];
	if (ext === 'abr') {
		const list = parseAbr(bytes, base);
		if (!list.length) throw new Error('no picture tips inside');
		return list.slice(0, 40);
	}
	if (ext === 'brush' || ext === 'brushset')
		return (await parseProcreate(bytes, base, decode)).slice(0, 40);
	if (/^(png|jpe?g|webp)$/.test(ext ?? '')) {
		const img = await decode(bytes);
		return [{ name: base, tip: shrink(grayOf(img, false)) }];
	}
	throw new Error('use a .myb, .abr, .brush, .brushset or a picture');
}
