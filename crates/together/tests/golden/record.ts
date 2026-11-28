/**
 * Golden fixtures for crates/together: runs scripted sessions (scenarios.json) through the
 * studio's own together.ts, and writes what every app sent, received and ended with to
 * ../fixtures/. The Rust tests (tests/golden.rs) run the same scripts and compare.
 *
 *   cd apps/studio && node --no-warnings --import ../../crates/together/tests/golden/loader.mjs \
 *     ../../crates/together/tests/golden/record.ts
 *
 * (Needs the studio's node modules: `pnpm install` once. Nothing here touches the network.)
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { NO_STYLE } from '../../../../packages/paint/src/layer-style.ts';
import {
	decodeLines,
	encodeLines,
	STRIDE,
	type LineStroke
} from '../../../../packages/paint/src/lines.ts';
import { chatBody, chatText } from '../../../../apps/studio/src/lib/chat.ts';
import { cleanInvite, memberColour } from '../../../../apps/studio/src/lib/share.ts';
import type { Timeline } from '../../../../apps/studio/src/lib/timeline.ts';
import {
	applyLineEdit,
	decodeTile,
	encodeTile,
	fromShared,
	lineEdits,
	parseEdit,
	readSavedShare,
	splitBase,
	strokeToShare,
	toShared,
	Together,
	withBase
} from '../../../../apps/studio/src/lib/together.ts';
import { FakeCanvas, tile } from './fake.ts';

// the same ids every run (each app counts its new layers' ids from 0)
Math.random = () => 0;

const here = new URL('.', import.meta.url);
const out = new URL('../fixtures/', here);
const MEMBERS: Record<string, string> = {
	A: 'a'.repeat(64),
	B: 'b'.repeat(64),
	C: 'c'.repeat(64)
};

type Step = Record<string, any>;
type Out = ['e', number] | ['d', number];

const b64 = (b: Uint8Array) => Buffer.from(b).toString('base64');

/** a line stroke from (x, 0) to (x, 10), moved by dx */
function stroke(id: string, x: number, dx = 0): LineStroke {
	const pts = new Float32Array(3 * STRIDE);
	for (let i = 0; i < 3; i++) pts.set([x + dx, i * 5, 0.5, 0, 0, 0.01], i * STRIDE);
	return { id, pts, brush: 'pen', color: '#000000', size: 1, opacity: 1, radius: 2, seed: 7 };
}

function basePack(keys: [number, number][], gone: [number, number][] = []) {
	const o = new Uint8Array(4 + keys.length * 8 + 4 + 4 + gone.length * 8);
	const v = new DataView(o.buffer);
	let at = 0;
	v.setUint32(at, keys.length, true);
	at += 4;
	for (const [x, y] of keys) {
		v.setInt32(at, x, true);
		v.setInt32(at + 4, y, true);
		at += 8;
	}
	v.setUint32(at, 0, true);
	at += 4;
	v.setUint32(at, gone.length, true);
	at += 4;
	for (const [x, y] of gone) {
		v.setInt32(at, x, true);
		v.setInt32(at + 4, y, true);
		at += 8;
	}
	return o;
}

interface App {
	t: Together;
	c: FakeCanvas;
	name: string;
	notes: string[];
	tiles: string[];
	/** each time this app ran (an action of its own, or a message to it): what it sent */
	acts: Out[][];
	drawings: Map<string, number>;
	saved?: unknown;
}

class Session {
	seq = 0;
	apps = new Map<string, App>();
	parked = new Map<string, App>();
	queue: { to: string; run: (a: App) => Promise<unknown> }[] = [];
	log: { from: string; seq: number; body: unknown }[] = [];
	directs: { from: string; to: string; body: unknown }[] = [];
	current: Out[] | null = null;
	saves: Record<string, unknown> = {};

	net(me: string) {
		return {
			edit: (body: unknown) => {
				const seq = ++this.seq;
				const copy = structuredClone(body);
				this.log.push({ from: me, seq, body: copy });
				this.current?.push(['e', this.log.length - 1]);
				for (const [to] of this.apps)
					this.queue.push({ to, run: (a) => a.t.onEdit(seq, MEMBERS[me], structuredClone(copy)) });
			},
			direct: (to: string, body: unknown) => {
				const copy = structuredClone(body);
				const toKey = Object.keys(MEMBERS).find((k) => MEMBERS[k] === to)!;
				this.directs.push({ from: me, to: toKey, body: copy });
				this.current?.push(['d', this.directs.length - 1]);
				this.queue.push({
					to: toKey,
					run: (a) => a.t.onDirect(MEMBERS[me], structuredClone(copy))
				});
			}
		};
	}

