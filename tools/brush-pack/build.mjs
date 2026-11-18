#!/usr/bin/env node
// Vendors the MyPaint brush pack (mypaint-brushes v2.0.2, brush files CC0-1.0)
// into apps/web/static/brushes and writes manifest.json with categories.
//
// Usage: node tools/brush-pack/build.mjs <path-to-extracted-mypaint-brushes>
//   (get it with: curl -sL https://api.github.com/repos/mypaint/mypaint-brushes/tarball/v2.0.2 | tar xz)
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const src = resolve(process.argv[2] ?? '');
const brushes = join(src, 'brushes');
if (!existsSync(brushes)) {
	console.error('usage: build.mjs <extracted mypaint-brushes dir>');
	process.exit(1);
}
const out = resolve(import.meta.dirname, '../../apps/web/static/brushes');

const SETS = {
	classic: 'MyPaint Classic',
	deevad: 'David Revoy',
	Dieterle: 'Brien Dieterle',
	experimental: 'MyPaint Experimental',
	kaerhon_v1: 'Guillaume Loussarévian',
	ramon: 'Ramón Miranda',
	tanda: 'Marcelo “Tanda” Cerviño'
};

// First matching rule wins, so order matters.
const RULES = [
	['erasers', /eraser/i],
	['blend', /smudge|blend|blur|smear|dissolver|mop|pick_and_drag/i],
	['watercolour', /water|fringe|glazing/i],
	['pencils', /pencil|sketch|chalk|^8b|^2b|^4h|1pixel/i],
	['inks', /ink|pen|liner|calligraph|kabura|fountain|rigger|blot|sting|slowline/i],
	['markers', /marker|fill|opaque|flat_bar|round_bl|roundblock|bulk/i],
	['charcoal', /charcoal|pastel|grain|rough|dirty|shade|coarse|noise/i],
	['airbrush', /air|spray|soft|glow|beamlight|cloud/i],
	['paint', /paint|oil|acrylic|brush|round|flat|fan|knife|impression|modelling|dry_|detail|wet|puantil|^basic$|classic/i]
];
const CATEGORIES = [
	['pencils', 'Pencils'],
	['inks', 'Inks & pens'],
	['markers', 'Markers'],
	['charcoal', 'Charcoal & pastel'],
	['airbrush', 'Airbrush & soft'],
	['paint', 'Paint'],
	['watercolour', 'Watercolour'],
	['blend', 'Blend & smudge'],
	['fx', 'Texture & FX'],
	['erasers', 'Erasers']
];

const OVERRIDES = { DNA_brush: 'fx' };
const categorise = (name) => OVERRIDES[name] ?? RULES.find(([, re]) => re.test(name))?.[0] ?? 'fx';
const slug = (s) =>
	s
		.toLowerCase()
		.replace(/%/g, 'pct')
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-|-$/g, '');
const pretty = (s) =>
	s
		.replace(/airbruch/i, 'Airbrush')
		.replace(/#\d+$/, '')
		.replace(/_(sk|sm|s|l|c|a)$/i, '')
		.replace(/[_-]+/g, ' ')
		.replace(/\s+/g, ' ')
		.trim()
		.replace(/^./, (c) => c.toUpperCase());

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const list = [];
for (const [set, author] of Object.entries(SETS)) {
	for (const f of readdirSync(join(brushes, set)).filter((f) => f.endsWith('.myb')).sort()) {
		const name = f.slice(0, -4);
		const id = `${slug(set)}-${slug(name)}`;
		copyFileSync(join(brushes, set, f), join(out, `${id}.myb`));
		const prev = join(brushes, set, `${name}_prev.png`);
		const hasPreview = existsSync(prev);
		if (hasPreview) copyFileSync(prev, join(out, `${id}.png`));
		list.push({ id, name: pretty(name), author, category: categorise(name), preview: hasPreview });
	}
}

const ids = new Set(list.map((b) => b.id));
if (ids.size !== list.length) throw new Error('duplicate brush ids');

const manifest = {
	source: 'mypaint-brushes v2.0.2 (https://github.com/mypaint/mypaint-brushes)',
	license: 'CC0-1.0',
	categories: CATEGORIES.map(([id, name]) => ({ id, name })),
	brushes: list
};
writeFileSync(join(out, 'manifest.json'), JSON.stringify(manifest, null, '\t') + '\n');
const counts = Object.fromEntries(CATEGORIES.map(([c]) => [c, list.filter((b) => b.category === c).length]));
console.log(`${list.length} brushes`, counts);
