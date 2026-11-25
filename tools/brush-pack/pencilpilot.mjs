#!/usr/bin/env node
// PencilPilot's own brushes: the Animation set. Each one starts from a MyPaint brush in
// the pack and changes what its job needs. Writes its .myb, a preview drawn with the
// real paint engine, and its place in the manifest (the names in names.mjs too).
//
// Usage: node tools/brush-pack/pencilpilot.mjs   (after `pnpm wasm:brush`)
import { readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { deflateSync } from 'node:zlib';
import { nameOf, OWN_CATEGORIES } from './names.mjs';

const ROOT = resolve(import.meta.dirname, '../..');
const OUT = join(ROOT, 'apps/web/static/brushes');

/** pressure in → value change out, as MyPaint curves ([x, y] points) */
const press = (...pts) => ({ pressure: pts });
/** ink is full as soon as the pen touches (no faint start) */
const FULL_INK = press([0, 0], [0.02, 1], [1, 1]);

export const ANIMATION = [
	{
		id: 'pencilpilot-rough-pencil',
		name: 'Rough pencil',
		base: 'deevad-2b-pencil',
		description:
			'Light, grainy lines that build up as you go over them: for roughs and searching for the pose (draw them in blue or red and clean up on a layer above).',
		set: {
			opaque: [0.11],
			offset_by_random: [0.6, press([0, 0], [1, -0.3])],
			radius_logarithmic: [0.6, press([0, -0.55], [0.75, 0.2], [1, 0.3])]
		}
	},
	{
		id: 'pencilpilot-tie-down-pencil',
		name: 'Tie-down pencil',
		base: 'deevad-2b-pencil',
		description:
			'A firmer, cleaner pencil for tie-downs: the drawing pulled together over the rough, with less grain and a steadier line.',
		set: {
			opaque: [0.32],
			hardness: [0.45, press([0, 0], [1, 0.25])],
			offset_by_random: [0.15, press([0, 0], [1, -0.12])],
			radius_logarithmic: [0.45, press([0, -0.6], [0.75, 0.12], [1, 0.2])]
		}
	},
	{
		id: 'pencilpilot-clean-up-pen',
		name: 'Clean-up pen',
		base: 'deevad-liner',
		description:
			'A G-pen for clean-up and inking: solid black from the first touch, thin at light pressure and swelling as you press, so lines taper at both ends.',
		set: {
			color_h: [0],
			color_s: [0],
			anti_aliasing: [1.2],
			hardness: [0.92],
			opaque: [1],
			opaque_multiply: [0, FULL_INK],
			radius_logarithmic: [0.75, press([0, -1.1], [0.25, -0.5], [0.6, 0], [1, 0.25])],
			dabs_per_actual_radius: [4],
			slow_tracking: [1.6]
		}
	},
	{
		id: 'pencilpilot-mapping-pen',
		name: 'Mapping pen',
		base: 'deevad-liner',
		description:
			'A fine nib for small details (eyes, hair tips, hands) with a gentle swell: thinner and steadier than the Clean-up pen.',
		set: {
			color_h: [0],
			color_s: [0],
			anti_aliasing: [1.1],
			hardness: [0.95],
			opaque: [1],
			opaque_multiply: [0, FULL_INK],
			radius_logarithmic: [0.15, press([0, -0.8], [0.5, -0.15], [1, 0.2])],
			dabs_per_actual_radius: [4],
			slow_tracking: [1.8]
		}
	},
	{
		id: 'pencilpilot-tone-line',
		name: 'Tone line',
		base: 'deevad-liner',
		description:
			'An even, thin line that ignores pressure: for the coloured lines that mark where shadow and highlight shapes go, and closed shapes the fill can find.',
		set: {
			color_h: [0],
			color_s: [0],
			anti_aliasing: [1],
			hardness: [0.9],
			opaque: [1],
			opaque_multiply: [0, FULL_INK],
			radius_logarithmic: [0.35],
			slow_tracking: [1.4]
		}
	},
	{
		id: 'pencilpilot-cel-marker',
		name: 'Cel marker',
		base: 'deevad-liner',
		description:
			'Flat, solid colour with crisp edges and no pressure: for touching up flats and filling small gaps the fill missed, without halos.',
		set: {
			color_h: [0],
			color_s: [0],
			anti_aliasing: [0.5],
			hardness: [1],
			opaque: [1],
			opaque_multiply: [0, FULL_INK],
			radius_logarithmic: [2.3],
			dabs_per_actual_radius: [3],
			stroke_duration_logarithmic: [4],
			stroke_holdtime: [0]
		}
	},
	{
		id: 'pencilpilot-soft-shade',
		name: 'Soft shade',
		base: 'deevad-airbrush',
		description:
			'A big, soft, see-through brush that builds up gently: for soft shading, glows and blush over flats.',
		set: {
			color_h: [0],
			color_s: [0],
			opaque: [0.4],
			hardness: [0.2],
			radius_logarithmic: [3.4, { custom: [[-2, 0.45], [2, -0.45]] }]
		}
	}
];

/** a brush from its base: each setting's base value, and its inputs when given */
function makeMyb(b) {
	const myb = JSON.parse(readFileSync(join(OUT, `${b.base}.myb`), 'utf8'));
	for (const [k, [value, inputs]] of Object.entries(b.set)) {
		if (!myb.settings[k]) throw new Error(`${b.id}: no setting ${k}`);
		myb.settings[k] = { base_value: value, inputs: inputs ?? {} };
	}
	myb.description = `${b.name}: ${b.description}`;
	myb.notes = `PencilPilot Animation set, from ${b.base} (CC0).`;
	myb.parent_brush_name = '';
	return myb;
}

// ---- previews: an S stroke, light → firm → light, as the brush picker draws one ----

export async function engine() {
	const pkg = join(ROOT, 'crates/brush-wasm/pkg');
	const mod = await import(join(pkg, 'brush.js'));
	const out = await mod.default({ module_or_path: readFileSync(join(pkg, 'brush_bg.wasm')) });
	return { ...mod, memory: out.memory };
}

/** paint a stroke with `brush` along `at(t)` (t 0–1), pressure light → firm → light */
function strokeAlong(paint, brush, at, steps = 90) {
	paint.beginStroke();
	for (let i = 0; i <= steps; i++) {
		const t = i / steps;
		const [x, y] = at(t);
		paint.strokeTo(brush, x, y, 0.08 + 0.92 * Math.sin(t * Math.PI), 0, 0, 0.012);
	}
	paint.endStroke(brush);
}

/** three coloured bands, for a brush that only moves or takes away paint to work on */
function underlay(e, paint, W, H) {
	const band = new e.PaintBrush(readFileSync(join(OUT, 'deevad-liner.myb'), 'utf8'));
	try {
		band.setOpacity(1);
		band.setSizeScale(1);
		// (the engine's size scale goes in as log2 onto a natural-log radius: a factor
		// f takes a scale of 2^ln f)
		band.setSizeScale(2 ** Math.log((H * 0.09) / band.radius()));
		for (const [k, c] of [
			[0.25, [216, 96, 74]],
			[0.5, [74, 127, 216]],
			[0.75, [232, 192, 74]]
		]) {
			band.setColor(...c);
			strokeAlong(paint, band, (t) => [-10 + t * (W + 20), H * k], 40);
		}
	} finally {
		band.free();
	}
}

/**
 * A brush's picture: an S stroke, as the brush picker draws one. A brush that leaves
 * nothing on blank paper (a blender, an eraser) is drawn over coloured bands instead.
 */
export function preview(e, src, W = 128, H = 128) {
	const draw = (under) => {
		const brush = new e.PaintBrush(src);
		brush.setColor(22, 22, 26);
		brush.setOpacity(1);
		brush.setSizeScale(1);
		brush.setSizeScale(Math.min(1, (H * 0.1) / brush.radius()));
		const paint = new e.PaintCanvas(W, H);
		try {
			if (under) underlay(e, paint, W, H);
			const r = Math.min(H * 0.25, brush.radius() * 1.5);
			const m = 14 + r;
			// a diagonal S, bottom left to top right
			strokeAlong(paint, brush, (t) => [
				m + t * (W - 2 * m) + Math.sin(t * Math.PI * 2) * W * 0.12,
				H - m - t * (H - 2 * m)
			]);
			paint.flush();
			return new Uint8Array(e.memory.buffer, paint.rgbaPtr(), W * H * 4).slice();
		} finally {
			paint.free();
			brush.free();
		}
	};
	let px = draw(false);
	// (the canvas starts as white paper: ink is what isn't)
	let ink = 0;
	for (let i = 0; i < px.length; i += 4)
		if (px[i + 3] > 24 && Math.min(px[i], px[i + 1], px[i + 2]) < 235) ink++;
	if (ink < 40) px = draw(true);
	return png(px, W, H);
}

/** RGBA (straight alpha) over white paper → an RGB PNG */
export function png(rgba, W, H) {
	const raw = Buffer.alloc((W * 3 + 1) * H);
	for (let y = 0; y < H; y++) {
		raw[y * (W * 3 + 1)] = 0;
		for (let x = 0; x < W; x++) {
			const i = (y * W + x) * 4;
			const a = rgba[i + 3] / 255;
			for (let c = 0; c < 3; c++)
				raw[y * (W * 3 + 1) + 1 + x * 3 + c] = Math.round(rgba[i + c] * a + 255 * (1 - a));
		}
	}
	const table = Array.from({ length: 256 }, (_, n) => {
		let c = n;
		for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
		return c >>> 0;
	});
	const crc = (b) => {
		let c = 0xffffffff;
		for (const x of b) c = table[(c ^ x) & 0xff] ^ (c >>> 8);
		return (c ^ 0xffffffff) >>> 0;
	};
	const chunk = (type, data) => {
		const t = Buffer.concat([Buffer.from(type, 'ascii'), data]);
		const len = Buffer.alloc(4);
		len.writeUInt32BE(data.length);
		const sum = Buffer.alloc(4);
		sum.writeUInt32BE(crc(t));
		return Buffer.concat([len, t, sum]);
	};
	const ihdr = Buffer.alloc(13);
	ihdr.writeUInt32BE(W, 0);
	ihdr.writeUInt32BE(H, 4);
	ihdr.set([8, 2, 0, 0, 0], 8);
	return Buffer.concat([
		Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
		chunk('IHDR', ihdr),
		chunk('IDAT', deflateSync(raw, { level: 9 })),
		chunk('IEND', Buffer.alloc(0))
	]);
}

async function main() {
	const e = await engine();
	const manifest = JSON.parse(readFileSync(join(OUT, 'manifest.json'), 'utf8'));
	for (const b of ANIMATION) {
		const src = JSON.stringify(makeMyb(b), null, 4) + '\n';
		writeFileSync(join(OUT, `${b.id}.myb`), src);
		writeFileSync(join(OUT, `${b.id}.png`), preview(e, src));
	}
	// the Animation set first, then everything else in its order (renamed where clearer)
	const ours = new Set(ANIMATION.map((b) => b.id));
	manifest.brushes = [
		...ANIMATION.map((b) => ({
			id: b.id,
			name: b.name,
			author: 'PencilPilot',
			category: 'animation',
			preview: true
		})),
		...manifest.brushes
			.filter((b) => !ours.has(b.id))
			.map((b) => ({ ...b, name: nameOf(b.id, b.name) }))
	];
	const have = new Set(manifest.categories.map((c) => c.id));
	manifest.categories = [
		...OWN_CATEGORIES.filter(([id]) => !have.has(id)).map(([id, name]) => ({ id, name })),
		...manifest.categories
	];
	writeFileSync(join(OUT, 'manifest.json'), JSON.stringify(manifest, null, '\t') + '\n');
	console.log(`${ANIMATION.length} Animation brushes; ${manifest.brushes.length} in all`);
}

if (import.meta.url === `file://${process.argv[1]}`) await main();
