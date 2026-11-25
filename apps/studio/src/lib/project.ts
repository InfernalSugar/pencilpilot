/**
 * Saving and opening `.ppstudio` projects (format: crates/ppstudio/FORMAT.md).
 *
 * The document crosses to the app's Rust side as one packet of raw bytes (layer pixels
 * are far too big for JSON): `"PPS1"`, the header length (u32, little-endian), the
 * header (JSON) and the blobs it points into. src-tauri/src/project.rs is the other end.
 */
import type { GroupInfo, LayerSnapshot } from '@pencilpilot/paint/PaintCanvas.svelte';
import type { Stroke } from '@pencilpilot/paint/strokes';
import { readStyle, type LayerStyle } from '@pencilpilot/paint/layer-style';
import type { FileMouthChart, FileScene } from './timeline';

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
	/** the animation (drawings named "D<id>"); none: one frame of the layers as they are */
	scenes?: FileScene[];
	/** every drawing the scenes show, with the layer it's on */
	drawings?: ProjectDrawing[];
	/** static layers (one drawing all cut long) */
	statics?: number[];
	/** lip-sync mouth charts (their drawings are among `drawings`) */
	mouths?: FileMouthChart[];
	/** drawing marks: "D<id>" → key / breakdown / inbetween */
	kinds?: Record<string, string>;
	/** the sound files the scenes play (the key they use, the file as imported) */
	audio?: { file: string; bytes: Uint8Array }[];
	/** the timelapse: JPEG pictures and when each was taken (ms since 1970) */
	timelapse?: { jpeg: Uint8Array; t: number }[];
	/** font files text layers use: the family they know it by, the file as imported */
	fonts?: { family: string; file: string; bytes: Uint8Array }[];
}

export interface ProjectDrawing {
	id: number;
	layer: number;
	/** straight-alpha RGBA, canvas size; none: it's in the app's frame store */
	rgba?: Uint8Array;
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
	style?: LayerStyle | null;
	text?: string | null;
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
	scenes?: FileScene[];
	staticLayers?: number[];
	mouthCharts?: FileMouthChart[];
	drawingKinds?: Record<string, string>;
	audio?: { file: string; bytes: [number, number] }[];
	drawings?: { id: number; layer: number; rgba?: [number, number] | null }[];
	timelapse?: { t: number; bytes: [number, number] }[];
	fonts?: { family: string; file: string; bytes: [number, number] }[];
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
	const assisted = doc.layers.filter((l) => l.assist).map((l) => l.id);
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
			...(l.style ? { style: l.style } : {}),
			...(l.text ? { text: l.text } : {}),
			rgba: put(l.rgba),
			mask: l.mask ? put(l.mask) : null
		})),
		groups: doc.groups,
		activeLayer: doc.activeLayer,
		strokes: doc.strokes,
		// drawing assist is the app's, not the file format's: kept with the app's settings
		extra: assisted.length ? { ...doc.extra, assistLayers: assisted } : doc.extra,
		created: doc.created ?? null,
		thumbnail: doc.thumbnail ? put(doc.thumbnail) : null,
		...(doc.timelapse?.length
			? { timelapse: doc.timelapse.map((f) => ({ t: Math.round(f.t), bytes: put(f.jpeg) })) }
			: {}),
		...(doc.fonts?.length
			? { fonts: doc.fonts.map((f) => ({ family: f.family, file: f.file, bytes: put(f.bytes) })) }
			: {}),
		...(doc.scenes?.length
			? {
					scenes: doc.scenes,
					staticLayers: doc.statics ?? [],
					...(doc.mouths?.length ? { mouthCharts: doc.mouths } : {}),
					...(doc.kinds && Object.keys(doc.kinds).length ? { drawingKinds: doc.kinds } : {}),
					audio: (doc.audio ?? []).map((a) => ({ file: a.file, bytes: put(a.bytes) })),
					drawings: (doc.drawings ?? []).map((d) => ({
						id: d.id,
						layer: d.layer,
						rgba: d.rgba ? put(d.rgba) : null
					}))
				}
			: {})
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
	const extra = h.extra ?? {};
	const assisted = Array.isArray(extra.assistLayers) ? (extra.assistLayers as unknown[]) : [];
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
			...(readStyle(l.style) ? { style: readStyle(l.style) } : {}),
			...(l.text ? { text: l.text } : {}),
			rgba: get(l.rgba),
			mask: l.mask ? get(l.mask) : undefined,
			...(assisted.includes(l.id) ? { assist: true } : {})
		})),
		groups: h.groups ?? [],
		activeLayer: h.activeLayer,
		strokes: h.strokes ?? [],
		extra,
		created: h.created ?? undefined,
		thumbnail: h.thumbnail ? get(h.thumbnail) : null,
		scenes: h.scenes ?? [],
		...(h.staticLayers?.length ? { statics: h.staticLayers } : {}),
		...(h.mouthCharts?.length ? { mouths: h.mouthCharts } : {}),
		...(h.drawingKinds ? { kinds: h.drawingKinds } : {}),
		...(h.audio?.length
			? { audio: h.audio.map((a) => ({ file: a.file, bytes: get(a.bytes) })) }
			: {}),
		...(h.timelapse?.length
			? { timelapse: h.timelapse.map((f) => ({ t: f.t, jpeg: get(f.bytes) })) }
			: {}),
		...(h.fonts?.length
			? { fonts: h.fonts.map((f) => ({ family: f.family, file: f.file, bytes: get(f.bytes) })) }
			: {}),
		drawings: (h.drawings ?? []).map((d) => ({
			id: d.id,
			layer: d.layer,
			...(d.rgba ? { rgba: get(d.rgba) } : {})
		}))
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

