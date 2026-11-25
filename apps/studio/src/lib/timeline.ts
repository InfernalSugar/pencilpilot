/**
 * The animation timeline: scenes → cuts → each layer's exposures (one drawing held
 * for some frames: 1 = "on ones", 2 = "on twos"). Drawings are the paint engine's ids
 * (0 = an empty cel); the engine keeps their pixels (PaintCanvas.showDrawings).
 *
 * A static layer (a background, an overlay) shows one drawing all cut long: its first
 * drawing in the cut. Its track keeps any others (turn it back into a cel layer and
 * they animate again), and doesn't count towards the cut's length.
 *
 * Each cut can frame part of the canvas (its camera); playback shows what's inside.
 *
 * These functions change a track or cut in place (so they work on `$state`) and are
 * pure otherwise. Frames count from 0.
 */

export interface Exposure {
	/** the drawing shown (0 = an empty cel) */
	drawing: number;
	/** frames it is held for (≥ 1) */
	length: number;
}

/** One layer's exposures in a cut, in order. */
export type Track = Exposure[];

/** A rectangle of the canvas, in canvas pixels. */
export interface Camera {
	x: number;
	y: number;
	width: number;
	height: number;
}

export interface Cut {
	id: number;
	name: string;
	/** layer id → its exposures; a layer missing here is empty all cut long */
	tracks: Record<number, Track>;
	/** the part of the canvas the cut shows (none: all of it) */
	camera?: Camera;
	/** exposure-sheet notes, frame → text: what happens (action) and what's said (dialogue) */
	action?: Record<number, string>;
	dialogue?: Record<number, string>;
	/** how it goes into the next cut (none: a hard cut) */
	transition?: 'flash' | 'dissolve';
}

export interface Scene {
	id: number;
	name: string;
	cuts: Cut[];
	/** sound clips along the scene (its cuts one after another) */
	audio?: AudioClip[];
	/** sync markers on scene frames */
	markers?: Marker[];
}

/** A sound file placed in a scene. */
export interface AudioClip {
	id: number;
	name: string;
	/** the sound file's key (the studio keeps the bytes) */
	file: string;
	/** the scene frame it starts on (negative: it started before the scene) */
	start: number;
	/** loudness (1 = as recorded) */
	gain: number;
	muted: boolean;
}

/** A marker on a scene frame (a beat, an accent, a line). */
export interface Marker {
	frame: number;
	label: string;
}

/** The scene frame a cut starts on (its cuts one after another). */
export function cutStart(scene: Scene, cut: number, statics: number[] = []): number {
	let n = 0;
	for (let i = 0; i < cut && i < scene.cuts.length; i++) n += cutLength(scene.cuts[i], statics);
	return n;
}

/** Add a marker (or, on a frame that has one, take it away). Returns whether it was added. */
export function toggleMarker(scene: Scene, frame: number, label = ''): boolean {
	// (read back after making it: with `$state`, the stored list is a reactive copy)
	if (!scene.markers) scene.markers = [];
	const m = scene.markers;
	const i = m.findIndex((x) => x.frame === frame);
	if (i >= 0) {
		m.splice(i, 1);
		if (!m.length) delete scene.markers;
		return false;
	}
	m.push({ frame, label });
	m.sort((a, b) => a.frame - b.frame);
	return true;
}

export interface Timeline {
	scenes: Scene[];
	/** static layers' ids: one drawing all cut long */
	statics: number[];
	/** lip-sync mouth charts, one per mouth layer */
	mouths?: MouthChart[];
	/** what each drawing is in the animation: a key, a breakdown or an in-between */
	kinds?: Record<number, DrawingKind>;
	/** each layer's production stage (layers with none show in every view) */
	stages?: Record<number, Stage>;
}

/** A layer's production stage, in the order the work goes. */
export type Stage = 'board' | 'rough' | 'tiedown' | 'clean' | 'tone' | 'flats' | 'shade' | 'comp';
export const STAGES: Stage[] = [
	'board',
	'rough',
	'tiedown',
	'clean',
	'tone',
	'flats',
	'shade',
	'comp'
];
export const STAGE_LABELS: Record<Stage, string> = {
	board: 'Storyboard',
	rough: 'Rough',
	tiedown: 'Tie-down',
	clean: 'Clean-up',
	tone: 'Tone lines',
	flats: 'Colour',
	shade: 'Shading',
	comp: 'Compositing'
};
export const isStage = (s: unknown): s is Stage => (STAGES as unknown[]).includes(s);