	/** run something of app `k`'s, and wait until it has sent everything it will */
	async act(k: string, f: (a: App) => unknown) {
		const a = this.apps.get(k)!;
		const outs: Out[] = [];
		this.current = outs;
		await f(a);
		let quiet = 0;
		let n = -1;
		while (quiet < 6) {
			await new Promise((r) => setTimeout(r, 1));
			const now = this.log.length + this.directs.length;
			quiet = now === n ? quiet + 1 : 0;
			n = now;
		}
		this.current = null;
		a.acts.push(outs);
	}

	async step(n: number) {
		for (let i = 0; i < n && this.queue.length; i++) {
			const q = this.queue.shift()!;
			if (this.apps.has(q.to)) await this.act(q.to, (a) => q.run(a));
		}
	}

	async settle() {
		while (this.queue.length) await this.step(1);
	}
}

function layerId(a: App, name: string) {
	const l = a.c.layerList.filter((x) => x.name === name).at(-1);
	if (!l) throw new Error(`no layer ${name}`);
	return l.id;
}
function groupId(a: App, name: string) {
	const g = a.c.groupList.find((x) => x.name === name);
	if (!g) throw new Error(`no group ${name}`);
	return g.id;
}
/** a layer's or group's shared id, as this app knows it */
function lidOf(a: App, s: Step) {
	const lid = s.group ? a.t.gidOf(groupId(a, s.group)) : a.t.lidOf(layerId(a, s.layer));
	if (!lid) throw new Error('not shared');
	return lid;
}
function drawing(a: App, ref: string | number) {
	if (ref === 0) return 0;
	if (typeof ref === 'number') return ref;
	if (!a.drawings.has(ref)) a.drawings.set(ref, a.c.newDrawing());
	return a.drawings.get(ref)!;
}
const cut0 = (tl: Timeline) => tl.scenes[0].cuts[0];

/** what an app ends with */
function state(a: App) {
	const c = a.c;
	const tiles = (m: Map<string, Uint8Array> | null | undefined) =>
		m ? Object.fromEntries([...m].map(([k, px]) => [k, [px[0], px.every((v) => v === px[0])]])) : null;
	return {
		layers: c.layerList.map((l) => ({
			id: l.id,
			name: l.name,
			locked: l.locked,
			visible: l.visible,
			opacity: l.opacity,
			blend: l.blend,
			adjust: l.adjust,
			group: l.group,
			style: l.style,
			drawing: l.drawing,
			lines: c.lineLayers.has(l.id),
			mask: tiles(l.mask)
		})),
		groups: c.groupList,
		selected: c.selected,
		drawings: Object.fromEntries(
			[...c.drawings].filter(([, m]) => m.size).map(([d, m]) => [d, tiles(m)])
		),
		strokes: Object.fromEntries(
			[...c.strokes].map(([d, list]) => [d, b64(encodeLines(list))])
		),
		plain: [...c.plain].sort((x, y) => x - y),
		baseIn: c.baseIn.map(([d, b]) => [d, b64(b)]),
		sounds: Object.fromEntries(
			[...c.sounds].map(([f, s]) => [f, [s.name, s.bytes.length, s.bytes.reduce((t, v) => t + v, 0)]])
		),
		tl: c.tl,
		shared: a.t.shared.snapshot(),
		ids: [...a.t.ids],
		gids: [...a.t.gids],
		synced: a.t.synced,
		// (which drawing here each shared one is: the numbers here are this app's own)
		dids: a.t.save('pp1-x', []).dids,
		notes: a.notes,
		tilesFrom: a.tiles
	};
}

