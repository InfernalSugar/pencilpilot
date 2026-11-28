#!/usr/bin/env node
// The current app's own drawing-together code (apps/studio/src/lib/together.ts, bundled
// as is with esbuild) as the oracle for the native port (crates/together): what it makes of
// each edit, and what the shared state becomes after a sequence of them.
//
//   node tools/native-qa/together_oracle.mjs bundle <out.mjs>          (needs pnpm install)
//   node tools/native-qa/together_oracle.mjs gen <n> <seed> > corpus.jsonl
//   node tools/native-qa/together_oracle.mjs run <bundle.mjs> < corpus.jsonl > expected.jsonl
//
// The corpus: edits of every kind, well made and broken on purpose (wrong types, missing
// and extra fields, ids of the wrong shape, control characters, numbers at the limits,
// unknown kinds, AI-made content), each with who sent it. `run` writes, per line, the
// parsed edit (null: dropped), whether the shared state took it, and the shared state
// after it (layers, order, groups, asks).

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');
const [cmd, ...args] = process.argv.slice(2);

if (cmd === 'bundle') {
	const pnpm = path.join(root, 'node_modules/.pnpm');
	const dir = fs.readdirSync(pnpm).find((d) => d.startsWith('esbuild@'));
	if (!dir) throw new Error('esbuild not found: run pnpm install');
	const esbuild = path.join(pnpm, dir, 'node_modules/esbuild/bin/esbuild');
	execFileSync(
		esbuild,
		['src/lib/together.ts', '--bundle', '--format=esm', '--platform=node', `--outfile=${path.resolve(args[0])}`, '--log-level=warning'],
		{ cwd: path.join(root, 'apps/studio'), stdio: 'inherit' }
	);
	process.exit(0);
}

// ---- a seeded generator ----
let seed = 1;
const rnd = () => {
	seed = (seed * 1103515245 + 12345) >>> 0;
	return seed / 2 ** 32;
};
const pick = (a) => a[Math.floor(rnd() * a.length)];
const hex = (n) => Array.from({ length: n }, () => '0123456789abcdef'[Math.floor(rnd() * 16)]).join('');

const MEMBERS = [hex(64), hex(64), hex(64)];
const lidOf = (m, n) => `${m.slice(0, 12)}:${n}`;
const LIDS = () => [
	lidOf(MEMBERS[0], 1),
	lidOf(MEMBERS[0], 2),
	lidOf(MEMBERS[1], 1),
	lidOf(MEMBERS[2], 7),
	lidOf(MEMBERS[1], 3)
];
const B64 = ['', 'AAAA', 'z:AAAA', 'r:' + 'A'.repeat(20), 'not base64!', 'QUJD', 'QUJD=', 'QU JD'];
const STRS = ['', ' ', 'Layer', '  padded  ', 'tab\there', 'ctl\u0001\u0007x', 'é ünïcödé 🎨', 'x'.repeat(70), '​', 'line\nbreak', 'emoji😀'.repeat(12)];
const NUMS = [0, 1, -1, 2, 7, 0.5, -0.0, 999999, 1000000, -999999, 1e6 + 1, 3.0000001, NaN, Infinity, null, '3', true, 1e21, -1e-7, 12];
const BLENDS = ['normal', 'multiply', 'screen', 'overlay', 'NORMAL', 'color-dodge', 'x'.repeat(21), '', 'bad blend', 'a1'];
const any = () =>
	pick([
		() => pick(STRS),
		() => pick(NUMS),
		() => pick(B64),
		() => null,
		() => undefined,
		() => [],
		() => ({}),
		() => [pick(NUMS), pick(NUMS)],
		() => pick([true, false])
	])();

const style = () =>
	pick([
		null,
		undefined,
		{},
		{ stroke: { on: true, size: pick(NUMS), color: pick(['#ff0000', '#FFF', 'red', 12]), position: pick(['outside', 'inside', 'center', 'x']), opacity: pick(NUMS) } },
		{ shadow: { on: pick([true, false, 1]), color: '#000000', opacity: pick(NUMS), angle: pick(NUMS), distance: pick(NUMS), size: pick(NUMS) } },
		{ glow: { on: true, color: '#00ff00', opacity: pick(NUMS), size: pick(NUMS) } },
		'style'
	]);

