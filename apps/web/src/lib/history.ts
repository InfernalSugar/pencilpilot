/**
 * Drawing history: the last 10 drawings, kept in this browser (IndexedDB) so
 * they can be opened again to keep drawing, or downloaded. Each keeps its
 * layers (pixels as PNG, masks, groups, settings), strokes and reference.
 */
import type { Reference } from './references';
import type { Stroke } from '@pencilpilot/paint/strokes';

export const HISTORY_SIZE = 10;

export interface SavedLayer {
	id: number;
	name: string;
	opacity: number;
	blend: string;
	visible: boolean;
	locked: boolean;
	alphaLock: boolean;
	clip: boolean;
	imported: boolean;
	group: number;
	/** the layer's own pixels */
	png: Blob;
	/** greyscale mask (255 = shown), if the layer has one */
	mask?: Blob;
}

export interface SavedGroup {
	id: number;
	name: string;
	visible: boolean;
	opacity: number;
}

export interface SavedDrawing {
	id: string;
	savedAt: number;
	title: string;
	/** handed in (vs a draft saved while drawing) */
	submitted: boolean;
	score?: number;
	width: number;
	height: number;
	/** small preview (data URL) */
	thumb: string;
	/** the flattened picture on paper */
	png: Blob;
	layers: SavedLayer[];
	groups: SavedGroup[];
	strokes: Stroke[];
	reference?: Reference;
	/** where it was drawn from, to reopen it the same way */
	category?: string;
	mode?: 'finished' | 'sketch';
	traced?: boolean;
}

const DB = 'pencilpilot';
const STORE = 'drawings';

function open(): Promise<IDBDatabase> {
	return new Promise((ok, bad) => {
		const req = indexedDB.open(DB, 1);
		req.onupgradeneeded = () => {
			if (!req.result.objectStoreNames.contains(STORE))
				req.result.createObjectStore(STORE, { keyPath: 'id' });
		};
		req.onsuccess = () => ok(req.result);
		req.onerror = () => bad(req.error);
	});
}

function tx<T>(mode: IDBTransactionMode, run: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
	return open().then(
		(db) =>
			new Promise<T>((ok, bad) => {
				const t = db.transaction(STORE, mode);
				const req = run(t.objectStore(STORE));
				t.oncomplete = () => {
					db.close();
					ok(req.result);
				};
				t.onerror = () => {
					db.close();
					bad(t.error);
				};
			})
	);
}

/** All saved drawings, newest first. */
export async function listDrawings(): Promise<SavedDrawing[]> {
	try {
		const all = await tx('readonly', (s) => s.getAll() as IDBRequest<SavedDrawing[]>);
		return all.sort((a, b) => b.savedAt - a.savedAt);
	} catch {
		return [];
	}
}

export const getDrawing = (id: string) =>
	tx('readonly', (s) => s.get(id) as IDBRequest<SavedDrawing | undefined>).catch(() => undefined);

export const deleteDrawing = (id: string) =>
	tx('readwrite', (s) => s.delete(id)).then(
		() => true,
		() => false
	);

/** Save (or update) a drawing, keeping only the newest `HISTORY_SIZE`. */
export async function saveDrawing(d: SavedDrawing): Promise<boolean> {
	try {
		await tx('readwrite', (s) => s.put(d));
		const all = await listDrawings();
		for (const old of all.slice(HISTORY_SIZE)) await deleteDrawing(old.id);
		return true;
	} catch {
		return false;
	}
}

/** Note the score once a handed-in drawing has been graded. */
export async function setScore(id: string, score: number) {
	const d = await getDrawing(id);
	if (d) await saveDrawing({ ...d, score, submitted: true });
}

// ---- pixels ↔ PNG ----

export async function rgbaToPng(rgba: Uint8Array | Uint8ClampedArray, w: number, h: number) {
	const c = new OffscreenCanvas(w, h);
	c.getContext('2d')!.putImageData(new ImageData(new Uint8ClampedArray(rgba), w, h), 0, 0);
	return c.convertToBlob({ type: 'image/png' });
}

export async function grayToPng(gray: Uint8Array, w: number, h: number) {
	const rgba = new Uint8ClampedArray(w * h * 4);
	for (let i = 0; i < gray.length; i++) rgba.set([gray[i], gray[i], gray[i], 255], i * 4);
	return rgbaToPng(rgba, w, h);
}

export async function pngToRgba(png: Blob, w: number, h: number): Promise<Uint8Array> {
	const bmp = await createImageBitmap(png, {
		premultiplyAlpha: 'none',
		colorSpaceConversion: 'none'
	});
	const c = new OffscreenCanvas(w, h);
	const g = c.getContext('2d')!;
	g.drawImage(bmp, 0, 0);
	bmp.close();
	return new Uint8Array(g.getImageData(0, 0, w, h).data.buffer);
}

export async function pngToGray(png: Blob, w: number, h: number): Promise<Uint8Array> {
	const rgba = await pngToRgba(png, w, h);
	const gray = new Uint8Array(w * h);
	for (let i = 0; i < gray.length; i++) gray[i] = rgba[i * 4];
	return gray;
}