async function run(steps: Step[]) {
	const s = new Session();
	for (const st of steps) {
		const k = st.app as string;
		const a = (s.apps.get(k) ?? s.parked.get(k))!;
		const c = a?.c;
		switch (st.do) {
			// ---- the session ----
			case 'app': {
				const app: App = {
					t: null as unknown as Together,
					c: new FakeCanvas(st.layers ?? []),
					name: st.name,
					notes: [],
					tiles: [],
					acts: [],
					drawings: new Map()
				};
				app.t = new Together(MEMBERS[k], st.name, app.c, s.net(k));
				app.t.onnote = (t) => app.notes.push(t);
				app.t.ontiles = (f) => app.tiles.push(f);
				s.apps.set(k, app);
				break;
			}
			case 'settle':
				await s.settle();
				break;
			case 'step':
				await s.step(st.n);
				break;
			case 'forge':
				s.net(st.from).edit(st.body);
				break;
			case 'leave':
				s.parked.set(k, a);
				s.apps.delete(k);
				break;
			case 'back': {
				const was = s.parked.get(k)!;
				s.parked.delete(k);
				const app: App = { ...was, notes: [], tiles: [] };
				app.t = new Together(MEMBERS[k], was.name, was.c, s.net(k));
				app.t.onnote = (t) => app.notes.push(t);
				app.t.ontiles = (f) => app.tiles.push(f);
				s.apps.set(k, app);
				break;
			}
			// ---- this app's side of drawing together ----
			case 'host':
				await s.act(k, (a) => a.t.startHost());
				break;
			case 'join':
				await s.act(k, (a) => a.t.startJoin(MEMBERS[st.host]));
				break;
			case 'local':
				await s.act(k, (a) => a.t.localChanged());
				break;
			case 'timeline':
				await s.act(k, (a) => a.t.timelineChanged());
				break;
			case 'ask':
				await s.act(k, (a) => a.t.ask(lidOf(a, st)));
				break;
			case 'grant':
				await s.act(k, (a) => a.t.grant(lidOf(a, st), MEMBERS[st.to], !!st.handover));
				break;
			case 'deny':
				await s.act(k, (a) => a.t.deny(lidOf(a, st), MEMBERS[st.to]));
				break;
			case 'revoke':
				await s.act(k, (a) => a.t.revoke(lidOf(a, st), MEMBERS[st.to]));
				break;
			case 'mergeDown':
				await s.act(k, (a) => void a.t.mergeDown(layerId(a, st.layer)));
				break;
			case 'deleteLayer':
				await s.act(k, (a) => void a.t.deleteLayer(layerId(a, st.layer)));
				break;
			case 'addGroup':
				await s.act(k, (a) => void a.t.addGroup(layerId(a, st.layer)));
				break;
			case 'moveLayerTo':
				await s.act(
					k,
					(a) =>
						void a.t.moveLayerTo(
							layerId(a, st.layer),
							st.to,
							st.group ? groupId(a, st.group) : 0
						)
				);
				break;
			case 'changeGroup':
				await s.act(
					k,
					(a) =>
						void a.t.changeGroup(groupId(a, st.group), () =>
							a.c.setGroup(groupId(a, st.group), {
								name: st.name,
								visible: st.visible,
								opacity: st.opacity
							})
						)
				);
				break;
			case 'ungroup':
				await s.act(k, (a) => void a.t.ungroup(groupId(a, st.group)));
				break;
			case 'deleteGroup':
				await s.act(k, (a) => void a.t.deleteGroup(groupId(a, st.group)));
				break;
			case 'mergeGroup':
				await s.act(k, (a) => void a.t.mergeGroup(groupId(a, st.group)));
				break;
			case 'moveGroup':
				await s.act(k, (a) => void a.t.moveGroup(groupId(a, st.group), !!st.up));
				break;
			case 'moveOwn':
				await s.act(k, (a) => a.t.moveOwn(lidOf(a, st), st.to));
				break;
			case 'removeOwn':
				await s.act(k, (a) => a.t.removeOwn(lidOf(a, st)));
				break;
			case 'shareAbove':
				await s.act(k, (a) => a.t.shareAbove(layerId(a, st.layer), layerId(a, st.above)));
				break;
			case 'addOwnLayer':
				await s.act(k, (a) => a.t.addOwnLayer(st.name ?? '', !!st.lines));
				break;
			case 'save': {
				const saved = JSON.parse(
					JSON.stringify(
						a.t.save(
							'pp1-code',
							(st.members ?? []).map((m: string) => ({ id: MEMBERS[m], name: m, addr: {} }))
						)
					)
				);
				s.saves[k] = saved;
				a.saved = saved;
				break;
			}
			case 'restore':
				await s.act(k, (a) => a.t.restore(readSavedShare(a.saved)!));
				break;
			case 'rejoin':
				await s.act(k, (a) => a.t.rejoin(MEMBERS[st.host]));
				break;
			case 'resumeHost':
				await s.act(k, (a) => a.t.resumeHost());
				break;
			// ---- the canvas ----
			case 'busy':
				c.busy = st.on;
				break;
			case 'paint':
				c.paint(layerId(a, st.layer), st.x, st.y, st.v);
				break;
			case 'fill':
				for (let i = 0; i < st.n; i++) c.paint(layerId(a, st.layer), i % 20, Math.floor(i / 20), st.v);
				break;
			case 'wipe':
				c.drawings.get(c.drawingOf(layerId(a, st.layer)))!.clear();
				break;
			case 'paintD':
				c.drawings.get(drawing(a, st.d))!.set(`${st.x},${st.y}`, tile(st.v));
				break;
			case 'addLayer':
				c.addLayer(st.name, st.adjust);
				break;
			case 'set':
				c.setLayer(layerId(a, st.layer), st.patch);
				break;
			case 'mask': {
				const l = c.layerList.find((x) => x.id === layerId(a, st.layer))!;
				l.mask = st.tiles
					? new Map(st.tiles.map(([x, y, v]: number[]) => [`${x},${y}`, tile(v)]))
					: null;
				break;
			}
			case 'adjust':
				c.setAdjust(layerId(a, st.layer), st.spec);
				break;
			case 'group':
				c.addGroup(layerId(a, st.layer));
				break;
			case 'groupSet':
				c.setGroup(groupId(a, st.group), {
					name: st.name,
					visible: st.visible,
					opacity: st.opacity
				});
				break;
			case 'lineLayer':
				c.setLineLayer(layerId(a, st.layer), st.on);
				break;
			case 'strokes': {
				// (strokes that stay as they were stay the same objects, as the app keeps them)
				const d = c.drawingOf(layerId(a, st.layer));
				const was = c.lineStrokes(d);
				const list = st.list.map(([id, x, dx]: [string, number, number?]) => {
					const s = stroke(id, x, dx ?? 0);
					const old = was.find((o) => o.id === id);
					return old && old.pts[0] === s.pts[0] ? old : s;
				});
				c.setLineStrokes(d, list, !!st.plain);
				break;
			}
			case 'base': {
				const d = c.drawingOf(layerId(a, st.layer));
				if (st.out) c.baseOut.set(d, basePack(st.out[0], st.out[1]));
				if (st.full) c.baseFull.set(d, basePack(st.full[0], st.full[1]));
				break;
			}
			case 'drawingOf':
				c.drawingOf(layerId(a, st.layer));
				break;
			case 'track':
				cut0(c.tl).tracks[layerId(a, st.layer)] = st.track.map(([d, n]: [string | number, number]) => ({
					drawing: drawing(a, d),
					length: n
				}));
				break;
			case 'showFrame':
				c.showFrame(st.f);
				break;
			case 'fresh': {
				const d = drawing(a, st.d);
				const id = layerId(a, st.layer);
				c.layerList.find((l) => l.id === id)!.drawing = d;
				cut0(c.tl).tracks[id] = [{ drawing: d, length: 1 }];
				break;
			}
			case 'follow': {
				const from = layerId(a, st.from);
				const to = layerId(a, st.to);
				const made = new Map<number, number>();
				for (const sc of c.tl.scenes)
					for (const ct of sc.cuts) {
						const t = ct.tracks[from];
						if (!t) continue;
						ct.tracks[to] = t.map((e) => {
							if (!e.drawing) return { drawing: 0, length: e.length };
							if (!made.has(e.drawing)) made.set(e.drawing, c.newDrawing());
							return { drawing: made.get(e.drawing)!, length: e.length };
						});
					}
				break;
			}
			case 'sound': {
				const bytes = new Uint8Array(st.size).map((_, i) => i % 251);
				c.sounds.set(st.file, { bytes, name: st.name });
				break;
			}
			case 'audio':
				c.tl.scenes[0].audio = st.clips.map((x: object, i: number) => ({ id: i + 1, ...x }));
				break;
			case 'stage':
				c.tl.stages = { ...(c.tl.stages ?? {}), [layerId(a, st.layer)]: st.stage };
				break;
			case 'palettes':
				c.tl.palettes = st.list;
				break;
			case 'markers':
				c.tl.scenes[0].markers = st.list;
				break;
			case 'cut':
				Object.assign(cut0(c.tl), st.set);
				break;
			case 'scene':
				c.tl.scenes.push({ id: c.tl.scenes.length + 1, name: st.name, cuts: [{ id: 1, name: 'Cut 1', tracks: {} }] });
				break;
			case 'mouths':
				c.tl.mouths = st.list.map((m: { layer: string; shapes: Record<string, string> }) => ({
					layer: layerId(a, m.layer),
					shapes: Object.fromEntries(Object.entries(m.shapes).map(([sh, d]) => [sh, drawing(a, d)]))
				}));
				break;
			case 'kinds':
				c.tl.kinds = Object.fromEntries(st.list.map(([d, kind]: [string, string]) => [drawing(a, d), kind]));
				break;
			case 'statics':
				c.tl.statics = st.layers.map((n: string) => layerId(a, n));
				break;
			default:
				throw new Error('unknown step ' + st.do);
		}
	}
	const final = Object.fromEntries([...s.apps].map(([k, a]) => [k, state(a)]));
	const acts = Object.fromEntries(
		[...s.apps, ...s.parked].map(([k, a]) => [k, a.acts])
	);
	return { log: s.log, directs: s.directs, acts, saves: s.saves, final };
}

