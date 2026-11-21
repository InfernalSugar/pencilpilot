<script lang="ts">
	/**
	 * The timeline under the canvas: the scene and cut, a frame ruler, and a row per
	 * layer (top layer first) with its exposures. Click a frame to go there; drag an
	 * exposure to move it, or its right edge to hold it longer or shorter. A static layer
	 * (a background, an overlay) shows one drawing all cut long; the camera frames the
	 * part of the canvas the cut shows.
	 *
	 * The timeline is changed in place; the studio is told (`onedit`) so it can show
	 * the right drawings and count the change as unsaved.
	 */
	import type { LayerInfo } from '@pencilpilot/paint/PaintCanvas.svelte';
	import {
		addCut,
		addScene,
		cameraOf,
		cutLength,
		cutStart,
		deleteFrameAll,
		drawingAt,
		drawingNumbers,
		exposureAt,
		insertExposure,
		insertFrameAll,
		moveExposure,
		fitCamera,
		removeExposure,
		setStatic,
		setLength,
		setTiming,
		startOf,
		staticDrawing,
		trackIn,
		type Camera,
		type Timeline,
		type Track,
		type DrawingKind
	} from './timeline';
	import type { PlayMode, PlayRange } from './playback';
	import {
		BOIL_DEFAULTS,
		boilFrames,
		copyFrames,
		cycleFrames,
		duplicateFrames,
		exposureRange,
		inRange as inSel,
		pasteFrames,
		reverseFrames,
		type BoilSettings,
		type FrameRange
	} from './frametools';
	import type { OnionSettings } from './onion';
	import type { AudioBank } from './audio';
	import { AUDIO_TYPES } from './audio';
	import { TRANSITIONS } from './exporting';

	let {
		timeline,
		scene = $bindable(0),
		cut = $bindable(0),
		frame = $bindable(0),
		fps = $bindable(24),
		playing = false,
		playMode = $bindable<PlayMode>('loop'),
		playScope = $bindable<'cut' | 'scene'>('cut'),
		playRange = $bindable<PlayRange | null>(null),
		onplay,
		onrange,
		onion,
		pinned = $bindable<number[]>([]),
		onpin,
		onflip,
		onmark,
		cameraView = $bindable({ show: true, safe: false }),
		canvasSize,
		layers,
		active,
		newDrawing,
		drawings,
		onlayer,
		audio,
		soundTick = 0,
		onsound,
		onlipsync,
		onmarker,
		onedit
	}: {
		timeline: Timeline;
		scene?: number;
		cut?: number;
		frame?: number;
		fps?: number;
		playing?: boolean;
		playMode?: PlayMode;
		/** play this cut, or the whole scene with its transitions */
		playScope?: 'cut' | 'scene';
		/** the frames played (none: the whole cut) */
		playRange?: PlayRange | null;
		onplay: () => void;
		/** onion skin settings (changed in place) */
		onion: OnionSettings;
		/** frames on the light table */
		pinned?: number[];
		/** pin or unpin the current frame */
		onpin: () => void;
		/** flipping: pressed (start) or let go (stop) */
		onflip?: (kind: 'roll' | 'keys', down: boolean) => void;
		/** mark the drawing here: key → breakdown → in-between → none */
		onmark?: () => void;
		/** show the camera frame over the canvas, and its safe areas */
		cameraView?: { show: boolean; safe: boolean };
		canvasSize: { width: number; height: number };
		/** loop from / to the current frame */
		onrange: (end: 'from' | 'to') => void;
		/** bottom → top, as the canvas lists them */
		layers: LayerInfo[];
		/** the active layer's id */
		active: number;
		/** a new, empty drawing's id (from the canvas) */
		newDrawing: () => number;
		/** drawings for the frame tools: brought from the store, copied, boiled */
		drawings: {
			ensure: (ids: number[]) => Promise<void>;
			copy: (drawing: number) => number;
			boil: (drawing: number, amount: number, seed: number) => number;
		};
		onlayer: (id: number) => void;
		/** the project's sounds (decoded, with waveforms) */
		audio?: AudioBank;
		/** changes when sounds are added or loaded (to draw them again) */
		soundTick?: number;
		/** sound files picked to add at this frame */
		onsound?: (files: File[]) => void;
		/** lip-sync a sound clip (by id) */
		onlipsync?: (clip: number) => void;
		/** add or take away a marker on this frame */
		onmarker?: () => void;
		/** the timeline changed (or the frame did): show it */
		onedit: (changed: boolean) => void;
	} = $props();

	/** px per frame */
	const CELL = 22;
	const theScene = $derived(timeline.scenes[scene] ?? timeline.scenes[0]);
	const theCut = $derived(theScene.cuts[cut] ?? theScene.cuts[0]);
	const statics = $derived(timeline.statics);
	const length = $derived(cutLength(theCut, statics));
	/** room to draw past the end */
	const shown = $derived(Math.max(length + 8, 24));
	const rows = $derived([...layers].reverse().filter((l) => !l.adjust));

	const trackOf = (layer: number): Track => trackIn(theCut, layer);
	const activeTrack = $derived(theCut.tracks[active] ?? []);
	const KIND_NAME: Record<DrawingKind, string> = {
		key: 'Key',
		breakdown: 'Breakdown',
		inbetween: 'In-between'
	};
	const KIND_SHORT: Record<DrawingKind, string> = { key: 'K', breakdown: 'B', inbetween: 'i' };
	/** the active layer is static: its timing doesn't apply */
	const activeStatic = $derived(statics.includes(active));
	const here = $derived(activeStatic ? null : exposureAt(activeTrack, frame));

	function go(f: number) {
		frame = Math.max(0, f);
		onedit(false);
	}

	/** One frame back or on (the frame-step keys). */
	export function step(by: number) {
		go(frame + by);
	}
	function edited() {
		onedit(true);
	}

	/** a drawing's number on its layer: 1, 2, 3… in order (exposed again, the same number) */
	function numbers(t: Track): number[] {
		const n = drawingNumbers(t);
		return t.map((e) => n.get(e.drawing) ?? 0);
	}

	// ---- buttons ----
	/** a new, empty drawing after this one (or here, past the end), and go to it */
	export function addDrawing() {
		if (activeStatic) return;
		const t = trackOf(active);
		const d = newDrawing();
		if (!d) return;
		if (here) {
			const at = here.index + 1;
			insertExposure(t, at, { drawing: d, length: t[here.index].length });
			frame = startOf(t, at);
		} else {
			const end = t.reduce((n, e) => n + e.length, 0);
			if (frame > end) insertExposure(t, t.length, { drawing: 0, length: frame - end });
			insertExposure(t, t.length, { drawing: d, length: 1 });
		}
		edited();
	}
	function hold(by: number) {
		if (!here) return;
		setLength(activeTrack, here.index, activeTrack[here.index].length + by);
		edited();
	}
	function timing(n: number) {
		if (!here) return;
		setLength(activeTrack, here.index, n);
		edited();
	}
	function timingAll(n: number) {
		setTiming(trackOf(active), n);
		edited();
	}
	function removeHere() {
		if (!here) return;
		removeExposure(activeTrack, here.index);
		edited();
	}
	function insertFrame() {
		insertFrameAll(theCut, frame, statics);
		edited();
	}
	function deleteFrame() {
		deleteFrameAll(theCut, frame, statics);
		frame = Math.min(frame, cutLength(theCut, statics) - 1);
		edited();
	}
	function toggleStatic(layer: number) {
		setStatic(timeline, layer, !statics.includes(layer));
		edited();
	}

	// ---- the camera ----
	const camera = $derived(cameraOf(theCut, canvasSize));
	const ASPECTS: [string, number][] = [
		['16:9', 16 / 9],
		['4:3', 4 / 3],
		['1:1', 1],
		['2.39:1', 2.39],
		['9:16', 9 / 16]
	];
	function setCamera(c: Camera | null) {
		if (c) theCut.camera = cameraOf({ ...theCut, camera: c }, canvasSize);
		else delete theCut.camera;
		cameraView.show = true;
		edited();
	}
	/** one of the camera's numbers changed */
	function cameraField(key: keyof Camera, v: number) {
		if (!Number.isFinite(v)) return;
		setCamera({ ...camera, [key]: v });
	}
	/** zoom the camera about its centre, keeping its shape */
	function zoomCamera(k: number) {
		const width = Math.min(canvasSize.width, camera.width * k);
		const height = Math.min(canvasSize.height, (width * camera.height) / camera.width);
		const w = (height * camera.width) / camera.height;
		setCamera({
			x: camera.x + (camera.width - w) / 2,
			y: camera.y + (camera.height - height) / 2,
			width: w,
			height
		});
	}
	let cameraOpen = $state(false);

	function newCut() {
		cut = addCut(theScene, cut);
		frame = 0;
		edited();
	}
	function newScene() {
		scene = addScene(timeline, scene);
		cut = 0;
		frame = 0;
		edited();
	}

	// ---- dragging an exposure (move) or its edge (hold) ----
	let drag = $state<{
		layer: number;
		index: number;
		mode: 'move' | 'hold';
		x0: number;
		length0: number;
		/** frames moved so far */
		by: number;
	} | null>(null);

	function dragStart(e: PointerEvent, layer: number, index: number, mode: 'move' | 'hold') {
		if (e.button !== 0) return;
		e.stopPropagation();
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		const t = trackOf(layer);
		drag = { layer, index, mode, x0: e.clientX, length0: t[index].length, by: 0 };
		onlayer(layer);
		frame = startOf(t, index);
		onedit(false);
	}
	function dragMove(e: PointerEvent) {
		if (!drag) return;
		const by = Math.round((e.clientX - drag.x0) / CELL);
		if (by === drag.by) return;
		drag.by = by;
		if (drag.mode === 'hold') {
			setLength(trackOf(drag.layer), drag.index, drag.length0 + by);
			edited();
		}
	}
	function dragEnd() {
		if (!drag) return;
		const d = drag;
		drag = null;
		if (d.mode !== 'move' || !d.by) return;
		const t = trackOf(d.layer);
		// drop it where the pointer landed, before or after the exposure there
		const target = startOf(t, d.index) + d.by;
		const at = exposureAt(t, Math.max(0, target));
		let to = at ? at.index : t.length - 1;
		if (to > d.index && at && target < at.start + t[at.index].length / 2) to--;
		if (to < d.index && at && target >= at.start + t[at.index].length / 2) to++;
		to = Math.max(0, Math.min(t.length - 1, to));
		moveExposure(t, d.index, to);
		frame = startOf(t, Math.min(to, t.length - 1));
		edited();
	}

	// ---- sound and markers: placed along the scene, shown for this cut ----
	/** the scene frame this cut starts on */
	const sceneAt = $derived(cutStart(theScene, cut, statics));
	const clips = $derived(theScene.audio ?? []);
	const markersHere = $derived(
		(theScene.markers ?? [])
			.map((m) => ({ ...m, f: m.frame - sceneAt }))
			.filter((m) => m.f >= 0 && m.f < shown)
	);
	/** a sound's length in frames (`tick`: sounds decoded since, so this runs again) */
	// eslint-disable-next-line @typescript-eslint/no-unused-vars -- `tick` is the dependency
	const soundFrames = (file: string, f: number, tick: number) => audio?.frames(file, f) ?? 0;
	let fileInput = $state<HTMLInputElement>();
	let markersOpen = $state(false);

	/** Draw a clip's waveform for the frames it covers in this cut. */
	function waveform(
		el: HTMLCanvasElement,
		args: { file: string; from: number; frames: number; fps: number; tick: number }
	) {
		const draw = (a: typeof args) => {
			if (!audio) return;
			const peaks = audio.peaks(a.file, a.fps);
			const dpr = window.devicePixelRatio || 1;
			const [w, h] = [Math.max(1, a.frames * CELL), 24];
			el.width = Math.round(w * dpr);
			el.height = Math.round(h * dpr);
			el.style.width = `${w}px`;
			const g = el.getContext('2d');
			if (!g) return;
			g.scale(dpr, dpr);
			g.clearRect(0, 0, w, h);
			g.fillStyle = '#9fd3b6';
			for (let f = 0; f < a.frames; f++) {
				const p = peaks[a.from + f] ?? 0;
				// (silence: no bar)
				if (p < 0.004) continue;
				const bar = Math.max(1, p * (h - 2));
				g.fillRect(f * CELL + 1, (h - bar) / 2, CELL - 2, bar);
			}
		};
		draw(args);
		return { update: draw };
	}

	/** Slide a clip along the scene (dragging it). */
	let clipDrag = $state<{ id: number; x0: number; start0: number } | null>(null);
	function clipDown(e: PointerEvent, id: number) {
		if (e.button !== 0) return;
		e.stopPropagation();
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		const c = clips.find((x) => x.id === id);
		if (c) clipDrag = { id, x0: e.clientX, start0: c.start };
	}
	function clipMove(e: PointerEvent) {
		if (!clipDrag) return;
		const c = clips.find((x) => x.id === clipDrag!.id);
		const start = clipDrag.start0 + Math.round((e.clientX - clipDrag.x0) / CELL);
		if (c && c.start !== start) {
			c.start = start;
			edited();
		}
	}
	function removeClip(id: number) {
		const list = theScene.audio ?? [];
		const i = list.findIndex((c) => c.id === id);
		if (i >= 0) list.splice(i, 1);
		if (!list.length) delete theScene.audio;
		edited();
	}

	function cellClick(e: MouseEvent, layer: number) {
		const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
		onlayer(layer);
		pick(e, Math.floor((e.clientX - box.left) / CELL));
	}

	// ---- frame tools: on the chosen frames (Shift+click), or the drawing here ----
	/** frames chosen with Shift+click (none: the active layer's drawing on this frame) */
	let sel = $state<FrameRange | null>(null);
	/** a click goes to the frame; Shift+click chooses the frames from here to there */
	function pick(e: MouseEvent, f: number) {
		if (e.shiftKey) {
			sel = { from: Math.min(frame, f), to: Math.max(frame, f) };
			return;
		}
		sel = null;
		go(f);
	}
	let toolsOpen = $state(false);
	let scope = $state<'layer' | 'all'>('layer');
	let cycleTimes = $state(2);
	let cycleMode = $state<'loop' | 'pingpong'>('loop');
	let boilSet = $state<BoilSettings>({ ...BOIL_DEFAULTS });
	/** copied frames: one layer's (pasted on the active layer), or every layer's */
	let clip = $state<{ one: number[] } | { all: Record<number, number[]> } | null>(null);
	let toolNote = $state('');

	/** the frames the tools work on */
	const target = $derived<FrameRange | null>(
		sel ?? (activeStatic ? null : exposureRange(activeTrack, frame))
	);
	/** the tracks the tools work on: the active layer's, or every cel layer's */
	function targets(): Track[] {
		if (scope === 'layer') return activeStatic ? [] : [trackOf(active)];
		return rows.filter((l) => !statics.includes(l.id)).map((l) => trackOf(l.id));
	}
	const rangeLabel = (r: FrameRange | null) =>
		!r
			? 'nothing here'
			: r.from === r.to
				? `frame ${r.from + 1}`
				: `frames ${r.from + 1}–${r.to + 1}`;

	/** Copy the frames (the same drawings, to paste elsewhere). */
	export function copyRange() {
		const r = target;
		if (!r) return;
		clip =
			scope === 'layer'
				? { one: copyFrames(trackOf(active), r) }
				: {
						all: Object.fromEntries(
							rows
								.filter((l) => !statics.includes(l.id))
								.map((l) => [l.id, copyFrames(trackOf(l.id), r)])
						)
					};
		toolNote = `Copied ${rangeLabel(r)}.`;
	}
	/** Paste copied frames before this one (the same drawings, so changing one changes both). */
	export function pasteRange() {
		const c = clip;
		if (!c) return;
		if ('one' in c) {
			if (activeStatic) return;
			pasteFrames(trackOf(active), frame, c.one);
		} else for (const [l, f] of Object.entries(c.all)) pasteFrames(trackOf(+l), frame, f);
		toolNote = 'Pasted: the same drawings as the copied frames.';
		edited();
	}
	/** The frames again right after them, as new drawings to change. */
	export async function duplicateRange() {
		const r = target;
		const ts = targets();
		if (!r || !ts.length) return;
		await drawings.ensure(ts.flatMap((t) => copyFrames(t, r)));
		let at = r.to + 1;
		for (const t of ts) at = duplicateFrames(t, r, drawings.copy);
		sel = null;
		frame = at;
		toolNote = 'Duplicated as new drawings.';
		edited();
	}
	function reverseRange() {
		const r = target;
		if (!r) return;
		for (const t of targets()) reverseFrames(t, r);
		edited();
	}
	function cycleRange() {
		const r = target;
		if (!r) return;
		for (const t of targets()) cycleFrames(t, r, cycleTimes, cycleMode);
		toolNote = `${rangeLabel(r)} ${cycleMode === 'loop' ? 'looped' : 'back and forth'} ${cycleTimes}× more.`;
		edited();
	}
	let boiling = $state(false);
	async function boilRange() {
		const r = target;
		const ts = targets();
		if (!r || !ts.length || boiling) return;
		boiling = true;
		try {
			await drawings.ensure(ts.flatMap((t) => copyFrames(t, r)));
			// a new wobble each time (so boiling twice doesn't repeat itself)
			const seed = Math.floor(Math.random() * 1e6);
			let made = 0;
			for (const t of ts)
				made += boilFrames(t, r, boilSet, (d, n) => drawings.boil(d, boilSet.amount, seed + n));
			toolNote = made
				? `Boiled: ${made} wobbled cop${made === 1 ? 'y' : 'ies'}, swapped every ${boilSet.step} frames.`
				: `Nothing held longer than ${boilSet.step} frames there to boil.`;
			if (made) edited();
		} finally {
			boiling = false;
		}
	}

	const seconds = (f: number) => `${Math.floor(f / fps)}+${String(f % fps).padStart(2, '0')}`;

	// ---- scrubbing: drag along the frame numbers ----
	let scrubbing = false;
	function scrubAt(e: PointerEvent) {
		const ruler = e.currentTarget as HTMLElement;
		const f = Math.floor((e.clientX - ruler.getBoundingClientRect().left) / CELL);
		if (f !== frame) go(Math.max(0, Math.min(shown - 1, f)));
	}
	const inRange = (f: number) => !!playRange && f >= playRange.from && f <= playRange.to;
	let onionOpen = $state(false);
