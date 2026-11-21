/**
 * The animation timeline: scenes → cuts → each layer's exposures (one drawing held
 * for some frames: 1 = "on ones", 2 = "on twos"). Drawings are the paint engine's ids
 * (0 = an empty cel); the engine keeps their pixels (PaintCanvas.showDrawings).
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

export interface Cut {
	id: number;
	name: string;
	/** layer id → its exposures; a layer missing here is empty all cut long */
	tracks: Record<number, Track>;
}

export interface Scene {
	id: number;
	name: string;
	cuts: Cut[];
}

export interface Timeline {
	scenes: Scene[];
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

/** Frames long: its longest track, at least one frame. */
export const cutLength = (cut: Cut) => Math.max(1, ...Object.values(cut.tracks).map(trackLength));

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

/** Every track in the cut: one more frame at `frame` (each holds what it shows). */
export function insertFrameAll(cut: Cut, frame: number) {
	for (const t of Object.values(cut.tracks)) insertFrame(t, frame);
}

/** Every track in the cut: `frame` taken out. */
export function deleteFrameAll(cut: Cut, frame: number) {
	for (const t of Object.values(cut.tracks)) deleteFrame(t, frame);
}

/** What each layer shows on `frame`: [layer id, drawing id] pairs, for every layer given. */
export function showing(cut: Cut, layers: number[], frame: number): [number, number][] {
	return layers.map((l) => [l, drawingAt(cut.tracks[l], frame)]);
}

/** Every drawing a timeline uses (each once). */
export function drawingsIn(tl: Timeline): number[] {
	const out = new Set<number>();
	for (const s of tl.scenes)
		for (const c of s.cuts)
			for (const t of Object.values(c.tracks)) for (const e of t) if (e.drawing) out.add(e.drawing);
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
	return null;
}

/** A new timeline: one scene, one cut, each layer showing its drawing for a frame. */
export function newTimeline(layers: { id: number; drawing: number }[]): Timeline {
	const tracks: Record<number, Track> = {};
	for (const l of layers) tracks[l.id] = l.drawing ? [{ drawing: l.drawing, length: 1 }] : [];
	return { scenes: [{ id: 1, name: 'Scene 1', cuts: [{ id: 1, name: 'Cut 1', tracks }] }] };
}

const nextId = (items: { id: number }[]) => Math.max(0, ...items.map((i) => i.id)) + 1;

/** A new empty cut after `index` in a scene; returns its index. */
export function addCut(scene: Scene, index: number): number {
	const id = nextId(scene.cuts);
	scene.cuts.splice(index + 1, 0, { id, name: `Cut ${scene.cuts.length + 1}`, tracks: {} });
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
}
export interface FileScene {
	name: string;
	cuts: FileCut[];
}

/** The timeline as the file keeps it, for the layers that exist. */
export function toFile(tl: Timeline, layers: number[]): FileScene[] {
	return tl.scenes.map((s) => ({
		name: s.name,
		cuts: s.cuts.map((c) => ({
			name: c.name,
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

/** A file's scenes back into a timeline; `layerIds` maps the file's layer ids to the canvas's. */
export function fromFile(scenes: FileScene[], layerIds: Map<number, number>): Timeline {
	return {
		scenes: scenes.map((s, si) => ({
			id: si + 1,
			name: s.name,
			cuts: s.cuts.map((c, ci) => ({
				id: ci + 1,
				name: c.name,
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
