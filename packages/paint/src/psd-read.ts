/**
 * Reading Photoshop files (.psd, and large documents, .psb) into layers the canvas
 * can take: 8- or 16-bit RGB or greyscale, raw, PackBits or ZIP channels; layer
 * names, opacity, blend modes, visibility, clipping, layer masks and groups. A file
 * without layers comes in as its flattened picture. What the canvas can't keep
 * (adjustment and fill layers, layer effects, groups inside groups…) is listed in
 * `notes`, so the person knows. (psd.ts writes these files.)
 */

export interface ReadLayer {
	name: string;
	/** straight-alpha RGBA, canvas size */
	rgba: Uint8Array;
	/** 0–1 */
	opacity: number;
	/** the canvas's blend names (normal, multiply…) */
	blend: string;
	visible: boolean;
	/** clipped to the layer below */
	clip: boolean;
	/** greyscale, one byte per canvas pixel (255 = shown) */
	mask?: Uint8Array;
	/** the group it's in: 1-based index into `groups` (0: none) */
	group: number;
}

export interface ReadGroup {
	name: string;
	opacity: number;
	visible: boolean;
}

export interface ReadPsd {
	width: number;
	height: number;
	/** bottom → top */
	layers: ReadLayer[];
	groups: ReadGroup[];
	/** what didn't come in as it was, in plain words */
	notes: string[];
}

/** A file that can't be read, with why in plain words. */
export class PsdError extends Error {}

const BLENDS: Record<string, string> = {
	norm: 'normal',
	pass: 'normal',
	'mul ': 'multiply',
	scrn: 'screen',
	over: 'overlay',
	dark: 'darken',
	lite: 'lighten',
	lddg: 'add',
	fsub: 'subtract',
	'hue ': 'hue',
	'sat ': 'saturation',
	colr: 'color'
};

/** layers that are settings, not pixels: adjustment and fill layers */
const ADJUSTMENTS = new Set([
	'levl',
	'curv',
	'brit',
	'blnc',
	'hue ',
	'hue2',
	'selc',
	'mixr',
	'grdm',
	'phfl',
	'expA',
	'vibA',
	'thrs',
	'post',
	'nvrt',
	'blwh',
	'clrL',
	'SoCo',
	'GdFl',
	'PtFl'
]);

/** in a .psb, these blocks' lengths are 8 bytes */
const LONG_KEYS = new Set([
	'LMsk',
	'Lr16',
	'Lr32',
	'Layr',
	'Mt16',
	'Mt32',
	'Mtrn',
	'Alph',
	'FMsk',
	'lnk2',
	'FEid',
	'FXid',
	'PxSD'
]);

const MODES: Record<number, string> = {
	0: 'black-and-white (bitmap)',
	2: 'indexed colour',
	4: 'CMYK',
	7: 'multichannel',
	8: 'duotone',
	9: 'Lab colour'
};

class In {
	p = 0;
	readonly v: DataView;
	constructor(readonly b: Uint8Array) {
		this.v = new DataView(b.buffer, b.byteOffset, b.byteLength);
	}
	need(n: number) {
		if (n < 0 || this.p + n > this.b.length)
			throw new PsdError('the file ends too soon (is it whole?)');
	}
	u8() {
		this.need(1);
		return this.v.getUint8(this.p++);
	}
	u16() {
		this.need(2);
		return ((this.p += 2), this.v.getUint16(this.p - 2));
	}
	i16() {
		this.need(2);
		return ((this.p += 2), this.v.getInt16(this.p - 2));
	}
	u32() {
		this.need(4);
		return ((this.p += 4), this.v.getUint32(this.p - 4));
	}
	i32() {
		this.need(4);
		return ((this.p += 4), this.v.getInt32(this.p - 4));
	}
	u64() {
		const hi = this.u32();
		return hi * 2 ** 32 + this.u32();
	}
	str(n: number) {
		this.need(n);
		return String.fromCharCode(...this.b.subarray(this.p, (this.p += n)));
	}
	bytes(n: number) {
		this.need(n);
		return this.b.subarray(this.p, (this.p += n));
	}
	skip(n: number) {
		this.need(n);
		this.p += n;
	}
}

interface Rect {
	top: number;
	left: number;
	bottom: number;
	right: number;
}
const rw = (r: Rect) => Math.max(0, r.right - r.left);
const rh = (r: Rect) => Math.max(0, r.bottom - r.top);

interface Record_ {
	name: string;
	rect: Rect;
	channels: { id: number; len: number; data?: Uint8Array }[];
	blend: string;
	opacity: number;
	clip: boolean;
	visible: boolean;
	mask?: { rect: Rect; fill: number; off: boolean };
	/** lsct / lsdk: 1 or 2 open / closed folder, 3 end of a folder */
	section: number;
	keys: Set<string>;
}

