/**
 * Saving and opening `.ppstudio` projects (format: crates/ppstudio/FORMAT.md).
 *
 * The document crosses to the app's Rust side as one packet of raw bytes (layer pixels
 * are far too big for JSON): `"PPS1"`, the header length (u32, little-endian), the
 * header (JSON) and the blobs it points into. src-tauri/src/project.rs is the other end.
 */
import type { GroupInfo, LayerSnapshot } from '@pencilpilot/paint/PaintCanvas.svelte';
import type { Stroke } from '@pencilpilot/paint/strokes';

export const EXTENSION = 'ppstudio';

/** Everything a project holds that the window knows about. */
export interface ProjectDoc {
	width: number;
	height: number;
	fps: number;
	layers: LayerSnapshot[];
	groups: GroupInfo[];
	activeLayer: number;
	strokes: Stroke[];
	/** the app's own settings (brush, colour…) */
	extra: Record<string, unknown>;
	/** when the project was first saved (kept across saves) */
	created?: string;
	/** PNG for the project browser */
	thumbnail?: Uint8Array | null;
}

interface PacketLayer {
	id: number;
	name: string;
	visible: boolean;
	opacity: number;
	blend: string;
	locked: boolean;
	alphaLock: boolean;
	clip: boolean;
	imported: boolean;
	group: number;
	adjust?: string | null;
	rgba: [number, number];
	mask?: [number, number] | null;
}

interface Header {
	canvas: { width: number; height: number };
	fps: number;
	layers: PacketLayer[];
	groups: GroupInfo[];
	activeLayer: number;
	strokes: Stroke[];
	extra: Record<string, unknown>;
	created?: string | null;
	thumbnail?: [number, number] | null;
}

const MAGIC = [0x50, 0x50, 0x53, 0x31]; // "PPS1"

/** The document as a packet. */
export function pack(doc: ProjectDoc): Uint8Array {
	const blobs: Uint8Array[] = [];
	let at = 0;
	const put = (b: Uint8Array): [number, number] => {
		blobs.push(b);
		const r: [number, number] = [at, b.length];
		at += b.length;
		return r;
	};
	const header: Header = {
		canvas: { width: doc.width, height: doc.height },
		fps: doc.fps,
		layers: doc.layers.map((l) => ({
			id: l.id,
			name: l.name,
			visible: l.visible,
			opacity: l.opacity,
			blend: l.blend,
			locked: l.locked,
			alphaLock: l.alphaLock,
			clip: l.clip,
			imported: l.imported,
			group: l.group,
			adjust: l.adjust ?? null,
			rgba: put(l.rgba),
			mask: l.mask ? put(l.mask) : null
		})),
		groups: doc.groups,
		activeLayer: doc.activeLayer,
		strokes: doc.strokes,
		extra: doc.extra,
		created: doc.created ?? null,
		thumbnail: doc.thumbnail ? put(doc.thumbnail) : null
	};
	const json = new TextEncoder().encode(JSON.stringify(header));
	const out = new Uint8Array(8 + json.length + at);
	out.set(MAGIC, 0);
	new DataView(out.buffer).setUint32(4, json.length, true);
	out.set(json, 8);
	let o = 8 + json.length;
	for (const b of blobs) {
		out.set(b, o);
		o += b.length;
	}
	return out;
}

/** A packet back into the document. */
export function unpack(packet: ArrayBuffer | Uint8Array): ProjectDoc {
	const bytes = packet instanceof Uint8Array ? packet : new Uint8Array(packet);
	if (bytes.length < 8 || MAGIC.some((m, i) => bytes[i] !== m))
		throw new Error('not a project packet');
	const n = new DataView(bytes.buffer, bytes.byteOffset).getUint32(4, true);
	if (8 + n > bytes.length) throw new Error('the packet ends inside its header');
	const h = JSON.parse(new TextDecoder().decode(bytes.subarray(8, 8 + n))) as Header;
	const blobs = bytes.subarray(8 + n);
	const get = ([o, len]: [number, number]) => {
		if (o + len > blobs.length) throw new Error('a layer points past the end of the packet');
		return blobs.slice(o, o + len);
	};
	return {
		width: h.canvas.width,
		height: h.canvas.height,
		fps: h.fps,
		layers: h.layers.map((l) => ({
			id: l.id,
			name: l.name,
			visible: l.visible,
			opacity: l.opacity,
			blend: l.blend,
			locked: l.locked,
			alphaLock: l.alphaLock,
			clip: l.clip,
			imported: l.imported,
			group: l.group,
			adjust: l.adjust ?? null,
			rgba: get(l.rgba),
			mask: l.mask ? get(l.mask) : undefined
		})),
		groups: h.groups ?? [],
		activeLayer: h.activeLayer,
		strokes: h.strokes ?? [],
		extra: h.extra ?? {},
		created: h.created ?? undefined,
		thumbnail: h.thumbnail ? get(h.thumbnail) : null
	};
}

/** The file's name without folders or extension ("Untitled" for none). */
export function projectName(path: string | null): string {
	if (!path) return 'Untitled';
	const base = path.split(/[/\\]/).pop() ?? path;
	return base.replace(/\.ppstudio$/i, '') || 'Untitled';
}

/** Save to `path` (the app adds .ppstudio if missing). Returns where it went. */
export async function saveProject(path: string, doc: ProjectDoc): Promise<string> {
	const { invoke } = await import('@tauri-apps/api/core');
	return invoke<string>('project_save', pack(doc), {
		headers: { path: encodeURIComponent(path) }
	});
}

export async function openProject(path: string): Promise<ProjectDoc> {
	const { invoke } = await import('@tauri-apps/api/core');
	const bytes = await invoke<ArrayBuffer>('project_open', undefined, {
		headers: { path: encodeURIComponent(path) }
	});
	return unpack(bytes);
}

/** Ask where to save (a native dialog); null if cancelled. */
export async function askSavePath(name: string): Promise<string | null> {
	const { save } = await import('@tauri-apps/plugin-dialog');
	return save({
		defaultPath: `${name}.${EXTENSION}`,
		filters: [{ name: 'PencilPilot project', extensions: [EXTENSION] }]
	});
}

/** Ask which project to open; null if cancelled. */
export async function askOpenPath(): Promise<string | null> {
	const { open } = await import('@tauri-apps/plugin-dialog');
	const p = await open({
		multiple: false,
		directory: false,
		filters: [{ name: 'PencilPilot project', extensions: [EXTENSION] }]
	});
	return typeof p === 'string' ? p : null;
}
