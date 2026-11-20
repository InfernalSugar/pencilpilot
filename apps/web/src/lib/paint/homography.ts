/**
 * Projective maps for the Distort transform: the lifted box's four corners are
 * pinned to any four points (perspective, skew, free distort).
 * Matrices are row-major 3×3: dest = (h0 x + h1 y + h2, h3 x + h4 y + h5) / (h6 x + h7 y + h8).
 */
export type Pt = [number, number];
export type Quad = [Pt, Pt, Pt, Pt];
export type H = [number, number, number, number, number, number, number, number, number];

export function mul(a: H, b: H): H {
	const o = new Array(9).fill(0) as H;
	for (let r = 0; r < 3; r++)
		for (let c = 0; c < 3; c++)
			for (let k = 0; k < 3; k++) o[r * 3 + c] += a[r * 3 + k] * b[k * 3 + c];
	return o;
}

/** `h` applied to p; null behind the "camera" (w ≤ 0). */
export function apply(h: H, p: Pt): Pt | null {
	const w = h[6] * p[0] + h[7] * p[1] + h[8];
	if (w <= 1e-9) return null;
	return [(h[0] * p[0] + h[1] * p[1] + h[2]) / w, (h[3] * p[0] + h[4] * p[1] + h[5]) / w];
}

/** The affine matrix [a, b, c, d, e, f] (dest = a x + c y + e, b x + d y + f) as a 3×3. */
export const fromAffine = ([a, b, c, d, e, f]: number[]): H => [a, c, e, b, d, f, 0, 0, 1];

/**
 * The map taking the box [x0, y0, x1, y1]'s corners (top-left, top-right,
 * bottom-right, bottom-left) to `q`, in that order (Heckbert's square-to-quad).
 */
export function boxToQuad(box: [number, number, number, number], q: Quad): H {
	const [[x0, y0], [x1, y1], [x2, y2], [x3, y3]] = q;
	const sx = x0 - x1 + x2 - x3;
	const sy = y0 - y1 + y2 - y3;
	let sq: H;
	if (Math.abs(sx) < 1e-9 && Math.abs(sy) < 1e-9) {
		sq = [x1 - x0, x3 - x0, x0, y1 - y0, y3 - y0, y0, 0, 0, 1];
	} else {
		const [dx1, dx2, dy1, dy2] = [x1 - x2, x3 - x2, y1 - y2, y3 - y2];
		const den = dx1 * dy2 - dx2 * dy1;
		const g = (sx * dy2 - dx2 * sy) / den;
		const h = (dx1 * sy - sx * dy1) / den;
		sq = [x1 - x0 + g * x1, x3 - x0 + h * x3, x0, y1 - y0 + g * y1, y3 - y0 + h * y3, y0, g, h, 1];
	}
	const [bx0, by0, bx1, by1] = box;
	const [w, ht] = [Math.max(1e-9, bx1 - bx0), Math.max(1e-9, by1 - by0)];
	return mul(sq, [1 / w, 0, -bx0 / w, 0, 1 / ht, -by0 / ht, 0, 0, 1]);
}

/** A quad the transform can show: convex, not folded over, with some area. */
export function usableQuad(q: Quad): boolean {
	let sign = 0;
	for (let i = 0; i < 4; i++) {
		const [a, b, c] = [q[i], q[(i + 1) % 4], q[(i + 2) % 4]];
		const cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
		if (Math.abs(cross) < 1e-6) return false;
		const s = Math.sign(cross);
		if (sign && s !== sign) return false;
		sign = s;
	}
	return true;
}