async function inflate(data: Uint8Array): Promise<Uint8Array> {
	try {
		const s = new Blob([data as BlobPart]).stream().pipeThrough(new DecompressionStream('deflate'));
		return new Uint8Array(await new Response(s).arrayBuffer());
	} catch {
		throw new PsdError('a layer’s compressed pixels are damaged');
	}
}

/** PackBits rows (Photoshop's RLE) into `out`, each row `rowBytes` long. */
function unpackRows(src: In, counts: number[], rowBytes: number, out: Uint8Array, at: number) {
	for (const n of counts) {
		const row = src.bytes(n);
		let [p, o] = [0, at];
		const end = at + rowBytes;
		while (p < row.length && o < end) {
			const h = row[p++];
			if (h < 128) {
				const k = Math.min(h + 1, end - o, row.length - p);
				out.set(row.subarray(p, p + k), o);
				o += k;
				p += h + 1;
			} else if (h > 128) {
				const k = Math.min(257 - h, end - o);
				out.fill(row[p++] ?? 0, o, o + k);
				o += k;
			}
		}
		at += rowBytes;
	}
}

/**
 * Some channels' pixels: `planes` channels of cols × rows samples, compressed as
 * `comp`, from `src` (its next `len` bytes, or to the end). 8-bit, back.
 */
async function decode(
	src: In,
	comp: number,
	cols: number,
	rows: number,
	planes: number,
	depth: number,
	big: boolean,
	len: number
): Promise<Uint8Array> {
	const bps = depth / 8;
	const rowBytes = cols * bps;
	let raw = new Uint8Array(rowBytes * rows * planes);
	if (comp === 0) raw.set(src.bytes(Math.min(raw.length, len)));
	else if (comp === 1) {
		const counts: number[] = [];
		for (let i = 0; i < rows * planes; i++) counts.push(big ? src.u32() : src.u16());
		unpackRows(src, counts, rowBytes, raw, 0);
	} else if (comp === 2 || comp === 3) {
		const z = await inflate(src.bytes(len));
		raw.set(z.subarray(0, raw.length));
		if (comp === 3) {
			// "with prediction": each sample is the difference from the one before it on its row
			for (let r = 0; r < rows * planes; r++) {
				const o = r * rowBytes;
				if (bps === 1)
					for (let x = 1; x < cols; x++) raw[o + x] = (raw[o + x] + raw[o + x - 1]) & 255;
				else
					for (let x = 1; x < cols; x++) {
						const i = o + x * 2;
						const v = (((raw[i] << 8) | raw[i + 1]) + ((raw[i - 2] << 8) | raw[i - 1])) & 0xffff;
						raw[i] = v >> 8;
						raw[i + 1] = v & 255;
					}
			}
		}
	} else throw new PsdError(`a layer uses a compression the app doesn’t know (${comp})`);
	if (bps === 2) {
		const out = new Uint8Array(cols * rows * planes);
		for (let i = 0; i < out.length; i++) out[i] = raw[i * 2];
		raw = out;
	}
	return raw;
}

function readRect(s: In): Rect {
	const [top, left, bottom, right] = [s.i32(), s.i32(), s.i32(), s.i32()];
	return { top, left, bottom, right };
}

function readRecord(s: In, big: boolean): Record_ {
	const rect = readRect(s);
	const n = s.u16();
	if (n > 56) throw new PsdError('a layer has too many channels to be real');
	const channels = Array.from({ length: n }, () => ({ id: s.i16(), len: big ? s.u64() : s.u32() }));
	const sig = s.str(4);
	if (sig !== '8BIM' && sig !== '8B64') throw new PsdError('a layer’s description is damaged');
	const blendKey = s.str(4);
	const opacity = s.u8() / 255;
	const clip = s.u8() === 1;
	const flags = s.u8();
	s.u8();
	const extra = s.u32();
	const end = s.p + extra;
	let mask: Record_['mask'];
	const maskLen = s.u32();
	if (maskLen >= 20) {
		const at = s.p;
		const r = readRect(s);
		const fill = s.u8();
		const mflags = s.u8();
		mask = { rect: r, fill, off: (mflags & 2) !== 0 };
		s.p = at + maskLen;
	} else s.skip(maskLen);
	s.skip(s.u32()); // blending ranges
	const nameLen = s.u8();
	let name = s.str(nameLen);
	s.skip((4 - ((nameLen + 1) % 4)) % 4);
	let section = 0;
	const keys = new Set<string>();
	while (s.p + 12 <= end) {
		const bsig = s.str(4);
		if (bsig !== '8BIM' && bsig !== '8B64') break;
		const key = s.str(4);
		const len = big && LONG_KEYS.has(key) ? s.u64() : s.u32();
		const at = s.p;
		keys.add(key);
		if (key === 'luni' && len >= 4) {
			const k = Math.min(s.u32(), (len - 4) >> 1);
			let u = '';
			for (let i = 0; i < k; i++) u += String.fromCharCode(s.u16());
			name = u.replace(/\0+$/, '') || name;
		} else if ((key === 'lsct' || key === 'lsdk') && len >= 4) section = s.u32();
		s.p = Math.min(end, at + len + (len % 2));
	}
	s.p = end;
	return {
		name,
		rect,
		channels,
		blend: blendKey,
		opacity,
		clip,
		visible: (flags & 2) === 0,
		mask,
		section,
		keys
	};
}