const look = () =>
	pick([
		{ visible: pick([true, false, 0, 'no', undefined]), opacity: pick(NUMS), blend: pick(BLENDS), alphaLock: pick([true, false, 1]), clip: pick([true, false, 'yes']), ...(rnd() < 0.5 ? { style: style() } : {}) },
		null,
		'look',
		{ opacity: 0.5, blend: 'normal' },
		{ visible: false, opacity: 2, blend: 'screen', alphaLock: true, clip: true }
	]);

const tiles = () => {
	const n = pick([0, 1, 2, 3, 48, 193]);
	const set = Array.from({ length: Math.min(n, 4) }, () =>
		pick([[pick(NUMS), pick(NUMS), pick(B64)], [1, 2, 'z:AAAA'], [1, 2], 'tile', [3, 4, 5]])
	);
	if (n > 4) for (let i = 4; i < n; i++) set.push([i, 0, 'r:AAAA']);
	return { set: pick([set, set, null, 'x']), clear: pick([[], [[1, 2]], [[pick(NUMS), pick(NUMS)]], null, [[1]]]) };
};

const tl = () =>
	pick([
		{ scenes: [] },
		{ scenes: [{ name: 'S', cuts: [{ name: 'C', tracks: { [pick(LIDS())]: [{ d: '', n: 1 }] } }] }] },
		{ scenes: 'x' },
		null,
		{}
	]);

const lineEdit = () => ({
	del: pick([[], ['abc'], ['bad id!'], 'x', ['a'.repeat(41)], [1]]),
	put: pick([[], [[0, 'QUJD']], [[-1, 'QUJD']], [[0, 'not b64']], [[1e7, 'QUJD']], 'x', [[0.5, 'QUJD']]]),
	...(rnd() < 0.4 ? { base: pick(B64) } : {})
});

const KINDS = {
	layer: () => ({ lid: pick(LIDS()), name: pick(STRS), ...(rnd() < 0.3 ? { adjust: pick(['{"curves":1}', '[1]', 'x', '{}', 5]) } : {}), ...(rnd() < 0.3 ? { lines: pick([true, 1, false]) } : {}) }),
	lines: () => ({ lid: pick(LIDS()), did: pick([pick(LIDS()), 'bad', '']), ...lineEdit() }),
	pixels: () => ({ lid: pick(LIDS()) }),
	adjust: () => ({ lid: pick(LIDS()), spec: pick(['{"a":1}', '[]', 'x', 3, '{"a":' + '1'.repeat(9000) + '}']) }),
	mask: () => ({ lid: pick(LIDS()), on: pick([true, false, undefined, 0]), ...tiles() }),
	rename: () => ({ lid: pick(LIDS()), name: pick(STRS) }),
	look: () => ({ lid: pick(LIDS()), look: look() }),
	remove: () => ({ lid: pick(LIDS()) }),
	move: () => ({ lid: pick(LIDS()), to: pick(NUMS) }),
	tiles: () => ({ lid: pick(LIDS()), did: pick([pick(LIDS()), 'bad', 5]), ...tiles() }),
	timeline: () => ({ lid: pick(['', pick(LIDS())]), tl: tl() }),
	sound: () => ({ lid: '', file: pick(['a.wav', 'x/../y', '', 'abc123.mp3']), name: pick(STRS), i: pick(NUMS), n: pick(NUMS), data: pick(B64) }),
	ask: () => ({ lid: pick(LIDS()), why: pick(['draw', 'merge', 'group', 'delete', 'steal', undefined]), own: pick([true, false, 1]), ...(rnd() < 0.4 ? { with: pick([pick(LIDS()), 'bad']) } : {}) }),
	group: () => ({ lid: pick(LIDS()), gid: pick([pick(LIDS()), 'g', lidOf(pick(MEMBERS), 99)]), name: pick(STRS), lids: pick([[pick(LIDS())], [pick(LIDS()), pick(LIDS())], [], 'x', ['bad']]) }),
	gset: () => ({ lid: pick([lidOf(MEMBERS[0], 99), pick(LIDS())]), name: pick(STRS), visible: pick([true, false, 0]), opacity: pick(NUMS) }),
	place: () => ({ lid: pick(LIDS()), to: pick(NUMS), gid: pick(['', lidOf(MEMBERS[0], 99), 'bad']) }),
	ungroup: () => ({ lid: pick([lidOf(MEMBERS[0], 99), pick(LIDS())]) }),
	block: () => ({ lid: pick([lidOf(MEMBERS[0], 99), pick(LIDS())]), to: pick(NUMS) }),
	grant: () => ({ lid: pick(LIDS()), to: pick([...MEMBERS, 'short', MEMBERS[0].toUpperCase()]), handover: pick([true, false, 'yes']) }),
	deny: () => ({ lid: pick(LIDS()), to: pick([...MEMBERS, 'x']) }),
	revoke: () => ({ lid: pick(LIDS()), to: pick([...MEMBERS, 'x']) })
};
// not on the list: dropped on arrival (AI-made content among them)
const UNKNOWN = [
	() => ({ k: 'ai', lid: pick(LIDS()), prompt: 'a cat', image: 'z:AAAA' }),
	() => ({ k: 'generate', lid: pick(LIDS()), model: 'qwen-image', tiles: [] }),
	() => ({ k: 'ai_image', lid: pick(LIDS()), did: pick(LIDS()), set: [], clear: [] }),
	() => ({ k: 'Tiles', lid: pick(LIDS()), did: pick(LIDS()), set: [], clear: [] }),
	() => ({ k: 'frames', lid: pick(LIDS()) }),
	() => ({ k: 'chat', text: 'hi' }),
	() => ({ k: 'snap.tiles', lid: pick(LIDS()), did: pick(LIDS()), set: [] }),
	() => ({ k: 'v2-layer', lid: pick(LIDS()), name: 'from a newer app' }),
	() => ({ lid: pick(LIDS()), name: 'no kind' }),
	() => ({ k: 5 }),
	() => 'just text',
	() => [1, 2, 3],
	() => null,
	() => 42
];