/** A file name inside a folder (none: just the name). */
async function inFolder(folder: string | null | undefined, name: string): Promise<string> {
	if (!folder) return name;
	const { join } = await import('@tauri-apps/api/path');
	return join(folder, name);
}

/** Ask where to save (a native dialog, starting in `folder`); null if cancelled. */
export async function askSavePath(name: string, folder?: string | null): Promise<string | null> {
	const { save } = await import('@tauri-apps/plugin-dialog');
	return save({
		defaultPath: await inFolder(folder, `${name}.${EXTENSION}`),
		filters: [{ name: 'PencilPilot project', extensions: [EXTENSION] }]
	});
}

/** Ask which project to open (starting in `folder`); null if cancelled. */
export async function askOpenPath(folder?: string | null): Promise<string | null> {
	const { open } = await import('@tauri-apps/plugin-dialog');
	const p = await open({
		multiple: false,
		directory: false,
		defaultPath: folder ?? undefined,
		filters: [{ name: 'PencilPilot project', extensions: [EXTENSION] }]
	});
	return typeof p === 'string' ? p : null;
}

// ---- recovery copies (autosave) and versions (see src-tauri/src/store.rs) ----

export interface RecoveryItem {
	id: string;
	/** the project file it belongs to (null: never saved) */
	path: string | null;
	savedAt: string;
	width: number;
	height: number;
	/** base64 PNG */
	thumbnail: string | null;
}

export interface VersionItem {
	id: string;
	savedAt: string;
	bytes: number;
	thumbnail: string | null;
}

/** A fresh id for a document's recovery copy (letters, digits and dashes). */
export const newDocId = () => crypto.randomUUID().toLowerCase();

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
	const { invoke } = await import('@tauri-apps/api/core');
	return invoke<T>(cmd, args);
}

/** Keep the unsaved document as recovery copy `id` (replacing the last one). */
export async function writeRecovery(id: string, path: string | null, doc: ProjectDoc) {
	const { invoke } = await import('@tauri-apps/api/core');
	await invoke('recovery_write', pack(doc), {
		headers: { id, path: encodeURIComponent(path ?? '') }
	});
}

export const listRecovery = () => call<RecoveryItem[]>('recovery_list');
export const discardRecovery = (id: string) => call<void>('recovery_discard', { id });
export const openRecovery = async (id: string) =>
	unpack(await call<ArrayBuffer>('recovery_open', { id }));

export const listVersions = (path: string) => call<VersionItem[]>('versions_list', { path });
export const openVersion = async (path: string, id: string) =>
	unpack(await call<ArrayBuffer>('version_open', { path, id }));

// ---- exports ----

/** Ask where to export (PNG or PSD); null if cancelled. */
const EXPORT_KINDS = {
	png: { name: 'PNG picture', ext: 'png' },
	psd: { name: 'Photoshop file', ext: 'psd' },
	mp4: { name: 'MP4 video', ext: 'mp4' },
	prores: { name: 'ProRes video', ext: 'mov' },
	webm: { name: 'WebM video', ext: 'webm' },
	gif: { name: 'GIF animation', ext: 'gif' },
	apng: { name: 'Animated PNG', ext: 'png' },
	'png-seq': { name: 'PNG sequence (numbered from this name)', ext: 'png' },
	'psd-seq': { name: 'PSD per frame (numbered from this name)', ext: 'psd' },
	layers: { name: 'PNG per layer (named from this name)', ext: 'png' }
};