/** The layer info block: its records, then every layer's channels. */
async function readLayers(s: In, end: number, big: boolean, depth: number): Promise<Record_[]> {
	const count = Math.abs(s.i16());
	const records = Array.from({ length: count }, () => readRecord(s, big));
	for (const r of records)
		for (const c of r.channels) {
			const at = s.p;
			if (c.len < 2) {
				s.p = at + c.len;
				continue;
			}
			const comp = s.u16();
			const box = c.id === -2 && r.mask ? r.mask.rect : r.rect;
			if (c.id >= -2 && rw(box) && rh(box))
				c.data = await decode(s, comp, rw(box), rh(box), 1, depth, big, c.len - 2);
			s.p = at + c.len;
			if (s.p > end) throw new PsdError('the layers run past their section (is the file whole?)');
		}
	return records;
}

/** A record's pixels on a canvas-size RGBA (straight alpha). */
function paint(r: Record_, w: number, h: number, grey: boolean): Uint8Array {
	const out = new Uint8Array(w * h * 4);
	const ch = (id: number) => r.channels.find((c) => c.id === id)?.data;
	const [R, G, B] = grey ? [ch(0), ch(0), ch(0)] : [ch(0), ch(1), ch(2)];
	const A = ch(-1);
	const cw = rw(r.rect);
	for (let y = Math.max(0, r.rect.top); y < Math.min(h, r.rect.bottom); y++)
		for (let x = Math.max(0, r.rect.left); x < Math.min(w, r.rect.right); x++) {
			const i = (y - r.rect.top) * cw + (x - r.rect.left);
			const o = (y * w + x) * 4;
			out[o] = R?.[i] ?? 0;
			out[o + 1] = G?.[i] ?? 0;
			out[o + 2] = B?.[i] ?? 0;
			out[o + 3] = A ? A[i] : 255;
		}
	return out;
}

function maskOf(r: Record_, w: number, h: number): Uint8Array | undefined {
	const data = r.channels.find((c) => c.id === -2)?.data;
	if (!r.mask || r.mask.off || !data) return undefined;
	const out = new Uint8Array(w * h).fill(r.mask.fill);
	const m = r.mask.rect;
	const mw = rw(m);
	for (let y = Math.max(0, m.top); y < Math.min(h, m.bottom); y++)
		for (let x = Math.max(0, m.left); x < Math.min(w, m.right); x++)
			out[y * w + x] = data[(y - m.top) * mw + (x - m.left)];
	return out;
}

