/**
 * Frames in, for rotoscoping: an image sequence or a video's frames become drawings on
 * a new layer (under the one you're drawing on, half see-through), one after another
 * from the current frame.
 */
import { spell, unspell } from './frametools';
import type { Track } from './timeline';

/** Picture files sorted as people number them ("walk2" before "walk10"). */
export function naturalOrder<T extends { name: string }>(files: T[]): T[] {
	const c = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
	return [...files].sort((a, b) => c.compare(a.name, b.name));
}

/** Where a picture of `w`×`h` goes inside `W`×`H`: as large as fits, centred. */
export function fitInside(w: number, h: number, W: number, H: number) {
	// (the side that fills is exact, not a rounding of k)
	const wide = W / w <= H / h;
	const [fw, fh] = wide ? [W, (h * W) / w] : [(w * H) / h, H];
	return { x: (W - fw) / 2, y: (H - fh) / 2, width: fw, height: fh };
}

/** Put drawings on a track from `from`, each held `hold` frames (in place). */
export function placeFrames(t: Track, from: number, drawings: number[], hold = 1) {
	const n = Math.max(1, Math.round(hold));
	const f = spell(t, from + drawings.length * n);
	drawings.forEach((d, i) => {
		for (let k = 0; k < n; k++) f[from + i * n + k] = d;
	});
	t.splice(0, t.length, ...unspell(f));
}

/** A layer name from a file name ("walk_0001.png" → "walk"). */
export function sequenceName(file: string): string {
	const base = file.split(/[/\\]/).pop() ?? file;
	return (
		base
			.replace(/\.[^.]+$/, '')
			.replace(/[\s._-]*\d+$/, '')
			.trim() || 'Reference'
	);
}
