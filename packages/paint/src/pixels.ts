/** Pixel helpers shared by the studio and the grader. */

/** Convert RGBA canvas pixels to grayscale bytes (luma, transparent = white). */
export function toGray(rgba: Uint8ClampedArray): Uint8Array {
	const out = new Uint8Array(rgba.length / 4);
	for (let i = 0; i < out.length; i++) {
		const [r, g, b, a] = [rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2], rgba[i * 4 + 3] / 255];
		const luma = 0.299 * r + 0.587 * g + 0.114 * b;
		out[i] = Math.round(luma * a + 255 * (1 - a));
	}
	return out;
}