/** Put a layer in a stage (null: in none). */
export function setStage(tl: Timeline, layer: number, stage: Stage | null) {
	if (!layer) return;
	if (!tl.stages) tl.stages = {};
	const st = tl.stages;
	if (stage) st[layer] = stage;
	else delete st[layer];
	if (!Object.keys(st).length) delete tl.stages;
}

/** A layer's stage (null: none). */
export const stageOf = (tl: Timeline, layer: number): Stage | null => tl.stages?.[layer] ?? null;

/** What a drawing is in the animation (shown on the timeline and the X-sheet). */
export type DrawingKind = 'key' | 'breakdown' | 'inbetween';
export const DRAWING_KINDS: DrawingKind[] = ['key', 'breakdown', 'inbetween'];

/** Mark a drawing as a key, a breakdown or an in-between (null: unmarked). */
export function setKind(tl: Timeline, drawing: number, kind: DrawingKind | null) {
	if (!drawing) return;
	if (!tl.kinds) tl.kinds = {};
	const k = tl.kinds;
	if (kind) k[drawing] = kind;
	else delete k[drawing];
	if (!Object.keys(k).length) delete tl.kinds;
}

/** The next mark for a drawing: none → key → breakdown → in-between → none. */
export function nextKind(kind: DrawingKind | undefined): DrawingKind | null {
	if (!kind) return 'key';
	return DRAWING_KINDS[DRAWING_KINDS.indexOf(kind) + 1] ?? null;
}

/** Lip-sync's mouth shapes (Rhubarb's: A–F, and the extended G, H and X). */
export const MOUTH_SHAPES = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'X'] as const;
export type MouthShape = (typeof MOUTH_SHAPES)[number];

/** A mouth chart: the drawing of a layer drawn for each mouth shape. */
export interface MouthChart {
	layer: number;
	shapes: Partial<Record<MouthShape, number>>;
}

/** The longest an exposure can be held. */
export const HOLD_MAX = 999;

/**
 * A layer's track in a cut, made if it has none. (Reads it back after making it: with
 * `$state`, the stored array is a reactive copy, and changes must go to that copy.)
 */
export function trackIn(cut: Cut, layer: number): Track {
	if (!cut.tracks[layer]) cut.tracks[layer] = [];
	return cut.tracks[layer];
}

export const trackLength = (t: Track) => t.reduce((n, e) => n + e.length, 0);

/** Frames long: its longest track (static layers aside), at least one frame. */
export const cutLength = (cut: Cut, statics: number[] = []) =>
	Math.max(
		1,
		...Object.entries(cut.tracks)
			.filter(([l]) => !statics.includes(+l))
			.map(([, t]) => trackLength(t))
	);

/** What a static layer shows all cut long: its first drawing (0: none yet). */
export const staticDrawing = (t: Track | undefined) => t?.find((e) => e.drawing)?.drawing ?? 0;

/** Give a static layer its drawing for the cut (in place of its first one). */
export function setStaticDrawing(t: Track, drawing: number) {
	const e = t.find((x) => x.drawing);
	if (e) e.drawing = drawing;
	else setDrawing(t, 0, drawing);
}

/** Make a layer static (one drawing all cut long) or a cel layer again. */
export function setStatic(tl: Timeline, layer: number, on: boolean) {
	const i = tl.statics.indexOf(layer);
	if (on && i < 0) tl.statics.push(layer);
	if (!on && i >= 0) tl.statics.splice(i, 1);
}

/** The exposure showing on `frame`: its index and first frame (null past the end). */
export function exposureAt(t: Track, frame: number): { index: number; start: number } | null {
	let start = 0;
	for (let index = 0; index < t.length; index++) {
		if (frame < start + t[index].length) return { index, start };
		start += t[index].length;
	}
	return null;
}

