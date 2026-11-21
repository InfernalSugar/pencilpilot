import { describe, expect, it } from 'vitest';
import { pack, projectName, unpack, type ProjectDoc } from './project';

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
			adjust: null
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
	thumbnail: Uint8Array.from([137, 80, 78, 71])
});

describe('project packets', () => {
	it('round-trip a document, pixels and all', () => {
		const back = unpack(pack(doc()));
		expect(back).toEqual(doc());
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
