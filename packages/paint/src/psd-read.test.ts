import { describe, expect, it } from 'vitest';
import { deflateSync } from 'node:zlib';
import { readPsd, PsdError } from './psd-read';
import { writePsd, type PsdLayer } from './psd';

const W = 6;
const H = 4;

/** a canvas-size RGBA with one colour in a box */
function boxed(rgba: number[], x0: number, y0: number, x1: number, y1: number) {
	const px = new Uint8Array(W * H * 4);
	for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) px.set(rgba, (y * W + x) * 4);
	return px;
}
const at = (px: Uint8Array, x: number, y: number) => [
	...px.subarray((y * W + x) * 4, (y * W + x) * 4 + 4)
];

function layer(name: string, px: Uint8Array, more: Partial<PsdLayer> = {}): PsdLayer {
	return {
		kind: 'layer',
		name,
		rgba: px,
		opacity: 1,
		blend: 'normal',
		visible: true,
		clip: false,
		...more
	};
}

// ---- a small writer for files psd.ts doesn't make (16-bit, ZIP, nested folders…) ----
interface Rec {
	name: string;
	rect?: [number, number, number, number];
	/** id → raw samples (big-endian for 16-bit), compression 0, 2 (ZIP) or 3 (ZIP, predicted) */
	channels?: { id: number; comp: 0 | 2 | 3; data: Uint8Array; cols?: number }[];
	blend?: string;
	opacity?: number;
	keys?: [string, Uint8Array][];
	section?: number;
}
function bytesOf(build: (o: number[]) => void) {
	const o: number[] = [];
	build(o);
	return o;
}
const u16 = (o: number[], v: number) => o.push((v >> 8) & 255, v & 255);
const u32 = (o: number[], v: number) =>
	o.push((v >>> 24) & 255, (v >> 16) & 255, (v >> 8) & 255, v & 255);
const ascii = (o: number[], s: string) => o.push(...[...s].map((c) => c.charCodeAt(0)));
function block(comp: 0 | 2 | 3, data: Uint8Array, cols: number, bps: number) {
	let d = data;
	if (comp === 3) {
		d = data.slice();
		const rowBytes = cols * bps;
		for (let r = 0; r < d.length / rowBytes; r++)
			for (let x = cols - 1; x > 0; x--) {
				const i = r * rowBytes + x * bps;
				if (bps === 1) d[i] = (d[i] - d[i - 1]) & 255;
				else {
					const v =
						((((d[i] << 8) | d[i + 1]) - ((data[i - 2] << 8) | data[i - 1])) & 0xffff) >>> 0;
					d[i] = v >> 8;
					d[i + 1] = v & 255;
				}
			}
	}
	const body = comp === 0 ? d : deflateSync(d);
	return [0, comp, ...body];
}
function psdFile(o: { w: number; h: number; depth?: number; mode?: number; records: Rec[] }) {
	const bps = (o.depth ?? 8) / 8;
	const blocks = o.records.map((r) =>
		(r.channels ?? []).map((c) =>
			block(c.comp, c.data, c.cols ?? (r.rect ? r.rect[3] - r.rect[1] : 0), bps)
		)
	);
	const li = bytesOf((x) => {
		u16(x, o.records.length);
		o.records.forEach((r, k) => {
			for (const v of r.rect ?? [0, 0, 0, 0]) u32(x, v);
			u16(x, (r.channels ?? []).length);
			(r.channels ?? []).forEach((c, j) => {
				u16(x, c.id & 0xffff);
				u32(x, blocks[k][j].length);
			});
			ascii(x, '8BIM');
			ascii(x, r.blend ?? 'norm');
			x.push(Math.round((r.opacity ?? 1) * 255), 0, 0, 0);
			const extra = bytesOf((e) => {
				u32(e, 0);
				u32(e, 0);
				e.push(r.name.length);
				ascii(e, r.name);
				for (let n = r.name.length + 1; n % 4; n++) e.push(0);
				for (const [key, data] of [
					...(r.keys ?? []),
					...(r.section
						? ([['lsct', Uint8Array.from([0, 0, 0, r.section])]] as [string, Uint8Array][])
						: [])
				]) {
					ascii(e, '8BIM');
					ascii(e, key);
					u32(e, data.length);
					e.push(...data);
				}
			});
			u32(x, extra.length);
			x.push(...extra);
		});
		for (const b of blocks.flat()) x.push(...b);
	});
	return Uint8Array.from(
		bytesOf((x) => {
			ascii(x, '8BPS');
			u16(x, 1);
			x.push(0, 0, 0, 0, 0, 0);
			u16(x, 3);
			u32(x, o.h);
			u32(x, o.w);
			u16(x, o.depth ?? 8);
			u16(x, o.mode ?? 3);
			u32(x, 0);
			u32(x, 0);
			u32(x, li.length + 8);
			u32(x, li.length);
			x.push(...li);
			u32(x, 0);
			// the flattened picture: raw, black
			u16(x, 0);
			x.push(...new Array(o.w * o.h * 3 * bps).fill(0));
		})
	);
}

