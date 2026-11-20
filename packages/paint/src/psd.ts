/**
 * A small Photoshop (.psd) writer: 8-bit RGB, layers with names, opacity,
 * blend modes, visibility, clipping, layer masks and groups, plus the
 * flattened picture. Uncompressed: simple and readable by Photoshop, Krita,
 * GIMP, Photopea and others.
 */

export interface PsdLayer {
	kind: 'layer';
	name: string;
	/** straight-alpha RGBA, canvas size */
	rgba: Uint8Array | Uint8ClampedArray;
	/** 0–1 */
	opacity: number;
	blend: string;
	visible: boolean;
	/** clipped to the layer below */
	clip: boolean;
	/** greyscale mask, one byte per canvas pixel (255 = shown) */
	mask?: Uint8Array;
}

export interface PsdGroup {
	kind: 'group';
	name: string;
	opacity: number;
	visible: boolean;
	/** bottom → top */
	children: PsdLayer[];
}

export interface PsdDoc {
	width: number;
	height: number;
	/** the flattened picture, RGBA (alpha ignored: drawn on white paper) */
	composite: Uint8Array | Uint8ClampedArray;
	/** bottom → top */
	items: (PsdLayer | PsdGroup)[];
}

const BLEND_KEYS: Record<string, string> = {
	normal: 'norm',
	multiply: 'mul ',
	screen: 'scrn',
	overlay: 'over',
	darken: 'dark',
	lighten: 'lite',
	add: 'lddg'
};

class Out {
	#chunks: Uint8Array[] = [];
	#cur = new Uint8Array(1 << 16);
	#n = 0;
	length = 0;

