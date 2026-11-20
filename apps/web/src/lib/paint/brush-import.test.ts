import { describe, expect, it } from 'vitest';
import {
	grayOf,
	parseAbr,
	parseMyb,
	parseProcreate,
	shrink,
	unzip,
	zipEntries
} from './brush-import';

/** a zip of stored (uncompressed) files */
function zip(files: Record<string, Uint8Array>): Uint8Array {
	const parts: number[] = [];
	const central: number[] = [];
	const u16 = (a: number[], v: number) => a.push(v & 255, (v >> 8) & 255);
	const u32 = (a: number[], v: number) => (u16(a, v & 0xffff), u16(a, v >>> 16));
	let count = 0;
	for (const [name, data] of Object.entries(files)) {
		const nb = new TextEncoder().encode(name);
		const offset = parts.length;
		u32(parts, 0x04034b50);
		u16(parts, 20);
		u16(parts, 0);
		u16(parts, 0); // stored
		u32(parts, 0);
		u32(parts, 0); // crc (not checked)
		u32(parts, data.length);
		u32(parts, data.length);
		u16(parts, nb.length);
		u16(parts, 0);
		parts.push(...nb, ...data);
		u32(central, 0x02014b50);
		u16(central, 20);
		u16(central, 20);
		u16(central, 0);
		u16(central, 0);
		u32(central, 0);
		u32(central, 0);
		u32(central, data.length);
		u32(central, data.length);
		u16(central, nb.length);
		u16(central, 0);
		u16(central, 0);
		u16(central, 0);
		u16(central, 0);
		u32(central, 0);
		u32(central, offset);
		central.push(...nb);
		count++;
	}
	const cdOffset = parts.length;
	const end: number[] = [];
	u32(end, 0x06054b50);
	u16(end, 0);
	u16(end, 0);
	u16(end, count);
	u16(end, count);
	u32(end, central.length);
	u32(end, cdOffset);
	u16(end, 0);
	return new Uint8Array([...parts, ...central, ...end]);
}

/** a v6 .abr with one raw 3×2 tip */
function abr(): Uint8Array {
	const b: number[] = [];
	const u16 = (v: number) => b.push((v >> 8) & 255, v & 255);
	const u32 = (v: number) => (u16(v >>> 16), u16(v & 0xffff));
	u16(6);
	u16(2);
	b.push(...new TextEncoder().encode('8BIMsamp'));
	const item: number[] = [];
	const iu16 = (v: number) => item.push((v >> 8) & 255, v & 255);
	const iu32 = (v: number) => (iu16(v >>> 16), iu16(v & 0xffff));
	item.push(...new Array(301).fill(0));
	iu32(0);
	iu32(0);
	iu32(2); // bottom
	iu32(3); // right
	iu16(8); // depth
	item.push(0); // raw
	item.push(0, 128, 255, 255, 128, 0);
	while (item.length % 4) item.push(0);
	u32(item.length + 4);
	u32(item.length);
	b.push(...item);
	return new Uint8Array(b);
}

const img = (
	w: number,
	h: number,
	f: (x: number, y: number) => [number, number, number, number]
) => {
	const data = new Uint8ClampedArray(w * h * 4);
	for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) data.set(f(x, y), (y * w + x) * 4);
	return { width: w, height: h, data, colorSpace: 'srgb' } as ImageData;
};

describe('brush import', () => {
	it('reads a MyPaint brush and refuses other JSON', () => {
		expect(
			parseMyb('{"settings":{"radius_logarithmic":{"base_value":2,"inputs":{}}}}', 'X').myb
		).toContain('radius_logarithmic');
		expect(() => parseMyb('{"a":1}', 'X')).toThrow(/MyPaint/);
	});

	it('reads the sampled tips of a Photoshop brush file', () => {
		const list = parseAbr(abr(), 'Set');
		expect(list).toHaveLength(1);
		expect(list[0].tip).toEqual({ w: 3, h: 2, px: new Uint8Array([0, 128, 255, 255, 128, 0]) });
	});

	it('refuses Photoshop versions it does not know', () => {
		expect(() => parseAbr(new Uint8Array([0, 5, 0, 1]), 'x')).toThrow(/version 5/);
	});

	it('finds the shape and grain of each Procreate brush in a zip', async () => {
		const z = zip({
			'A1/Shape.png': new Uint8Array([1]),
			'A1/Grain.png': new Uint8Array([2]),
			'B2/Shape.png': new Uint8Array([1]),
			'QuickLook/Thumbnail.png': new Uint8Array([3])
		});
		expect(zipEntries(z).map((e) => e.name)).toContain('A1/Shape.png');
		expect(await unzip(z, zipEntries(z)[1])).toEqual(new Uint8Array([2]));
		const decode = async (b: Uint8Array) =>
			img(4, 4, () => (b[0] === 1 ? [255, 255, 255, 255] : [0, 0, 0, 255]));
		const list = await parseProcreate(z, 'Pack', decode);
		expect(list).toHaveLength(2);
		expect(list[0].tip?.px[0]).toBe(255); // white = paint
		expect(list[0].grain?.px[0]).toBe(0);
		expect(list[1].grain).toBeUndefined();
	});

	it('turns pictures into grey shapes and shrinks big ones', () => {
		const dark = grayOf(
			img(2, 1, (x) => (x ? [255, 255, 255, 255] : [0, 0, 0, 255])),
			false
		);
		expect([...dark.px]).toEqual([255, 0]);
		const big = shrink({ w: 1000, h: 500, px: new Uint8Array(500_000).fill(200) }, 100);
		expect([big.w, big.h]).toEqual([100, 50]);
		expect(big.px[0]).toBe(200);
	});
});
