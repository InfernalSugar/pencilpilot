import init, { PaintBrush, PaintCanvas } from 'pencilpilot-brush-wasm';

export interface BrushInfo {
	id: string;
	name: string;
	author: string;
	category: string;
	preview: boolean;
}

export interface BrushManifest {
	source: string;
	license: string;
	categories: { id: string; name: string }[];
	brushes: BrushInfo[];
}

let engine: Promise<WebAssembly.Memory> | undefined;

/** Load the WASM paint engine once; resolves to its memory (for zero-copy pixel reads). */
export function loadEngine(): Promise<WebAssembly.Memory> {
	engine ??= init().then((out) => out.memory);
	return engine;
}

let manifest: Promise<BrushManifest> | undefined;

export function loadManifest(fetcher: typeof fetch = fetch): Promise<BrushManifest> {
	manifest ??= fetcher('/brushes/manifest.json').then((r) => {
		if (!r.ok) throw new Error(`brush manifest: HTTP ${r.status}`);
		return r.json();
	});
	return manifest;
}

const sources = new Map<string, Promise<string>>();

/** Fetch a brush's .myb and build an engine brush (each call returns a fresh instance). */
export async function loadBrush(id: string): Promise<PaintBrush> {
	await loadEngine();
	let src = sources.get(id);
	if (!src) {
		src = fetch(`/brushes/${id}.myb`).then((r) => {
			if (!r.ok) throw new Error(`brush ${id}: HTTP ${r.status}`);
			return r.text();
		});
		sources.set(id, src);
	}
	return new PaintBrush(await src);
}

export const previewUrl = (id: string) => `/brushes/${id}.png`;

export { PaintBrush, PaintCanvas };

/** Default brush shown first. */
export const DEFAULT_BRUSH = 'deevad-2b-pencil';
