import init, { PaintBrush, PaintCanvas } from 'pencilpilot-brush-wasm';
import { applyTweaks, customBrush, isCustom } from './custom-brushes';

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

/** A built-in brush's .myb source (fetched once). */
export function brushSource(id: string): Promise<string> {
	let src = sources.get(id);
	if (!src) {
		src = fetch(`/brushes/${id}.myb`).then((r) => {
			if (!r.ok) throw new Error(`brush ${id}: HTTP ${r.status}`);
			return r.text();
		});
		sources.set(id, src);
	}
	return src;
}

/**
 * Fetch a brush's .myb and build an engine brush (each call returns a fresh
 * instance). "My brushes" (ids starting `my-`) are a built-in brush plus tweaks,
 * or an imported brush's own settings. `edit` changes the source first (the
 * Smudge tool turns any brush into a smudging one).
 */
export async function loadBrush(id: string, edit?: (myb: string) => string): Promise<PaintBrush> {
	await loadEngine();
	let src: string;
	if (isCustom(id)) {
		const c = customBrush(id);
		if (!c) throw new Error(`brush ${id}: not in My brushes on this device`);
		src = applyTweaks(c.source ?? (await brushSource(c.base)), c.tweaks);
	} else src = await brushSource(id);
	return new PaintBrush(edit ? edit(src) : src);
}

/** An engine brush straight from .myb source (the brush editor's preview). */
export async function brushFromSource(myb: string): Promise<PaintBrush> {
	await loadEngine();
	return new PaintBrush(myb);
}

export const previewUrl = (id: string) => `/brushes/${id}.png`;

export { PaintBrush, PaintCanvas };

/** Default brush shown first. */
export const DEFAULT_BRUSH = 'deevad-2b-pencil';

/** A brush's display name: built-in from the manifest, or one of "My brushes". */
export const brushLabel = (manifest: BrushManifest | null | undefined, id: string) =>
	(isCustom(id) ? customBrush(id)?.name : manifest?.brushes.find((b) => b.id === id)?.name) ?? '…';