describe('reading Photoshop files', () => {
	it('read back what the app writes: layers, opacity, blend, hidden, clipping, masks, groups', async () => {
		const mask = new Uint8Array(W * H).fill(255);
		mask[0] = 0;
		const file = writePsd({
			width: W,
			height: H,
			composite: new Uint8Array(W * H * 4),
			items: [
				layer('Paper', boxed([255, 255, 255, 255], 0, 0, W, H)),
				{
					kind: 'group',
					name: 'Character',
					opacity: 0.5,
					visible: false,
					children: [
						layer('Ébauche ✏', boxed([200, 10, 20, 128], 1, 1, 4, 3), {
							opacity: 0.6,
							blend: 'multiply',
							mask
						}),
						layer('Shade', boxed([0, 0, 255, 255], 2, 2, 3, 3), { clip: true, visible: false })
					]
				}
			]
		});
		const r = await readPsd(file);
		expect([r.width, r.height]).toEqual([W, H]);
		expect(r.groups).toEqual([
			{ name: 'Character', opacity: expect.closeTo(0.5, 2), visible: false }
		]);
		expect(r.layers.map((l) => [l.name, l.group, l.blend, l.visible, l.clip])).toEqual([
			['Paper', 0, 'normal', true, false],
			['Ébauche ✏', 1, 'multiply', true, false],
			['Shade', 1, 'normal', false, true]
		]);
		expect(r.layers[1].opacity).toBeCloseTo(0.6, 2);
		expect(at(r.layers[1].rgba, 1, 1)).toEqual([200, 10, 20, 128]);
		expect(at(r.layers[1].rgba, 0, 0)).toEqual([0, 0, 0, 0]);
		expect([...r.layers[1].mask!.subarray(0, 2)]).toEqual([0, 255]);
		expect(r.layers[0].mask).toBeUndefined();
		expect(at(r.layers[0].rgba, 5, 3)).toEqual([255, 255, 255, 255]);
		expect(r.notes).toEqual([]);
	});

	it('read a file without layers as its flattened picture', async () => {
		const composite = boxed([10, 200, 30, 255], 0, 0, W, H);
		const r = await readPsd(writePsd({ width: W, height: H, composite, items: [] }));
		expect(r.layers).toHaveLength(1);
		expect(r.layers[0].name).toBe('Background');
		expect(at(r.layers[0].rgba, 3, 2)).toEqual([10, 200, 30, 255]);
	});

	it('read ZIP channels, with and without prediction, and 16-bit greyscale', async () => {
		const ramp = Uint8Array.from([10, 20, 30, 40, 50, 60]);
		const zip = await readPsd(
			psdFile({
				w: 3,
				h: 2,
				records: [
					{
						name: 'zip',
						rect: [0, 0, 2, 3],
						channels: [
							{ id: 0, comp: 2, data: ramp },
							{ id: 1, comp: 3, data: ramp },
							{ id: 2, comp: 3, data: ramp },
							{ id: -1, comp: 0, data: new Uint8Array(6).fill(255) }
						]
					}
				]
			})
		);
		const px = zip.layers[0].rgba;
		expect([...px.subarray(0, 8)]).toEqual([10, 10, 10, 255, 20, 20, 20, 255]);
		expect([...px.subarray(20, 24)]).toEqual([60, 60, 60, 255]);

		// 16-bit grey: the high byte of each sample (no alpha channel: opaque)
		const grey16 = Uint8Array.from([0x12, 0x34, 0xab, 0xcd, 0x00, 0xff, 0xff, 0x00]);
		const g = await readPsd(
			psdFile({
				w: 2,
				h: 2,
				depth: 16,
				mode: 1,
				records: [{ name: 'g', rect: [0, 0, 2, 2], channels: [{ id: 0, comp: 3, data: grey16 }] }]
			})
		);
		expect([...g.layers[0].rgba.subarray(0, 16)]).toEqual([
			0x12, 0x12, 0x12, 255, 0xab, 0xab, 0xab, 255, 0, 0, 0, 255, 0xff, 0xff, 0xff, 255
		]);
	});

	it('fold groups inside groups into the outer one, and say what was left out', async () => {
		const one = (name: string, extra: Partial<Rec> = {}): Rec => ({
			name,
			rect: [0, 0, 1, 1],
			channels: [{ id: 0, comp: 0, data: Uint8Array.from([99]) }],
			...extra
		});
		const r = await readPsd(
			psdFile({
				w: 1,
				h: 1,
				records: [
					{ name: '</end>', section: 3 },
					one('outer layer'),
					{ name: '</end>', section: 3 },
					one('inner layer'),
					{ name: 'Inner', section: 1, opacity: 0.5 },
					{ name: 'Outer', section: 1 },
					one('Levels', { keys: [['levl', new Uint8Array(4)]] }),
					one('Glow', { blend: 'vLit', keys: [['lfx2', new Uint8Array(4)]] })
				]
			})
		);
		expect(r.groups.map((g) => g.name)).toEqual(['Outer']);
		expect(r.layers.map((l) => [l.name, l.group, Math.round(l.opacity * 100), l.blend])).toEqual([
			['outer layer', 1, 100, 'normal'],
			['inner layer', 1, 50, 'normal'],
			['Glow', 0, 100, 'normal']
		]);
		expect(r.notes).toEqual([
			'Groups inside groups became part of the group around them.',
			'Adjustment and fill layers were left out (their effect isn’t in the pixels).',
			'Layer effects (shadows, strokes, glows…) were left out.',
			'Blend modes the app doesn’t have became Normal.'
		]);
	});

	it('refuse what it can’t read, saying why', async () => {
		await expect(readPsd(new TextEncoder().encode('not a psd at all, no'))).rejects.toThrow(
			/isn’t a Photoshop file/
		);
		const cmyk = psdFile({ w: 1, h: 1, mode: 4, records: [] });
		await expect(readPsd(cmyk)).rejects.toThrow(/CMYK: save it as RGB first/);
		const whole = writePsd({
			width: W,
			height: H,
			composite: new Uint8Array(W * H * 4),
			items: [layer('a', boxed([1, 2, 3, 255], 0, 0, W, H))]
		});
		await expect(readPsd(whole.subarray(0, 80))).rejects.toBeInstanceOf(PsdError);
	});
});
