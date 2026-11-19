import { describe, expect, it } from 'vitest';
import { writePsd, type PsdLayer } from './psd';

/** undo PackBits for `rows` rows given their byte counts */
function unpack(src: Uint8Array, counts: number[], cols: number): Uint8Array {
	const out = new Uint8Array(counts.length * cols);
	let [p, o] = [0, 0];
	for (const n of counts) {
		const end = p + n;
		while (p < end) {
			const h = src[p++];
			if (h < 128) {
				out.set(src.subarray(p, p + h + 1), o);
				o += h + 1;
				p += h + 1;
			} else if (h > 128) {
				out.fill(src[p++], o, o + 257 - h);
				o += 257 - h;
			}
		}
	}
	return out;
}

/** a channel block (compression mark + data) back to its pixels */
function decodeBlock(block: Uint8Array, cols: number, rows: number): Uint8Array {
	const comp = (block[0] << 8) | block[1];
	if (comp === 0) return block.subarray(2);
	expect(comp).toBe(1);
	const counts = Array.from({ length: rows }, (_, y) => (block[2 + y * 2] << 8) | block[3 + y * 2]);
	return unpack(block.subarray(2 + rows * 2), counts, cols);
}

/** read back the bits a test needs (header, layer names / flags, flattened picture) */
function read(b: Uint8Array) {
	const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
	let p = 0;
	const u8 = () => v.getUint8(p++);
	const u16 = () => ((p += 2), v.getUint16(p - 2));
	const i16 = () => ((p += 2), v.getInt16(p - 2));
	const u32 = () => ((p += 4), v.getUint32(p - 4));
	const i32 = () => ((p += 4), v.getInt32(p - 4));
	/** advance past n bytes (read the length first: `p += u32()` would use the old p) */
	const skip = (n: number) => {
		p += n;
	};
	const str = (n: number) => String.fromCharCode(...b.subarray(p, (p += n)));
	expect(str(4)).toBe('8BPS');
	expect(u16()).toBe(1);
	p += 6;
	const channels = u16();
	const height = u32();
	const width = u32();
	expect(u16()).toBe(8);
	expect(u16()).toBe(3);
	skip(u32());
	skip(u32());
	const lmLen = u32();
	const lmEnd = p + lmLen;
	const liLen = u32();
	const liEnd = p + liLen;
	const count = i16();
	const layers: {
		name: string;
		rect: number[];
		blend: string;
		opacity: number;
		clip: number;
		flags: number;
		channels: [number, number][];
		section?: number;
		mask?: number[];
	}[] = [];
	for (let i = 0; i < count; i++) {
		const rect = [i32(), i32(), i32(), i32()];
		const nc = u16();
		const chans: [number, number][] = [];
		for (let c = 0; c < nc; c++) chans.push([i16(), u32()]);
		expect(str(4)).toBe('8BIM');
		const blend = str(4);
		const opacity = u8();
		const clip = u8();
		const flags = u8();
		p++;
		const extra = u32();
		const extraEnd = p + extra;
		const maskLen = u32();
		const mask = maskLen ? [i32(), i32(), i32(), i32()] : undefined;
		if (maskLen) p += maskLen - 16;
		skip(u32());
		const nlen = u8();
		p += nlen;
		for (let n = nlen + 1; n % 4; n++) p++;
		let name = '';
		let section: number | undefined;
		while (p < extraEnd) {
			expect(str(4)).toBe('8BIM');
			const key = str(4);
			const len = u32();
			const end = p + len;
			if (key === 'luni') {
				const n = u32();
				for (let k = 0; k < n; k++) name += String.fromCharCode(u16());
			}
			if (key === 'lsct') section = u32();
			p = end;
		}
		layers.push({ name, rect, blend, opacity, clip, flags, channels: chans, section, mask });
	}
	// channel data: decode every block (it must be exactly its declared length)
	const pixels: Uint8Array[][] = [];
	for (const l of layers) {
		const [top, left, bottom, right] = l.rect;
		pixels.push(
			l.channels.map(([id, len]) => {
				const block = b.subarray(p, p + len);
				p += len;
				const [cols, rows] =
					id === -2 && l.mask
						? [l.mask[3] - l.mask[1], l.mask[2] - l.mask[0]]
						: [right - left, bottom - top];
				const px = decodeBlock(block, cols, rows);
				expect(px.length).toBe(cols * rows);
				return px;
			})
		);
	}
	expect(p).toBeLessThanOrEqual(liEnd);
	expect(liLen % 2).toBe(0);
	p = lmEnd;
	const comp = u16();
	let flat: Uint8Array;
	if (comp === 0) flat = b.subarray(p);
	else {
		expect(comp).toBe(1);
		const counts = Array.from({ length: height * channels }, () => u16());
		flat = unpack(b.subarray(p), counts, width);
	}
	expect(flat.length).toBe(width * height * channels);
	return { width, height, channels, layers, pixels, flat };
}

const W = 8;
const H = 6;
function layer(over: Partial<PsdLayer> = {}): PsdLayer {
	const rgba = new Uint8Array(W * H * 4);
	// one red pixel at (2, 3)
	rgba.set([255, 0, 0, 255], (3 * W + 2) * 4);
	return {
		kind: 'layer',
		name: 'Ink',
		rgba,
		opacity: 1,
		blend: 'normal',
		visible: true,
		clip: false,
		...over
	};
}

