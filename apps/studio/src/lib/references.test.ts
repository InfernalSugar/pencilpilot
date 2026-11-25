import { describe, expect, it } from 'vitest';
import {
	canvasFromPicture,
	gridOnPicture,
	mimeOf,
	pictureOnCanvas,
	REF_MAX_BYTES,
	RefShelf,
	skippedNote
} from './references.svelte';

const bytes = (...v: number[]) => Uint8Array.from(v);

/** a shelf that reads any bytes starting with 1 as a 10×20 picture */
function shelf() {
	const dropped: string[] = [];
	let n = 0;
	const s = new RefShelf(
		(_b, file) => `url:${file}:${n++}`,
		(url) => dropped.push(url),
		async (b) => (b[0] === 1 ? { width: 10, height: 20 } : null)
	);
	return { s, dropped };
}

describe('reference pictures', () => {
	it('add pictures in order, the first new one showing', async () => {
		const { s } = shelf();
		const r = await s.add([
			{ name: 'a.png', bytes: bytes(1, 1) },
			{ name: 'b.jpg', bytes: bytes(1, 2) }
		]);
		expect(r).toEqual({ added: 2, skipped: [] });
		expect(s.pics.map((p) => [p.name, p.width, p.height])).toEqual([
			['a.png', 10, 20],
			['b.jpg', 10, 20]
		]);
		expect(s.current).toBe(0);
		await s.add([{ name: 'c.png', bytes: bytes(1, 3) }]);
		expect(s.pic?.name).toBe('c.png');
	});

	it('leave out what isn’t a picture, is too big or is already there', async () => {
		const { s } = shelf();
		await s.add([{ name: 'a.png', bytes: bytes(1, 1) }]);
		const big = new Uint8Array(REF_MAX_BYTES + 1);
		big[0] = 1;
		const r = await s.add([
			{ name: 'notes.txt', bytes: bytes(9, 9) },
			{ name: 'huge.png', bytes: big },
			{ name: 'again.png', bytes: bytes(1, 1) }
		]);
		expect(r.added).toBe(0);
		expect(r.skipped.map((x) => x.why)).toEqual(['not a picture', 'too big', 'already here']);
		expect(s.pics).toHaveLength(1);
		expect(skippedNote(r)).toBe(
			'Left out: huge.png is over 64 MB; notes.txt isn’t a picture the app can read.'
		);
		expect(skippedNote({ added: 1, skipped: [] })).toBe('');
	});

	it('remove a picture, step round them, and keep them for the file', async () => {
		const { s, dropped } = shelf();
		await s.add([
			{ name: 'a.png', bytes: bytes(1, 1) },
			{ name: 'b.png', bytes: bytes(1, 2) },
			{ name: 'c.png', bytes: bytes(1, 3) }
		]);
		s.step(-1);
		expect(s.pic?.name).toBe('c.png');
		s.step(1);
		expect(s.pic?.name).toBe('a.png');
		s.current = 2;
		s.remove(2);
		expect(s.pic?.name).toBe('b.png');
		expect(dropped).toEqual(['url:c.png:2']);
		s.remove(0);
		expect(s.pic?.name).toBe('b.png');
		expect(s.toFile()).toEqual([{ name: 'b.png', file: 'b.png', bytes: bytes(1, 2) }]);
	});

	it('load a project’s pictures in place of the ones showing', async () => {
		const { s, dropped } = shelf();
		await s.add([{ name: 'old.png', bytes: bytes(1, 1) }]);
		await s.load([{ name: 'p.jpg', file: 'p.jpg', bytes: bytes(1, 5) }]);
		expect(dropped).toHaveLength(1);
		expect(s.pics.map((p) => p.name)).toEqual(['p.jpg']);
		await s.load(undefined);
		expect(s.pics).toEqual([]);
	});

	it('know a picture’s type from its name', () => {
		expect(mimeOf('a.JPG')).toBe('image/jpeg');
		expect(mimeOf('a.webp')).toBe('image/webp');
		expect(mimeOf('noext')).toBe('application/octet-stream');
	});

	it('put the canvas grid on the picture where it fits inside the canvas', () => {
		// a 100×50 canvas, a 50×50 picture: fitted, it's 50×50 in the middle (x 25–75)
		const g = gridOnPicture(
			[[50, 0, 50, 50]],
			{ width: 100, height: 50 },
			{ width: 50, height: 50 }
		);
		expect(g).toEqual([[0.5, 0, 0.5, 1]]);
		expect(gridOnPicture([], { width: 1, height: 1 }, { width: 1, height: 1 })).toBeNull();
		expect(
			gridOnPicture([[0, 0, 1, 1]], { width: 1, height: 1 }, { width: 0, height: 0 })
		).toBeNull();
	});

	it('a point drawn on the picture lands where the grid puts it on the canvas', () => {
		// a square picture on a wide canvas: centred, the canvas's height
		const at = pictureOnCanvas({ width: 200, height: 100 }, { width: 400, height: 400 });
		expect(at).toEqual([50, 0, 100, 100]);
		expect(canvasFromPicture(at!, [0, 0])).toEqual([50, 0]);
		expect(canvasFromPicture(at!, [0.5, 0.5])).toEqual([100, 50]);
		expect(canvasFromPicture(at!, [1, 1])).toEqual([150, 100]);
		// past the picture's edge: past it on the canvas too (not squashed onto the edge)
		expect(canvasFromPicture(at!, [-0.25, 1.25])).toEqual([25, 125]);
		// and the grid agrees: the canvas's middle line is the picture's middle
		expect(
			gridOnPicture([[100, 0, 100, 100]], { width: 200, height: 100 }, { width: 400, height: 400 })
		).toEqual([[0.5, 0, 0.5, 1]]);
		expect(pictureOnCanvas({ width: 100, height: 100 }, { width: 0, height: 5 })).toBeNull();
	});
});
