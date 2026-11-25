#!/usr/bin/env node
// Built-in brush textures from pictures: paper grains and brush tips, each checked to be
// free to ship in a closed-source app.
//
// - ambientCG (https://ambientcg.com), CC0: papers (their height maps).
// - "Paper-Textures (seamless)" by Leschge (https://opengameart.org/content/paper-textures-seamless), CC0.
// - Krita's default resources (the Krita 4 bundle's PNG tips and patterns): by David
//   Revoy with brushes derived from Ramón Miranda, Razvanc, Radian, Wolthera, Storm,
//   Scottyp and others; Krita's data README puts its brushes under CC-BY 3.0, attributed
//   to David Revoy and the Blender Foundation. Credited as that in About (Settings).
//
// Each picture is made grey, sized, evened out and turned the right way round for the
// engine (255 = paint), and written with a manifest the app reads.
//
// Usage: node tools/brush-pack/textures.mjs <sources>   (needs ImageMagick 7: `magick`)
//   <sources>/ambientcg/<Asset>/<Asset>_1K-JPG_Displacement.jpg
//   <sources>/ogapaper/out/Textures/Papier<n>.png
//   <sources>/krita/{brushes,patterns}/*.png   (Krita_4_Default_Resources.bundle, unzipped)
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '../..');
const OUT = join(ROOT, 'apps/web/static/brushes/textures');

/**
 * How a picture becomes a texture:
 * - height: a height map, high = paper's tooth = paint
 * - paper: a scan of paper, its light peaks taking the paint (uneven light evened out)
 * - ink: dark marks are the paint (patterns drawn in ink, screentones, brush tips)
 */
const GRAIN = 256;
const TIP = 128;

/** [id, name, kind, how, source file (under <sources>), credit] */
export const TEXTURES = [
	// papers: ambientCG (CC0)
	['acg-paper-1', 'Smooth sketch paper', 'grain', 'height', 'ambientcg/Paper001/Paper001_1K-JPG_Displacement.jpg', 'ambientcg'],
	['acg-paper-2', 'Laid paper', 'grain', 'height', 'ambientcg/Paper002/Paper002_1K-JPG_Displacement.jpg', 'ambientcg'],
	['acg-paper-3', 'Crumpled paper', 'grain', 'height', 'ambientcg/Paper003/Paper003_1K-JPG_Displacement.jpg', 'ambientcg'],
	['acg-paper-4', 'Fibre paper', 'grain', 'height', 'ambientcg/Paper004/Paper004_1K-JPG_Displacement.jpg', 'ambientcg'],
	['acg-paper-5', 'Watercolour paper', 'grain', 'height', 'ambientcg/Paper005/Paper005_1K-JPG_Displacement.jpg', 'ambientcg'],
	['acg-paper-6', 'Pastel paper', 'grain', 'height', 'ambientcg/Paper006/Paper006_1K-JPG_Displacement.jpg', 'ambientcg'],
	// papers: Leschge (CC0)
	['oga-paper-1', 'Rag paper', 'grain', 'paper', 'ogapaper/out/Textures/Papier1.png', 'leschge'],
	['oga-paper-2', 'Toothy paper', 'grain', 'paper', 'ogapaper/out/Textures/Papier4.png', 'leschge'],
	['oga-paper-3', 'Mottled paper', 'grain', 'paper', 'ogapaper/out/Textures/Papier7.png', 'leschge'],
	['oga-paper-4', 'Coarse paper', 'grain', 'paper', 'ogapaper/out/Textures/Papier10.png', 'leschge'],
	// papers and patterns: Krita (CC-BY 3.0)
	['kr-paper-default', 'Drawing paper', 'grain', 'paper', 'krita/patterns/03_default-paper.png', 'krita'],
	['kr-paper-c-grain', 'C-grain paper', 'grain', 'paper', 'krita/patterns/04_paper-C-grain.png', 'krita'],
	['kr-paper-torchon', 'Torchon paper', 'grain', 'paper', 'krita/patterns/05_paper-torchon.png', 'krita'],
	['kr-hard-grain', 'Hard grain', 'grain', 'paper', 'krita/patterns/06_hard-grain.png', 'krita'],
	['kr-rough-paper', 'Rough paper', 'grain', 'paper', 'krita/patterns/Rough_Paper.png', 'krita'],
	['kr-canvas', 'Fine canvas', 'grain', 'paper', 'krita/patterns/01_canvas.png', 'krita'],
	['kr-rough-canvas', 'Rough canvas', 'grain', 'paper', 'krita/patterns/02_rough-canvas.png', 'krita'],
	['kr-sand', 'Fine sand', 'grain', 'paper', 'krita/patterns/Sand_fine.png', 'krita'],
	['kr-crosshatch', 'Drawn crosshatch', 'grain', 'ink', 'krita/patterns/09_drawed_crosshatched.png', 'krita'],
	// (Krita's "dynamic screentones": a soft dot a tile, dots when repeated)
	['kr-tone-a', 'Halftone dots', 'grain', 'ink', 'krita/patterns/23-dynamic-screentone-A.png', 'krita'],
	['kr-tone-b', 'Halftone grid', 'grain', 'ink', 'krita/patterns/24-dynamic-screentone-B.png', 'krita'],
	['kr-tone-c', 'Halftone diamonds', 'grain', 'ink', 'krita/patterns/25-dynamic-screentone-C.png', 'krita'],
	// brush tips: Krita (CC-BY 3.0)
	['kr-tip-chalk', 'Chalk lump', 'tip', 'ink', 'krita/brushes/chalk.png', 'krita'],
	['kr-tip-chalk-hard', 'Hard chalk', 'tip', 'ink', 'krita/brushes/chalk_round_hard.png', 'krita'],
	['kr-tip-chalk-sparse', 'Sparse chalk', 'tip', 'ink', 'krita/brushes/chalk_sparse.png', 'krita'],
	['kr-tip-bristles', 'Round bristles', 'tip', 'ink', 'krita/brushes/bristles_circle_dense.png', 'krita'],
	['kr-tip-chisel', 'Chisel bristles', 'tip', 'ink', 'krita/brushes/bristles_chisel_dense.png', 'krita'],
	['kr-tip-oil', 'Oil bristle', 'tip', 'ink', 'krita/brushes/oil_bristle.png', 'krita'],
	['kr-tip-rake', 'Rake', 'tip', 'ink', 'krita/brushes/plain_rake.png', 'krita'],
	['kr-tip-rake-dense', 'Dense rake', 'tip', 'ink', 'krita/brushes/rake_dense.png', 'krita'],
	['kr-tip-square', 'Rough square', 'tip', 'ink', 'krita/brushes/square_rough.png', 'krita'],
	['kr-tip-spike', 'Eroded spike', 'tip', 'ink', 'krita/brushes/spike_eroded.png', 'krita'],
	['kr-tip-splat', 'Splat dots', 'tip', 'ink', 'krita/brushes/splat_dots.png', 'krita'],
	['kr-tip-smoke', 'Smoke puff', 'tip', 'ink', 'krita/brushes/smoke.png', 'krita'],
	['kr-tip-freckles', 'Freckles', 'tip', 'ink', 'krita/brushes/freckles.png', 'krita'],
	['kr-tip-scribbles', 'Scribbles', 'tip', 'ink', 'krita/brushes/scribbles.png', 'krita']
];

