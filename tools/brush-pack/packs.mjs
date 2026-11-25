#!/usr/bin/env node
// More brush packs, each checked to be free to ship in a closed-source app (CC0):
//
// - Aotz, by Anderson Prado (https://github.com/andeon/aotz): its README and each
//   brush file say "License: CC0 / Public domain".
// - Dirty, by Anna Timm (the MyPaint forum's "Dirty" pack): its Readme.txt waives all
//   rights under CC0. (Her separate "Offset" pack carries no licence: left out.)
// - Concept Design, by Ramón Miranda: the brush settings (.myb) are CC0 by an exception
//   in its readme; the rest of that pack (its previews) is CC-BY 3.0, so previews here
//   are drawn anew.
//
// Each brush gets a PencilPilot name and category, and a preview drawn by the paint
// engine. The old MyPaint text format (Concept Design's "version 2") is converted.
//
// Usage: node tools/brush-pack/packs.mjs <sources>
//   <sources>/aotz/aotz/*.myb       (git clone https://github.com/andeon/aotz)
//   <sources>/dirty/Anti/*.myb      (the Dirty pack's zip, unpacked)
//   <sources>/cd/CD_concept/*.myb   (Concept.Design.zip from mypaint-brushes' releases)
import { readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { engine, preview } from './pencilpilot.mjs';

const ROOT = resolve(import.meta.dirname, '../..');
const OUT = join(ROOT, 'apps/web/static/brushes');

/** [file stem, name, category] for each pack */
export const PACKS = [
	{
		id: 'aotz',
		title: 'Aotz',
		author: 'Anderson Prado',
		dir: 'aotz/aotz',
		brushes: [
			['001_sketch', 'Sketch pencil', 'pencils'],
			['002_sketch_b', 'Sketch pencil B', 'pencils'],
			['003_sketch_6b', 'Soft sketch pencil 6B', 'pencils'],
			['004_sketch_f', 'Fine sketch pencil F', 'pencils'],
			['005_ink', 'Brush pen', 'inks'],
			['006_ink_maru', 'Round nib', 'inks'],
			['007_ink_gpen', 'G nib', 'inks'],
			['008_ink_pen_rough', 'Rough nib', 'inks'],
			['009_fill', 'Flat fill', 'markers'],
			['010_paint', 'Concept brush', 'paint'],
			['011_oilbrush', 'Oil brush', 'paint'],
			['014_impasto', 'Impasto', 'paint'],
			['016_gouache', 'Gouache', 'paint'],
			['018_airbrush', 'Soft airbrush', 'airbrush'],
			['020_water', 'Water diffuser', 'watercolour'],
			['028_splatter', 'Splatter', 'fx'],
			['038_crayon', 'Crayon', 'charcoal'],
			['039_chalk', 'Chalk stick', 'charcoal'],
			['040_marker', 'Quick marker', 'markers'],
			['041_hatch', 'Hatching', 'fx'],
			['042_hatch_rough', 'Rough hatching', 'fx'],
			['043_hatch mesh', 'Cross-hatching mesh', 'fx'],
			['044_hatch_sparse', 'Sparse hatching', 'fx'],
			['045_stipple', 'Stipple', 'fx'],
			['046_rake', 'Rake', 'fx'],
			['050_clouds', 'Cloud puffs', 'fx'],
			['051_grass', 'Grass tufts', 'fx'],
			['052_leaves', 'Foliage', 'fx'],
			['053_meadown', 'Meadow', 'fx'],
			['054_blossom', 'Blossom', 'fx'],
			['058_fur', 'Fur strokes', 'fx'],
			['059_fur2', 'Fur strokes 2', 'fx'],
			['060_texture_ground', 'Ground texture', 'fx'],
			['061_texture_stone', 'Stone texture', 'fx'],
			['063_texture_sand', 'Sand texture', 'fx'],
			['065_texture_water', 'Water texture', 'fx'],
			['070_surface_broad', 'Broad surface', 'paint'],
			['073_surface_sketch', 'Surface sketch', 'paint'],
			['074_surface_sketch2', 'Surface sketch 2', 'paint'],
			['090_smear', 'Smear blender', 'blend'],
			['095_grainy_blend', 'Grainy blender', 'blend'],
			['098_kneaded_eraser', 'Soft kneaded eraser', 'erasers'],
			['099_eraser', 'Clean eraser', 'erasers']
		]
	},
	{
		id: 'dirty',
		title: 'Dirty',
		author: 'Anna Timm',
		dir: 'dirty/Anti',
		brushes: [
			['Bleeding_Ink', 'Bleeding ink', 'inks'],
			['Blocky_Eraser', 'Blocky eraser', 'erasers'],
			['Blotchy_Airbrush_1', 'Blotchy airbrush 1', 'airbrush'],
			['Blotchy_Airbrush_2', 'Blotchy airbrush 2', 'airbrush'],
			['Blotchy_Airbrush_3', 'Blotchy airbrush 3', 'airbrush'],
			['Broken_Line', 'Broken line', 'inks'],
			['Calligraphy_Sketch', 'Calligraphic sketch', 'inks'],
			['Calligraphy_Sketch_2', 'Calligraphic sketch 2', 'inks'],
			['Censored', 'Mosaic', 'fx'],
			['Chalk_Chunks', 'Chalk chunks', 'charcoal'],
			['Chosshatch_Layers', 'Layered crosshatch', 'fx'],
			['Crosshatch_Blender', 'Crosshatch blender', 'blend'],
			['Dirty_Brush', 'Dirty brush', 'paint'],
			['Dirty_Knife', 'Dirty knife', 'paint'],
			['Dirty_Pencil', 'Dirty pencil', 'pencils'],
			['Dry_Bristles', 'Dry bristles', 'paint'],
			['Dry_Brush_Blender', 'Dry brush blender', 'blend'],
			['FineSketch', 'Fine sketch', 'pencils'],
			['FX_4_Liner', 'Four-line liner', 'fx'],
			['FX_Crossline', 'Cross line', 'fx'],
			['FX_Dashes', 'Dashes', 'fx'],
			['FX_Wet_Line', 'Wet line', 'fx'],
			['FX_ZigZag', 'Zigzag', 'fx'],
			['HairLine', 'Hairline', 'inks'],
			['Hairy_Brush', 'Hairy brush', 'paint'],
			['Inker_01', 'Inker 1', 'inks'],
			['Inker_02', 'Inker 2', 'inks'],
			['Nervous_Crosshatch', 'Nervous crosshatch', 'fx'],
			['Noisy_Ballpoint', 'Noisy ballpoint', 'inks'],
			['Noisy_Colorize', 'Noisy tint', 'fx'],
			['Pigment_Pusher', 'Pigment pusher', 'blend'],
			['PixlPointl', 'Pixel pointillism', 'fx'],
			['PointiLine', 'Dotted line', 'fx'],
			['Pointilism', 'Dot pointillism', 'fx'],
			['Rough_Blender', 'Rough blender', 'blend'],
			['Rough_Coal', 'Rough charcoal', 'charcoal'],
			['Rough_Pencil', 'Rough graphite', 'pencils'],
			['Silverpoint', 'Silverpoint', 'pencils'],
			['SlowPointilism', 'Slow pointillism', 'fx'],
			['Smearaser', 'Smear eraser', 'erasers'],
			['Smudgy_Fingers', 'Smudgy fingers', 'blend'],
			['SoftFine', 'Soft fine pencil', 'pencils'],
			['Solid_Eraser', 'Solid eraser', 'erasers'],
			['Spit', 'Spatter', 'fx'],
			['ThinBrush', 'Thin brush', 'paint'],
			['Water_dabs', 'Water dabs', 'watercolour'],
			['Water_Line', 'Water line', 'watercolour'],
			['Wet_Blops', 'Wet blobs', 'watercolour'],
			['Wet_Crosshatch', 'Wet crosshatch', 'watercolour']
		]
	},
	{
		id: 'cd',
		title: 'Concept Design',
		author: 'Ramón Miranda',
		dir: 'cd/CD_concept',
		brushes: [
			['Draw_1', 'Concept pencil 1', 'pencils'],
			['Draw_2', 'Concept pencil 2', 'pencils'],
			['04_pen', 'Fineliner 0.4', 'inks'],
			['08_pen', 'Fineliner 0.8', 'inks'],
			['InkBrush_and_eraser', 'Ink brush', 'inks'],
			['Penbrush', 'Pen brush', 'inks'],
			['Light_Marker', 'Light marker', 'markers'],
			['Medium_Marker', 'Medium marker', 'markers'],
			['Dark_Marker', 'Dark marker', 'markers'],
			['Blender_Marker', 'Marker blender', 'markers'],
			['Permanent_Black', 'Permanent marker', 'markers'],
			['Text_Marker', 'Highlighter', 'markers'],
			['Wide_small', 'Wide marker small', 'markers'],
			['Wide_Big', 'Wide marker large', 'markers'],
			['Big_BG', 'Big background brush', 'paint'],
			['Eraser', 'Concept eraser', 'erasers']
		]
	}
];

const slug = (s) =>
	s
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-|-$/g, '');