// ---- edits, strokes and timelines on their own ----

async function pure() {
	const A = MEMBERS.A;
	const lid = 'aaaaaaaaaaaa:1';
	const did = 'aaaaaaaaaaaa:5';
	const look = { visible: true, opacity: 0.25, blend: 'multiply', alphaLock: true, clip: false };
	const enc = strokeToShare(stroke('s1', 4));
	const bodies: unknown[] = [
		null,
		5,
		'layer',
		[],
		{},
		{ k: 'ai-image', lid, png: 'x' },
		{ k: 'generate', lid, prompt: 'a cat' },
		{ k: 'layer', lid, name: 'Hi' },
		{ k: 'layer', lid, name: '  a\u0007b\u0000c  ' },
		{ k: 'layer', lid, name: 'x'.repeat(80) },
		{ k: 'layer', lid, name: '😀'.repeat(40) },
		{ k: 'layer', lid, name: 5 },
		{ k: 'layer', lid, name: '﻿  sp  ' },
		{ k: 'layer', lid: '../etc', name: 'x' },
		{ k: 'layer', lid: 'AAAAAAAAAAAA:1', name: 'x' },
		{ k: 'layer', lid: 'aaaaaaaaaaaa:1234567890123', name: 'x' },
		{ k: 'layer', lid, name: 'A', adjust: '{"hue":3}' },
		{ k: 'layer', lid, name: 'A', adjust: '[1]', lines: true },
		{ k: 'layer', lid, name: 'A', adjust: '{bad', lines: true },
		{ k: 'layer', lid, name: 'A', adjust: '{"x":"' + 'y'.repeat(8000) + '"}' },
		{ k: 'lines', lid, did, del: ['x', 7, '../', 'A'], put: [[0, enc]] },
		{ k: 'lines', lid, did, del: [], put: [[0, enc]], base: '' },
		{ k: 'lines', lid, did, del: [], put: [[1.5, enc]] },
		{ k: 'lines', lid, did, del: [], put: [[0, '']] },
		{ k: 'lines', lid, did, del: [], put: [], base: null },
		{ k: 'lines', lid, did: 'mask', del: [], put: [] },
		{ k: 'pixels', lid },
		{ k: 'adjust', lid, spec: '{"a":1}' },
		{ k: 'adjust', lid, spec: 'null' },
		{ k: 'mask', lid, on: false, set: [], clear: [[1, 2]] },
		{ k: 'mask', lid, set: [[0, 0, 'z:x'], [1, 'a', 'z:'], [2, 2]], clear: [[1e6, 0], [-5, 3], [2.5, 1]] },
		{ k: 'mask', lid, set: Array.from({ length: 193 }, () => [0, 0, 'z:']), clear: [] },
		{ k: 'mask', lid, set: Array.from({ length: 192 }, () => [0, 0, 'z:']), clear: [] },
		{ k: 'rename', lid, name: '' },
		{ k: 'rename', lid, name: ' Sky ' },
		{ k: 'look', lid, look },
		{ k: 'look', lid, look: { ...look, opacity: 7, visible: 0 } },
		{ k: 'look', lid, look: { ...look, blend: 'Multiply' } },
		{ k: 'look', lid, look: { ...look, opacity: '1' } },
		{ k: 'look', lid, look: { ...look, style: null } },
		{ k: 'look', lid, look: { ...look, style: NO_STYLE } },
		{ k: 'look', lid, look: { ...look, style: [] } },
		{
			k: 'look',
			lid,
			look: {
				...look,
				style: {
					shadow: { on: true, angle: -30.5, distance: 900, colour: '#ABCDEF', opacity: -1 },
					glow: { on: 1, size: 0 }
				}
			}
		},
		{ k: 'look', lid, look: { ...look, style: { shadow: { angle: 720 } } } },
		{ k: 'remove', lid },
		{ k: 'move', lid, to: 3 },
		{ k: 'move', lid, to: -1 },
		{ k: 'move', lid, to: 2.0 },
		{ k: 'move', lid, to: 1e6 },
		{ k: 'move', lid, to: '3' },
		{ k: 'tiles', lid, did, set: [[0, 0, 'z:abc']], clear: [] },
		{ k: 'tiles', lid, did: 'bad', set: [], clear: [] },
		{ k: 'tiles', lid, did, set: {}, clear: [] },
		{ k: 'timeline', tl: { scenes: [] } },
		{ k: 'timeline', lid, tl: { scenes: 5 } },
		{ k: 'timeline', tl: [] },
		{ k: 'sound', file: 's1-abc.mp3', name: 'song', i: 0, n: 3, data: 'AAAA' },
		{ k: 'sound', file: 's1-abc.mp3', i: 2, n: 3, data: 'AAAA' },
		{ k: 'sound', file: 's1-abc.mp3', name: null, i: 0, n: 1, data: 'AAAA' },
		{ k: 'sound', file: 's1-abc.mp3', name: 12, i: 0, n: 1, data: 'AAAA' },
		{ k: 'sound', file: 's1-abc.mp3', name: 'x'.repeat(200), i: 0, n: 80, data: '' },
		{ k: 'sound', file: 's1-abc.mp3', i: 0, n: 81, data: 'AAAA' },
		{ k: 'sound', file: 's1-abc.mp3', i: 3, n: 3, data: 'AAAA' },
		{ k: 'sound', file: '../x', i: 0, n: 1, data: 'AAAA' },
		{ k: 'ask', lid },
		{ k: 'ask', lid, why: 'merge', own: true, with: 'aaaaaaaaaaaa:2' },
		{ k: 'ask', lid, why: 'steal', own: 1, with: 'nope' },
		{ k: 'group', lid, gid: 'aaaaaaaaaaaa:9', name: '', lids: [lid, 'x', 'aaaaaaaaaaaa:2'] },
		{ k: 'group', lid, gid: 'aaaaaaaaaaaa:9', lids: ['x'] },
		{ k: 'group', lid, gid: 'bad', lids: [lid] },
		{ k: 'gset', lid, name: 'G', visible: false, opacity: 2 },
		{ k: 'gset', lid, opacity: 0.5 },
		{ k: 'gset', lid, name: 'G' },
		{ k: 'place', lid, to: 0, gid: '' },
		{ k: 'place', lid, to: 2, gid: 'aaaaaaaaaaaa:9' },
		{ k: 'place', lid, to: 2 },
		{ k: 'place', lid, to: 2, gid: 'x' },
		{ k: 'ungroup', lid },
		{ k: 'block', lid, to: 1 },
		{ k: 'grant', lid, to: A },
		{ k: 'grant', lid, to: A, handover: true },
		{ k: 'grant', lid, to: A.toUpperCase() },
		{ k: 'deny', lid, to: A },
		{ k: 'revoke', lid, to: A },
		{ k: 'revoke', lid }
	];
	const parsed = bodies.map((b) => ({ body: b, edit: parseEdit(b) }));

	// line strokes with every part: their bytes, read back, and the edits between lists
	const fancy: LineStroke[] = [
		stroke('a', 1),
		{
			...stroke('tex', 2),
			color: '#AbCdEf',
			size: 2.5,
			opacity: 0.3,
			radius: 11.25,
			seed: 4294967295,
			texture: {
				grain: 'paper',
				grainScale: 1.5,
				grainStrength: 0.25,
				tip: null,
				dual: 'dots',
				dualSize: 0.5,
				dualScatter: 0.1,
				dualCount: 3,
				strands: { count: 8.6, spread: 50, wobble: 0.5, taper: -1 }
			} as never,
			reach: 4.5,
			smudge: true
		},
		{
			...stroke('poly', 3),
			shape: { kind: 'poly', pts: [[0, 0], [10.5, 3], [4, 9]], closed: true, pressure: 0.75, smooth: true }
		},
		{
			...stroke('ell', 4),
			shape: { kind: 'ellipse', c: [5, 6], a: 3, b: 2.5, theta: 0.3, circle: false, pressure: 1 },
			link: { g: 'grp1', k: 1, mode: 'radial', n: 6, cx: 100, cy: 50.5 }
		},
		{ ...stroke('er', 5), erase: true },
		{ ...stroke('cut', 6), dabs: [2, 9], span: [0, 2] },
		{ ...stroke('clipped', 7), clip: [{ ...stroke('m1', 8), erase: true }, { ...stroke('m2', 9), erase: true }] },
		{ ...stroke('tiny', 1e-7), size: 0.01, radius: 1e21 / 1e18, opacity: 1 / 3 }
	];
	const one = (s: LineStroke) => {
		const bytes = encodeLines([s]);
		return { b64: b64(bytes), back: b64(encodeLines(decodeLines(bytes))) };
	};
	const strokes = fancy.map(one);
	const all = b64(encodeLines(fancy));
	const allBack = b64(encodeLines(decodeLines(encodeLines(fancy))));
	const before = fancy.slice(0, 5);
	const after = [fancy[0], { ...fancy[2] }, fancy[1], ...fancy.slice(5)];
	const edits = lineEdits(lid, did, before, after);
	const applied = b64(encodeLines(applyLineEdit(before, edits[0])));
	const base = basePack([[0, 0], [1, 0]], [[5, 5]]);
	const withBaseEdits = withBase(lineEdits(lid, did, [], [fancy[0]]), lid, did, base);
	// a base too big for one edit: several parts
	const big = new Uint8Array(4 + 70 * 8 + 4 + 70 * (8 + 16384) + 4);
	{
		const v = new DataView(big.buffer);
		v.setUint32(0, 70, true);
		for (let i = 0; i < 70; i++) {
			v.setInt32(4 + i * 8, i, true);
			v.setInt32(8 + i * 8, -i, true);
		}
		let o = 4 + 70 * 8;
		v.setUint32(o, 70, true);
		o += 4;
		for (let i = 0; i < 70; i++) {
			v.setInt32(o, i, true);
			v.setInt32(o + 4, -i, true);
			big.fill(i * 3, o + 8, o + 8 + 16384);
			o += 8 + 16384;
		}
		v.setUint32(o, 0, true);
	}
	const bigParts = splitBase(big)!.map(b64);
	const bigEdits = withBase([], lid, did, big).map((e) => ({ ...e, base: e.base!.length }));

	// tiles: the bytes on the wire
	const T = 16384;
	const grad = new Uint8Array(T).map((_, i) => (i * 7 + (i >> 8)) & 255);
	let seed = 12345;
	const noise = new Uint8Array(T).map(() => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) >> 16) & 255);
	const soft = new Uint8Array(T);
	for (let y = 0; y < 64; y++)
		for (let x = 0; x < 64; x++) {
			const a = Math.max(0, Math.min(255, Math.round(255 * (1 - Math.hypot(x - 32, y - 32) / 30))));
			soft.set([20, 40, 200, a], (y * 64 + x) * 4);
		}
	const tiles = [];
	for (const t of [tile(0), tile(7), grad, noise, soft]) tiles.push({ raw: b64(t), wire: await encodeTile(t) });
	const decoded = [];
	for (const w of ['r:' + b64(tile(3)), 'r:' + b64(tile(3).subarray(1)), 'z:', 'z:@@', 'x:abc', 'r:AAA'])
		decoded.push({ wire: w, px: (await decodeTile(w)) ? b64((await decodeTile(w))!) : null });

	// timelines: this app's for the session, and the session's (odd ones too) as this app's
	const local: Timeline = {
		scenes: [
			{
				id: 1,
				name: 'Opening',
				markers: [{ frame: 3, label: 'beat' }],
				audio: [{ id: 1, name: 'song', file: 's1-a.mp3', start: -2, gain: 0.5, muted: true }],
				cuts: [
					{
						id: 1,
						name: 'Wide',
						camera: { x: 1, y: 2.5, width: 640, height: 360 },
						action: { 0: 'runs', 12: 'stops' },
						dialogue: { 4: 'Hi!' },
						transition: 'dissolve',
						tracks: {
							1: [{ drawing: 11, length: 2 }, { drawing: 0, length: 1 }],
							2: [{ drawing: 21, length: 3 }],
							9: [{ drawing: 91, length: 1 }]
						}
					}
				]
			}
		],
		statics: [2, 9],
		mouths: [{ layer: 1, shapes: { A: 11, X: 0 } }],
		kinds: { 11: 'key', 21: 'breakdown', 0: 'key' } as never,
		stages: { 1: 'rough', 9: 'comp' },
		palettes: [{ name: 'Mina', colours: [{ name: 'Skin', hex: '#f2c9a8' }] }]
	};
	const lidOfL = (l: number) => (l === 1 ? 'aaaaaaaaaaaa:1' : l === 2 ? 'aaaaaaaaaaaa:2' : null);
	const didOfD = (d: number) => `aaaaaaaaaaaa:${d}`;
	const toSharedOut = toShared(local, lidOfL, didOfD);
	const odd: unknown[] = [
		toSharedOut,
		{ scenes: [] },
		{ scenes: 'x', statics: 5, kinds: [], stages: null, palettes: null },
		{
			scenes: [
				null,
				{
					name: 7,
					audio: [{ file: '../bad' }, { file: 'ok.mp3', start: 2.5, gain: 99, muted: 'yes', name: null }, 5],
					markers: [{ frame: -1 }, { frame: 2.5 }, { frame: 4, label: 9 }, null],
					cuts: [
						null,
						{
							name: null,
							camera: { x: 0, y: 0, width: 0, height: 5 },
							action: { '-1': 'no', '2.5': 'no', '3': '', '07': 'x', ' 8 ': 'y', '1e1': 'z', '': 'e' },
							dialogue: ['zero', 'one'],
							transition: 'wipe',
							tracks: {
								'aaaaaaaaaaaa:1': [{ d: 'aaaaaaaaaaaa:11', n: 0 }, { d: 'bad', n: 2000 }, null, { n: 2.5 }],
								'aaaaaaaaaaaa:7': [{ d: 'aaaaaaaaaaaa:3', n: 1 }],
								'aaaaaaaaaaaa:2': 'nope'
							}
						}
					]
				}
			],
			statics: ['aaaaaaaaaaaa:2', 'zzz', 5],
			mouths: [{ lid: 'aaaaaaaaaaaa:1', shapes: { A: 'aaaaaaaaaaaa:11', Q: 'aaaaaaaaaaaa:12', B: '' } }, { lid: 5 }],
			kinds: { 'aaaaaaaaaaaa:11': 'key', 'aaaaaaaaaaaa:12': 'nope', bad: 'key' },
			stages: { 'aaaaaaaaaaaa:1': 'clean', 'aaaaaaaaaaaa:2': 'nope' },
			palettes: [{ name: '  ', colours: [{ hex: '#ABCDEF' }, { hex: 'red' }, { name: 'x'.repeat(50), hex: '#000000' }] }, null, 5, []]
		},
		{ scenes: [{ cuts: [{ tracks: {} }] }], stages: 5 }
	];
	const fromSharedOut = odd.map((tl) => {
		let next = 500;
		const made: Record<string, number> = {};
		const back = fromShared(
			tl as never,
			(l) => ({ 'aaaaaaaaaaaa:1': 1, 'aaaaaaaaaaaa:2': 2 })[l as string],
			(d) => (made[d] ??= next++),
			local,
			(l) => l === 1 || l === 2
		);
		return { tl, back, made };
	});

	const chats = [
		{ k: 'chat', text: ' hi ' },
		{ k: 'chat', text: 'a\u0000b\u0007c\td\ne\u007f' },
		{ k: 'chat', text: 'x'.repeat(900) },
		{ k: 'chat', text: '😀'.repeat(300) },
		{ k: 'chat', text: 5 },
		null
	].map((b) => ({ body: b, text: chatText(b) }));
	const typed = [' hello ', '   ', 'y'.repeat(600)].map((t) => ({ typed: t, body: chatBody(t) }));
	const colours = [MEMBERS.A, MEMBERS.B, MEMBERS.C, 'abc', ''].map((id) => ({ id, colour: memberColour(id) }));
	const invites = ['join me:\n pp1-eyJw_b3-xyz  thanks!', 'pp1-', 'hello', 'xpp1-ab.cd'].map((t) => ({
		text: t,
		code: cleanInvite(t)
	}));
	return {
		parsed,
		strokes,
		all,
		allBack,
		lineEdits: edits,
		applied,
		withBase: withBaseEdits,
		bigParts: bigParts.map((p) => p.length),
		bigPartsHead: bigParts.map((p) => p.slice(0, 64)),
		bigEdits,
		tiles,
		decoded,
		toShared: toSharedOut,
		fromShared: fromSharedOut,
		chats,
		typed,
		colours,
		invites
	};
}

const scenarios = JSON.parse(readFileSync(new URL('scenarios.json', here), 'utf8')) as Record<
	string,
	Step[]
>;
const only = process.argv[2];
for (const [name, steps] of Object.entries(scenarios)) {
	if (only && only !== name) continue;
	const r = await run(steps);
	writeFileSync(new URL(`${name}.json`, out), JSON.stringify({ steps, ...r }) + '\n');
	console.log(name, r.log.length, 'edits', r.directs.length, 'direct');
}
if (!only || only === 'pure') {
	writeFileSync(new URL('pure.json', out), JSON.stringify(await pure()) + '\n');
	console.log('pure');
}
