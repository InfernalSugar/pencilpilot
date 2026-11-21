<script lang="ts">
	import { resolve } from '$app/paths';
	/**
	 * The drawing studio, in the app: the website's studio (same layout, tools and panels,
	 * from @pencilpilot/paint) without the website's practice parts, plus projects:
	 * Ctrl+S saves (.ppstudio), Ctrl+Shift+S saves as, Ctrl+O opens.
	 */
	import Studio from '@pencilpilot/paint/Studio.svelte';
	import ToolRail, { type ToolId } from '@pencilpilot/paint/ToolRail.svelte';
	import ToolOptionsBar from '@pencilpilot/paint/ToolOptionsBar.svelte';
	import QuickMenu, { type QuickAction } from '@pencilpilot/paint/QuickMenu.svelte';
	import TimelapsePlayer from '@pencilpilot/paint/TimelapsePlayer.svelte';
	import { ToolOptions } from '@pencilpilot/paint/tool-options.svelte.js';
	import { anyGuide, defaultGuides, type GuideSettings } from '@pencilpilot/paint/guides';
	import PaintCanvas, {
		BLANK_STATUS,
		type CanvasStatus
	} from '@pencilpilot/paint/PaintCanvas.svelte';
	import LayersPanel from '@pencilpilot/paint/LayersPanel.svelte';
	import BrushPicker from '@pencilpilot/paint/BrushPicker.svelte';
	import BrushTexturePanel from '@pencilpilot/paint/BrushTexturePanel.svelte';
	import ColorPanel from '@pencilpilot/paint/ColorPanel.svelte';
	import HistoryPanel from '@pencilpilot/paint/HistoryPanel.svelte';
	import Swatches from '@pencilpilot/paint/Swatches.svelte';
	import { brushTexture, onTextures, textureOn } from '@pencilpilot/paint/brush-textures';
	import {
		brushLabel,
		DEFAULT_BRUSH,
		loadManifest,
		type BrushManifest
	} from '@pencilpilot/paint/brushes';
	import Logo from '@pencilpilot/ui/Logo.svelte';
	import { onMount, tick, untrack } from 'svelte';
	import { isTauri } from '@tauri-apps/api/core';
	import {
		askExportPath,
		askOpenPath,
		askSavePath,
		discardRecovery,
		newDocId,
		openProject,
		openRecovery,
		openVersion,
		projectName,
		saveProject,
		writeExport,
		writeRecovery,
		parseStart,
		type ProjectDoc
	} from '#lib/project.js';
	import { beforeNavigate, goto } from '$app/navigation';
	import { loadSettings, settings } from '#lib/settings.svelte.js';
	import { actionFor, comboOf, showKey, type ActionId } from '#lib/shortcuts.js';
	import { page } from '$app/state';
	import VersionsMenu from '#lib/VersionsMenu.svelte';
	import TimelinePanel from '#lib/TimelinePanel.svelte';
	import { FrameCache } from '#lib/frames.js';
	import { cleanOnion, onionEntries, type OnionSettings } from '#lib/onion.js';
	import {
		clampRange,
		frameAt,
		framesOf,
		previewShrink,
		type PlayMode,
		type PlayRange
	} from '#lib/playback.js';
	import {
		cutLength,
		drawingAt,
		drawingsIn,
		findDrawing,
		fromFile,
		layerOfDrawing,
		newTimeline,
		setDrawing,
		showing,
		trackIn,
		toFile,
		type Timeline
	} from '#lib/timeline.js';

	let canvas = $state<PaintCanvas>();
	let status = $state<CanvasStatus | null>(null);
	let tool = $state<string>('brush');
	let eraser = $state(false);
	let size = $state(1);
	let opacity = $state(1);
	let color = $state('#16161a');
	let brushId = $state(DEFAULT_BRUSH);
	let manifest = $state<BrushManifest | null>(null);
	let tab = $state('layers');
	let dockOpen = $state(false);
	let smoothing = $state(0.2);
	/** the canvas size (a project brings its own) */
	/** what home asked for: a new canvas, a project file, or a recovery copy */
	const start = parseStart(page.url.searchParams);
	let canvasSize = $state(
		start.kind === 'new'
			? { width: start.width, height: start.height }
			: { width: 1920, height: 1080 }
	);
	let fps = $state(start.kind === 'new' ? start.fps : 24);
	/** bumps for a new, empty canvas (opening a project of another size remounts it) */
	let canvasKey = $state(0);

	// ---- the animation: which drawing each layer shows on each frame ----
	let timeline = $state<Timeline>(newTimeline([]));
	/** the timeline belongs to this canvas (a new canvas starts a new one) */
	let timelineFor = $state(-1);
	let scene = $state(0);
	let cutIndex = $state(0);
	let frame = $state(0);
	let timelinePanel = $state<TimelinePanel>();
	const theCut = () => {
		const s = timeline.scenes[scene] ?? timeline.scenes[0];
		return s.cuts[cutIndex] ?? s.cuts[0];
	};
	/** changes to the document (the canvas's and the timeline's); moving between frames isn't one */
	let docVersion = $state(0);
	/** the canvas is changing frames: its report isn't an edit */
	let quiet = false;
	/** the frame the canvas shows (the timeline's frame can be ahead of it for a moment) */
	let onCanvas = { scene: 0, cut: 0, frame: 0 };

	/** drawings not showing live in the app's frame store (crates/anim) */
	const cache = new FrameCache(() => canvas, isTauri());

	function canvasChanged(s: CanvasStatus) {
		status = s;
		if (quiet) return;
		docVersion++;
		// what changed: the drawings showing, or one undo / redo just changed
		cache.touch([
			...s.layers.map((l) => l.drawing),
			canvas?.undoDrawing() ?? 0,
			canvas?.redoDrawing() ?? 0
		]);
		// drawing on an empty cel started a drawing (or a pasted picture got one): it's
		// on this frame now
		if (timelineFor !== canvasKey) return;
		const cut = timeline.scenes[onCanvas.scene]?.cuts[onCanvas.cut];
		if (!cut) return;
		const f = onCanvas.frame;
		for (const l of s.layers)
			if (!l.adjust && l.drawing && drawingAt(cut.tracks[l.id], f) !== l.drawing)
				setDrawing(trackIn(cut, l.id), f, l.drawing);
	}

	/** frame changes, one after another (fetching drawings takes a moment) */
	let showQueue: Promise<void> = Promise.resolve();
	/** waiting for the store: the canvas takes no strokes meanwhile */
	let switching = $state(false);

	/** Show what the timeline has on the current frame (once the store sends its drawings). */
	function showFrame(): Promise<void> {
		showQueue = showQueue.then(showNow).catch((e) => note(`Couldn’t show that frame: ${e}`));
		return showQueue;
	}

	async function showNow() {
		if (!canvas || !status) return;
		// pictures pasted or imported here get drawings first, so they're kept
		canvas.ensureDrawings();
		const pairs = () =>
			showing(
				theCut(),
				(status?.layers ?? []).filter((l) => !l.adjust).map((l) => l.id),
				frame
			);
		if (cache.needs(pairs().map(([, d]) => d))) {
			switching = true;
			try {
				await cache.ensure(pairs().map(([, d]) => d));
			} finally {
				switching = false;
			}
		}
		const c = canvas;
		if (!c) return;
		const leaving = (status?.layers ?? []).map((l) => l.drawing);
		const now = pairs();
		quiet = true;
		const ok = c.showDrawings(now);
		quiet = false;
		if (ok) {
			onCanvas = { scene, cut: cutIndex, frame };
			cache.shown(now.map(([, d]) => d));
			void updateOnion();
			// what stopped showing goes to the store; the engine keeps a few
			const gone = leaving.filter((d) => d && !now.some(([, n]) => n === d));
			void cache.push(gone).then(() => cache.trim());
		} else {
			// stay where the canvas is
			[scene, cutIndex, frame] = [onCanvas.scene, onCanvas.cut, onCanvas.frame];
			note('Finish what you’re doing first (a transform, an adjustment or a mask).');
		}
	}

	/** The timeline changed (or the frame did). */
	function timelineEdited(changed: boolean) {
		if (changed) docVersion++;
		showFrame();
	}

	/** Undo / redo a stroke from another frame: go there first, so it's seen. */
	async function reveal(drawing: number) {
		if (!drawing || !status || status.layers.some((l) => l.drawing === drawing)) return;
		const at = findDrawing(timeline, drawing, { scene, cut: cutIndex });
		if (!at) {
			// not on the timeline any more: still bring it back, so undo has it
			await cache.ensure([drawing]);
			return;
		}
		[scene, cutIndex, frame] = [at.scene, at.cut, at.frame];
		await showFrame();
	}

	// ---- onion skin and light table: other drawings, tinted, over the canvas ----
	let onion = $state<OnionSettings>(cleanOnion(readLocal('pp.app.onion', null)));
	$effect(() => keepLocal('pp.app.onion', $state.snapshot(onion)));
	/** frames pinned to the light table (in this cut) */
	let pinned = $state<number[]>([]);
	let onionImage = $state.raw<ImageBitmap | null>(null);
	let onionToken = 0;

	/** Make the onion skin for the frame the canvas shows. */
	async function updateOnion() {
		const token = ++onionToken;
		const c = canvas;
		if (!c || !status || timelineFor !== canvasKey) return;
		const cut = timeline.scenes[onCanvas.scene]?.cuts[onCanvas.cut];
		if (!cut) return;
		const layers = status.layers.filter((l) => !l.adjust).map((l) => l.id);
		const entries = onionEntries(cut, layers, status.active, onCanvas.frame, onion, pinned);
		let bmp: ImageBitmap | null = null;
		if (entries.length) {
			await cache.ensure(entries.map((e) => e.drawing));
			if (token !== onionToken || !canvas) return;
			const img = canvas.onionRgba(entries, Math.max(1, Math.ceil(canvasSize.width / 1920)));
			if (img) bmp = await createImageBitmap(img);
		}
		if (token !== onionToken) return bmp?.close();
		onionImage?.close();
		onionImage = bmp;
	}
	// settings, pins or the active layer changed
	$effect(() => {
		void $state.snapshot(onion);
		void [...pinned];
		void status?.active;
		untrack(() => void updateOnion());
	});
	// another cut: its own pins
	$effect(() => {
		void [scene, cutIndex];
		untrack(() => (pinned = []));
	});
	function pinFrame() {
		pinned = pinned.includes(frame) ? pinned.filter((f) => f !== frame) : [...pinned, frame];
	}

	// ---- playback: frames rendered small, once each, then played at the frame rate ----
	let playing = $state(false);
	let playMode = $state<PlayMode>('loop');
	/** the frames to play (none: the whole cut) */
	let playRange = $state<PlayRange | null>(null);
	/** rendering frames before playing: how far along */
	let preparing = $state<{ done: number; total: number } | null>(null);
	let playerEl = $state<HTMLCanvasElement>();
	/** rendered frames, by what's on them (drawings and their changes, the layers) */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- a cache, not state
	let rendered = new Map<string, ImageBitmap>();
	let renderedFor = -1;
	let playTimer = 0;

	/** What a frame looks like depends on: its drawings (and their changes), the layers. */
	function frameKey(pairs: [number, number][], shrink: number) {
		const layers = (status?.layers ?? []).map((l) => [
			l.id,
			l.visible,
			l.opacity,
			l.blend,
			l.clip,
			l.mask,
			l.group,
			l.adjust,
			l.drawing ? 0 : l.empty
		]);
		return JSON.stringify([
			pairs.map(([l, d]) => [l, d, cache.version(d)]),
			layers,
			status?.groups ?? [],
			shrink
		]);
	}

	function drawPlayer(bmp: ImageBitmap | undefined) {
		const el = playerEl;
		if (!el || !bmp) return;
		const dpr = window.devicePixelRatio || 1;
		const [w, h] = [Math.round(el.clientWidth * dpr), Math.round(el.clientHeight * dpr)];
		if (el.width !== w || el.height !== h) [el.width, el.height] = [w, h];
		const g = el.getContext('2d');
		if (!g) return;
		const k = Math.min(w / bmp.width, h / bmp.height);
		const [dw, dh] = [bmp.width * k, bmp.height * k];
		g.fillStyle = getComputedStyle(el).backgroundColor || '#e8e4da';
		g.fillRect(0, 0, w, h);
		g.imageSmoothingQuality = 'high';
		g.drawImage(bmp, (w - dw) / 2, (h - dh) / 2, dw, dh);
	}

	/** Play the range (or the cut) from the start of it, or stop. */
	async function togglePlay() {
		if (playing) return stopPlay();
		if (!canvas || !status) return;
		await showQueue;
		const cut = theCut();
		const range = clampRange(playRange ?? { from: 0, to: cutLength(cut) - 1 }, cutLength(cut));
		const shrink = previewShrink(canvasSize.width);
		const layerIds = status.layers.filter((l) => !l.adjust).map((l) => l.id);
		if (renderedFor !== canvasKey) {
			for (const b of rendered.values()) b.close();
			rendered = new Map();
			renderedFor = canvasKey;
		}
		playing = true;
		// eslint-disable-next-line svelte/prefer-svelte-reactivity -- a local lookup, not state
		const keys = new Map<number, string>();
		const todo = framesOf(range);
		preparing = { done: 0, total: todo.length };
		try {
			for (const f of todo) {
				if (!playing || !canvas) return;
				const pairs = showing(cut, layerIds, f);
				const key = frameKey(pairs, shrink);
				keys.set(f, key);
				if (!rendered.has(key)) {
					await cache.ensure(pairs.map(([, d]) => d));
					const r = canvas?.renderFrame(pairs, shrink);
					if (!r) return stopPlay();
					rendered.set(key, await createImageBitmap(new ImageData(r.rgba, r.width, r.height)));
					cache.trim();
				}
				preparing = { done: preparing!.done + 1, total: todo.length };
			}
		} catch (e) {
			note(`Couldn’t play: ${e}`);
			return stopPlay();
		} finally {
			preparing = null;
		}
		// a few hundred frames at most stay rendered
		while (rendered.size > 400) {
			const [k, b] = rendered.entries().next().value!;
			b.close();
			rendered.delete(k);
		}
		const t0 = performance.now();
		const tick = () => {
			if (!playing) return;
			const n = Math.floor(((performance.now() - t0) * fps) / 1000);
			const f = frameAt(n, range, playMode);
			if (f === null) return stopPlay();
			if (f !== frame) frame = f;
			drawPlayer(rendered.get(keys.get(f)!));
			playTimer = requestAnimationFrame(tick);
		};
		tick();
	}

	function stopPlay() {
		if (!playing) return;
		playing = false;
		preparing = null;
		cancelAnimationFrame(playTimer);
		// the canvas shows where playback stopped
		void showFrame();
	}

	/** Loop from (or to) the current frame. */
	function setRange(end: 'from' | 'to') {
		const len = cutLength(theCut());
		const r = playRange ?? { from: 0, to: len - 1 };
		playRange = clampRange(end === 'from' ? { ...r, from: frame } : { ...r, to: frame }, len);
		if (end === 'from' && playRange.to < frame) playRange = { from: frame, to: len - 1 };
	}

	// a new, empty canvas: a new timeline
	$effect(() => {
		if (!status || pending || timelineFor === canvasKey) return;
		untrack(() => {
			timeline = newTimeline(status!.layers.map((l) => ({ id: l.id, drawing: l.drawing })));
			timelineFor = canvasKey;
			[scene, cutIndex, frame] = [0, 0, 0];
			onCanvas = { scene: 0, cut: 0, frame: 0 };
			void cache.clear().catch(() => {});
		});
	});

	// ---- the tools' own options (the same as the website's studio) ----
	const toolOpts = new ToolOptions({
		canvas: () => canvas,
		status: () => status ?? BLANK_STATUS,
		color: () => color,
		setTool: (t) => (tool = t),
		onguides: (g) => keepLocal('pp.app.guides', { w: canvasSize.width, h: canvasSize.height, g })
	});
	function keepLocal(key: string, value: unknown) {
		try {
			localStorage.setItem(key, JSON.stringify(value));
		} catch {
			/* storage blocked */
		}
	}
	function readLocal<T>(key: string, fallback: T): T {
		try {
			return (JSON.parse(localStorage.getItem(key) ?? 'null') as T) ?? fallback;
		} catch {
			return fallback;
		}
	}
	/** guides kept for another canvas size are scaled to this one */
	function loadGuides(w: number, h: number) {
		const saved = readLocal<{ w: number; h: number; g: GuideSettings } | null>(
			'pp.app.guides',
			null
		);
		if (!saved?.g) {
			toolOpts.guides = defaultGuides(w, h);
			return;
		}
		const [kx, ky] = [w / saved.w, h / saved.h];
		const sc = (p: [number, number]): [number, number] => [p[0] * kx, p[1] * ky];
		const g = saved.g;
		toolOpts.guides = {
			grid: { ...g.grid, size: Math.round(g.grid.size * Math.min(kx, ky)) },
			perspective: { ...g.perspective, vps: g.perspective.vps.map(sc) },
			ruler: { ...g.ruler, a: sc(g.ruler.a), b: sc(g.ruler.b) },
			symmetry: g.symmetry ? { ...g.symmetry, c: sc(g.symmetry.c) } : defaultGuides(w, h).symmetry
		};
	}
	$effect(() => {
		const { width, height } = canvasSize;
		untrack(() => loadGuides(width, height));
	});
	toolOpts.quickShape = readLocal('pp.app.quickShape', true);

	/** a short note over the canvas (a snapped shape, a locked layer…) */
	let canvasNote = $state('');
	let noteTimer: ReturnType<typeof setTimeout> | undefined;
	function note(text: string, ms = 2200) {
		canvasNote = text;
		clearTimeout(noteTimer);
		noteTimer = setTimeout(() => (canvasNote = ''), ms);
	}
	/** the quick menu (right-click / long-press), at screen x, y */
	let quickMenu = $state<{ x: number; y: number } | null>(null);
	let timelapseOpen = $state(false);
	function quickActions(): QuickAction[] {
		return [
			{
				id: 'undo',
				label: 'Undo',
				icon: 'M9 14 4 9l5-5M4 9h11a5 5 0 0 1 0 10h-3',
				disabled: !status?.canUndo,
				run: () => undo()
			},
			{
				id: 'redo',
				label: 'Redo',
				icon: 'm15 14 5-5-5-5M20 9H9a5 5 0 0 0 0 10h3',
				disabled: !status?.canRedo,
				run: () => redo()
			},
			{
				id: 'picker',
				label: 'Eyedropper',
				icon: 'm3 21 3-1 10-10-2-2L4 18Zm11-13 3-3a2 2 0 0 1 3 3l-3 3',
				run: () => (tool = 'picker')
			},
			{
				id: 'eraser',
				label: eraser ? 'Brush' : 'Eraser',
				icon: eraser
					? 'M4 20c3 0 4-2 4-4l9-11 3 3-11 9c-2 0-4 1-4 4Z'
					: 'M8 20h12M5 16l9-9 5 5-6 6H8Z',
				run: () => {
					eraser = !eraser;
					tool = 'brush';
				}
			},
			{
				id: 'mirror',
				label: 'Mirror view',
				icon: 'M12 3v18M8 7 3 12l5 5ZM16 7l5 5-5 5Z',
				run: () => canvas?.toggleMirror()
			},
			{
				id: 'deselect',
				label: 'Deselect',
				icon: 'M4 4h4M4 4v4M20 4h-4M20 4v4M4 20h4M4 20v-4M20 20h-4M20 20v-4M9 9l6 6M15 9l-6 6',
				disabled: !status?.selection,
				run: () => canvas?.deselect()
			}
		];
	}
	/** undo also cancels an open Adjust panel */
	async function undo() {
		// (a frame still coming from the store: let it arrive first)
		await showQueue;
		if (toolOpts.adjustOpen) toolOpts.closeAdjust(false);
		await reveal(canvas?.undoDrawing() ?? 0);
		canvas?.undo();
	}
	/** The History panel: several steps, each with its drawing brought back first. */
	async function historyGo(steps: number) {
		for (let i = 0; i < Math.abs(steps); i++) await (steps < 0 ? undo() : redo());
	}
	async function redo() {
		// (a frame still coming from the store: let it arrive first)
		await showQueue;
		if (toolOpts.adjustOpen) toolOpts.closeAdjust(false);
		await reveal(canvas?.redoDrawing() ?? 0);
		canvas?.redo();
	}
	// paste a picture (or what was copied) as a new layer
	$effect(() => {
		const onPaste = (e: ClipboardEvent) => {
			if ((e.target as HTMLElement | null)?.closest('input, textarea, [contenteditable]')) return;
			const file = [...(e.clipboardData?.files ?? [])].find((f) => f.type.startsWith('image/'));
			if (!file && canvas?.hasClip()) {
				e.preventDefault();
				return toolOpts.pasteCopied();
			}
			if (!file || !canvas) return;
			e.preventDefault();
			void canvas.importImage(file, 'Pasted picture').then((r) => toolOpts.imported(r));
		};
		window.addEventListener('paste', onPaste);
		return () => window.removeEventListener('paste', onPaste);
	});
	const mac = typeof navigator !== 'undefined' && /Mac|iPad/.test(navigator.platform);
	/** the rail's tooltips show the keys chosen in Settings */
	const railKeys = $derived(
		Object.fromEntries(
			(Object.keys(settings.keys) as ActionId[]).map((id) => [id, showKey(settings.keys[id], mac)])
		) as Partial<Record<ToolId, string>>
	);
	/** a key's label for a tooltip ("Ctrl+S", "⌘S") */
	const keyOf = (id: ActionId) => showKey(settings.keys[id], mac);

	// ---- the project file ----
	let path = $state<string | null>(null);
	let created = $state<string | undefined>(undefined);
	/** this document's recovery copy (a new one for each opened or new document) */
	let docId = newDocId();
	/** the canvas version when last saved or opened (anything newer is unsaved; -1: all of it) */
	let savedVersion = $state(1);
	/** the canvas version in the recovery copy */
	let recoveredVersion = 0;
	let busy = $state<'' | 'saving' | 'opening' | 'exporting'>('');
	let fileNote = $state('');
	/** something is drawn, on this frame or another */
	const drawn = $derived(!!status && (!status.blank || drawingsIn(timeline).length > 0));
	const unsaved = $derived(
		!!status && (savedVersion === -1 || (drawn && docVersion !== savedVersion))
	);
	/** a document being loaded, waiting for its canvas (and whether it starts unsaved) */
	let pending = $state.raw<{ doc: ProjectDoc; unsaved: boolean } | null>(null);
	let exportOpen = $state(false);

	/** Everything the window knows, as a project document. */
	async function currentDoc(): Promise<ProjectDoc> {
		if (!canvas) throw new Error('the canvas isn’t ready');
		canvas.ensureDrawings();
		// the store gets what changed; then the file is made from it, no pixels sent
		await cache.push();
		const snap = canvas.snapshotLayers();
		if (!snap) throw new Error('the canvas isn’t ready');
		const layerIds = snap.layers.filter((l) => !l.adjust).map((l) => l.id);
		const c = canvas;
		const drawings = drawingsIn(timeline).flatMap((id) => {
			const layer = layerOfDrawing(timeline, id);
			if (layer === null || !layerIds.includes(layer)) return [];
			if (cache.native) return [{ id, layer }];
			// (no store: the pixels go in the packet; an empty drawing as no bytes)
			const rgba = c.drawingEmpty(id) ? new Uint8Array(0) : c.drawingRgba(id);
			return [{ id, layer, rgba }];
		});
		// an animation's layer pixels are its drawings: not needed twice
		if (cache.native) for (const l of snap.layers) if (!l.adjust) l.rgba = new Uint8Array(0);
		return {
			width: canvasSize.width,
			height: canvasSize.height,
			fps,
			layers: snap.layers,
			groups: snap.groups,
			activeLayer: status?.active ?? 0,
			strokes: snap.strokes,
			extra: { brushId, color, size, opacity, smoothing, scene, cut: cutIndex, frame },
			created,
			thumbnail: await canvas.thumbnail(),
			scenes: toFile(timeline, layerIds),
			drawings
		};
	}

	async function save(as = false) {
		if (!canvas || busy) return;
		const target =
			as || !path ? await askSavePath(projectName(path), settings.projectFolder) : path;
		if (!target) return;
		busy = 'saving';
		fileNote = '';
		try {
			const versionAtSave = docVersion;
			path = await saveProject(target, await currentDoc());
			created ??= new Date().toISOString();
			savedVersion = versionAtSave;
			// saved for real: the recovery copy has done its job
			await discardRecovery(docId).catch(() => {});
			recoveredVersion = 0;
		} catch (e) {
			fileNote = `Couldn’t save: ${e}`;
		} finally {
			busy = '';
		}
	}

	/** Unsaved changes would be lost: ask first. True to go ahead. */
	async function okToDrop(): Promise<boolean> {
		if (!unsaved) return true;
		const { ask } = await import('@tauri-apps/plugin-dialog');
		return ask(`“${projectName(path)}” has changes that aren’t saved. Lose them?`, {
			title: 'Unsaved changes',
			kind: 'warning',
			okLabel: 'Lose changes',
			cancelLabel: 'Keep working'
		});
	}

	/** Put a document on a fresh canvas of its size. */
	function load(doc: ProjectDoc, opts: { path: string | null; unsaved: boolean; id?: string }) {
		path = opts.path;
		created = doc.created;
		fps = doc.fps;
		docId = opts.id ?? newDocId();
		recoveredVersion = 0;
		const x = doc.extra;
		if (typeof x.brushId === 'string') brushId = x.brushId;
		if (typeof x.color === 'string') color = x.color;
		if (typeof x.size === 'number') size = x.size;
		if (typeof x.opacity === 'number') opacity = x.opacity;
		if (typeof x.smoothing === 'number') smoothing = x.smoothing;
		status = null;
		canvasSize = { width: doc.width, height: doc.height };
		canvasKey++;
		pending = { doc, unsaved: opts.unsaved };
	}

	async function run(what: 'opening', job: () => Promise<void>) {
		if (busy) return;
		busy = what;
		fileNote = '';
		try {
			await job();
			await tick();
		} catch (e) {
			fileNote = `Couldn’t open: ${e}`;
			busy = '';
		}
	}

	async function open() {
		if (busy || !(await okToDrop())) return;
		const target = await askOpenPath(settings.projectFolder);
		if (!target) return;
		const old = docId;
		await run('opening', async () => {
			load(await openProject(target), { path: target, unsaved: false });
			await discardRecovery(old).catch(() => {});
		});
	}

	async function openOldVersion(id: string) {
		if (!path || busy || !(await okToDrop())) return;
		const file = path;
		await run('opening', async () =>
			load(await openVersion(file, id), { path: file, unsaved: true })
		);
	}

	async function exportAs(kind: 'png' | 'psd') {
		if (!canvas || busy) return;
		const target = await askExportPath(projectName(path), kind, settings.projectFolder);
		if (!target) return;
		busy = 'exporting';
		fileNote = '';
		try {
			let bytes: Uint8Array;
			if (kind === 'png') {
				const d = canvas.exportDrawing();
				if (!d) throw new Error('nothing to export');
				bytes = new Uint8Array(await (await fetch(d.png)).arrayBuffer());
			} else {
				const doc = canvas.exportLayered();
				if (!doc) throw new Error('nothing to export');
				const { writePsd } = await import('@pencilpilot/paint/psd');
				bytes = writePsd(doc);
			}
			await writeExport(target, bytes);
		} catch (e) {
			fileNote = `Couldn’t export: ${e}`;
		} finally {
			busy = '';
		}
	}

	// the new canvas is ready: put the document's layers on it
	$effect(() => {
		const p = pending;
		if (!p || !canvas || !status) return;
		pending = null;
		const doc = p.doc;
		const anim = !!doc.scenes?.length;
		const c = canvas;
		untrack(() => {
			// an animation's layers start empty: their drawings come next
			c.restoreLayers(
				anim
					? doc.layers.map((l) => ({ ...l, rgba: new Uint8Array(doc.width * doc.height * 4) }))
					: doc.layers,
				doc.groups,
				doc.strokes
			);
			// the canvas gave the layers new ids (in the same order)
			const now = status?.layers ?? [];
			const ids = new Map(doc.layers.map((l, i) => [l.id, now[i]?.id ?? 0] as [number, number]));
			const at = (v: unknown) => (typeof v === 'number' && v >= 0 ? Math.floor(v) : 0);
			if (anim) {
				// (drawings in the frame store come when their frames show)
				for (const d of doc.drawings ?? []) if (d.rgba) c.putDrawing(d.id, d.rgba);
				cache.stored((doc.drawings ?? []).filter((d) => !d.rgba).map((d) => d.id));
				timeline = fromFile(doc.scenes!, ids);
				scene = Math.min(at(doc.extra.scene), timeline.scenes.length - 1);
				cutIndex = Math.min(at(doc.extra.cut), timeline.scenes[scene].cuts.length - 1);
				frame = at(doc.extra.frame);
			} else {
				c.ensureDrawings();
				timeline = newTimeline(
					(status?.layers ?? []).map((l) => ({ id: l.id, drawing: l.drawing }))
				);
				[scene, cutIndex, frame] = [0, 0, 0];
			}
			timelineFor = canvasKey;
			onCanvas = { scene, cut: cutIndex, frame };
			if (anim) {
				onCanvas = { scene: -1, cut: -1, frame: -1 };
				showFrame();
			}
			const active = ids.get(doc.activeLayer);
			if (active) c.selectLayer(active);
			// opened: nothing unsaved; recovered or an old version: all of it is unsaved
			savedVersion = p.unsaved ? -1 : docVersion;
		});
		busy = '';
	});

	// leaving for home with unsaved work: ask; leaving on purpose drops the recovery copy
	let leaving = false;
	beforeNavigate((nav) => {
		if (leaving || nav.type === 'leave' || !nav.to) return;
		const to = nav.to.url;
		if (!unsaved) {
			void discardRecovery(docId).catch(() => {});
			return;
		}
		nav.cancel();
		void okToDrop().then(async (ok) => {
			if (!ok) return;
			leaving = true;
			await discardRecovery(docId).catch(() => {});
			// the navigation that was held back (its link was already resolved)
			// eslint-disable-next-line svelte/no-navigation-without-resolve
			await goto(to);
		});
	});

	// autosave: a recovery copy of unsaved work, as often as Settings says, while it changes
	async function autosave() {
		if (!unsaved || busy || !status || docVersion === recoveredVersion) return;
		const version = docVersion;
		try {
			await writeRecovery(docId, path, await currentDoc());
			recoveredVersion = version;
		} catch {
			// a failed autosave is retried next time; the work is still on screen
		}
	}

	$effect(() => {
		const secs = settings.autosaveSeconds;
		if (!secs || !isTauri()) return;
		const timer = setInterval(autosave, secs * 1000);
		return () => clearInterval(timer);
	});

	onMount(() => {
		void loadSettings();
		if (!isTauri()) return;
		// what home asked for
		if (start.kind === 'open') {
			const file = start.path;
			void run('opening', async () =>
				load(await openProject(file), { path: file, unsaved: false })
			);
		} else if (start.kind === 'recover') {
			const { id, path: file } = start;
			// keeps the same recovery copy, so more autosaves update it
			void run('opening', async () =>
				load(await openRecovery(id), { path: file, unsaved: true, id })
			);
		}
		// closing with unsaved work: ask; closing on purpose drops the recovery copy
		let stop: (() => void) | undefined;
		import('@tauri-apps/api/window').then(({ getCurrentWindow }) =>
			getCurrentWindow()
				.onCloseRequested(async (e) => {
					if (unsaved && !(await okToDrop())) return e.preventDefault();
					await discardRecovery(docId).catch(() => {});
				})
				.then((un) => (stop = un))
		);
		return () => stop?.();
	});

	onMount(() => {
		loadManifest()
			.then((m) => (manifest = m))
			.catch(() => (manifest = null));
	});

	let texVersion = $state(0);
	$effect(() => onTextures(() => texVersion++));
	const texture = $derived.by(() => {
		void texVersion;
		const t = brushTexture(brushId);
		return textureOn(t) ? t : null;
	});

	const TABS = $derived([
		{
			id: 'layers',
			label: 'Layers',
			meta: [
				status?.layers.find((l) => l.id === status?.active)?.name,
				`${status?.layers.length ?? 1} layers`
			]
				.filter(Boolean)
				.join(' · ')
		},
		{ id: 'brushes', label: 'Brushes', meta: manifest ? brushLabel(manifest, brushId) : '…' },
		{ id: 'colour', label: 'Colour' },
		{ id: 'history', label: 'History', meta: status?.history.undo.at(-1) }
	]);

	/** tools whose key switches to them (and back to the brush) */
	const TOOL_ACTIONS: Partial<Record<ActionId, string>> = {
		picker: 'picker',
		fill: 'fill',
		'lasso-fill': 'lasso-fill',
		measure: 'measure',
		smudge: 'smudge',
		pen: 'pen',
		transform: 'transform',
		liquify: 'liquify',
		'select-rect': 'select-rect',
		'select-lasso': 'select-lasso',
		'select-wand': 'select-wand',
		gradient: 'gradient'
	};

	function onkey(e: KeyboardEvent) {
		if ((e.target as HTMLElement).closest('input, textarea, select')) return;
		// keys that belong to the tool in use
		if (tool === 'transform' && e.key === 'Enter') return canvas?.transformCommit();
		if (tool === 'transform' && e.key === 'Escape') return canvas?.transformCancel();
		if (tool === 'pen' && e.key === 'Enter') {
			if (canvas?.penStroke(toolOpts.penTaper)) canvas.penClear();
			return;
		}
		if (tool === 'pen' && e.key === 'Escape') return canvas?.penClear();
		if (tool === 'pen' && e.key === 'Backspace') {
			e.preventDefault();
			return canvas?.penBack();
		}
		if (e.key === 'Escape' && toolOpts.adjustOpen) return toolOpts.closeAdjust(false);
		if (e.key === 'Escape' && status?.selection) return canvas?.deselect();
		const combo = comboOf(e);
		const action = combo ? actionFor(settings.keys, combo) : null;
		// while playing: Esc or any other shortcut stops first
		if (playing && (e.key === 'Escape' || (action && action !== 'play'))) stopPlay();
		if (!action) return;
		e.preventDefault();
		const toolFor = TOOL_ACTIONS[action];
		if (toolFor) tool = tool === toolFor ? 'brush' : toolFor;
		else if (action === 'brush' || action === 'eraser') {
			eraser = action === 'eraser';
			tool = 'brush';
		} else if (action === 'smaller') size = Math.max(0.25, +(size / 1.25).toFixed(2));
		else if (action === 'bigger') size = Math.min(6, +(size * 1.25).toFixed(2));
		else if (action === 'undo') undo();
		else if (action === 'redo') redo();
		else if (action === 'prev-frame') timelinePanel?.step(-1);
		else if (action === 'next-frame') timelinePanel?.step(1);
		else if (action === 'new-drawing') timelinePanel?.addDrawing();
		else if (action === 'play') void togglePlay();
		else if (action === 'set-in') setRange('from');
		else if (action === 'set-out') setRange('to');
		else if (action === 'onion') onion.on = !onion.on;
		else if (action === 'pin-frame') pinFrame();
		else if (action === 'adjust') toolOpts.openAdjust();
		else if (action === 'copy' || action === 'cut') toolOpts.copyCut(action === 'cut');
		else if (action === 'copy-to-layer') {
			if (canvas?.copySelection(false)) toolOpts.pasteCopied();
		} else if (action === 'select-all') canvas?.selectAll();
		else if (action === 'deselect') canvas?.deselect();
		else if (action === 'invert-selection') canvas?.invertSelection();
		else if (action === 'clear-selected') {
			if (status?.selection) canvas?.clear();
		} else if (action === 'zoom-in') canvas?.zoomIn();
		else if (action === 'zoom-out') canvas?.zoomOut();
		else if (action === 'fit') canvas?.resetView();
		else if (action === 'rotate-left') canvas?.rotateBy(-15);
		else if (action === 'rotate-right') canvas?.rotateBy(15);
		else if (action === 'rotate-reset') canvas?.resetRotation();
		else if (action === 'mirror') canvas?.toggleMirror();
		else if (action === 'save') void save(false);
		else if (action === 'save-as') void save(true);
		else if (action === 'open') void open();
	}