/** The drawing a track shows on `frame` (0: an empty cel, or past its end). */
export function drawingAt(t: Track | undefined, frame: number): number {
	if (!t) return 0;
	const at = exposureAt(t, frame);
	return at ? t[at.index].drawing : 0;
}

/** Where a track's exposure starts. */
export const startOf = (t: Track, index: number) =>
	t.slice(0, index).reduce((n, e) => n + e.length, 0);

/** Make the track reach `frame` (empty cels fill the gap); returns its exposure index. */
function reach(t: Track, frame: number): number {
	const end = trackLength(t);
	if (frame >= end) {
		const gap = frame - end;
		if (gap > 0) t.push({ drawing: 0, length: gap });
		t.push({ drawing: 0, length: 1 });
	}
	return exposureAt(t, frame)!.index;
}

/**
 * Show `drawing` for the whole exposure on `frame` (drawing on an empty cel starts a
 * drawing there). Past the end, the track grows to reach it.
 */
export function setDrawing(t: Track, frame: number, drawing: number) {
	t[reach(t, frame)].drawing = drawing;
	merge(t);
}

/** Hold the exposure at `index` for `length` frames (later ones move along). */
export function setLength(t: Track, index: number, length: number) {
	if (!t[index]) return;
	t[index].length = Math.min(HOLD_MAX, Math.max(1, Math.round(length)));
}

/** Every exposure held for `n` frames ("on twos" is 2). */
export function setTiming(t: Track, n: number) {
	for (let i = 0; i < t.length; i++) setLength(t, i, n);
}

/** Insert an exposure before `index` (at the end when `index` is the length). */
export function insertExposure(t: Track, index: number, e: Exposure) {
	t.splice(Math.max(0, Math.min(index, t.length)), 0, {
		drawing: e.drawing,
		length: Math.min(HOLD_MAX, Math.max(1, e.length))
	});
}

/** Remove an exposure (the ones after it move back). Returns it. */
export function removeExposure(t: Track, index: number): Exposure | undefined {
	const [e] = t.splice(index, 1);
	trimEnd(t);
	return e;
}

/** Move an exposure to another place in the order (dragging it). */
export function moveExposure(t: Track, from: number, to: number) {
	if (from === to || !t[from]) return;
	const [e] = t.splice(from, 1);
	t.splice(Math.max(0, Math.min(to, t.length)), 0, e);
	merge(t);
}

/** Hold the frame one frame longer (insert a frame in this track). */
export function insertFrame(t: Track, frame: number) {
	const at = exposureAt(t, frame);
	if (at) setLength(t, at.index, t[at.index].length + 1);
}

/** Take one frame out of this track (its exposure gets shorter, or goes). */
export function deleteFrame(t: Track, frame: number) {
	const at = exposureAt(t, frame);
	if (!at) return;
	if (t[at.index].length > 1) t[at.index].length--;
	else removeExposure(t, at.index);
}

/** Empty cels at the end of a track don't count. */
function trimEnd(t: Track) {
	while (t.length && t.at(-1)!.drawing === 0) t.pop();
}

/** Empty cels side by side become one. */
function merge(t: Track) {
	for (let i = t.length - 1; i > 0; i--)
		if (t[i].drawing === 0 && t[i - 1].drawing === 0) {
			t[i - 1].length += t[i].length;
			t.splice(i, 1);
		}
	trimEnd(t);
}

/** The cel layers' tracks in a cut (static layers' don't follow frames). */
const celTracks = (cut: Cut, statics: number[]) =>
	Object.entries(cut.tracks)
		.filter(([l]) => !statics.includes(+l))
		.map(([, t]) => t);

/** Every cel track in the cut: one more frame at `frame` (each holds what it shows). */
export function insertFrameAll(cut: Cut, frame: number, statics: number[] = []) {
	for (const t of celTracks(cut, statics)) insertFrame(t, frame);
	shiftNotes(cut, frame, 1);
}

/** Every cel track in the cut: `frame` taken out. */
export function deleteFrameAll(cut: Cut, frame: number, statics: number[] = []) {
	for (const t of celTracks(cut, statics)) deleteFrame(t, frame);
	shiftNotes(cut, frame, -1);
}