// well made, as an app sends them (only the access rules decide whether they count)
const okName = () => pick(['Ink', 'Colour', 'Line art', 'BG', 'é 🎨', '  spaced  ']);
const okTiles = () => ({ set: [[pick([0, 1, 5, 29]), pick([0, 2, 16]), pick(['z:AAAA', 'r:' + 'A'.repeat(8)])]], clear: pick([[], [[3, 4]]]) });
const CLEAN = {
	layer: (L) => ({ lid: L, name: okName(), ...(rnd() < 0.2 ? { lines: true } : {}) }),
	tiles: (L) => ({ lid: L, did: L.replace(/:\d+$/, ':' + (1000 + Math.floor(rnd() * 3))), ...okTiles() }),
	mask: (L) => ({ lid: L, on: rnd() < 0.8, ...okTiles() }),
	rename: (L) => ({ lid: L, name: okName() }),
	look: (L) => ({ lid: L, look: { visible: rnd() < 0.7, opacity: pick([1, 0.5, 0.25, 0.123456]), blend: pick(['normal', 'multiply', 'screen']), alphaLock: rnd() < 0.2, clip: rnd() < 0.2 } }),
	move: (L) => ({ lid: L, to: pick([0, 1, 2, 3, 10]) }),
	remove: (L) => ({ lid: L }),
	ask: (L) => ({ lid: L, why: pick(['draw', 'merge', 'group', 'delete']), own: rnd() < 0.3 }),
	grant: (L) => ({ lid: L, to: pick(MEMBERS), handover: rnd() < 0.3 }),
	deny: (L) => ({ lid: L, to: pick(MEMBERS) }),
	revoke: (L) => ({ lid: L, to: pick(MEMBERS) }),
	group: (L) => ({ lid: L, gid: lidOf(pick(MEMBERS), 90 + Math.floor(rnd() * 3)), name: 'Group', lids: [L, pick(LIDS())] }),
	gset: () => ({ lid: lidOf(pick(MEMBERS), 90 + Math.floor(rnd() * 3)), name: okName(), visible: rnd() < 0.7, opacity: pick([1, 0.5]) }),
	place: (L) => ({ lid: L, to: pick([0, 1, 2]), gid: pick(['', lidOf(pick(MEMBERS), 90)]) }),
	ungroup: () => ({ lid: lidOf(pick(MEMBERS), 90 + Math.floor(rnd() * 3)) }),
	block: () => ({ lid: lidOf(pick(MEMBERS), 90 + Math.floor(rnd() * 3)), to: pick([0, 1]) }),
	pixels: (L) => ({ lid: L }),
	timeline: (L) => ({ lid: '', tl: { scenes: [{ name: 'S', cuts: [{ name: 'C', tracks: { [L]: [{ d: '', n: 1 }] } }] }] } })
};