/** JSON as MyPaint 2 writes it, forgiving a trailing comma (one Aotz file has one) */
const lenientJson = (t) => JSON.parse(t.replace(/,(\s*[}\]])/g, '$1'));

/**
 * The old MyPaint text format ("version 2"): a setting a line, its base value, then
 * `| input (x y), (x y)…` for each input. As a MyPaint 2 JSON brush.
 */
export function fromV2(text) {
	const settings = {};
	let parent = '';
	for (const raw of text.split(/\r?\n/)) {
		const line = raw.trim();
		if (!line || line.startsWith('#') || line.startsWith('version')) continue;
		const [head, ...ins] = line.split('|');
		const [name, value] = head.trim().split(/\s+/, 2);
		if (name === 'parent_brush_name') {
			parent = (value ?? '').trim();
			continue;
		}
		const inputs = {};
		for (const part of ins) {
			const [input, ...rest] = part.trim().split(/\s+/);
			const pts = [...rest.join(' ').matchAll(/\(\s*(-?[\d.e+-]+)\s+(-?[\d.e+-]+)\s*\)/gi)].map(
				(m) => [Number(m[1]), Number(m[2])]
			);
			if (input && pts.length) inputs[input] = pts;
		}
		const base = Number(value);
		if (Number.isFinite(base)) settings[name] = { base_value: base, inputs };
	}
	return {
		comment: 'MyPaint brush file',
		description: '',
		group: '',
		notes: '',
		parent_brush_name: parent,
		settings,
		version: 3
	};
}