/** Notes from `frame` on move with the frames (a deleted frame's note goes). */
function shiftNotes(cut: Cut, frame: number, by: 1 | -1) {
	for (const key of ['action', 'dialogue'] as const) {
		const notes = cut[key];
		if (!notes) continue;
		const moved: Record<number, string> = {};
		for (const [f, text] of Object.entries(notes)) {
			const n = +f;
			if (n < frame) moved[n] = text;
			else if (by > 0) moved[n + 1] = text;
			else if (n > frame) moved[n - 1] = text;
		}
		cut[key] = moved;
	}
}

/** Write (or, empty, clear) a note on a frame. */
export function setNote(cut: Cut, key: 'action' | 'dialogue', frame: number, text: string) {
	if (!cut[key]) cut[key] = {};
	const notes = cut[key];
	const t = text.trim();
	if (t) notes[frame] = t;
	else delete notes[frame];
	if (!Object.keys(notes).length) delete cut[key];
}

/**
 * Drawings numbered on a track as people read them: 1, 2, 3… in order of first
 * appearance (a drawing exposed again keeps its number).
 */
export function drawingNumbers(t: Track): Map<number, number> {
	const out = new Map<number, number>();
	for (const e of t) if (e.drawing && !out.has(e.drawing)) out.set(e.drawing, out.size + 1);
	return out;
}

/**
 * Write a drawing on a frame as on an exposure sheet: it shows from that frame for the
 * rest of the hold it falls in (0: an empty cel). Past the end, empty cels fill the gap.
 */
export function setKey(t: Track, frame: number, drawing: number) {
	const frames: number[] = [];
	for (const e of t) for (let i = 0; i < e.length; i++) frames.push(e.drawing);
	if (frame >= frames.length) {
		while (frames.length < frame) frames.push(0);
		frames.push(drawing);
	} else {
		const old = frames[frame];
		// (the rest of this exposure: frames after it that belonged to it)
		const at = exposureAt(t, frame)!;
		const end = at.start + t[at.index].length;
		for (let k = frame; k < end && frames[k] === old; k++) frames[k] = drawing;
	}
	const out: Track = [];
	for (const d of frames) {
		const last = out.at(-1);
		if (last && last.drawing === d) last.length++;
		else out.push({ drawing: d, length: 1 });
	}
	while (out.length && out.at(-1)!.drawing === 0) out.pop();
	t.splice(0, t.length, ...out);
}

/** What each layer shows on `frame`: [layer id, drawing id] pairs, for every layer given. */
export function showing(
	cut: Cut,
	layers: number[],
	frame: number,
	statics: number[] = []
): [number, number][] {
	return layers.map((l) => [
		l,
		statics.includes(l) ? staticDrawing(cut.tracks[l]) : drawingAt(cut.tracks[l], frame)
	]);
}

// ---- the camera: the part of the canvas a cut shows ----

/** A cut's camera (the whole canvas when it has none), kept on the canvas. */
export function cameraOf(cut: Cut, canvas: { width: number; height: number }): Camera {
	const c = cut.camera;
	if (!c) return { x: 0, y: 0, width: canvas.width, height: canvas.height };
	const width = Math.max(1, Math.min(canvas.width, Math.round(c.width)));
	const height = Math.max(1, Math.min(canvas.height, Math.round(c.height)));
	return {
		x: Math.max(0, Math.min(canvas.width - width, Math.round(c.x))),
		y: Math.max(0, Math.min(canvas.height - height, Math.round(c.y))),
		width,
		height
	};
}

/** The biggest frame of an aspect ratio (width / height) that fits the canvas, centred. */
export function fitCamera(aspect: number, canvas: { width: number; height: number }): Camera {
	let width = canvas.width;
	let height = Math.round(width / aspect);
	if (height > canvas.height) {
		height = canvas.height;
		width = Math.round(height * aspect);
	}
	return {
		x: Math.round((canvas.width - width) / 2),
		y: Math.round((canvas.height - height) / 2),
		width,
		height
	};
}