</script>

<svelte:window onkeydown={onkey} />

<div class="app">
	<Studio backHref={resolve('/')} tabs={TABS} bind:tab bind:dockOpen>
		{#snippet title()}
			<div class="brand">
				<Logo /><span data-testid="project-name">{projectName(path)}</span>
				{#if unsaved}<span class="dot" title="Not saved yet" aria-label="not saved">●</span>{/if}
			</div>
		{/snippet}
		{#snippet actions()}
			{#if fileNote}<span class="note" role="alert">{fileNote}</span>{/if}
			<button
				type="button"
				class="act"
				disabled={!!busy}
				onclick={open}
				title="Open a project ({keyOf('open')})"
			>
				{busy === 'opening' ? 'Opening…' : 'Open'}
			</button>
			<button
				type="button"
				class="act"
				disabled={!!busy}
				onclick={() => save(false)}
				title="Save ({keyOf('save')}); Save as: {keyOf('save-as')}"
			>
				{busy === 'saving' ? 'Saving…' : 'Save'}
			</button>
			<VersionsMenu {path} disabled={!!busy} onopen={openOldVersion} />
			<div class="export">
				<button
					type="button"
					class="act"
					aria-expanded={exportOpen}
					disabled={!!busy || !status || status.blank}
					onclick={() => (exportOpen = !exportOpen)}
				>
					{busy === 'exporting' ? 'Exporting…' : 'Export'}
				</button>
				{#if exportOpen}
					<div class="menu" role="group" aria-label="Export as">
						<button
							type="button"
							onclick={() => {
								exportOpen = false;
								void exportAs('png');
							}}
						>
							<b>PNG picture</b><span>One flat image</span>
						</button>
						<button
							type="button"
							onclick={() => {
								exportOpen = false;
								void exportAs('psd');
							}}
						>
							<b>Photoshop file (.psd)</b><span>Keeps layers, groups and masks</span>
						</button>
					</div>
				{/if}
			</div>
			<button
				type="button"
				class="act"
				disabled={!status?.canUndo}
				onclick={undo}
				title="Undo ({keyOf('undo')})"
			>
				Undo
			</button>
			<button
				type="button"
				class="act"
				disabled={!status?.canRedo}
				onclick={redo}
				title="Redo ({keyOf('redo')})"
			>
				Redo
			</button>
			<a class="act" href={resolve('/settings')}>Settings</a>
		{/snippet}
		{#snippet rail()}
			<ToolRail
				bind:tool
				bind:eraser
				bind:size
				bind:opacity
				keys={railKeys}
				adjustOpen={toolOpts.adjustOpen}
				onadjust={() => toolOpts.openAdjust()}
			/>
		{/snippet}
		{#snippet options()}
			<span class="brush-name">{manifest ? brushLabel(manifest, brushId) : ''}</span>
			<Swatches bind:color onmore={() => ((tab = 'colour'), (dockOpen = true))} />
			<label class="slider">
				<span>Smoothing <b>{Math.round(smoothing * 100)}%</b></span>
				<input
					type="range"
					min="0"
					max="1"
					step="0.05"
					bind:value={smoothing}
					title="Steadies wobbly lines (like StreamLine)"
				/>
			</label>
			<button
				type="button"
				class="opt"
				aria-pressed={anyGuide(toolOpts.guides)}
				aria-expanded={toolOpts.guidesOpen}
				title="Guides: grid, perspective, ruler and symmetry"
				onclick={() => (toolOpts.guidesOpen = !toolOpts.guidesOpen)}
			>
				Guides
			</button>
			<button
				type="button"
				class="opt"
				aria-pressed={toolOpts.quickShape}
				title="QuickShape: hold at the end of a stroke to snap it to a line, circle or rectangle"
				onclick={() => {
					toolOpts.quickShape = !toolOpts.quickShape;
					keepLocal('pp.app.quickShape', toolOpts.quickShape);
				}}
			>
				QuickShape
			</button>
			<button
				type="button"
				class="opt"
				disabled={!status || status.blank}
				title="Watch the drawing being made, stroke by stroke"
				onclick={() => (timelapseOpen = true)}
			>
				Timelapse
			</button>
		{/snippet}
		{#snippet context()}
			{#if status}
				<ToolOptionsBar opts={toolOpts} bind:tool {status} {canvas} />
			{/if}
		{/snippet}
		{#snippet stage()}
			{#key canvasKey}
				<PaintCanvas
					bind:this={canvas}
					fill
					width={canvasSize.width}
					height={canvasSize.height}
					{brushId}
					{eraser}
					{size}
					{opacity}
					{color}
					tool={tool as never}
					{smoothing}
					penPressure={settings.penPressure}
					fingers={settings.fingers}
					tapGestures={settings.tapGestures}
					brushTexture={texture}
					allowImport
					onpick={(c) => (color = c)}
					onsize={(v) => (size = v)}
					fillTolerance={toolOpts.fillTolerance}
					fillAllLayers={toolOpts.fillAllLayers}
					fillReference={toolOpts.fillReference}
					distort={toolOpts.distort}
					warp={toolOpts.warp}
					quickShape={toolOpts.quickShape}
					selectMode={toolOpts.selectMode}
					wandTolerance={toolOpts.wandTolerance}
					wandContiguous={toolOpts.wandContiguous}
					liquifySize={toolOpts.liquifySize}
					liquifyStrength={toolOpts.liquifyStrength}
					smudgeStrength={toolOpts.smudgeStrength}
					gradientKind={toolOpts.gradientKind}
					gradientEnd={toolOpts.gradientEndValue}
					guides={toolOpts.guides}
					onguides={toolOpts.onguides}
					onpenpath={toolOpts.onpenpath}
					onimport={toolOpts.onimport}
					onshape={(kind) => note(`Snapped to a ${kind}`, 1800)}
					ontransformend={(r) => {
						tool = 'brush';
						if (r === 'empty') note('Nothing to move on this layer (or it’s locked).');
					}}
					onblocked={(why) =>
						note(
							why === 'hidden'
								? 'This layer is hidden. Show it or pick another layer to draw.'
								: 'This layer is locked. Unlock it or pick another layer to draw.'
						)}
					onquickmenu={(x, y) => (quickMenu = { x, y })}
					onchange={canvasChanged}
					locked={switching}
					onionImage={playing ? null : onionImage}
				/>
			{/key}
			{#if playing}
				<canvas
					class="player"
					bind:this={playerEl}
					aria-label="Playing frame {frame + 1}"
					data-testid="player"
				></canvas>
				{#if preparing}
					<p class="canvas-note" role="status">
						Getting frames ready… {preparing.done} of {preparing.total}
					</p>
				{/if}
			{/if}
			{#if canvasNote}<p class="canvas-note" role="status">{canvasNote}</p>{/if}
		{/snippet}
		{#snippet bottom()}
			{#if status && timelineFor === canvasKey}
				<TimelinePanel
					bind:this={timelinePanel}
					{timeline}
					bind:scene
					bind:cut={cutIndex}
					bind:frame
					bind:fps
					{playing}
					bind:playMode
					bind:playRange
					{onion}
					bind:pinned
					onpin={pinFrame}
					onplay={togglePlay}
					onrange={setRange}
					layers={status.layers}
					active={status.active}
					newDrawing={() => canvas?.newDrawing() ?? 0}
					onlayer={(id) => canvas?.selectLayer(id)}
					onedit={(changed) => {
						if (playing) stopPlay();
						timelineEdited(changed);
					}}
				/>
			{/if}
		{/snippet}
		{#snippet panel(t: string)}
			{#if t === 'layers' && status}
				<LayersPanel
					{status}
					controls={canvas}
					aspect={canvasSize.width / canvasSize.height}
					onadjustment={(id) => toolOpts.adjustmentLayer(id)}
				/>
			{:else if t === 'brushes'}
				{#if manifest}
					<BrushPicker {manifest} bind:selected={brushId} />
					<BrushTexturePanel {brushId} onpick={(id) => (brushId = id)} />
				{:else}
					<p>Loading brushes…</p>
				{/if}
			{:else if t === 'history' && status}
				<HistoryPanel history={status.history} ongo={historyGo} />
			{:else if t === 'colour'}
				<ColorPanel bind:color onclose={() => (tab = 'layers')} />
			{/if}
		{/snippet}
		{#snippet after()}
			{#if quickMenu}
				<QuickMenu
					x={quickMenu.x}
					y={quickMenu.y}
					actions={quickActions()}
					onclose={() => (quickMenu = null)}
				/>
			{/if}
			{#if timelapseOpen && canvas}
				<TimelapsePlayer film={canvas.timelapse()} onclose={() => (timelapseOpen = false)} />
			{/if}
		{/snippet}
	</Studio>
</div>

<style>
	.app {
		height: 100vh;
		padding: 8px;
		box-sizing: border-box;
	}
	.brand {
		display: flex;
		align-items: center;
		gap: 10px;
		font: 700 18px var(--pp-font-display);
	}
	.act {
		min-height: 40px;
		padding: 0 14px;
		display: inline-grid;
		place-items: center;
		border: 0;
		border-radius: 12px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 14px var(--pp-font-body);
		text-decoration: none;
		cursor: pointer;
	}
	.act:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.export {
		position: relative;
	}
	.menu {
		position: absolute;
		z-index: 30;
		top: calc(100% + 8px);
		right: 0;
		display: grid;
		gap: 4px;
		width: 260px;
		padding: 8px;
		border-radius: 18px;
		background: var(--pp-surface);
		box-shadow: 0 12px 32px rgba(0, 0, 0, 0.18);
	}
	.menu button {
		display: grid;
		gap: 2px;
		padding: 10px 12px;
		border: 0;
		border-radius: 12px;
		background: none;
		color: var(--pp-ink);
		text-align: left;
		font: 14px var(--pp-font-body);
		cursor: pointer;
	}
	.menu button:hover {
		background: var(--pp-ground);
	}
	.menu span {
		color: var(--pp-muted);
		font-size: 13px;
	}
	.dot {
		color: var(--pp-accent);
		font-size: 12px;
	}
	.note {
		max-width: 320px;
		color: var(--pp-accent-ink);
		font-size: 13px;
	}
	.brush-name {
		font-weight: 700;
		white-space: nowrap;
	}
	.slider {
		display: grid;
		gap: 2px;
		width: 140px;
		font-size: 13px;
		color: var(--pp-on-ink-muted);
	}
	.slider b {
		color: var(--pp-on-ink);
	}
	.slider input {
		accent-color: var(--pp-yellow);
	}
	.opt {
		height: 38px;
		padding: 0 12px;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.opt[aria-pressed='true'] {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.opt:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}
	.player {
		position: absolute;
		inset: 0;
		z-index: 15;
		width: 100%;
		height: 100%;
		background: var(--pp-surface-sunk);
	}
	.canvas-note {
		position: absolute;
		left: 50%;
		bottom: 16px;
		z-index: 20;
		transform: translateX(-50%);
		margin: 0;
		padding: 8px 14px;
		border-radius: 12px;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		font-size: 14px;
	}
</style>