/** ImageMagick steps for each way a picture becomes a texture */
function steps(how, kind) {
	const flat = ['-background', 'white', '-alpha', 'remove', '-colorspace', 'Gray'];
	const size =
		kind === 'grain'
			? // (stretched, not cropped: a seamless picture stays seamless)
				['-resize', `${GRAIN}x${GRAIN}!`]
			: ['-resize', `${TIP}x${TIP}>`];
	if (how === 'height') return [...flat, ...size, '-normalize'];
	if (how === 'paper')
		return [
			...flat,
			...size,
			// minus a blur of itself: only the tooth is left, not the light across the scan
			'(', '+clone', '-blur', '0x12', ')',
			'+swap', '-compose', 'Mathematics', '-define', 'compose:args=0,1,-1,0.5', '-composite',
			'-normalize'
		];
	return [...flat, ...size, '-negate', '-normalize'];
}

/** an 8-bit grey PNG's size, from its header */
function pngSize(buf) {
	return [buf.readUInt32BE(16), buf.readUInt32BE(20)];
}

function main() {
	const src = resolve(process.argv[2] ?? '');
	rmSync(OUT, { recursive: true, force: true });
	mkdirSync(OUT, { recursive: true });
	const list = [];
	for (const [id, name, kind, how, file, credit] of TEXTURES) {
		const out = join(OUT, `${id}.png`);
		execFileSync('magick', [join(src, file), ...steps(how, kind), '-depth', '8', '-define', 'png:color-type=0', out]);
		const [w, h] = pngSize(readFileSync(out));
		list.push({ id: `pic-${id}`, name, kind, file: `${id}.png`, w, h, credit });
	}
	writeFileSync(join(OUT, 'manifest.json'), JSON.stringify(list, null, '\t') + '\n');
	console.log(`${list.length} textures`);
}

if (import.meta.url === `file://${process.argv[1]}`) main();
