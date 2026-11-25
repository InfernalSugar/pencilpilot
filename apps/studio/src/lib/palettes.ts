/**
 * Character palettes: a project's named sets of named colours (a character's skin, skin
 * shadow, hair…), kept in the timeline (saved with the project, undone, shared when
 * drawing together), and taken from a model sheet: the flat colours of a reference
 * picture, without its paper and lines.
 */

export interface PaletteColour {
	name: string;
	hex: string;
}
export interface Palette {
	name: string;
	colours: PaletteColour[];
}

export const MAX_PALETTES = 32;
export const MAX_COLOURS = 64;
const NAME_MAX = 40;

const HEX = /^#[0-9a-f]{6}$/;
const cleanName = (v: unknown, fallback: string) =>
	(typeof v === 'string' ? v.trim().slice(0, NAME_MAX) : '') || fallback;

/** Palettes as kept (a file or a peer): anything unusable dropped, sizes capped. */
export function readPalettes(raw: unknown): Palette[] {
	if (!Array.isArray(raw)) return [];
	const out: Palette[] = [];
	for (const p of raw.slice(0, MAX_PALETTES)) {
		if (!p || typeof p !== 'object') continue;
		const q = p as { name?: unknown; colours?: unknown };
		const colours: PaletteColour[] = [];
		for (const c of Array.isArray(q.colours) ? q.colours.slice(0, MAX_COLOURS) : []) {
			const hex = typeof c?.hex === 'string' ? c.hex.toLowerCase() : '';
			if (HEX.test(hex)) colours.push({ name: cleanName(c.name, hex), hex });
		}
		out.push({ name: cleanName(q.name, `Palette ${out.length + 1}`), colours });
	}
	return out;
}

// ---- changes: each returns new palettes (the timeline keeps them as given) ----

export function addPalette(
	list: Palette[],
	name: string,
	colours: PaletteColour[] = []
): Palette[] {
	if (list.length >= MAX_PALETTES) return list;
	return [
		...list,
		{ name: cleanName(name, `Palette ${list.length + 1}`), colours: colours.slice(0, MAX_COLOURS) }
	];
}

export function removePalette(list: Palette[], i: number): Palette[] {
	return list.filter((_, k) => k !== i);
}

export function renamePalette(list: Palette[], i: number, name: string): Palette[] {
	return list.map((p, k) => (k === i ? { ...p, name: cleanName(name, p.name) } : p));
}

/** Add a colour to a palette (named "Colour n" unless named). */
export function addColour(list: Palette[], i: number, hex: string, name = ''): Palette[] {
	const h = hex.toLowerCase();
	if (!HEX.test(h)) return list;
	return list.map((p, k) =>
		k === i && p.colours.length < MAX_COLOURS
			? {
					...p,
					colours: [
						...p.colours,
						{ name: cleanName(name, `Colour ${p.colours.length + 1}`), hex: h }
					]
				}
			: p
	);
}

/** Change a palette colour's name and / or colour. */
export function setColour(
	list: Palette[],
	i: number,
	j: number,
	change: Partial<PaletteColour>
): Palette[] {
	return list.map((p, k) =>
		k !== i
			? p
			: {
					...p,
					colours: p.colours.map((c, m) => {
						if (m !== j) return c;
						const hex = change.hex?.toLowerCase();
						return {
							name: change.name === undefined ? c.name : cleanName(change.name, c.name),
							hex: hex && HEX.test(hex) ? hex : c.hex
						};
					})
				}
	);
}

export function removeColour(list: Palette[], i: number, j: number): Palette[] {
	return list.map((p, k) => (k === i ? { ...p, colours: p.colours.filter((_, m) => m !== j) } : p));
}

// ---- from a model sheet: its flat colours ----

const hexOf = (r: number, g: number, b: number) =>
	'#' + [r, g, b].map((v) => Math.round(v).toString(16).padStart(2, '0')).join('');

/**
 * The flat colours of a picture (a model sheet), most used first: pixels of the paper
 * (near white), the lines (near black) and edges (unlike their neighbours: antialiasing
 * between two colours) are left out; close colours count as one; colours covering less
 * than `minShare` of the rest are dropped. At most `max`.
 */
export function flatColours(
	rgba: Uint8Array | Uint8ClampedArray,
	w: number,
	h: number,
	max = 12,
	minShare = 0.004
): string[] {
	const at = (x: number, y: number) => (y * w + x) * 4;
	const near = (i: number, j: number) =>
		Math.abs(rgba[i] - rgba[j]) +
			Math.abs(rgba[i + 1] - rgba[j + 1]) +
			Math.abs(rgba[i + 2] - rgba[j + 2]) <=
		24;
	// buckets of 5 bits a channel: count and sum (for their mean)
	const sums = new Map<number, [number, number, number, number]>();
	let kept = 0;
	for (let y = 1; y < h - 1; y++)
		for (let x = 1; x < w - 1; x++) {
			const i = at(x, y);
			if (rgba[i + 3] < 128) continue;
			const [r, g, b] = [rgba[i], rgba[i + 1], rgba[i + 2]];
			const hi = Math.max(r, g, b);
			const lo = Math.min(r, g, b);
			if (lo > 235) continue; // paper
			if (hi < 45) continue; // lines
			// a flat: like (nearly) all four neighbours
			let like = 0;
			for (const j of [at(x - 1, y), at(x + 1, y), at(x, y - 1), at(x, y + 1)])
				if (near(i, j)) like++;
			if (like < 4) continue;
			const key = ((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3);
			const s = sums.get(key) ?? [0, 0, 0, 0];
			s[0] += r;
			s[1] += g;
			s[2] += b;
			s[3]++;
			sums.set(key, s);
			kept++;
		}
	if (!kept) return [];
	// biggest first, each taking in the buckets close to it
	const buckets = [...sums.values()].sort((a, b) => b[3] - a[3]);
	const used = new Array(buckets.length).fill(false);
	const out: { hex: string; n: number }[] = [];
	for (let a = 0; a < buckets.length && out.length < max; a++) {
		if (used[a]) continue;
		const [r0, g0, b0, n0] = buckets[a];
		const c0 = [r0 / n0, g0 / n0, b0 / n0];
		const sum = [r0, g0, b0];
		let n = n0;
		for (let b = a + 1; b < buckets.length; b++) {
			if (used[b]) continue;
			const [r, g, bl, k] = buckets[b];
			const d = Math.hypot(r / k - c0[0], g / k - c0[1], bl / k - c0[2]);
			if (d > 28) continue;
			used[b] = true;
			sum[0] += r;
			sum[1] += g;
			sum[2] += bl;
			n += k;
		}
		if (n / kept < minShare) continue;
		out.push({ hex: hexOf(sum[0] / n, sum[1] / n, sum[2] / n), n });
	}
	return out.sort((a, b) => b.n - a.n).map((c) => c.hex);
}