describe('writePsd', () => {
	it('writes layers with names, bounds, blend, opacity, flags and the flattened picture', () => {
		const composite = new Uint8Array(W * H * 4).fill(255);
		const b = writePsd({
			width: W,
			height: H,
			composite,
			items: [
				layer(),
				layer({ name: 'Schatten ✏️', blend: 'multiply', opacity: 0.5, visible: false, clip: true })
			]
		});
		const f = read(b);
		expect([f.width, f.height, f.channels]).toEqual([W, H, 3]);
		expect(f.layers.map((l) => l.name)).toEqual(['Ink', 'Schatten ✏️']);
		expect(f.layers[0].rect).toEqual([3, 2, 4, 3]);
		expect(f.layers[0].channels.map(([id]) => id)).toEqual([-1, 0, 1, 2]);
		// the one red pixel: alpha 255, red 255, green 0, blue 0
		expect(f.pixels[0].map((px) => px[0])).toEqual([255, 255, 0, 0]);
		expect(f.layers[1].blend).toBe('mul ');
		expect(f.layers[1].opacity).toBe(128);
		expect(f.layers[1].clip).toBe(1);
		expect(f.layers[1].flags & 2).toBe(2);
		expect(f.flat.every((v) => v === 255)).toBe(true);
	});

	it('writes groups as folders and masks as a mask channel', () => {
		const mask = new Uint8Array(W * H).fill(255);
		const b = writePsd({
			width: W,
			height: H,
			composite: new Uint8Array(W * H * 4),
			items: [
				layer({ name: 'Paper' }),
				{
					kind: 'group',
					name: 'Head',
					opacity: 0.4,
					visible: true,
					children: [layer({ name: 'Eyes', mask })]
				}
			]
		});
		const f = read(b);
		expect(f.layers.map((l) => l.name)).toEqual(['Paper', '</Layer group>', 'Eyes', 'Head']);
		expect(f.layers[1].section).toBe(3);
		expect(f.layers[3].section).toBe(1);
		expect(f.layers[3].blend).toBe('pass');
		expect(f.layers[3].opacity).toBe(102);
		expect(f.layers[2].mask).toEqual([0, 0, H, W]);
		expect(f.layers[2].channels.at(-1)![0]).toBe(-2);
		expect(f.pixels[2].at(-1)!.every((v) => v === 255)).toBe(true);
	});

	it('handles an empty layer', () => {
		const f = read(
			writePsd({
				width: W,
				height: H,
				composite: new Uint8Array(W * H * 4),
				items: [layer({ rgba: new Uint8Array(W * H * 4) })]
			})
		);
		expect(f.layers[0].rect).toEqual([0, 0, 0, 0]);
		expect(f.layers[0].channels.every(([, len]) => len === 2)).toBe(true);
	});
});

describe('writePsd compression', () => {
	it('packs a big, mostly empty layer far below its raw size and reads back exactly', () => {
		const [w, h] = [1024, 768];
		const rgba = new Uint8Array(w * h * 4);
		for (let y = 300; y < 340; y++)
			for (let x = 100; x < 900; x++) rgba.set([20, 20, 26, 255], (y * w + x) * 4);
		const composite = new Uint8Array(w * h * 4).fill(255);
		const b = writePsd({
			width: w,
			height: h,
			composite,
			items: [
				{
					kind: 'layer',
					name: 'Ink',
					rgba,
					opacity: 1,
					blend: 'normal',
					visible: true,
					clip: false
				}
			]
		});
		// raw would be about w × h × 7 bytes (layer RGBA + flattened RGB)
		expect(b.length).toBeLessThan(w * h * 7 * 0.05);
		const f = read(b);
		expect(f.flat.every((v) => v === 255)).toBe(true);
		expect(f.pixels[0][1].every((v) => v === 20)).toBe(true); // red channel inside the bounds
	});
});

describe('PackBits worst cases', () => {
	it('never overflows on alternating pairs or noise, and reads back exactly', () => {
		const [w, h] = [301, 7];
		const rgba = new Uint8Array(w * h * 4);
		for (let i = 0; i < w * h; i++) {
			// a b b c d d … in every channel, plus pseudo-random noise rows
			const v = Math.floor(i / 3) % 2 ? (i * 37) % 251 : ((i * 7919) % 255) + (i % 3 === 0 ? 1 : 0);
			rgba.set([v, v, v, 255], i * 4);
		}
		const b = writePsd({
			width: w,
			height: h,
			composite: rgba,
			items: [
				{
					kind: 'layer',
					name: 'Noise',
					rgba,
					opacity: 1,
					blend: 'normal',
					visible: true,
					clip: false
				}
			]
		});
		const f = read(b);
		for (let i = 0; i < w * h; i++) expect(f.pixels[0][1][i]).toBe(rgba[i * 4]);
		for (let i = 0; i < w * h; i++) expect(f.flat[i]).toBe(rgba[i * 4]);
	});
});