/** A frame shrunk about its centre (safe areas: action 90%, title 80%). */
export function inset(c: Camera, share: number): Camera {
	const [w, h] = [c.width * share, c.height * share];
	return { x: c.x + (c.width - w) / 2, y: c.y + (c.height - h) / 2, width: w, height: h };
}

/** Every drawing a timeline uses (each once). */
export function drawingsIn(tl: Timeline): number[] {
	const out = new Set<number>();
	for (const s of tl.scenes)
		for (const c of s.cuts)
			for (const t of Object.values(c.tracks)) for (const e of t) if (e.drawing) out.add(e.drawing);
	// (a mouth chart's drawings are kept whether a frame shows them or not)
	for (const m of tl.mouths ?? []) for (const d of Object.values(m.shapes)) if (d) out.add(d);
	return [...out];
}

/** Where a drawing shows first: its scene, cut and frame (null: nowhere). */
export function findDrawing(
	tl: Timeline,
	drawing: number,
	prefer?: { scene: number; cut: number }
): { scene: number; cut: number; frame: number; layer: number } | null {
	const places: { scene: number; cut: number }[] = [];
	if (prefer) places.push(prefer);
	tl.scenes.forEach((s, si) => s.cuts.forEach((_, ci) => places.push({ scene: si, cut: ci })));
	for (const p of places) {
		const cut = tl.scenes[p.scene]?.cuts[p.cut];
		if (!cut) continue;
		for (const [layer, t] of Object.entries(cut.tracks)) {
			const i = t.findIndex((e) => e.drawing === drawing);
			if (i >= 0) return { ...p, frame: startOf(t, i), layer: +layer };
		}
	}
	return null;
}

/** A layer's drawings in a timeline (to find which layer a drawing belongs to). */
export function layerOfDrawing(tl: Timeline, drawing: number): number | null {
	for (const s of tl.scenes)
		for (const c of s.cuts)
			for (const [layer, t] of Object.entries(c.tracks))
				if (t.some((e) => e.drawing === drawing)) return +layer;
	for (const m of tl.mouths ?? []) if (Object.values(m.shapes).includes(drawing)) return m.layer;
	return null;
}

/**
 * Give layer `to` the timing of layer `from` (a layer to trace it on): in every cut where
 * `from` has a track, a new empty drawing (`make()`) wherever `from` shows one, held for
 * as long, and empty cels where it shows none. A drawing `from` shows again (a cycle)
 * gets the same new one again; each new drawing is marked as its own is (key, breakdown,
 * in-between). Static if `from` is. Returns the new drawings, in the order they were made.
 */
export function followTiming(tl: Timeline, from: number, to: number, make: () => number): number[] {
	const made = new Map<number, number>();
	const copy = (d: number) => {
		if (!d) return 0;
		let n = made.get(d);
		if (n === undefined) {
			n = make();
			made.set(d, n);
			const k = tl.kinds?.[d];
			if (n && k) setKind(tl, n, k);
		}
		return n;
	};
	for (const s of tl.scenes)
		for (const c of s.cuts) {
			const t = c.tracks[from];
			if (!t) continue;
			c.tracks[to] = t.map((e) => ({ drawing: copy(e.drawing), length: e.length }));
		}
	setStatic(tl, to, tl.statics.includes(from));
	return [...made.values()].filter((d) => d);
}

/** A new timeline: one scene, one cut, each layer showing its drawing for a frame. */
export function newTimeline(layers: { id: number; drawing: number }[]): Timeline {
	const tracks: Record<number, Track> = {};
	for (const l of layers) tracks[l.id] = l.drawing ? [{ drawing: l.drawing, length: 1 }] : [];
	return {
		scenes: [{ id: 1, name: 'Scene 1', cuts: [{ id: 1, name: 'Cut 1', tracks }] }],
		statics: []
	};
}

const nextId = (items: { id: number }[]) => Math.max(0, ...items.map((i) => i.id)) + 1;

/** A new empty cut after `index` in a scene; returns its index. */
export function addCut(scene: Scene, index: number): number {
	const id = nextId(scene.cuts);
	const camera = scene.cuts[index]?.camera;
	scene.cuts.splice(index + 1, 0, {
		id,
		name: `Cut ${scene.cuts.length + 1}`,
		tracks: {},
		// (framed like the cut before it)
		...(camera ? { camera: { ...camera } } : {})
	});
	return index + 1;
}

