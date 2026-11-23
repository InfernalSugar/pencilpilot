import { describe, expect, it } from 'vitest';
import { crc32, deflateSync } from 'node:zlib';
import { chunkTypes, exportText, tagPng } from './png-meta';

/** A 1×1 PNG as a canvas makes one (no colour chunk). */
function tinyPng(): Uint8Array {
	const chunk = (type: string, data: Buffer) => {
		const len = Buffer.alloc(4);
		len.writeUInt32BE(data.length);
		const body = Buffer.concat([Buffer.from(type, 'latin1'), data]);
		const crc = Buffer.alloc(4);
		crc.writeUInt32BE(crc32(body));
		return Buffer.concat([len, body, crc]);
	};
	const ihdr = Buffer.alloc(13);
	ihdr.writeUInt32BE(1, 0);
	ihdr.writeUInt32BE(1, 4);
	ihdr.set([8, 6, 0, 0, 0], 8);
	return new Uint8Array(
		Buffer.concat([
			Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
			chunk('IHDR', ihdr),
			chunk('IDAT', deflateSync(Buffer.from([0, 255, 0, 0, 255]))),
			chunk('IEND', Buffer.alloc(0))
		])
	);
}

/** every chunk's CRC is right, and its texts */
function read(png: Uint8Array) {
	const b = Buffer.from(png);
	const texts: [string, string][] = [];
	for (let at = 8; at < b.length;) {
		const len = b.readUInt32BE(at);
		const type = b.toString('latin1', at + 4, at + 8);
		const data = b.subarray(at + 8, at + 8 + len);
		expect(b.readUInt32BE(at + 8 + len)).toBe(crc32(b.subarray(at + 4, at + 8 + len)));
		if (type === 'tEXt') {
			const z = data.indexOf(0);
			texts.push([data.toString('latin1', 0, z), data.toString('latin1', z + 1)]);
		}
		at += 12 + len;
	}
	return texts;
}

describe('PNG tags', () => {
	it('say sRGB and carry the text, before the pixels', () => {
		const out = tagPng(tinyPng(), [
			['Software', 'PencilPilot'],
			['Author', 'Zoë'],
			['Credits', 'Ana, 李']
		]);
		expect(chunkTypes(out)).toEqual(['IHDR', 'sRGB', 'tEXt', 'tEXt', 'tEXt', 'IDAT', 'IEND']);
		expect(read(out)).toEqual([
			['Software', 'PencilPilot'],
			['Author', 'Zoë'],
			['Credits', 'Ana, ?']
		]);
	});

	it('leave a PNG that names its colours, and anything that isn’t a PNG', () => {
		const once = tagPng(tinyPng());
		expect(chunkTypes(tagPng(once))).toEqual(['IHDR', 'sRGB', 'IDAT', 'IEND']);
		const junk = new Uint8Array([1, 2, 3]);
		expect(tagPng(junk)).toBe(junk);
	});

	it('credit the people who drew it together, or nothing when switched off', () => {
		const when = new Date('2026-11-23T10:00:00Z');
		expect(exportText({ on: false, author: 'Ana', with: ['Bo'] })).toEqual([]);
		expect(
			exportText({ on: true, version: '0.1', author: 'Ana', with: ['Ana', 'Bo'], when })
		).toEqual([
			['Software', 'PencilPilot 0.1'],
			['Author', 'Ana'],
			['Credits', 'Drawn together by Ana, Bo'],
			['Creation Time', '2026-11-23T10:00:00.000Z']
		]);
		expect(exportText({ on: true, author: '', with: [], when }).map(([k]) => k)).toEqual([
			'Software',
			'Creation Time'
		]);
	});
});
