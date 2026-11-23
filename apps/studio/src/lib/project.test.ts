import { describe, expect, it } from 'vitest';
import {
	ago,
	pack,
	parseStart,
	projectName,
	studioQuery,
	unpack,
	type ProjectDoc
} from './project';
import { NO_STYLE } from '@pencilpilot/paint/layer-style';

const doc = (): ProjectDoc => ({
	width: 4,
	height: 2,
	fps: 24,
	layers: [
		{
			id: 1,
			name: 'Ink',
			opacity: 0.8,
			blend: 'multiply',
			visible: true,
			locked: false,
			alphaLock: true,
			clip: false,
			imported: false,
			group: 2,
			rgba: Uint8Array.from({ length: 32 }, (_, i) => i),
			mask: Uint8Array.from([255, 0, 255, 0, 255, 0, 255, 0]),
			adjust: null,
			style: {
				...NO_STYLE,
				stroke: { on: true, size: 3, colour: '#102030', position: 'outside', opacity: 1 }
			}
		},
		{
			id: 5,
			name: 'Curves',
			opacity: 1,
			blend: 'normal',
			visible: true,
			locked: false,
			alphaLock: false,
			clip: false,
			imported: false,
			group: 0,
			rgba: new Uint8Array(32),
			adjust: '{"hue":10}'
		}
	],
	groups: [{ id: 2, name: 'Character', visible: true, opacity: 1 }],
	activeLayer: 5,
	strokes: [{ brushId: 'pen', eraser: false, points: [{ x: 1, y: 2, pressure: 0.5, t: 0 }] }],
	extra: { color: '#ff0000' },
	created: '2026-11-21T00:00:00Z',
	thumbnail: Uint8Array.from([137, 80, 78, 71]),
	scenes: [
		{
			name: 'Scene 1',
			cuts: [
				{
					name: 'Cut 1',
					camera: { x: 0, y: 0, width: 4, height: 2 },
					tracks: {
						1: [
							{ drawing: 'D3', length: 2 },
							{ drawing: null, length: 1 },
							{ drawing: 'D4', length: 1 }
						]
					}
				}
			]
		}
	],
	drawings: [
		{ id: 3, layer: 1, rgba: Uint8Array.from({ length: 32 }, (_, i) => i) },
		{ id: 4, layer: 1, rgba: Uint8Array.from({ length: 32 }, (_, i) => 31 - i) }
	],
	statics: [5]
});

describe('project packets', () => {
	it('round-trip a document, pixels and all', () => {
		const back = unpack(pack(doc()));
		expect(back).toEqual(doc());
	});

	it('keep drawing assist on its layers (with the app’s settings)', () => {
		const d = doc();
		d.layers[0].assist = true;
		const back = unpack(pack(d));
		expect(back.layers.map((l) => !!l.assist)).toEqual([true, false]);
		expect(back.extra).toEqual({ color: '#ff0000', assistLayers: [1] });
	});

	it('round-trip text layers and the fonts they use', () => {
		const d = doc();
		d.layers[0].text = '{"text":"Hello","font":"My Hand","size":40}';
		d.fonts = [
			{ family: 'My Hand', file: 'MyHand.woff2', bytes: Uint8Array.from([119, 79, 70, 50]) }
		];
		const back = unpack(pack(d));
		expect(back).toEqual(d);
		expect(back.layers[1].text).toBeUndefined();
	});

	it('round-trip line layers and their drawings’ strokes (old files have none)', () => {
		const d = doc();
		d.layers[0].lines = true;
		d.drawings![0].lines = Uint8Array.from([0x50, 0x50, 0x4c, 0x31, 9, 9]);
		const back = unpack(pack(d));
		expect(back).toEqual(d);
		expect(back.layers[1].lines).toBeUndefined();
		expect(back.drawings![1].lines).toBeUndefined();
	});

	it('start with the magic word and the header length', () => {
		const p = pack(doc());
		expect([...p.subarray(0, 4)]).toEqual([0x50, 0x50, 0x53, 0x31]);
		const n = new DataView(p.buffer).getUint32(4, true);
		expect(JSON.parse(new TextDecoder().decode(p.subarray(8, 8 + n))).canvas).toEqual({
			width: 4,
			height: 2
		});
	});

	it('refuse junk and cut-off packets', () => {
		expect(() => unpack(new Uint8Array([1, 2, 3]))).toThrow(/not a project/);
		const p = pack(doc());
		expect(() => unpack(p.subarray(0, p.length - 20))).toThrow(/past the end/);
	});

	it('name projects after their file', () => {
		expect(projectName(null)).toBe('Untitled');
		expect(projectName('/home/me/Walk cycle.ppstudio')).toBe('Walk cycle');
		expect(projectName('C:\\art\\Fox.PPSTUDIO')).toBe('Fox');
	});
});

describe('times', () => {
	it('read like people talk', () => {
		const now = Date.parse('2026-11-21T12:00:00Z');
		expect(ago('2026-11-21T11:59:30Z', now)).toBe('just now');
		expect(ago('2026-11-21T11:45:00Z', now)).toBe('15 min ago');
		expect(ago('2026-11-21T09:00:00Z', now)).toBe('3 h ago');
		expect(ago('junk', now)).toBe('');
	});
});

describe('the way into the studio', () => {
	it('round-trips every kind of start', () => {
		const starts = [
			{ kind: 'new', width: 1280, height: 720, fps: 12 },
			{ kind: 'open', path: '/home/me/A cat & a dog.ppstudio' },
			{ kind: 'recover', id: 'doc-1', path: '/home/me/Fox.ppstudio' },
			{ kind: 'recover', id: 'doc-2', path: null }
		] as const;
		for (const s of starts)
			expect(parseStart(new URLSearchParams(studioQuery(s).slice(1)))).toEqual(s);
	});

	it('falls back to an HD canvas, and keeps sizes in range', () => {
		const hd = { kind: 'new', width: 1920, height: 1080, fps: 24 };
		expect(parseStart(new URLSearchParams(''))).toEqual(hd);
		expect(parseStart(new URLSearchParams('new=junk'))).toEqual(hd);
		expect(parseStart(new URLSearchParams('recover=../x'))).toEqual(hd);
		expect(parseStart(new URLSearchParams('new=99999x2&fps=500'))).toEqual({
			kind: 'new',
			width: 8192,
			height: 64,
			fps: 24
		});
	});
});