/** Read a .psd or .psb. */
export async function readPsd(bytes: Uint8Array): Promise<ReadPsd> {
	const s = new In(bytes);
	if (bytes.length < 26 || s.str(4) !== '8BPS') throw new PsdError('it isn’t a Photoshop file');
	const version = s.u16();
	if (version !== 1 && version !== 2)
		throw new PsdError(`it’s a Photoshop file version the app doesn’t know (${version})`);
	const big = version === 2;
	s.skip(6);
	const nChannels = s.u16();
	const height = s.u32();
	const width = s.u32();
	const depth = s.u16();
	const mode = s.u16();
	if (mode !== 1 && mode !== 3)
		throw new PsdError(
			`it’s in ${MODES[mode] ?? 'a colour mode the app doesn’t know'}: save it as RGB first`
		);
	if (depth !== 8 && depth !== 16)
		throw new PsdError(
			depth === 32
				? 'it’s 32 bits per channel: save it as 8 or 16 bits first'
				: `it’s ${depth} bit: save it as 8-bit RGB first`
		);
	if (!width || !height) throw new PsdError('it has no pixels');
	const grey = mode === 1;
	s.skip(s.u32()); // colour mode data
	s.skip(s.u32()); // image resources
	const lmLen = big ? s.u64() : s.u32();
	const lmEnd = s.p + lmLen;
	let records: Record_[] = [];
	if (lmLen) {
		const liLen = big ? s.u64() : s.u32();
		const liEnd = s.p + liLen;
		if (liLen) records = await readLayers(s, liEnd, big, depth);
		s.p = liEnd;
		// 16-bit files keep their layers further on, in their own block
		if (s.p + 4 <= lmEnd) s.skip(s.u32()); // global layer mask
		while (!records.length && s.p + 12 <= lmEnd) {
			const sig = s.str(4);
			if (sig !== '8BIM' && sig !== '8B64') break;
			const key = s.str(4);
			const len = big && LONG_KEYS.has(key) ? s.u64() : s.u32();
			const at = s.p;
			if ((key === 'Lr16' || key === 'Layr') && len)
				records = await readLayers(s, at + len, big, depth);
			else if (key === 'Lr32')
				throw new PsdError('it’s 32 bits per channel: save it as 8 or 16 bits first');
			s.p = at + len;
			while (s.p % 4 && s.p < lmEnd && s.b[s.p] === 0) s.p++;
		}
	}
	s.p = lmEnd;

	const notes = new Set<string>();
	const layers: ReadLayer[] = [];
	const groups: ReadGroup[] = [];
	/** the open folders, outermost first (only the outermost is a group in the app: index > 0) */
	interface Folder {
		index: number;
		opacity: number;
		visible: boolean;
	}
	const open: Folder[] = [];
	/** layers inside folders within a group: those folders' settings go onto them */
	const inner = new Map<ReadLayer, Folder[]>();
	for (const r of records) {
		if (r.section === 3) {
			// the end of a folder (layers are listed bottom-up, so this opens it as we read)
			if (!open.length) {
				groups.push({ name: 'Group', opacity: 1, visible: true });
				open.push({ index: groups.length, opacity: 1, visible: true });
			} else {
				open.push({ index: -1, opacity: 1, visible: true });
				notes.add('Groups inside groups became part of the group around them.');
			}
			continue;
		}
		if (r.section === 1 || r.section === 2) {
			const f = open.pop();
			if (!f) continue;
			f.opacity = r.opacity;
			f.visible = r.visible;
			if (f.index > 0)
				groups[f.index - 1] = { name: r.name || 'Group', opacity: r.opacity, visible: r.visible };
			continue;
		}
		if ([...r.keys].some((k) => ADJUSTMENTS.has(k))) {
			notes.add('Adjustment and fill layers were left out (their effect isn’t in the pixels).');
			continue;
		}
		if (r.keys.has('lfx2') || r.keys.has('lrFX') || r.keys.has('lmfx'))
			notes.add('Layer effects (shadows, strokes, glows…) were left out.');
		if (r.keys.has('TySh') || r.keys.has('SoLd') || r.keys.has('PlLd'))
			notes.add('Text and smart objects came in as pictures.');
		if (r.keys.has('vmsk') || r.keys.has('vsms')) notes.add('Vector masks were left out.');
		// (some writers put the key backwards: "mron")
		const blend = BLENDS[r.blend] ?? BLENDS[[...r.blend].reverse().join('')];
		if (!blend) notes.add('Blend modes the app doesn’t have became Normal.');
		const layer: ReadLayer = {
			name: r.name || 'Layer',
			rgba: paint(r, width, height, grey),
			opacity: r.opacity,
			blend: blend ?? 'normal',
			visible: r.visible,
			clip: r.clip,
			group: open[0]?.index ?? 0
		};
		const m = maskOf(r, width, height);
		if (m) layer.mask = m;
		if (open.length > 1) inner.set(layer, open.slice(1));
		layers.push(layer);
	}
	for (const [layer, folders] of inner)
		for (const f of folders) {
			layer.opacity *= f.opacity;
			layer.visible &&= f.visible;
		}

	if (!records.length) {
		// no layers: the flattened picture
		const comp = s.u16();
		const planes = Math.max(1, Math.min(nChannels, grey ? 1 : 3));
		const all = await decode(s, comp, width, height, nChannels, depth, big, bytes.length - s.p);
		const px = new Uint8Array(width * height * 4);
		const n = width * height;
		for (let i = 0; i < n; i++) {
			const [r, g, b] =
				planes === 1 ? [all[i], all[i], all[i]] : [all[i], all[n + i], all[2 * n + i]];
			px.set([r, g, b, 255], i * 4);
		}
		layers.push({
			name: 'Background',
			rgba: px,
			opacity: 1,
			blend: 'normal',
			visible: true,
			clip: false,
			group: 0
		});
	}
	return { width, height, layers, groups, notes: [...notes] };
}
