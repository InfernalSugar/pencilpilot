// Today's app's PSD writer (packages/paint/src/psd.ts, as it is in the given checkout) on a
// document described by psd_check.py: the bytes it writes, to compare with the native app's.
//
//   node psd_ts.mjs <path to psd.ts> <doc.json> <out.psd>
//
// doc.json: {width, height, composite: <raw RGBA file>, items: [layer | group]}, a layer
// {kind: "layer", name, rgba: <raw RGBA file>, opacity, blend, visible, clip, mask?: <raw grey
// file>}, a group {kind: "group", name, opacity, visible, children: [layer]}; files are
// relative to doc.json, bottom first.
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

const [psdTs, docPath, outPath] = process.argv.slice(2);
const { writePsd } = await import(psdTs);
const base = dirname(docPath);
const doc = JSON.parse(readFileSync(docPath, 'utf8'));
const bytes = (f) => new Uint8Array(readFileSync(join(base, f)));
const layer = (l) => ({
	kind: 'layer',
	name: l.name,
	rgba: bytes(l.rgba),
	opacity: l.opacity,
	blend: l.blend,
	visible: l.visible,
	clip: l.clip,
	...(l.mask ? { mask: bytes(l.mask) } : {})
});
const items = doc.items.map((it) =>
	it.kind === 'layer'
		? layer(it)
		: { kind: 'group', name: it.name, opacity: it.opacity, visible: it.visible, children: it.children.map(layer) }
);
const out = writePsd({ width: doc.width, height: doc.height, composite: bytes(doc.composite), items });
writeFileSync(outPath, out);
console.log(JSON.stringify({ bytes: out.length }));
