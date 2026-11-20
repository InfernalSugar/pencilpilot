/** Colour harmonies for the colour panel (as in Procreate): colours that go with the current one. */

export const HARMONIES = [
	{ id: 'complementary', name: 'Complementary', hint: 'the opposite hue: strong contrast' },
	{ id: 'analogous', name: 'Analogous', hint: 'the neighbours: calm and natural' },
	{ id: 'triadic', name: 'Triadic', hint: 'three hues evenly spaced: lively' },
	{ id: 'split', name: 'Split', hint: 'the two hues beside the opposite: contrast, softer' },
	{ id: 'shades', name: 'Shades', hint: 'the same hue lighter and darker: for shading' }
] as const;
export type Harmony = (typeof HARMONIES)[number]['id'];

function toHsv(hex: string): [number, number, number] {
	const n = parseInt(hex.slice(1), 16) || 0;
	const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => v / 255);
	const max = Math.max(r, g, b);
	const d = max - Math.min(r, g, b);
	let h = 0;
	if (d) {
		if (max === r) h = ((g - b) / d) % 6;
		else if (max === g) h = (b - r) / d + 2;
		else h = (r - g) / d + 4;
	}
	return [(h * 60 + 360) % 360, max ? d / max : 0, max];
}

function toHex(h: number, s: number, v: number): string {
	h = ((h % 360) + 360) % 360;
	const c = v * s;
	const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
	const m = v - c;
	const [r, g, b] =
		h < 60
			? [c, x, 0]
			: h < 120
				? [x, c, 0]
				: h < 180
					? [0, c, x]
					: h < 240
						? [0, x, c]
						: h < 300
							? [x, 0, c]
							: [c, 0, x];
	return (
		'#' +
		[r, g, b]
			.map((u) =>
				Math.round((u + m) * 255)
					.toString(16)
					.padStart(2, '0')
			)
			.join('')
	);
}

/** The current colour first, then the colours that go with it (no repeats). */
export function harmony(hex: string, kind: Harmony): string[] {
	const [h, s, v] = toHsv(hex);
	const hue = (dh: number) => toHex(h + dh, s, v);
	const out =
		kind === 'complementary'
			? [hue(180)]
			: kind === 'analogous'
				? [hue(-30), hue(30)]
				: kind === 'triadic'
					? [hue(120), hue(240)]
					: kind === 'split'
						? [hue(150), hue(210)]
						: [
								toHex(h, s * 0.35, Math.min(1, v + (1 - v) * 0.7)),
								toHex(h, s * 0.7, Math.min(1, v + (1 - v) * 0.35)),
								toHex(h, Math.min(1, s * 1.05), v * 0.65),
								toHex(h, Math.min(1, s * 1.1), v * 0.35)
							];
	return [...new Set([hex.toLowerCase(), ...out.map((c) => c.toLowerCase())])];
}