/** A new scene (with one empty cut) after `index`; returns its index. */
export function addScene(tl: Timeline, index: number): number {
	const id = nextId(tl.scenes);
	tl.scenes.splice(index + 1, 0, {
		id,
		name: `Scene ${tl.scenes.length + 1}`,
		cuts: [{ id: 1, name: 'Cut 1', tracks: {} }]
	});
	return index + 1;
}

// ---- the project file (crates/ppstudio/FORMAT.md: drawings are named "D<id>") ----

export interface FileExposure {
	drawing: string | null;
	length: number;
}
export interface FileCut {
	name: string;
	tracks: Record<string, FileExposure[]>;
	camera?: Camera;
	action?: Record<string, string>;
	dialogue?: Record<string, string>;
	transition?: string;
}
export interface FileScene {
	name: string;
	cuts: FileCut[];
	audio?: { name: string; file: string; start: number; gain: number; muted: boolean }[];
	markers?: Marker[];
}

/** The timeline as the file keeps it, for the layers that exist. */
export function toFile(tl: Timeline, layers: number[]): FileScene[] {
	return tl.scenes.map((s) => ({
		name: s.name,
		...(s.audio?.length
			? {
					audio: s.audio.map((a) => ({
						name: a.name,
						file: a.file,
						start: a.start,
						gain: a.gain,
						muted: a.muted
					}))
				}
			: {}),
		...(s.markers?.length ? { markers: s.markers.map((m) => ({ ...m })) } : {}),
		cuts: s.cuts.map((c) => ({
			name: c.name,
			...(c.camera ? { camera: { ...c.camera } } : {}),
			...(c.action && Object.keys(c.action).length ? { action: { ...c.action } } : {}),
			...(c.dialogue && Object.keys(c.dialogue).length ? { dialogue: { ...c.dialogue } } : {}),
			...(c.transition ? { transition: c.transition } : {}),
			tracks: Object.fromEntries(
				Object.entries(c.tracks)
					.filter(([l]) => layers.includes(+l))
					.map(([l, t]) => [
						l,
						t.map((e) => ({ drawing: e.drawing ? `D${e.drawing}` : null, length: e.length }))
					])
			)
		}))
	}));
}

/** The static layers the file names, for the layers that exist. */
export const staticsToFile = (tl: Timeline, layers: number[]) =>
	tl.statics.filter((l) => layers.includes(l));

/** Drawing marks as the file keeps them ("D<id>" → kind), for drawings it keeps. */
export function kindsToFile(tl: Timeline, drawings: number[]): Record<string, DrawingKind> {
	const keep = new Set(drawings);
	return Object.fromEntries(
		Object.entries(tl.kinds ?? {})
			.filter(([d]) => keep.has(+d))
			.map(([d, k]) => [`D${d}`, k])
	);
}

/** A file's drawing marks back (unknown kinds left out). */
export function kindsFromFile(
	raw: Record<string, string> | undefined
): Record<number, DrawingKind> {
	const out: Record<number, DrawingKind> = {};
	for (const [d, k] of Object.entries(raw ?? {})) {
		const id = Number(d.replace(/^D/, ''));
		if (id > 0 && (DRAWING_KINDS as string[]).includes(k)) out[id] = k as DrawingKind;
	}
	return out;
}

/** Layer stages as the file keeps them (layer id → stage), for the layers that exist. */
export function stagesToFile(tl: Timeline, layers: number[]): Record<string, Stage> {
	const keep = new Set(layers);
	return Object.fromEntries(
		Object.entries(tl.stages ?? {}).filter(([l, st]) => keep.has(+l) && isStage(st))
	);
}

/** A file's layer stages back; `layerIds` maps the file's layer ids to the canvas's (unknown stages and layers left out). */
export function stagesFromFile(
	raw: Record<string, string> | undefined,
	layerIds: Map<number, number>
): Record<number, Stage> {
	const out: Record<number, Stage> = {};
	for (const [l, st] of Object.entries(raw ?? {})) {
		const id = layerIds.get(+l);
		if (id !== undefined && id > 0 && isStage(st)) out[id] = st;
	}
	return out;
}