function clean() {
	const k = pick(Object.keys(CLEAN));
	const L = pick(LIDS());
	// mostly from the layer's maker (so the rules let it through), else anyone
	const owner = MEMBERS.find((m) => L.startsWith(m.slice(0, 12)));
	return { from: rnd() < 0.6 ? owner : pick(MEMBERS), body: { k, ...CLEAN[k](L) } };
}

function edit() {
	if (rnd() < 0.08) return pick(UNKNOWN)();
	const k = pick(Object.keys(KINDS));
	const e = { k, ...KINDS[k]() };
	// broken on purpose now and then: a field dropped or swapped for anything
	if (rnd() < 0.2) {
		const keys = Object.keys(e).filter((x) => x !== 'k');
		if (keys.length) {
			const f = pick(keys);
			if (rnd() < 0.5) delete e[f];
			else e[f] = any();
		}
	}
	// a field a newer app might add
	if (rnd() < 0.1) e[pick(['ai', 'v', 'extra', 'generated'])] = pick([true, 1, 'x']);
	return e;
}

if (cmd === 'gen') {
	const n = +args[0] || 2000;
	seed = +args[1] || 1;
	// a session's start: the members make layers, a group, ask for access
	const lines = [];
	const L = LIDS();
	lines.push({ from: MEMBERS[0], body: { k: 'layer', lid: L[0], name: 'Ink' } });
	lines.push({ from: MEMBERS[0], body: { k: 'layer', lid: L[1], name: 'Colour' } });
	lines.push({ from: MEMBERS[1], body: { k: 'layer', lid: L[2], name: 'Bo' } });
	lines.push({ from: MEMBERS[2], body: { k: 'layer', lid: L[3], name: 'Cy' } });
	lines.push({ from: MEMBERS[1], body: { k: 'ask', lid: L[0], why: 'draw', own: false } });
	lines.push({ from: MEMBERS[0], body: { k: 'grant', lid: L[0], to: MEMBERS[1], handover: false } });
	lines.push({ from: MEMBERS[0], body: { k: 'group', lid: L[0], gid: lidOf(MEMBERS[0], 99), name: 'G', lids: [L[0], L[1]] } });
	for (let i = 0; i < n; i++) lines.push(rnd() < 0.5 ? clean() : { from: pick(MEMBERS), body: edit() });
	for (const l of lines) process.stdout.write(JSON.stringify(l) + '\n');
	process.exit(0);
}

if (cmd === 'run') {
	const m = await import(path.resolve(args[0]));
	const shared = new m.Shared();
	const state = () => ({
		layers: [...shared.layers.values()],
		order: [...shared.order],
		groups: [...shared.groups.values()],
		asks: shared.asks.map((a) => ({ ...a }))
	});
	const rl = readline.createInterface({ input: process.stdin });
	for await (const line of rl) {
		if (!line.trim()) continue;
		const { from, body } = JSON.parse(line);
		let parsed = null;
		let applied = null;
		let error = null;
		try {
			parsed = m.parseEdit(body);
			if (parsed) applied = shared.apply(from, parsed);
		} catch (e) {
			error = String(e);
		}
		process.stdout.write(JSON.stringify({ parsed, applied, error, state: state() }) + '\n');
	}
	process.exit(0);
}

console.error('usage: together_oracle.mjs bundle <out> | gen <n> <seed> | run <bundle> < corpus');
process.exit(2);