	#room(k: number) {
		if (this.#n + k <= this.#cur.length) return;
		this.#chunks.push(this.#cur.subarray(0, this.#n));
		this.#cur = new Uint8Array(Math.max(1 << 16, k));
		this.#n = 0;
	}
	u8(v: number) {
		this.#room(1);
		this.#cur[this.#n++] = v & 255;
		this.length++;
	}
	u16(v: number) {
		this.u8(v >> 8);
		this.u8(v);
	}
	u32(v: number) {
		this.u16((v >>> 16) & 0xffff);
		this.u16(v & 0xffff);
	}
	i32(v: number) {
		this.u32(v >>> 0);
	}
	ascii(s: string) {
		for (const ch of s) this.u8(ch.charCodeAt(0));
	}
	bytes(b: Uint8Array) {
		if (b.length > 4096) {
			this.#chunks.push(this.#cur.subarray(0, this.#n), b);
			this.#cur = new Uint8Array(1 << 16);
			this.#n = 0;
			this.length += b.length;
			return;
		}
		this.#room(b.length);
		this.#cur.set(b, this.#n);
		this.#n += b.length;
		this.length += b.length;
	}
	/** write a section whose 4-byte length comes first */
	sized(body: (o: Out) => void, pad = 1) {
		const o = new Out();
		body(o);
		while (o.length % pad) o.u8(0);
		this.u32(o.length);
		this.bytes(o.done());
	}
	done(): Uint8Array {
		const all = [...this.#chunks, this.#cur.subarray(0, this.#n)];
		const out = new Uint8Array(this.length);
		let at = 0;
		for (const c of all) {
			out.set(c, at);
			at += c.length;
		}
		return out;
	}
}

/** PackBits one row (Apple RLE, as Photoshop uses): literal runs and repeats of up to 128. */
function packRow(src: Uint8Array, from: number, len: number, out: Uint8Array, at: number): number {
	const start = at;
	let i = 0;
	while (i < len) {
		// a repeat of 3+ identical bytes (pairs stay in literal runs, so the output never
		// grows by more than one byte per 128)
		let run = 1;
		while (i + run < len && run < 128 && src[from + i + run] === src[from + i]) run++;
		if (run >= 3) {
			out[at++] = 257 - run; // -(run - 1) as a byte
			out[at++] = src[from + i];
			i += run;
			continue;
		}
		// a literal run until the next 3-byte repeat (or 128 bytes)
		let lit = 1;
		while (
			i + lit < len &&
			lit < 128 &&
			!(
				i + lit + 2 < len &&
				src[from + i + lit] === src[from + i + lit + 1] &&
				src[from + i + lit] === src[from + i + lit + 2]
			)
		)
			lit++;
		out[at++] = lit - 1;
		out.set(src.subarray(from + i, from + i + lit), at);
		at += lit;
		i += lit;
	}
	return at - start;
}

/**
 * One channel's image data with its compression mark: PackBits (1) with the row
 * byte counts first, or raw (0) when that's smaller or the channel is empty.
 */
function channelBlock(data: Uint8Array, cols: number, rows: number): Uint8Array {
	const raw = () => {
		const b = new Uint8Array(2 + data.length);
		b.set(data, 2);
		return b;
	};
	if (!cols || !rows) return raw();
	const packed = new Uint8Array(cols + Math.ceil(cols / 128) + 2);
	const counts = new Uint16Array(rows);
	const rowsOut: Uint8Array[] = [];
	let total = 0;
	for (let y = 0; y < rows; y++) {
		const n = packRow(data, y * cols, cols, packed, 0);
		counts[y] = n;
		rowsOut.push(packed.slice(0, n));
		total += n;
	}
	if (total + rows * 2 >= data.length) return raw();
	const b = new Uint8Array(2 + rows * 2 + total);
	b[1] = 1;
	let at = 2;
	for (const c of counts) {
		b[at++] = c >> 8;
		b[at++] = c & 255;
	}
	for (const r of rowsOut) {
		b.set(r, at);
		at += r.length;
	}
	return b;
}

interface Rect {
	top: number;
	left: number;
	bottom: number;
	right: number;
}

/** the smallest rectangle around the layer's paint (empty: all zero) */
function bounds(rgba: Uint8Array | Uint8ClampedArray, w: number, h: number): Rect {
	let [top, left, bottom, right] = [h, w, 0, 0];
	for (let y = 0; y < h; y++)
		for (let x = 0; x < w; x++)
			if (rgba[(y * w + x) * 4 + 3]) {
				if (y < top) top = y;
				if (y >= bottom) bottom = y + 1;
				if (x < left) left = x;
				if (x >= right) right = x + 1;
			}
	return bottom ? { top, left, bottom, right } : { top: 0, left: 0, bottom: 0, right: 0 };
}

/** one channel of `rgba` (0–3) cut to `r` */
function plane(rgba: Uint8Array | Uint8ClampedArray, w: number, r: Rect, ch: number): Uint8Array {
	const [rw, rh] = [r.right - r.left, r.bottom - r.top];
	const out = new Uint8Array(rw * rh);
	for (let y = 0; y < rh; y++)
		for (let x = 0; x < rw; x++) out[y * rw + x] = rgba[((y + r.top) * w + x + r.left) * 4 + ch];
	return out;
}

interface Record_ {
	name: string;
	rect: Rect;
	/** [channel id, data] */
	channels: [number, Uint8Array][];
	blend: string;
	opacity: number;
	clip: boolean;
	visible: boolean;
	mask?: Rect;
	/** lsct: 1 open folder, 3 end of a folder */
	section?: 1 | 3;
}

function layerRecord(l: PsdLayer, w: number, h: number): Record_ {
	const rect = bounds(l.rgba, w, h);
	const [cw, ch] = [rect.right - rect.left, rect.bottom - rect.top];
	const channels: [number, Uint8Array][] = [-1, 0, 1, 2].map((id) => [
		id,
		channelBlock(plane(l.rgba, w, rect, id === -1 ? 3 : id), cw, ch)
	]);
	if (l.mask) channels.push([-2, channelBlock(l.mask, w, h)]);
	return {
		name: l.name,
		rect,
		channels,
		blend: BLEND_KEYS[l.blend] ?? 'norm',
		opacity: l.opacity,
		clip: l.clip,
		visible: l.visible,
		mask: l.mask ? { top: 0, left: 0, bottom: h, right: w } : undefined
	};
}

const EMPTY: Rect = { top: 0, left: 0, bottom: 0, right: 0 };
const noPixels = (): [number, Uint8Array][] =>
	[-1, 0, 1, 2].map((id) => [id, new Uint8Array(2)] as [number, Uint8Array]);

function pascal(o: Out, name: string) {
	// legacy name: ASCII only, up to 255 bytes, padded to a multiple of 4 (with the length byte)
	const ascii = name.replace(/[^\x20-\x7e]/g, '?').slice(0, 255);
	o.u8(ascii.length);
	o.ascii(ascii);
	for (let n = ascii.length + 1; n % 4; n++) o.u8(0);
}

function writeRecord(o: Out, r: Record_) {
	o.i32(r.rect.top);
	o.i32(r.rect.left);
	o.i32(r.rect.bottom);
	o.i32(r.rect.right);
	o.u16(r.channels.length);
	for (const [id, data] of r.channels) {
		o.u16(id & 0xffff);
		o.u32(data.length); // the block includes its 2-byte compression mark
	}
	o.ascii('8BIM');
	o.ascii(r.blend);
	o.u8(Math.round(Math.max(0, Math.min(1, r.opacity)) * 255));
	o.u8(r.clip ? 1 : 0);
	// bit 1: hidden; bits 3+4: "pixel data irrelevant" (folder markers)
	o.u8((r.visible ? 0 : 2) | (r.section ? 8 | 16 : 0));
	o.u8(0);
	o.sized((x) => {
		// layer mask
		if (r.mask) {
			x.u32(20);
			x.i32(r.mask.top);
			x.i32(r.mask.left);
			x.i32(r.mask.bottom);
			x.i32(r.mask.right);
			x.u8(255); // outside the mask: shown
			x.u8(0);
			x.u16(0);
		} else x.u32(0);
		x.u32(0); // blending ranges
		pascal(x, r.name);
		// Unicode name
		x.ascii('8BIMluni');
		x.sized((u) => {
			const name = r.name.slice(0, 255);
			u.u32(name.length);
			for (let i = 0; i < name.length; i++) u.u16(name.charCodeAt(i));
		}, 2);
		if (r.section) {
			x.ascii('8BIMlsct');
			x.sized((u) => {
				u.u32(r.section!);
				if (r.section === 1) {
					u.ascii('8BIM');
					u.ascii(r.blend);
				}
			});
		}
	});
}

/** The .psd file for `doc`. */
export function writePsd(doc: PsdDoc): Uint8Array {
	const { width: w, height: h } = doc;
	const records: Record_[] = [];
	for (const item of doc.items) {
		if (item.kind === 'layer') {
			records.push(layerRecord(item, w, h));
			continue;
		}
		// a folder: its end marker, its layers, then the folder itself (bottom → top)
		records.push({
			name: '</Layer group>',
			rect: EMPTY,
			channels: noPixels(),
			blend: 'norm',
			opacity: 1,
			clip: false,
			visible: true,
			section: 3
		});
		for (const c of item.children) records.push(layerRecord(c, w, h));
		records.push({
			name: item.name,
			rect: EMPTY,
			channels: noPixels(),
			blend: 'pass',
			opacity: item.opacity,
			clip: false,
			visible: item.visible,
			section: 1
		});
	}

	const o = new Out();
	// header
	o.ascii('8BPS');
	o.u16(1);
	for (let i = 0; i < 6; i++) o.u8(0);
	o.u16(3); // channels in the flattened picture
	o.u32(h);
	o.u32(w);
	o.u16(8);
	o.u16(3); // RGB
	o.u32(0); // colour mode data
	o.u32(0); // image resources
	// layers
	o.sized((lm) => {
		lm.sized((li) => {
			li.u16(records.length);
			for (const r of records) writeRecord(li, r);
			for (const r of records) for (const [, block] of r.channels) li.bytes(block);
		}, 2);
		lm.u32(0); // global layer mask
	});
	// the flattened picture, PackBits: every row's byte count (R, G then B rows), then the rows
	const full = { top: 0, left: 0, bottom: h, right: w };
	const packed = new Uint8Array(w + Math.ceil(w / 128) + 2);
	const counts: number[] = [];
	const rows: Uint8Array[] = [];
	for (let c = 0; c < 3; c++) {
		const p = plane(doc.composite, w, full, c);
		for (let y = 0; y < h; y++) {
			const n = packRow(p, y * w, w, packed, 0);
			counts.push(n);
			rows.push(packed.slice(0, n));
		}
	}
	o.u16(1);
	for (const n of counts) o.u16(n);
	for (const r of rows) o.bytes(r);
	return o.done();
}
