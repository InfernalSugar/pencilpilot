/**
 * PNGs the window makes (a picture, a PNG sequence) say what they are, as the app's own
 * do: their colours are sRGB (an `sRGB` chunk, unless they already name a colour
 * profile), and text about the work (software, author, credits) as `tEXt` chunks.
 */

const SIG = [137, 80, 78, 71, 13, 10, 26, 10];

let table: Uint32Array | null = null;
function crc32(bytes: Uint8Array): number {
	if (!table) {
		table = new Uint32Array(256);
		for (let n = 0; n < 256; n++) {
			let c = n;
			for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
			table[n] = c >>> 0;
		}
	}
	let c = 0xffffffff;
	for (const b of bytes) c = table[(c ^ b) & 255] ^ (c >>> 8);
	return (c ^ 0xffffffff) >>> 0;
}

function chunk(type: string, data: Uint8Array): Uint8Array {
	const out = new Uint8Array(12 + data.length);
	const v = new DataView(out.buffer);
	v.setUint32(0, data.length);
	for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
	out.set(data, 8);
	v.setUint32(8 + data.length, crc32(out.subarray(4, 8 + data.length)));
	return out;
}

/** Latin-1 bytes (PNG text's own); other characters become '?'. */
const latin1 = (s: string) =>
	Uint8Array.from(s, (c) => (c.charCodeAt(0) < 256 ? c.charCodeAt(0) : 63));

/** A PNG's chunk types, in order. */
export function chunkTypes(png: Uint8Array): string[] {
	const v = new DataView(png.buffer, png.byteOffset, png.byteLength);
	const out: string[] = [];
	for (let at = 8; at + 8 <= png.length;) {
		const len = v.getUint32(at);
		out.push(String.fromCharCode(...png.subarray(at + 4, at + 8)));
		at += 12 + len;
	}
	return out;
}

/** `png` with an sRGB chunk (unless it names its colours already) and these texts. */
export function tagPng(png: Uint8Array, text: [string, string][] = []): Uint8Array {
	if (png.length < 33 || SIG.some((b, i) => png[i] !== b)) return png;
	const types = chunkTypes(png);
	const extra: Uint8Array[] = [];
	// (0 = perceptual)
	if (!types.some((t) => t === 'sRGB' || t === 'iCCP')) extra.push(chunk('sRGB', Uint8Array.of(0)));
	for (const [k, v] of text) {
		const key = latin1(k.replace(/[^\x20-\x7e]/g, '').slice(0, 79));
		if (!key.length) continue;
		const data = new Uint8Array(key.length + 1 + v.length);
		data.set(key);
		data.set(latin1(v), key.length + 1);
		extra.push(chunk('tEXt', data));
	}
	if (!extra.length) return png;
	// after the header (signature + IHDR: 8 + 25 bytes)
	const head = 8 + 12 + new DataView(png.buffer, png.byteOffset).getUint32(8);
	const out = new Uint8Array(png.length + extra.reduce((n, c) => n + c.length, 0));
	out.set(png.subarray(0, head));
	let at = head;
	for (const c of extra) {
		out.set(c, at);
		at += c.length;
	}
	out.set(png.subarray(head), at);
	return out;
}

/** The text an export carries: the software, who made it, and who drew it with them. */
export function exportText(opts: {
	on: boolean;
	version?: string;
	author: string;
	/** others who drew it together */
	with: string[];
	when?: Date;
}): [string, string][] {
	if (!opts.on) return [];
	const out: [string, string][] = [
		['Software', `PencilPilot${opts.version ? ` ${opts.version}` : ''}`]
	];
	const author = opts.author.trim();
	if (author) out.push(['Author', author]);
	const others = opts.with.map((n) => n.trim()).filter((n) => n && n !== author);
	if (others.length)
		out.push(['Credits', `Drawn together by ${[author, ...others].filter(Boolean).join(', ')}`]);
	out.push(['Creation Time', (opts.when ?? new Date()).toISOString()]);
	return out;
}
