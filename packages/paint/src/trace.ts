/** Tracing: placing a picture over the canvas, and loading the artist's own pictures. */

/** Where a tracing picture sits on the canvas, on top of the usual "fit inside" placement. */
export interface TraceTransform {
	/** offset as a fraction of the canvas width / height */
	x: number;
	y: number;
	scale: number;
	/** degrees */
	rotate: number;
}

export const NO_TRANSFORM: TraceTransform = { x: 0, y: 0, scale: 1, rotate: 0 };

/**
 * Draw a picture onto a w×h white canvas exactly as the tracing overlay shows it
 * (fitted inside, then moved / scaled / rotated). Returns the pixels and a URL for display.
 */
export async function placedPixels(
	src: string,
	w: number,
	h: number,
	t: TraceTransform = NO_TRANSFORM
): Promise<{ data: ImageData; url: string }> {
	const img = new Image();
	img.src = src;
	await img.decode();
	const c = document.createElement('canvas');
	c.width = w;
	c.height = h;
	const ctx = c.getContext('2d')!;
	ctx.fillStyle = '#fff';
	ctx.fillRect(0, 0, w, h);
	const s = Math.min(w / img.naturalWidth, h / img.naturalHeight);
	const [dw, dh] = [img.naturalWidth * s, img.naturalHeight * s];
	ctx.translate(w / 2 + t.x * w, h / 2 + t.y * h);
	ctx.rotate((t.rotate * Math.PI) / 180);
	ctx.scale(t.scale, t.scale);
	ctx.drawImage(img, -dw / 2, -dh / 2, dw, dh);
	return { data: ctx.getImageData(0, 0, w, h), url: c.toDataURL('image/jpeg', 0.9) };
}

/** A picture from the student's device, shrunk for tracing. It never leaves the browser. */
export async function loadOwnPicture(file: File, maxSide = 1280): Promise<string> {
	const bmp = await createImageBitmap(file);
	const k = Math.min(1, maxSide / Math.max(bmp.width, bmp.height));
	const c = document.createElement('canvas');
	c.width = Math.max(1, Math.round(bmp.width * k));
	c.height = Math.max(1, Math.round(bmp.height * k));
	const ctx = c.getContext('2d')!;
	ctx.fillStyle = '#fff';
	ctx.fillRect(0, 0, c.width, c.height);
	ctx.drawImage(bmp, 0, 0, c.width, c.height);
	bmp.close();
	return c.toDataURL('image/jpeg', 0.88);
}