/** A mouth chart as the file keeps it (drawings named "D<id>"). */
export interface FileMouthChart {
	layer: number;
	shapes: Record<string, string>;
}

/** The mouth charts the file keeps, for the layers that exist. */
export const mouthsToFile = (tl: Timeline, layers: number[]): FileMouthChart[] =>
	(tl.mouths ?? [])
		.filter((m) => layers.includes(m.layer))
		.map((m) => ({
			layer: m.layer,
			shapes: Object.fromEntries(
				Object.entries(m.shapes).flatMap(([k, d]) => (d ? [[k, `D${d}`]] : []))
			)
		}))
		.filter((m) => Object.keys(m.shapes).length);

/** A file's scenes back into a timeline; `layerIds` maps the file's layer ids to the canvas's. */
export function fromFile(
	scenes: FileScene[],
	layerIds: Map<number, number>,
	statics: number[] = [],
	mouths: FileMouthChart[] = []
): Timeline {
	const camera = (c: Camera | undefined) =>
		c && [c.x, c.y, c.width, c.height].every(Number.isFinite) && c.width > 0 && c.height > 0
			? { camera: { x: c.x, y: c.y, width: c.width, height: c.height } }
			: {};
	const notes = (key: 'action' | 'dialogue', n: Record<string, string> | undefined) => {
		const out: Record<number, string> = {};
		for (const [f, text] of Object.entries(n ?? {}))
			if (Number.isInteger(+f) && +f >= 0 && typeof text === 'string' && text) out[+f] = text;
		return Object.keys(out).length ? { [key]: out } : {};
	};
	const charts = mouths.flatMap((m): MouthChart[] => {
		const layer = layerIds.get(m.layer);
		if (layer === undefined) return [];
		const shapes: MouthChart['shapes'] = {};
		for (const [k, d] of Object.entries(m.shapes ?? {})) {
			const id = Number(String(d).replace(/^D/, ''));
			if ((MOUTH_SHAPES as readonly string[]).includes(k) && id > 0) shapes[k as MouthShape] = id;
		}
		return [{ layer, shapes }];
	});
	return {
		...(charts.length ? { mouths: charts } : {}),
		statics: statics.flatMap((l) => {
			const id = layerIds.get(l);
			return id === undefined ? [] : [id];
		}),
		scenes: scenes.map((s, si) => ({
			id: si + 1,
			name: s.name,
			...(s.audio?.length
				? {
						audio: s.audio
							.filter((a) => typeof a.file === 'string')
							.map((a, i) => ({
								id: i + 1,
								name: String(a.name ?? 'Sound'),
								file: a.file,
								start: Number.isFinite(a.start) ? Math.round(a.start) : 0,
								gain: Number.isFinite(a.gain) ? Math.min(16, Math.max(0, a.gain)) : 1,
								muted: !!a.muted
							}))
					}
				: {}),
			...(s.markers?.length
				? {
						markers: s.markers
							.filter((m) => Number.isInteger(m.frame) && m.frame >= 0)
							.map((m) => ({ frame: m.frame, label: String(m.label ?? '') }))
					}
				: {}),
			cuts: s.cuts.map((c, ci) => ({
				id: ci + 1,
				name: c.name,
				...camera(c.camera),
				...notes('action', c.action),
				...notes('dialogue', c.dialogue),
				...(c.transition === 'flash' || c.transition === 'dissolve'
					? { transition: c.transition as 'flash' | 'dissolve' }
					: {}),
				tracks: Object.fromEntries(
					Object.entries(c.tracks).flatMap(([l, t]) => {
						const id = layerIds.get(+l);
						if (id === undefined) return [];
						return [
							[
								id,
								t.map((e) => ({
									drawing: e.drawing ? Number(e.drawing.replace(/^D/, '')) || 0 : 0,
									length: Math.max(1, e.length)
								}))
							]
						];
					})
				)
			}))
		}))
	};
}