export async function askExportPath(
	name: string,
	kind: keyof typeof EXPORT_KINDS,
	folder?: string | null
): Promise<string | null> {
	const { save } = await import('@tauri-apps/plugin-dialog');
	const k = EXPORT_KINDS[kind];
	return save({
		defaultPath: await inFolder(folder, `${name}.${k.ext}`),
		filters: [{ name: k.name, extensions: [k.ext] }]
	});
}

export async function writeExport(path: string, bytes: Uint8Array): Promise<string> {
	const { invoke } = await import('@tauri-apps/api/core');
	return invoke<string>('export_write', bytes, { headers: { path: encodeURIComponent(path) } });
}

/** "just now", "5 min ago", "3 h ago", or the date. */
export function ago(iso: string, now = Date.now()): string {
	const t = Date.parse(iso);
	if (Number.isNaN(t)) return '';
	const s = Math.max(0, (now - t) / 1000);
	if (s < 60) return 'just now';
	if (s < 3600) return `${Math.floor(s / 60)} min ago`;
	if (s < 86_400) return `${Math.floor(s / 3600)} h ago`;
	return new Date(t).toLocaleString();
}

// ---- the home screen: recent projects, templates, and the way into the studio ----

export interface RecentItem {
	path: string;
	/** false when the file has gone */
	found: boolean;
	width: number;
	height: number;
	fps: number;
	layers: number;
	modified: string;
	thumbnail: string | null;
	/** drawn together: who with (the invite code stays in the file) */
	shared?: { with: string[]; project?: string };
}

export const listRecent = () => call<RecentItem[]>('recent_list');
export const forgetRecent = (path: string) => call<void>('recent_remove', { path });

export interface Template {
	id: string;
	name: string;
	note: string;
	width: number;
	height: number;
	fps: number;
}

export const TEMPLATES: Template[] = [
	{ id: 'hd', name: 'HD video', note: '1920 × 1080 · 24 fps', width: 1920, height: 1080, fps: 24 },
	{ id: '4k', name: '4K video', note: '3840 × 2160 · 24 fps', width: 3840, height: 2160, fps: 24 },
	{
		id: 'square',
		name: 'Square',
		note: '1080 × 1080 · 12 fps',
		width: 1080,
		height: 1080,
		fps: 12
	},
	{
		id: 'vertical',
		name: 'Vertical',
		note: '1080 × 1920 · 24 fps, for phones',
		width: 1080,
		height: 1920,
		fps: 24
	},
	{
		id: 'board',
		name: 'Storyboard',
		note: '1280 × 720 · 12 fps',
		width: 1280,
		height: 720,
		fps: 12
	},
	{
		id: 'a4',
		name: 'Illustration',
		note: 'A4 at 300 dpi · 2480 × 3508',
		width: 2480,
		height: 3508,
		fps: 24
	}
];

/** canvas sides the paint engine accepts */
export const SIDE_MIN = 64;
export const SIDE_MAX = 8192;

/** What the studio is asked to start with (in its address). */
export type StudioStart =
	| { kind: 'new'; width: number; height: number; fps: number }
	| { kind: 'open'; path: string }
	| { kind: 'recover'; id: string; path: string | null };

/** The studio's address for a start. */
export function studioQuery(s: StudioStart): `?${string}` {
	const q = new URLSearchParams();
	if (s.kind === 'new') {
		q.set('new', `${s.width}x${s.height}`);
		q.set('fps', String(s.fps));
	} else if (s.kind === 'open') q.set('open', s.path);
	else {
		q.set('recover', s.id);
		if (s.path) q.set('path', s.path);
	}
	return `?${q}`;
}

/** Read a start from the studio's address; a plain HD canvas when there's none or it's wrong. */
export function parseStart(q: Pick<URLSearchParams, 'get'>): StudioStart {
	const open = q.get('open');
	if (open) return { kind: 'open', path: open };
	const recover = q.get('recover');
	if (recover && /^[a-z0-9-]{1,64}$/.test(recover))
		return { kind: 'recover', id: recover, path: q.get('path') };
	const m = /^(\d+)x(\d+)$/.exec(q.get('new') ?? '');
	const fps = Number(q.get('fps'));
	const side = (v: number) => Math.min(SIDE_MAX, Math.max(SIDE_MIN, Math.round(v)));
	return m
		? {
				kind: 'new',
				width: side(+m[1]),
				height: side(+m[2]),
				fps: fps >= 1 && fps <= 60 ? Math.round(fps) : 24
			}
		: { kind: 'new', width: 1920, height: 1080, fps: 24 };
}