</script>

<section class="timeline" aria-label="Timeline">
	<div class="bar">
		<label class="pick">
			<span class="sr">Scene</span>
			<select
				aria-label="Scene"
				value={scene}
				onchange={(e) => {
					scene = +e.currentTarget.value;
					cut = 0;
					go(0);
				}}
			>
				{#each timeline.scenes as s, i (s.id)}<option value={i}>{s.name}</option>{/each}
			</select>
		</label>
		<label class="pick">
			<select
				aria-label="Cut"
				value={cut}
				onchange={(e) => {
					cut = +e.currentTarget.value;
					go(0);
				}}
			>
				{#each theScene.cuts as c, i (c.id)}<option value={i}>
						{c.name} · {(cutLength(c, statics) / fps).toFixed(1)} s
					</option>{/each}
			</select>
		</label>
		<select
			aria-label="Into the next cut"
			title={TRANSITIONS.find((t) => t.id === (theCut.transition ?? ''))?.note}
			value={theCut.transition ?? ''}
			onchange={(e) => {
				const v = e.currentTarget.value;
				if (v === 'flash' || v === 'dissolve') theCut.transition = v;
				else delete theCut.transition;
				edited();
			}}
		>
			{#each TRANSITIONS as t (t.id)}
				<option value={t.id}>→ {t.label}</option>
			{/each}
		</select>
		<button type="button" class="b" title="A new cut after this one" onclick={newCut}>+ Cut</button>
		<button type="button" class="b" title="A new scene after this one" onclick={newScene}>
			+ Scene
		</button>
		<span class="sep"></span>
		<button
			type="button"
			class="b play"
			aria-pressed={playing}
			aria-label={playing ? 'Stop' : 'Play'}
			title={playing ? 'Stop' : 'Play the cut (or the loop range)'}
			onclick={onplay}
		>
			{playing ? '■' : '▶'}
		</button>
		<select aria-label="Frames a second" title="Frames a second" bind:value={fps}>
			{#each [6, 8, 12, 15, 24, 25, 30, 48, 50, 60] as n (n)}<option value={n}>
					{n} fps
				</option>{/each}
		</select>
		<div class="seg" role="radiogroup" aria-label="Play what">
			{#each [['cut', 'Cut'], ['scene', 'Scene']] as [id, label] (id)}
				<button
					type="button"
					class="b"
					role="radio"
					aria-checked={playScope === id}
					title={id === 'scene'
						? 'Play the whole scene, every cut, with the transitions between them'
						: 'Play this cut (or its loop range)'}
					onclick={() => (playScope = id as 'cut' | 'scene')}
				>
					{label}
				</button>
			{/each}
		</div>
		<div class="seg" role="radiogroup" aria-label="Play">
			{#each [['once', 'Once'], ['loop', 'Loop'], ['pingpong', 'Ping-pong']] as [id, label] (id)}
				<button
					type="button"
					class="b"
					role="radio"
					aria-checked={playMode === id}
					onclick={() => (playMode = id as PlayMode)}
				>
					{label}
				</button>
			{/each}
		</div>
		<button
			type="button"
			class="b"
			title="Play from this frame"
			aria-pressed={!!playRange && playRange.from === frame}
			onclick={() => onrange('from')}
		>
			In
		</button>
		<button
			type="button"
			class="b"
			title="Play to this frame"
			aria-pressed={!!playRange && playRange.to === frame}
			onclick={() => onrange('to')}
		>
			Out
		</button>
		{#if playRange}
			<button type="button" class="b" title="Play the whole cut" onclick={() => (playRange = null)}>
				Frames {playRange.from + 1}–{playRange.to + 1} ×
			</button>
		{/if}
		<span class="sep"></span>
		<div class="onion">
			<button
				type="button"
				class="b"
				aria-pressed={onion.on}
				title="Onion skin: the drawings before (red) and after (green), faintly"
				onclick={() => (onion.on = !onion.on)}
			>
				Onion
			</button>
			<button
				type="button"
				class="b caret"
				aria-label="Onion skin settings"
				aria-expanded={onionOpen}
				onclick={() => (onionOpen = !onionOpen)}
			>
				▾
			</button>
			{#if onionOpen}
				<div class="pop" role="group" aria-label="Onion skin">
					<label>
						Before <input type="number" min="0" max="5" bind:value={onion.before} />
						<input type="color" aria-label="Colour before" bind:value={onion.beforeColour} />
					</label>
					<label>
						After <input type="number" min="0" max="5" bind:value={onion.after} />
						<input type="color" aria-label="Colour after" bind:value={onion.afterColour} />
					</label>
					<label>
						Opacity <b>{Math.round(onion.opacity * 100)}%</b>
						<input type="range" min="0.05" max="1" step="0.05" bind:value={onion.opacity} />
					</label>
					<label class="check">
						<input type="checkbox" bind:checked={onion.keysOnly} />
						Keys and breakdowns only
					</label>
					<div class="seg" role="radiogroup" aria-label="Onion skin layers">
						{#each [['active', 'This layer'], ['all', 'All layers']] as [id, label] (id)}
							<button
								type="button"
								class="b"
								role="radio"
								aria-checked={onion.layers === id}
								onclick={() => (onion.layers = id as 'active' | 'all')}
							>
								{label}
							</button>
						{/each}
					</div>
				</div>
			{/if}
		</div>
		{#if onflip}
			{#each [['roll', 'Flip', 'Hold to flip the drawings around this one (R)'], ['keys', 'Flip keys', 'Hold to flip through the keys and breakdowns (Shift+R)']] as [kind, label, title] (kind)}
				<button
					type="button"
					class="b"
					{title}
					onpointerdown={(e) => {
						(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
						onflip(kind as 'roll' | 'keys', true);
					}}
					onpointerup={() => onflip(kind as 'roll' | 'keys', false)}
					onpointercancel={() => onflip(kind as 'roll' | 'keys', false)}
				>
					{label}
				</button>
			{/each}
		{/if}
		{#if onmark}
			{@const markable = drawingAt(activeTrack, frame)}
			{@const k = markable ? timeline.kinds?.[markable] : undefined}
			<button
				type="button"
				class="b"
				disabled={!markable}
				aria-label="Mark this drawing: {k ? KIND_NAME[k] : 'unmarked'}"
				title="Mark this drawing as a key, breakdown or in-between (Shift+K)"
				onclick={onmark}
			>
				{k ? KIND_NAME[k] : 'Mark'}
			</button>
		{/if}
		<button
			type="button"
			class="b"
			aria-pressed={pinned.includes(frame)}
			title="Light table: pin this frame to show it (blue) while you draw others"
			onclick={onpin}
		>
			Pin frame
		</button>
		{#if pinned.length}
			<button type="button" class="b" title="Unpin every frame" onclick={() => (pinned = [])}>
				{pinned.length} pinned ×
			</button>
		{/if}
		<span class="sep"></span>
		<button
			type="button"
			class="b"
			title="Add sound (WAV, MP3, OGG, M4A, FLAC) starting on this frame"
			onclick={() => fileInput?.click()}
		>
			+ Sound
		</button>
		<input
			bind:this={fileInput}
			class="sr"
			type="file"
			accept={AUDIO_TYPES}
			multiple
			aria-label="Sound files"
			tabindex="-1"
			onchange={(e) => {
				const files = [...(e.currentTarget.files ?? [])];
				e.currentTarget.value = '';
				if (files.length) onsound?.(files);
			}}
		/>
		<div class="onion">
			<button
				type="button"
				class="b"
				aria-pressed={markersHere.some((m) => m.f === frame)}
				title="A marker on this frame, for syncing to a beat or a line (Shift+M, even while playing)"
				onclick={() => onmarker?.()}
			>
				Marker
			</button>
			<button
				type="button"
				class="b caret"
				aria-label="Markers"
				aria-expanded={markersOpen}
				onclick={() => (markersOpen = !markersOpen)}
			>
				▾
			</button>
			{#if markersOpen}
				<div class="pop" role="group" aria-label="Markers in this scene">
					{#if !theScene.markers?.length}
						<p class="hint">No markers yet. Press Shift+M on a frame, or tap it while playing.</p>
					{/if}
					{#each theScene.markers ?? [] as m, i (m.frame)}
						<div class="line">
							<button
								type="button"
								class="b"
								title="Go to it"
								onclick={() => {
									const at = m.frame - sceneAt;
									if (at >= 0) go(at);
								}}
							>
								{m.frame + 1}
							</button>
							<input
								class="mark-label"
								aria-label="Marker {m.frame + 1} label"
								placeholder="label"
								bind:value={m.label}
								onchange={edited}
							/>
							<button
								type="button"
								class="b"
								aria-label="Remove marker {m.frame + 1}"
								onclick={() => {
									theScene.markers?.splice(i, 1);
									if (!theScene.markers?.length) delete theScene.markers;
									edited();
								}}
							>
								×
							</button>
						</div>
					{/each}
				</div>
			{/if}
		</div>
		<div class="onion">
			<button
				type="button"
				class="b"
				aria-pressed={cameraView.show && (!!theCut.camera || cameraView.safe)}
				title="Camera: the part of the canvas this cut shows (playback shows only that)"
				onclick={() => (cameraView.show = !cameraView.show)}
			>
				Camera
			</button>
			<button
				type="button"
				class="b caret"
				aria-label="Camera settings"
				aria-expanded={cameraOpen}
				onclick={() => (cameraOpen = !cameraOpen)}
			>
				▾
			</button>
			{#if cameraOpen}
				<div class="pop wide" role="group" aria-label="Camera">
					<div class="seg wrap" role="group" aria-label="Camera shape">
						<button
							type="button"
							class="b"
							aria-pressed={!theCut.camera}
							onclick={() => setCamera(null)}
						>
							Whole canvas
						</button>
						{#each ASPECTS as [label, k] (label)}
							<button type="button" class="b" onclick={() => setCamera(fitCamera(k, canvasSize))}>
								{label}
							</button>
						{/each}
					</div>
					<div class="nums">
						{#each [['x', 'X'], ['y', 'Y'], ['width', 'W'], ['height', 'H']] as [key, label] (key)}
							<label>
								{label}
								<input
									type="number"
									aria-label="Camera {key}"
									min="0"
									value={Math.round(camera[key as keyof Camera])}
									onchange={(e) => cameraField(key as keyof Camera, +e.currentTarget.value)}
								/>
							</label>
						{/each}
					</div>
					<div class="seg">
						<button
							type="button"
							class="b"
							title="Zoom in (a smaller frame)"
							onclick={() => zoomCamera(0.9)}
						>
							Zoom in
						</button>
						<button
							type="button"
							class="b"
							title="Zoom out (a bigger frame)"
							onclick={() => zoomCamera(1 / 0.9)}
						>
							Zoom out
						</button>
					</div>
					<label>
						<input type="checkbox" bind:checked={cameraView.safe} />
						Safe areas (action 90%, title 80%)
					</label>
					<small class="hint">Each cut has its own camera; a new cut starts with this one.</small>
				</div>
			{/if}
		</div>
		<span class="sep"></span>
		<button type="button" class="b" aria-label="Previous frame" onclick={() => go(frame - 1)}>
			‹
		</button>
		<span class="where" aria-live="polite">
			Frame <b data-testid="frame">{frame + 1}</b> of {length}
			<small>{seconds(frame)}</small>
		</span>
		<button type="button" class="b" aria-label="Next frame" onclick={() => go(frame + 1)}>›</button>
		<span class="sep"></span>
		<button
			type="button"
			class="b strong"
			title={activeStatic
				? 'A static layer shows one drawing all cut long'
				: 'A new, empty drawing after this one on the active layer'}
			disabled={activeStatic}
			onclick={addDrawing}
		>
			New drawing
		</button>
		<div class="seg" role="group" aria-label="Hold this drawing for">
			{#each [1, 2, 3] as n (n)}
				<button
					type="button"
					class="b"
					aria-pressed={here ? activeTrack[here.index].length === n : false}
					disabled={!here}
					title="Hold this drawing for {n} frame{n > 1 ? 's' : ''} (on {n === 1
						? 'ones'
						: n === 2
							? 'twos'
							: 'threes'})"
					onclick={() => timing(n)}
				>
					{n}s
				</button>
			{/each}
		</div>
		<button
			type="button"
			class="b"
			aria-label="Hold longer"
			title="Hold this drawing one frame longer"
			disabled={!here}
			onclick={() => hold(1)}
		>
			+1
		</button>
		<button
			type="button"
			class="b"
			aria-label="Hold shorter"
			title="Hold this drawing one frame less"
			disabled={!here || activeTrack[here.index].length < 2}
			onclick={() => hold(-1)}
		>
			−1
		</button>
		<select
			class="b"
			aria-label="Timing for the whole layer"
			title="Every drawing on this layer on ones, twos or threes"
			disabled={activeStatic}
			value=""
			onchange={(e) => {
				const n = +e.currentTarget.value;
				e.currentTarget.value = '';
				if (n) timingAll(n);
			}}
		>
			<option value="">Layer timing…</option>
			<option value="1">All on ones</option>
			<option value="2">All on twos</option>
			<option value="3">All on threes</option>
		</select>
		<span class="sep"></span>
		<button
			type="button"
			class="b"
			title="One more frame here on every layer (each holds what it shows)"
			onclick={insertFrame}
		>
			Insert frame
		</button>
		<button
			type="button"
			class="b"
			title="Take this frame out of every layer"
			onclick={deleteFrame}
		>
			Delete frame
		</button>
		<button
			type="button"
			class="b"
			title="Take this drawing out of the timeline (the ones after it move back)"
			disabled={!here}
			onclick={removeHere}
		>
			Remove drawing
		</button>
		<div class="onion">
			<button
				type="button"
				class="b"
				aria-expanded={toolsOpen}
				title="Frame tools: copy, paste, duplicate, reverse, cycle, boil"
				onclick={() => (toolsOpen = !toolsOpen)}
			>
				Frames ▾
			</button>
			{#if toolsOpen}
				<div class="pop wide right" role="group" aria-label="Frame tools">
					<p class="hint" data-testid="frames-target">
						On {rangeLabel(target)}{sel ? '' : ' (this drawing)'}. Shift+click frames to choose
						others.
					</p>
					<div class="seg" role="radiogroup" aria-label="Frame tools work on">
						{#each [['layer', 'This layer'], ['all', 'All layers']] as [id, label] (id)}
							<button
								type="button"
								class="b"
								role="radio"
								aria-checked={scope === id}
								onclick={() => (scope = id as 'layer' | 'all')}
							>
								{label}
							</button>
						{/each}
					</div>
					<div class="seg wrap">
						<button
							type="button"
							class="b"
							disabled={!target}
							title="Copy these frames (Alt+C)"
							onclick={copyRange}
						>
							Copy
						</button>
						<button
							type="button"
							class="b"
							disabled={!clip}
							title="Paste before this frame: the same drawings, so changing one changes both (Alt+V)"
							onclick={pasteRange}
						>
							Paste
						</button>
						<button
							type="button"
							class="b"
							disabled={!target}
							title="The frames again right after them, as new drawings to change (Shift+D)"
							onclick={duplicateRange}
						>
							Duplicate
						</button>
						<button
							type="button"
							class="b"
							disabled={!target || target.from === target.to}
							title="Play these frames backwards"
							onclick={reverseRange}
						>
							Reverse
						</button>
					</div>
					<div class="line">
						<span>Cycle</span>
						<input
							type="number"
							min="1"
							max="99"
							aria-label="Cycle times"
							bind:value={cycleTimes}
						/>
						<span>× more</span>
						<select aria-label="Cycle as" bind:value={cycleMode}>
							<option value="loop">Loop (A B C A B C)</option>
							<option value="pingpong">Back and forth (A B C B A)</option>
						</select>
						<button
							type="button"
							class="b"
							disabled={!target}
							title="Repeat these frames after them (the same drawings)"
							onclick={cycleRange}
						>
							Cycle
						</button>
					</div>
					<div class="line">
						<span>Boil</span>
						<select aria-label="Boil drawings" bind:value={boilSet.variants}>
							<option value={2}>2 drawings</option>
							<option value={3}>3 drawings</option>
							<option value={4}>4 drawings</option>
						</select>
						<select aria-label="Boil every" bind:value={boilSet.step}>
							<option value={1}>on ones</option>
							<option value={2}>on twos</option>
							<option value={3}>on threes</option>
						</select>
						<select aria-label="Boil strength" bind:value={boilSet.amount}>
							<option value={0.6}>Subtle</option>
							<option value={1}>Light</option>
							<option value={2}>Medium</option>
							<option value={3.5}>Strong</option>
						</select>
						<button
							type="button"
							class="b"
							disabled={!target || boiling}
							title="Held drawings swap between themselves and slightly wobbled copies, as if traced again"
							onclick={boilRange}
						>
							{boiling ? 'Boiling…' : 'Boil'}
						</button>
					</div>
					{#if toolNote}<p class="hint" aria-live="polite">{toolNote}</p>{/if}
				</div>
			{/if}
		</div>
	</div>

	<div class="grid" style="--cell: {CELL}px; --cols: {shown}">
		<div class="corner"></div>
		<div
			class="ruler"
			role="group"
			aria-label="Frames"
			onpointerdown={(e) => {
				if (e.button !== 0 || e.shiftKey) return;
				// (a plain click or drag here goes to frames: the chosen ones are let go)
				sel = null;
				scrubbing = true;
				(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
				scrubAt(e);
			}}
			onpointermove={(e) => scrubbing && scrubAt(e)}
			onpointerup={() => (scrubbing = false)}
			onpointercancel={() => (scrubbing = false)}
		>
			{#each Array.from({ length: shown }, (_, i) => i) as f (f)}
				<button
					type="button"
					class="tick"
					class:now={f === frame}
					class:past={f >= length}
					class:ranged={inRange(f)}
					class:pinned={pinned.includes(f)}
					class:chosen={inSel(sel, f)}
					class:marked={markersHere.some((m) => m.f === f)}
					title={markersHere.find((m) => m.f === f)?.label || undefined}
					aria-label="Frame {f + 1}{markersHere.some((m) => m.f === f) ? ', marker' : ''}"
					aria-current={f === frame ? 'true' : undefined}
					onclick={(e) => pick(e, f)}
				>
					{f === 0 || (f + 1) % 5 === 0 ? f + 1 : ''}
				</button>
			{/each}
		</div>
		{#each rows as l (l.id)}
			{@const t = theCut.tracks[l.id] ?? []}
			{@const nums = numbers(t)}
			{@const still = statics.includes(l.id)}
			<div class="name" class:on={l.id === active}>
				<button type="button" class="label" onclick={() => onlayer(l.id)} title={l.name}>
					{l.name}
				</button>
				<button
					type="button"
					class="kind"
					aria-pressed={still}
					aria-label="Static layer: {l.name}"
					title={still
						? 'Static: one drawing all cut long (a background or overlay). Click for a cel layer.'
						: 'Cel layer: a drawing per exposure. Click to make it static (a background or overlay).'}
					onclick={() => toggleStatic(l.id)}
				>
					{still ? 'S' : 'C'}
				</button>
			</div>
			<div
				class="row"
				class:on={l.id === active}
				class:still
				role="presentation"
				data-layer={l.id}
				onclick={(e) => cellClick(e, l.id)}
			>
				<div class="playhead" style="left: {frame * CELL}px"></div>
				{#if sel && (scope === 'all' || l.id === active) && !still}
					<div
						class="band"
						style="left: {sel.from * CELL}px; width: {(sel.to - sel.from + 1) * CELL}px"
					></div>
				{/if}
				{#if still}
					{#if staticDrawing(t)}
						<div
							class="cel static"
							class:here={l.id === active}
							style="left: 0; width: {length * CELL}px"
							aria-label="{l.name}: static, all cut long"
						>
							<span>Static</span>
						</div>
					{/if}
				{:else}
					{#each t as e, i (i)}
						{@const start = startOf(t, i)}
						{#if e.drawing}
							<button
								type="button"
								class="cel"
								class:here={l.id === active && here?.index === i}
								class:dragging={drag?.layer === l.id && drag.index === i}
								style="left: {start * CELL}px; width: {e.length * CELL}px; {drag?.layer === l.id &&
								drag.index === i &&
								drag.mode === 'move'
									? `transform: translateX(${drag.by * CELL}px)`
									: ''}"
								data-kind={timeline.kinds?.[e.drawing]}
								aria-label="{l.name}: drawing {nums[i]}{timeline.kinds?.[e.drawing]
									? ` (${KIND_NAME[timeline.kinds[e.drawing]]})`
									: ''}, frame {start + 1}{e.length > 1 ? `–${start + e.length}` : ''}"
								onpointerdown={(ev) => dragStart(ev, l.id, i, 'move')}
								onpointermove={dragMove}
								onpointerup={dragEnd}
								onpointercancel={() => (drag = null)}
								onclick={(ev) => ev.stopPropagation()}
							>
								<span>{nums[i]}</span>
								{#if timeline.kinds?.[e.drawing]}
									<span class="kind-mark" aria-hidden="true">
										{KIND_SHORT[timeline.kinds[e.drawing]]}
									</span>
								{/if}
								<span
									class="edge"
									role="presentation"
									title="Drag to hold it longer or shorter"
									onpointerdown={(ev) => dragStart(ev, l.id, i, 'hold')}
									onpointermove={dragMove}
									onpointerup={dragEnd}
								></span>
							</button>
						{/if}
					{/each}
				{/if}
			</div>
		{/each}
		{#each clips as c (c.id)}
			{@const len = soundFrames(c.file, fps, soundTick)}
			{@const from = Math.max(0, sceneAt - c.start)}
			{@const left = Math.max(0, c.start - sceneAt)}
			{@const frames = Math.max(0, Math.min(len - from, shown - left))}
			<div class="name sound" class:muted={c.muted}>
				<span class="label" title={c.name}>♪ {c.name}</span>
				<button
					type="button"
					class="kind"
					aria-pressed={!c.muted}
					aria-label="Sound on: {c.name}"
					title={c.muted ? 'Muted: click to hear it' : 'Click to mute'}
					onclick={() => {
						c.muted = !c.muted;
						edited();
					}}
				>
					{c.muted ? '×' : '♪'}
				</button>
				{#if onlipsync}
					<button
						type="button"
						class="kind"
						aria-label="Lip-sync: {c.name}"
						title="Lip-sync: mouths for this line"
						onclick={() => onlipsync(c.id)}
					>
						<svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
							<path
								d="M1.5 8c2-2.6 4-3.4 6.5-1.6C10.5 4.6 12.5 5.4 14.5 8c-2 2.8-4.2 4-6.5 4S3.5 10.8 1.5 8Z M1.5 8h13"
								fill="none"
								stroke="currentColor"
								stroke-width="1.5"
								stroke-linejoin="round"
							/>
						</svg>
					</button>
				{/if}
				<button
					type="button"
					class="kind"
					aria-label="Remove sound: {c.name}"
					title="Take this sound out"
					onclick={() => removeClip(c.id)}
				>
					−
				</button>
			</div>
			<div class="row sound" role="presentation" data-sound={c.id}>
				<div class="playhead" style="left: {frame * CELL}px"></div>
				{#if frames > 0}
					<div
						class="clip"
						class:dragging={clipDrag?.id === c.id}
						role="slider"
						tabindex="-1"
						aria-label="{c.name}: starts on scene frame {c.start + 1}"
						aria-valuenow={c.start + 1}
						title="Drag to move it along the scene"
						style="left: {left * CELL}px; width: {frames * CELL}px"
						onpointerdown={(e) => clipDown(e, c.id)}
						onpointermove={clipMove}
						onpointerup={() => (clipDrag = null)}
						onpointercancel={() => (clipDrag = null)}
					>
						<canvas use:waveform={{ file: c.file, from, frames, fps, tick: soundTick }}></canvas>
					</div>
				{/if}
			</div>
		{/each}
	</div>
</section>

<style>
	.timeline {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 8px 10px 10px;
		border-radius: 18px;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		font: 13px var(--pp-font-body);
	}
	.bar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
	}
	.bar > * {
		flex: none;
	}
	.b,
	select {
		height: 32px;
		padding: 0 10px;
		border: 0;
		border-radius: 10px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.b:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}
	.b[aria-pressed='true'],
	.b[aria-checked='true'],
	.b.strong {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.seg {
		display: flex;
		gap: 2px;
	}
	.onion {
		position: relative;
		display: flex;
		gap: 2px;
	}
	.caret {
		padding: 0 6px;
	}
	.pop {
		position: absolute;
		bottom: calc(100% + 8px);
		left: 0;
		z-index: 30;
		display: grid;
		gap: 8px;
		width: 230px;
		padding: 12px;
		border-radius: 14px;
		background: #1f1f24;
		box-shadow: 0 12px 32px rgba(0, 0, 0, 0.35);
	}
	.pop label {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.pop input[type='number'] {
		width: 48px;
		height: 28px;
		border: 0;
		border-radius: 8px;
		background: #2a2a30;
		color: var(--pp-on-ink);
	}
	.pop input[type='color'] {
		width: 34px;
		height: 26px;
		padding: 0;
		border: 0;
		background: none;
	}
	.pop input[type='range'] {
		flex: 1;
		accent-color: var(--pp-yellow);
	}
	.sep {
		width: 1px;
		height: 22px;
		background: rgba(255, 255, 255, 0.15);
	}
	.where {
		min-width: 120px;
		text-align: center;
	}
	.where small {
		color: var(--pp-on-ink-muted);
		margin-left: 4px;
	}
	.sr {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
	.grid {
		display: grid;
		grid-template-columns: 110px calc(var(--cell) * var(--cols));
		max-height: 168px;
		overflow: auto;
		scrollbar-width: thin;
	}
	.corner,
	.ruler {
		position: sticky;
		top: 0;
		z-index: 2;
		background: var(--pp-ink);
	}
	.corner {
		left: 0;
		z-index: 3;
	}
	.ruler {
		display: flex;
		height: 20px;
		touch-action: none;
	}
	.tick {
		flex: none;
		width: var(--cell);
		padding: 0;
		border: 0;
		border-left: 1px solid rgba(255, 255, 255, 0.08);
		background: none;
		color: var(--pp-on-ink-muted);
		font: 600 10px var(--pp-font-body);
		cursor: pointer;
	}
	.tick.past {
		opacity: 0.45;
	}
	.tick.marked {
		background-image: linear-gradient(135deg, #ffd650 0 6px, transparent 6px);
	}
	.name.sound .label {
		flex: 1;
		min-width: 0;
		padding: 0 4px 0 8px;
		font: 600 12px var(--pp-font-body);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.name.sound.muted {
		opacity: 0.55;
	}
	.row.sound {
		overflow: hidden;
		background: #1f1f24;
		cursor: default;
	}
	.clip {
		position: absolute;
		top: 2px;
		bottom: 2px;
		border-radius: 6px;
		background: #2c4a3b;
		cursor: grab;
		touch-action: none;
		overflow: hidden;
	}
	.clip.dragging {
		cursor: grabbing;
		outline: 2px solid #9fd3b6;
	}
	.clip canvas {
		display: block;
		height: 24px;
		pointer-events: none;
	}
	.mark-label {
		flex: 1;
		min-width: 0;
		height: 28px;
		border: 0;
		border-radius: 8px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		padding: 0 8px;
	}
	.tick.pinned {
		box-shadow: inset 0 -3px 0 #3d6bff;
	}
	.tick.ranged {
		background: rgba(255, 214, 80, 0.22);
		color: var(--pp-on-ink);
	}
	.tick.now {
		background: var(--pp-accent-strong);
		color: #fff;
		border-radius: 6px 6px 0 0;
	}
	.name {
		position: sticky;
		left: 0;
		z-index: 1;
		display: flex;
		align-items: center;
		height: 28px;
		margin: 2px 0;
		border-radius: 8px 0 0 8px;
		background: #1f1f24;
		color: var(--pp-on-ink-muted);
	}
	.name.on {
		color: #fff;
		background: #34343c;
	}
	.name .label {
		flex: 1;
		min-width: 0;
		height: 100%;
		padding: 0 4px 0 8px;
		border: 0;
		background: none;
		color: inherit;
		font: 600 12px var(--pp-font-body);
		text-align: left;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		cursor: pointer;
	}
	.name .kind {
		flex: none;
		width: 20px;
		height: 20px;
		margin-right: 4px;
		padding: 0;
		border: 0;
		border-radius: 6px;
		background: #2a2a30;
		color: var(--pp-on-ink-muted);
		font: 700 10px var(--pp-font-body);
		cursor: pointer;
	}
	.name .kind[aria-pressed='true'] {
		background: #3d6bff;
		color: #fff;
	}
	.cel.static {
		padding-left: 8px;
		background: #c9d6ff;
		cursor: default;
	}
	.cel.static.here {
		background: #9fb6ff;
	}
	.pop.wide {
		width: 300px;
	}
	.pop.right {
		left: auto;
		right: 0;
		width: 360px;
	}
	.pop .line {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
	}
	.pop .line select {
		height: 30px;
		padding: 0 6px;
	}
	.pop p {
		margin: 0;
	}
	.tick.chosen {
		box-shadow: inset 0 -3px 0 var(--pp-yellow);
	}
	.band {
		position: absolute;
		top: 0;
		bottom: 0;
		background: rgba(255, 214, 80, 0.16);
		outline: 1px solid rgba(255, 214, 80, 0.55);
		pointer-events: none;
	}
	.seg.wrap {
		flex-wrap: wrap;
	}
	.nums {
		display: grid;
		grid-template-columns: repeat(4, 1fr);
		gap: 6px;
	}
	.nums label {
		flex-direction: column;
		align-items: stretch;
		gap: 2px;
		font-size: 11px;
	}
	.pop .nums input[type='number'] {
		width: 100%;
	}
	.hint {
		color: var(--pp-on-ink-muted);
	}
	.row {
		position: relative;
		height: 28px;
		margin: 2px 0;
		background: repeating-linear-gradient(
			90deg,
			#1f1f24 0 calc(var(--cell) - 1px),
			#2a2a30 calc(var(--cell) - 1px) var(--cell)
		);
		cursor: pointer;
	}
	.row.on {
		background: repeating-linear-gradient(
			90deg,
			#28282f 0 calc(var(--cell) - 1px),
			#36363e calc(var(--cell) - 1px) var(--cell)
		);
	}
	.playhead {
		position: absolute;
		top: 0;
		bottom: 0;
		width: var(--cell);
		background: rgba(255, 90, 60, 0.18);
		pointer-events: none;
	}
	.kind-mark {
		margin-left: 4px;
		padding: 0 4px;
		border-radius: 999px;
		background: var(--pp-ink);
		color: var(--pp-paper, #fff);
		font-size: 9px;
		line-height: 14px;
	}
	.cel[data-kind='breakdown'] .kind-mark {
		border-radius: 3px;
		background: #6b5bd6;
	}
	.cel[data-kind='inbetween'] .kind-mark {
		background: transparent;
		color: inherit;
		box-shadow: inset 0 0 0 1px currentColor;
	}
	.cel[data-kind='key'] {
		box-shadow: inset 0 0 0 2px var(--pp-ink);
	}
	.pop .check {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.cel {
		position: absolute;
		top: 3px;
		bottom: 3px;
		display: flex;
		align-items: center;
		padding: 0 0 0 6px;
		border: 0;
		border-radius: 6px;
		background: #f4efe3;
		color: var(--pp-ink);
		font: 700 11px var(--pp-font-body);
		cursor: grab;
		touch-action: none;
		box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.25);
	}
	.cel.here {
		background: var(--pp-yellow);
	}
	.cel.dragging {
		z-index: 2;
		opacity: 0.85;
		cursor: grabbing;
	}
	.edge {
		position: absolute;
		top: 0;
		right: 0;
		bottom: 0;
		width: 7px;
		border-radius: 0 6px 6px 0;
		background: rgba(0, 0, 0, 0.18);
		cursor: ew-resize;
	}
</style>