async function main() {
	const src = resolve(process.argv[2] ?? '');
	const e = await engine();
	const manifest = JSON.parse(readFileSync(join(OUT, 'manifest.json'), 'utf8'));
	const names = new Set(manifest.brushes.map((b) => b.name.toLowerCase()));
	const added = [];
	for (const pack of PACKS) {
		for (const [stem, name, category] of pack.brushes) {
			const id = `${pack.id}-${slug(stem)}`;
			if (manifest.brushes.some((b) => b.id === id)) names.delete(name.toLowerCase());
			if (names.has(name.toLowerCase())) throw new Error(`${name}: a brush has that name`);
			names.add(name.toLowerCase());
			const text = readFileSync(join(src, pack.dir, `${stem}.myb`), 'utf8');
			const myb = text.trimStart().startsWith('{') ? lenientJson(text) : fromV2(text);
			myb.description = name;
			myb.notes = `From ${pack.title} by ${pack.author} (CC0), "${stem}" there.`;
			const out = JSON.stringify(myb, null, 4) + '\n';
			writeFileSync(join(OUT, `${id}.myb`), out);
			writeFileSync(join(OUT, `${id}.png`), preview(e, out));
			added.push({ id, name, author: pack.author, category, preview: true });
		}
	}
	const ours = new Set(added.map((b) => b.id));
	manifest.brushes = [...manifest.brushes.filter((b) => !ours.has(b.id)), ...added];
	writeFileSync(join(OUT, 'manifest.json'), JSON.stringify(manifest, null, '\t') + '\n');
	console.log(`${added.length} brushes from ${PACKS.length} packs; ${manifest.brushes.length} in all`);
}

if (import.meta.url === `file://${process.argv[1]}`) await main();
