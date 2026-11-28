<script lang="ts">
	import { resolve } from '$app/paths';
	/**
	 * The drawing studio, in the app: the website's studio (same layout, tools and panels,
	 * from @pencilpilot/paint) without the website's practice parts, plus projects:
	 * Ctrl+S saves (.ppstudio), Ctrl+Shift+S saves as, Ctrl+O opens.
	 */
	import Studio from '@pencilpilot/paint/Studio.svelte';
	import ToolRail, { ALL_TOOLS, type ToolId } from '@pencilpilot/paint/ToolRail.svelte';
	import ToolOptionsBar from '@pencilpilot/paint/ToolOptionsBar.svelte';
	import QuickMenu, { type QuickAction } from '@pencilpilot/paint/QuickMenu.svelte';
	import TimelapsePlayer from '@pencilpilot/paint/TimelapsePlayer.svelte';
	import { ToolOptions } from '@pencilpilot/paint/tool-options.svelte.js';
	import {
		compositionOn,
		drawingGuide,
		centredGuides,
		defaultGuides,
		gridLines,
		readGuides,
		type GuideSettings
	} from '@pencilpilot/paint/guides';
	import { smoothTool, type SmoothTool } from '@pencilpilot/paint/stabilizer';
	import PaintCanvas, {
		BLANK_STATUS,
		type CanvasStatus,
		type GroupSettings,
		type LayerSettings,
		type LiveEvent
	} from '@pencilpilot/paint/PaintCanvas.svelte';
	import LayersPanel from '@pencilpilot/paint/LayersPanel.svelte';
	import BrushPicker from '@pencilpilot/paint/BrushPicker.svelte';
	import BrushTexturePanel from '@pencilpilot/paint/BrushTexturePanel.svelte';
	import ColorPanel from '@pencilpilot/paint/ColorPanel.svelte';
	import HistoryPanel from '@pencilpilot/paint/HistoryPanel.svelte';
	import Swatches from '@pencilpilot/paint/Swatches.svelte';
	import { brushTexture, onTextures, textureOn } from '@pencilpilot/paint/brush-textures';
	import { addFont, fontsUsed, parseText } from '@pencilpilot/paint/text';
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
		askCompositionPath,
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
	import {
		loadSettings,
		saveSettings,
		settings,
		settingsLoaded,
		strokeMemory
	} from '#lib/settings.svelte.js';
	import WorkspaceDialog from '#lib/WorkspaceDialog.svelte';
	import { exportText, tagPng } from '#lib/png-meta.js';
	import type { ViewPrefs } from '@pencilpilot/paint/view-prefs';
	import { actionFor, comboOf, showKey, type ActionId } from '#lib/shortcuts.js';
	import { page } from '$app/state';
	import VersionsMenu from '#lib/VersionsMenu.svelte';
	import XSheet from '#lib/XSheet.svelte';
	import { psdStem, psdToDoc, tooBig } from '#lib/psd-open.js';
	import ReferencePanel from '#lib/ReferencePanel.svelte';
	import RefSplit from '#lib/RefSplit.svelte';
	import {
		canvasFromPicture,
		pictureOnCanvas,
		RefShelf,
		skippedNote
	} from '#lib/references.svelte.js';
	import { AudioBank, wav } from '#lib/audio.js';
	import ExportDialog from '#lib/ExportDialog.svelte';
	import LipSyncDialog from '#lib/LipSyncDialog.svelte';
	import FilmDialog from '#lib/FilmDialog.svelte';
	import ShareDialog from '#lib/ShareDialog.svelte';
	import TogetherChat from '#lib/TogetherChat.svelte';
	import { capLines, chatBody, chatText, type ChatLine } from '#lib/chat.js';
	import SaveChangesDialog from '#lib/SaveChangesDialog.svelte';
	import SettingsPanel from '#lib/SettingsPanel.svelte';
	import CanvasSizeDialog from '#lib/CanvasSizeDialog.svelte';
	import {
		checkResize,
		moveCameras,
		moveGuides,
		moveText,
		planCrop,
		type ResizePlan
	} from '#lib/canvas-resize.js';
	import { takeTogether } from '#lib/together-start.js';
	import {
		readSavedShare,
		Together,
		type Ask,
		type CanvasPort,
		type SavedShare,
		type SharedGroup,
		type SharedLayer
	} from '#lib/together.js';
	import {
		cleanRelay,
		Cursors,
		hostSession,
		isCursor,
		joinSession,
		leaveSession,
		rejoinSession,
		onShareEvent,
		sendDirect,
		sendEdit,
		sendPresence,
		sessionStatus,
		Throttle,
		type ShareInfo
	} from '#lib/share.js';
	import {
		FILM_DEFAULTS,
		FILM_FPS,
		ease,
		liveSchedule,
		liveSteps,
		runs as runsOf,
		strokesOfFrame,
		type FilmSettings
	} from '#lib/film.js';
	import { filmPlan } from '@pencilpilot/paint/timelapse';
	import type { Stroke } from '@pencilpilot/paint/strokes';
	import {
		LIPSYNC_DEFAULTS,
		autoChart,
		dialogueText,
		extendedOf,
		shapeFrames,
		writeMouths,
		type Cue,
		type LipSyncSettings
	} from '#lib/lipsync.js';
	import {
		EXPORT_DEFAULTS,
		FORMATS,
		cutsIn,
		outputSize,
		placeCamera,
		plan,
		frameFile,
		joinFrames,
		outputFrames,
		stageFile,
		type ExportFormat,
		type ExportShow,
		type Run,
		type ExportSettings
	} from '#lib/exporting.js';
	import TimelinePanel from '#lib/TimelinePanel.svelte';
	import { fitInside, naturalOrder, placeFrames, sequenceName } from '#lib/rotoscope.js';
	import { FLIP_FPS, drawingStarts, flipIndex, keyFrames, rollFrames } from '#lib/flip.js';
	import {
		bestMatch,
		colourFor,
		lineStartsAfter,
		pixelsOf,
		regions,
		seedIn,
		walls
	} from '#lib/colour.js';
	import { FrameCache } from '#lib/frames.js';
	import { onionEntries, type OnionSettings } from '#lib/onion.js';
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
		cutStart,
		drawingNumbers,
		toggleMarker,
		drawingAt,
		drawingsIn,
		findDrawing,
		fromFile,
		layerOfDrawing,
		newTimeline,
		setDrawing,
		setStaticDrawing,
		staticDrawing,
		staticsToFile,
		cameraOf,
		showing,
		trackIn,
		toFile,
		mouthsToFile,
		kindsToFile,
		kindsFromFile,
		nextKind,
		setKind,
		setStage,
		followTiming,
		STAGES,
		STAGE_LABELS,
		stagesFromFile,
		stagesToFile,
		isStage,
		type Stage,
		type Camera,
		type MouthChart,
		type MouthShape,
		type Timeline
	} from '#lib/timeline.js';
	import {
		exportPasses,
		guessStage,
		layersInView,
		lightTableLayers,
		LIGHT_TABLE_OFF,
		readLightTable,
		type LightTable,
		readView,
		singleStage,
		oneStageView,
		toneLinesAbove,
		traceName,
		traceStages,
		VIEW_ALL,
		viewLabel,
		type StageView
	} from '#lib/stages.js';
	import { flatColours, readPalettes, type Palette } from '#lib/palettes.js';
	import PalettePanel from '#lib/PalettePanel.svelte';

	let canvas = $state<PaintCanvas>();
	let status = $state<CanvasStatus | null>(null);
	let tool = $state<string>('brush');
	let eraser = $state(false);
	/** the current brush's diameter in pixels at its own size (sizes show in pixels) */
	let brushPx = $state(0);
	let size = $state(1);
	let opacity = $state(1);
	let color = $state('#16161a');
	let brushId = $state(DEFAULT_BRUSH);
	let manifest = $state<BrushManifest | null>(null);
	let tab = $state('layers');
	let dockOpen = $state(false);
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
	/** the drawing the canvas was last given for each layer */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- bookkeeping, not state
	const lastShown = new Map<number, number>();

	/** drawings not showing live in the app's frame store (crates/anim) */
	const cache = new FrameCache(() => canvas, isTauri());

	function canvasChanged(s: CanvasStatus) {
		status = s;
		if (canvas) Object.assign(strokeMemory, canvas.strokeMemory());
		togetherChanged();
		if (quiet) return;
		// a frame the canvas couldn't show when it was busy (someone's timeline came in
		// mid-stroke): shown now (after this change has gone on the timeline)
		if (showAgain) void showFrame();
		docVersion++;
		// what changed: the drawings showing, or one undo / redo just changed
		const touched = [
			...s.layers.map((l) => l.drawing),
			canvas?.undoDrawing() ?? 0,
			canvas?.redoDrawing() ?? 0
		];
		cache.touch(touched);
		together?.touch(touched);
		underChanged(s);
		// drawing on an empty cel started a drawing (or a pasted picture got one): it's
		// on this frame now
		if (timelineFor !== canvasKey) return;
		const cut = timeline.scenes[onCanvas.scene]?.cuts[onCanvas.cut];
		if (!cut) return;
		const f = onCanvas.frame;
		for (const l of s.layers) {
			if (l.adjust || !l.drawing) continue;
			// (only a drawing the canvas made itself: the timeline may have changed since it
			// showed the others, and the canvas is about to catch up)
			if (lastShown.get(l.id) === l.drawing) continue;
			lastShown.set(l.id, l.drawing);
			if (timeline.statics.includes(l.id)) {
				if (staticDrawing(cut.tracks[l.id]) !== l.drawing)
					setStaticDrawing(trackIn(cut, l.id), l.drawing);
			} else if (drawingAt(cut.tracks[l.id], f) !== l.drawing)
				setDrawing(trackIn(cut, l.id), f, l.drawing);
			// (one undo step with the stroke that made the drawing)
			tlJoin = true;
		}
	}

	// ---- undo for the timeline: each edit is a step in the canvas's history ----
	/** the timeline as of its last step, and which canvas that was on */
	let tlSaved = '';
	let tlFor = -1;
	/** step id → the timeline before and after it */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- history bookkeeping, never rendered
	const tlSteps = new Map<number, { before: string; after: string }>();
	let tlNext = 1;
	/** what the next timeline step is called in the History panel */
	let tlLabel = '';
	/** the next timeline step goes with the canvas's last step */
	let tlJoin = false;

	/** If the timeline changed since its last step, make the change a step. */
	function recordTimeline() {
		if (!canvas || timelineFor !== canvasKey || carrying) return;
		const now = JSON.stringify(timeline);
		if (tlFor !== canvasKey) {
			// a new document: its timeline is where history starts
			[tlSaved, tlFor] = [now, canvasKey];
			tlSteps.clear();
		}
		const [join, label] = [tlJoin, tlLabel || 'Timeline change'];
		[tlJoin, tlLabel] = [false, ''];
		if (now === tlSaved) return;
		// drawing together: everyone gets it (theirs come in through putTimeline, not here)
		together?.timelineChanged();
		const id = tlNext++;
		tlSteps.set(id, { before: tlSaved, after: now });
		tlSaved = now;
		canvas.historyNote(id, label, join);
	}
	$effect(() => {
		void docVersion;
		void timelineFor;
		untrack(recordTimeline);
	});

	/** Undo / redo reached a timeline step: put the timeline as it was. */
	function putTimeline(json: string | undefined) {
		if (!json) return;
		timeline = JSON.parse(json) as Timeline;
		tlSaved = json;
		scene = Math.min(scene, timeline.scenes.length - 1);
		cutIndex = Math.min(cutIndex, theScene().cuts.length - 1);
		docVersion++;
		void showFrame();
	}

	/** frame changes, one after another (fetching drawings takes a moment) */
	let showQueue: Promise<void> = Promise.resolve();
	/**
	 * the canvas was busy when it was to show the timeline's frame: shown at its next
	 * change (else it keeps a drawing the timeline doesn't have, e.g. after a timeline
	 * from drawing together came in mid-stroke)
	 */
	let showAgain = false;
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
				frame,
				timeline.statics
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
			showAgain = false;
			onCanvas = { scene, cut: cutIndex, frame };
			for (const [l, d] of now) lastShown.set(l, d);
			cache.shown(now.map(([, d]) => d));
			void updateOnion();
			underShown++;
			// what stopped showing goes to the store; the engine keeps a few
			const gone = leaving.filter((d) => d && !now.some(([, n]) => n === d));
			void cache.push(gone).then(() => cache.trim());
		} else {
			// stay where the canvas is, and show the timeline there once it can
			[scene, cutIndex, frame] = [onCanvas.scene, onCanvas.cut, onCanvas.frame];
			if (!showAgain)
				note('Finish what you’re doing first (a transform, an adjustment or a mask).');
			showAgain = true;
		}
	}

	/** The timeline changed (or the frame did). */
	function timelineEdited(changed: boolean) {
		if (changed) docVersion++;
		else scrubSound();
		showFrame();
	}

	// ---- sound: clips along each scene, markers (S3-T01) ----
	const sounds = new AudioBank();
	/** bumps when sounds are added or loaded (the timeline draws their waveforms again) */
	let soundTick = $state(0);
	const theScene = () => timeline.scenes[scene] ?? timeline.scenes[0];
	const sceneClips = () => theScene().audio ?? [];
	/** a frame of this cut, as a frame of its scene */
	const sceneFrameOf = (f: number) => cutStart(theScene(), cutIndex, timeline.statics) + f;
	let scrubbedAt = -1;
	/** Moving between frames: a snippet of the sound there (to find a word or a beat). */
	function scrubSound() {
		if (playing || !settings.scrubSound || !sceneClips().length) return;
		const f = sceneFrameOf(frame);
		if (f === scrubbedAt) return;
		scrubbedAt = f;
		sounds.scrub(sceneClips(), f, fps);
	}

	/** Sound files picked: each starts on this frame. */
	async function addSounds(files: File[]) {
		const s = theScene();
		for (const file of files) {
			try {
				const key = await sounds.add(new Uint8Array(await file.arrayBuffer()), file.name);
				if (!s.audio) s.audio = [];
				const list = s.audio;
				list.push({
					id: Math.max(0, ...list.map((c) => c.id)) + 1,
					name: file.name.replace(/\.[^.]+$/, ''),
					file: key,
					start: sceneFrameOf(frame),
					gain: 1,
					muted: false
				});
			} catch {
				fileNote = `Couldn’t read ${file.name} as sound.`;
			}
		}
		soundTick++;
		tlLabel = 'Add sound';
		docVersion++;
	}

	/** A marker on this frame (or off it). While playing, tapping only adds. */
	function markFrame() {
		const f = sceneFrameOf(frame);
		const s = theScene();
		if (playing && s.markers?.some((m) => m.frame === f)) return;
		toggleMarker(s, f);
		tlLabel = 'Marker';
		docVersion++;
	}

	// ---- reference pictures: the user's own, kept in the project (never exported) ----
	const refShelf = new RefShelf();
	let refInput = $state<HTMLInputElement>();
	/** the canvas grid, put on the reference too (the grid method) */
	const refGrid = () =>
		toolOpts.guides.grid.on
			? gridLines(toolOpts.guides, canvasSize.width, canvasSize.height)
			: null;
	function showReference(on = !settings.reference.shown) {
		settings.reference.shown = on;
	}
	// in the corner: the side panel shows, so the picture does
	$effect(() => {
		if (settings.reference.shown && settings.reference.place === 'corner')
			untrack(() => {
				settings.layout.dock = true;
				dockOpen = true;
			});
	});
	async function importReferences(files: File[]) {
		if (!files.length) return;
		showReference(true);
		try {
			const r = await refShelf.add(
				await Promise.all(
					files.map(async (f) => ({ name: f.name, bytes: new Uint8Array(await f.arrayBuffer()) }))
				)
			);
			if (r.added) docVersion++;
			const left = skippedNote(r);
			if (left) note(left, 5000);
		} catch (e) {
			note(`Couldn’t import the pictures: ${e}`, 5000);
		}
	}
	/**
	 * A pen on the reference ("Draw on it"): it draws on the canvas where that point of the
	 * picture sits (fitted inside the canvas, as the grid lays it), with whatever tool is on.
	 */
	function refPen(e: PointerEvent, frac: (x: number, y: number) => [number, number] | null) {
		const pic = refShelf.pic;
		const at = pic && pictureOnCanvas(canvasSize, pic);
		if (!canvas || !at) return;
		canvas.forwardPointer(e, (x, y) => {
			const f = frac(x, y);
			// (the picture isn't read yet: far off the canvas, so nothing lands)
			return f ? canvasFromPicture(at, f) : [-1e6, -1e6];
		});
	}
	/** the drawing, over the reference: the part of the canvas the picture covers */
	function refMirror(g: CanvasRenderingContext2D, w: number, h: number) {
		const src = canvas?.shownCanvas();
		const pic = refShelf.pic;
		const at = src && pic && pictureOnCanvas({ width: src.width, height: src.height }, pic);
		if (at) g.drawImage(src, at[0], at[1], at[2], at[3], 0, 0, w, h);
	}
	/** a colour picked from the reference: paint with it */
	function refPicked(c: string) {
		color = c;
		eraser = false;
		tool = 'brush';
	}

	// ---- frames in, for rotoscoping (S3-T04): image sequences, video frames ----
	let importOpen = $state(false);
	let sequenceInput = $state<HTMLInputElement>();
	let importStop = false;

	/**
	 * Bring frames in as drawings on a new layer, under the one being drawn on and half
	 * see-through, from this frame on. `next` gives each frame's RGBA (canvas size) until
	 * it gives none.
	 */
	async function importFrames(name: string, next: () => Promise<Uint8Array | null>) {
		if (!canvas || !status || busy) return;
		if (playing) stopPlay();
		const c = canvas;
		const drawingOn = status.active;
		const before = new Set(status.layers.map((l) => l.id));
		busy = 'importing';
		fileNote = '';
		importStop = false;
		const ids: number[] = [];
		let layer: number | undefined;
		/** the frames that came go on the timeline (all of them, or up to a stop) */
		const place = async () => {
			if (!layer || !ids.length) return;
			placeFrames(trackIn(theCut(), layer), frame, ids);
			tlLabel = 'Import frames';
			if (status?.layers.some((l) => l.id === drawingOn)) c.selectLayer(drawingOn);
			docVersion++;
			await showFrame();
		};
		try {
			c.addLayer(name);
			await tick();
			layer = status?.layers.find((l) => !before.has(l.id))?.id;
			if (!layer) throw new Error('no room for another layer');
			c.moveLayer(layer, 0);
			c.setLayer(layer, { opacity: 0.5 });
			for (let rgba = await next(); rgba; rgba = await next()) {
				const id = c.newDrawing();
				if (!c.putDrawing(id, rgba)) throw new Error('the canvas is busy');
				cache.touch([id]);
				ids.push(id);
				fileNote = `Importing frame ${ids.length}…`;
				// (to the frame store as it goes, so a long clip doesn't fill the window)
				if (ids.length % 12 === 0) {
					await cache.push(ids.slice(-12));
					cache.trim();
				}
				if (importStop) throw new Error('cancelled');
			}
			if (!ids.length) throw new Error('no frames came');
			await place();
			fileNote = `Imported ${ids.length} frame${ids.length === 1 ? '' : 's'} onto “${name}”.`;
		} catch (e) {
			await place();
			fileNote = String(e).includes('cancelled')
				? `Import stopped after ${ids.length} frame${ids.length === 1 ? '' : 's'}.`
				: `Couldn’t import: ${e instanceof Error ? e.message : e}`;
		} finally {
			busy = '';
		}
	}

	/** Picture files, in number order, fitted inside the canvas. */
	async function importSequence(files: File[]) {
		const list = naturalOrder(
			files.filter(
				(f) => f.type.startsWith('image/') || /\.(png|jpe?g|webp|gif|bmp)$/i.test(f.name)
			)
		);
		if (!list.length) return;
		const { width: W, height: H } = canvasSize;
		const off = new OffscreenCanvas(W, H);
		const g = off.getContext('2d', { willReadFrequently: true })!;
		g.imageSmoothingQuality = 'high';
		let i = 0;
		await importFrames(sequenceName(list[0].name), async () => {
			const f = list[i++];
			if (!f) return null;
			let bmp: ImageBitmap;
			try {
				bmp = await createImageBitmap(f);
			} catch {
				throw new Error(`${f.name} isn’t a picture the studio can read`);
			}
			const at = fitInside(bmp.width, bmp.height, W, H);
			g.clearRect(0, 0, W, H);
			g.drawImage(bmp, at.x, at.y, at.width, at.height);
			bmp.close();
			return new Uint8Array(g.getImageData(0, 0, W, H).data.buffer);
		});
	}

	/** A video's frames at the project's frame rate (ffmpeg reads it). */
	async function importVideo() {
		const { open: pick } = await import('@tauri-apps/plugin-dialog');
		const file = await pick({
			multiple: false,
			directory: false,
			filters: [{ name: 'Video', extensions: ['mp4', 'mov', 'm4v', 'webm', 'mkv', 'avi', 'gif'] }]
		});
		if (typeof file !== 'string') return;
		const { invoke } = await import('@tauri-apps/api/core');
		const { width, height } = canvasSize;
		try {
			await invoke('video_in_open', { path: file, width, height, fps });
		} catch (e) {
			fileNote = `Couldn’t import: ${e}`;
			return;
		}
		try {
			await importFrames(sequenceName(file), async () => {
				const b = await invoke<ArrayBuffer>('video_in_frame');
				return b.byteLength ? new Uint8Array(b) : null;
			});
		} finally {
			await invoke('video_in_close').catch(() => {});
		}
	}

	// ---- colouring (flats on a colour layer, lines from a line layer) ----
	/** a fill is being carried through frames (its timeline changes make one step with it) */
	let carrying = false;

	/** The shapes a line drawing's lines close off (and its tone lines', given theirs). */
	async function shapesOf(drawing: number, tones: number[] = []) {
		await cache.ensure([drawing, ...tones]);
		const { width: w, height: h } = canvasSize;
		const wall = walls(canvas!.drawingRgba(drawing), w, h);
		for (const t of tones) {
			const more = walls(canvas!.drawingRgba(t), w, h);
			for (let i = 0; i < wall.length; i++) wall[i] |= more[i];
		}
		return regions(wall, w, h);
	}

	/**
	 * The tone-line layers a fill on `layer` also stops at: a shading layer's fills stop
	 * at the tone lines right above their lines; flats fill whole shapes.
	 */
	function toneLayersFor(layer: number, reference: number): number[] {
		if (!status || !reference || timeline.stages?.[layer] !== 'shade') return [];
		return toneLinesAbove(status.layers, reference, (id) => timeline.stages?.[id]);
	}

	/**
	 * After a fill with its lines from a line layer: the colour drawing is held as long as
	 * the line drawing; with "Carry to next frames", the same shape in each following
	 * line drawing (to the loop range's end, or the cut's) gets the colour too. All of it
	 * is one undo step.
	 */
	async function carryFill(f: { x: number; y: number; hex: string; tolerance: number }) {
		const ref = toolOpts.fillReference;
		if (!canvas || !status || !ref || timelineFor !== canvasKey) return;
		const C = status.active;
		const cut = theCut();
		const line = cut.tracks[ref];
		if (C === ref || !line || timeline.statics.includes(C) || timeline.statics.includes(ref))
			return;
		const c = canvas;
		const f0 = frame;
		const make = (from: number) => {
			const id = from ? c.copyDrawing(from) : c.newDrawing();
			cache.touch([id]);
			return id;
		};
		carrying = true;
		let done = 0;
		try {
			// the drawing just filled: held as long as its line drawing
			colourFor(trackIn(cut, C), line, f0, make);
			if (!toolOpts.fillAcross) return;
			const d0 = drawingAt(line, f0);
			if (!d0) return;
			// a shadow's shapes are cut by the tone lines too (each frame's own)
			const toneTracks = toneLayersFor(C, ref)
				.map((id) => cut.tracks[id])
				.filter(Boolean);
			const tonesAt = (s: number) =>
				toneTracks.map((t) => drawingAt(t, s)).filter((d): d is number => !!d);
			const first = await shapesOf(d0, tonesAt(f0));
			const w = canvasSize.width;
			const at = first.labels[f.y * w + f.x];
			if (!at) return;
			let prev = pixelsOf(first.labels, at);
			const last =
				playRange && f0 <= playRange.to ? playRange.to : cutLength(cut, timeline.statics) - 1;
			// (each line drawing's frames: the colour and shadow drawings are timed as it)
			const starts = lineStartsAfter(line, f0, last);
			let skipped = 0;
			for (const s of starts) {
				note(`Carrying the fill… frame ${s + 1}`, 4000);
				const shapes = await shapesOf(drawingAt(line, s), tonesAt(s));
				const m = bestMatch(shapes.labels, shapes.sizes, prev);
				const seed = m.score >= 0.25 ? seedIn(shapes.labels, m.label, prev, w) : null;
				if (!seed) {
					skipped++;
					continue;
				}
				// (a colour drawing held over from before gets copied: it must be here)
				await cache.ensure([drawingAt(cut.tracks[C], s)]);
				colourFor(trackIn(cut, C), line, s, make);
				frame = s;
				await showFrame();
				if (canvas?.fillPoint(seed.x, seed.y, f.hex, f.tolerance)) {
					canvas.joinLastSteps();
					done++;
				}
				prev = pixelsOf(shapes.labels, m.label);
			}
			note(
				`Filled on ${done + 1} drawing${done ? 's' : ''}` +
					(skipped ? ` (${skipped} without a matching shape)` : '') +
					'.',
				3000
			);
		} finally {
			carrying = false;
			if (frame !== f0) {
				frame = f0;
				await showFrame();
			}
			// the colour drawings' timing goes in the same undo step as the fills
			tlJoin = true;
			tlLabel = done ? 'Fill across frames' : 'Fill';
			recordTimeline();
		}
	}

	/**
	 * A Colour layer under the line layer picked (fills take its lines), a Shadow and a
	 * Highlight clipped to it, and Tone lines above the lines (shadow and highlight fills
	 * stop at them too).
	 */
	async function setupColour() {
		if (!canvas || !status) return;
		const lineLayer = status.layers.find((l) => l.id === status!.active);
		if (!lineLayer || lineLayer.adjust) return;
		const c = canvas;
		const newLayer = async (name: string) => {
			const before = new Set((status?.layers ?? []).map((l) => l.id));
			c.addLayer(name);
			await tick();
			return status?.layers.find((l) => !before.has(l.id))?.id ?? 0;
		};
		const colour = await newLayer(`Colour (${lineLayer.name})`);
		if (!colour) return;
		// under the lines
		c.moveLayer(
			colour,
			status.layers.findIndex((l) => l.id === lineLayer.id)
		);
		await tick();
		const shadow = await newLayer(`Shadow (${lineLayer.name})`);
		if (shadow) c.setLayer(shadow, { clip: true, blend: 'multiply' });
		// highlights over the shadows, inside the colour too (fills stop at tone lines as theirs do)
		const highlight = await newLayer(`Highlight (${lineLayer.name})`);
		if (highlight) c.setLayer(highlight, { clip: true, blend: 'screen' });
		// tone lines right above the lines: shadow fills stop at them, Final leaves them out
		const tone = await newLayer(`Tone lines (${lineLayer.name})`);
		if (tone) {
			const at = (id: number) => status!.layers.findIndex((l) => l.id === id);
			const [t, l] = [at(tone), at(lineLayer.id)];
			const to = t < l ? l : l + 1;
			if (t !== to) c.moveLayer(tone, to);
			await tick();
		}
		// in their stages (the lines in clean-up, unless they're in one already)
		setStage(timeline, colour, 'flats');
		if (shadow) setStage(timeline, shadow, 'shade');
		if (highlight) setStage(timeline, highlight, 'shade');
		if (tone) setStage(timeline, tone, 'tone');
		if (!timeline.stages?.[lineLayer.id]) setStage(timeline, lineLayer.id, 'clean');
		tlJoin = true;
		tlLabel = 'Set up colour layers';
		docVersion++;
		c.selectLayer(colour);
		toolOpts.fillRefLayer = lineLayer.id;
		tool = 'fill';
		note(
			'Fill on the Colour layer. Draw tone lines (red for shadow, blue for highlight) on Tone lines: fills on Shadow and Highlight stop at them, and Final leaves them out.',
			6000
		);
	}

	// ---- lip-sync (S3-T02): Rhubarb in src-tauri/src/lipsync.rs ----
	let lipSync = $state<{
		clip: number;
		rhubarb: string | null;
		layer: number;
		dialog: string;
		toEnd: boolean;
		progress: number | null;
		error: string;
	} | null>(null);
	let lipSettings = $state<LipSyncSettings>({
		...LIPSYNC_DEFAULTS,
		...readLocal<Partial<LipSyncSettings>>('pp.app.lipsync', {})
	});
	$effect(() => keepLocal('pp.app.lipsync', $state.snapshot(lipSettings)));
	let lipStop = false;

	/** Layers mouths can go on: paint layers that animate (not static, not adjustments). */
	const mouthLayers = () =>
		(status?.layers ?? []).filter((l) => !l.adjust && !timeline.statics.includes(l.id));

	/** A layer's mouth chart (made if it has none; read back for `$state`). */
	function chartOf(layer: number): MouthChart {
		if (!timeline.mouths) timeline.mouths = [];
		const list = timeline.mouths;
		if (!list.some((m) => m.layer === layer)) list.push({ layer, shapes: {} });
		return list.find((m) => m.layer === layer)!;
	}

	/** A layer's drawings to choose mouths from: this cut's first (numbered), then others. */
	function drawingChoices(layer: number): { id: number; label: string }[] {
		const nums = drawingNumbers(theCut().tracks[layer] ?? []);
		const out = [...nums].map(([id, n]) => ({ id, label: `Drawing ${n}` }));
		for (const id of drawingsIn(timeline))
			if (!nums.has(id) && layerOfDrawing(timeline, id) === layer)
				out.push({ id, label: `Drawing #${id} (another cut)` });
		return out;
	}

	/** A small picture of one drawing, cropped to what's drawn on it. */
	async function drawingThumb(layer: number, drawing: number): Promise<string | null> {
		if (!canvas || !status) return null;
		await cache.ensure([drawing]);
		const k = Math.max(1, Math.ceil(Math.max(canvasSize.width, canvasSize.height) / 512));
		const pairs = status.layers
			.filter((l) => !l.adjust)
			.map((l): [number, number] => [l.id, l.id === layer ? drawing : 0]);
		const r = canvas.renderFrame(pairs, k, true);
		if (!r) return null;
		let [x0, y0, x1, y1] = [r.width, r.height, -1, -1];
		for (let y = 0; y < r.height; y++)
			for (let x = 0; x < r.width; x++)
				if (r.rgba[(y * r.width + x) * 4 + 3] > 16) {
					if (x < x0) x0 = x;
					if (x > x1) x1 = x;
					if (y < y0) y0 = y;
					if (y > y1) y1 = y;
				}
		if (x1 < 0) return null;
		const w = x1 - x0 + 1;
		const h = y1 - y0 + 1;
		const side = 112;
		const s = Math.min(1, (side - 8) / Math.max(w, h));
		const c = document.createElement('canvas');
		c.width = c.height = side;
		const g = c.getContext('2d')!;
		g.fillStyle = '#fff';
		g.fillRect(0, 0, side, side);
		const bmp = await createImageBitmap(new ImageData(r.rgba, r.width, r.height));
		g.drawImage(bmp, x0, y0, w, h, (side - w * s) / 2, (side - h * s) / 2, w * s, h * s);
		return c.toDataURL();
	}

	/** Where a clip's lip-sync goes in this cut: from which frame, how many. */
	function lipRange(clipId: number, toEnd: boolean) {
		const c = sceneClips().find((x) => x.id === clipId);
		if (!c) return null;
		const cs = cutStart(theScene(), cutIndex, timeline.statics);
		const len = sounds.frames(c.file, fps);
		const from = Math.max(0, c.start - cs);
		const end = c.start + len - cs;
		const cutLen = cutLength(theCut(), timeline.statics);
		const to = toEnd ? end : Math.min(end, cutLen);
		return { clip: c, from, frames: Math.max(0, to - from), longer: end > cutLen, cs };
	}

	async function openLipSync(clipId: number) {
		if (playing) stopPlay();
		const { invoke } = await import('@tauri-apps/api/core');
		const rhubarb = await invoke<string | null>('lipsync_tools').catch(() => null);
		const r = lipRange(clipId, false);
		if (!r) return;
		const layers = mouthLayers();
		const charted = timeline.mouths?.find((m) => layers.some((l) => l.id === m.layer));
		const layer =
			charted?.layer ??
			(layers.some((l) => l.id === status?.active) ? status!.active : layers[0]?.id);
		if (layer === undefined) return;
		lipSync = {
			clip: clipId,
			rhubarb,
			layer,
			dialog: dialogueText(theCut().dialogue, r.from, r.from + r.frames),
			// (a cut with nothing in it yet: the line makes it)
			toEnd: cutLength(theCut(), timeline.statics) <= 1,
			progress: null,
			error: ''
		};
	}

	async function runLipSync() {
		const ls = lipSync;
		if (!ls || !canvas || ls.progress !== null) return;
		const r = lipRange(ls.clip, ls.toEnd);
		if (!r || r.frames <= 0) {
			ls.error = 'This sound isn’t heard in this cut.';
			return;
		}
		const chart = chartOf(ls.layer);
		const { invoke } = await import('@tauri-apps/api/core');
		const { listen } = await import('@tauri-apps/api/event');
		ls.error = '';
		ls.progress = 0;
		lipStop = false;
		const off = await listen<number>('lipsync-progress', (e) => {
			if (lipSync && lipSync.progress !== null) lipSync.progress = e.payload;
		});
		try {
			const mix = await sounds.mixdown(
				[{ clips: [{ ...r.clip, muted: false, gain: 1 }], from: r.cs + r.from, frames: r.frames }],
				fps,
				16000
			);
			if (!mix) throw new Error('the sound couldn’t be read');
			await invoke('lipsync_audio', wav(mix));
			const cues = await invoke<Cue[]>('lipsync_run', {
				opts: { english: lipSettings.english, dialog: ls.dialog, extended: extendedOf(chart) }
			});
			if (lipStop) throw new Error('cancelled');
			const shapes = shapeFrames(cues, fps, r.frames, lipSettings.step, lipSettings.lead);
			writeMouths(trackIn(theCut(), ls.layer), r.from, shapes, chart);
			tlLabel = 'Lip-sync';
			lipSync = null;
			note(`Lip-synced ${r.frames} frames of “${r.clip.name}”.`, 3000);
			timelineEdited(true);
		} catch (e) {
			if (lipSync) {
				lipSync.progress = null;
				lipSync.error = String(e).includes('cancelled') ? '' : `Couldn’t lip-sync: ${e}`;
			}
		} finally {
			off();
		}
	}

	async function cancelLipSync() {
		lipStop = true;
		const { invoke } = await import('@tauri-apps/api/core');
		await invoke('lipsync_cancel').catch(() => {});
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
	// (kept with the settings, where Settings changes its colours and counts too)
	const onion: OnionSettings = $derived(settings.onion);
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
		// (static layers show the same drawing on every frame: nothing to onion)
		const layers = viewLayerIds().filter((id) => !timeline.statics.includes(id));
		const entries = onionEntries(
			cut,
			layers,
			status.active,
			onCanvas.frame,
			onion,
			pinned,
			timeline.kinds
		);
		let bmp: ImageBitmap | null = null;
		if (entries.length) {
			await cache.ensure(entries.map((e) => e.drawing));
			if (token !== onionToken || !canvas) return;
			const k = Math.max(1, Math.ceil(canvasSize.width / 1920));
			const moved = isShifted(shift.before) || isShifted(shift.after);
			if (!moved) {
				const img = canvas.onionRgba(entries, k);
				if (img) bmp = await createImageBitmap(img);
			} else {
				// shift & trace: each side drawn where it was moved to
				const sides = [
					{ list: entries.filter((e) => e.colour === onion.beforeColour), at: shift.before },
					{ list: entries.filter((e) => e.colour === onion.afterColour), at: shift.after },
					{
						list: entries.filter(
							(e) => e.colour !== onion.beforeColour && e.colour !== onion.afterColour
						),
						at: { x: 0, y: 0, r: 0 }
					}
				];
				const out = new OffscreenCanvas(
					Math.ceil(canvasSize.width / k),
					Math.ceil(canvasSize.height / k)
				);
				const g = out.getContext('2d')!;
				for (const side of sides) {
					if (!side.list.length) continue;
					const img = canvas.onionRgba(side.list, k);
					if (!img) continue;
					const part = await createImageBitmap(img);
					g.save();
					g.translate(out.width / 2 + side.at.x / k, out.height / 2 + side.at.y / k);
					g.rotate((side.at.r * Math.PI) / 180);
					g.drawImage(part, -out.width / 2, -out.height / 2);
					g.restore();
					part.close();
				}
				bmp = out.transferToImageBitmap();
			}
		}
		if (token !== onionToken) return bmp?.close();
		onionImage?.close();
		onionImage = bmp;
	}
	// ---- shift & trace: move the onion skin's drawings (only how they show) to line up ----
	type Shift = { x: number; y: number; r: number };
	let shift = $state<{ before: Shift; after: Shift }>({
		before: { x: 0, y: 0, r: 0 },
		after: { x: 0, y: 0, r: 0 }
	});
	const isShifted = (s: Shift) => s.x !== 0 || s.y !== 0 || s.r !== 0;
	// a new frame: back where they belong
	$effect(() => {
		void [frame, cutIndex, scene];
		untrack(() => {
			if (isShifted(shift.before) || isShifted(shift.after))
				shift = { before: { x: 0, y: 0, r: 0 }, after: { x: 0, y: 0, r: 0 } };
		});
	});
	// settings, pins or the active layer changed
	$effect(() => {
		void $state.snapshot(shift);
		void $state.snapshot(onion);
		void [...pinned];
		void status?.active;
		void viewKey;
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

	// ---- frame tools: drawings copied and boiled are new, so the store gets them ----
	const frameDrawings = {
		ensure: (ids: number[]) => cache.ensure(ids),
		copy: (d: number) => {
			const id = canvas?.copyDrawing(d) ?? 0;
			cache.touch([id]);
			return id;
		},
		// (the strength is for a 1080-pixel-high canvas: bigger canvases move further)
		boil: (d: number, amount: number, seed: number) => {
			const id = canvas?.boilDrawing(d, (amount * canvasSize.height) / 1080, seed) ?? 0;
			cache.touch([id]);
			return id;
		}
	};

	// ---- the cut's camera: what the cut shows of the canvas ----
	/** show the camera frame over the canvas, and its safe areas */
	let cameraView = $state(cleanCameraView(readLocal('pp.app.camera', null)));
	$effect(() => keepLocal('pp.app.camera', $state.snapshot(cameraView)));
	function cleanCameraView(raw: unknown): { show: boolean; safe: boolean } {
		const o = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
		return { show: o.show !== false, safe: o.safe === true };
	}
	/** the camera drawn over the canvas (none: no camera set, or it's hidden) */
	const cameraOverlay = $derived.by(() => {
		if (!cameraView.show || playing || timelineFor !== canvasKey) return null;
		const cut = theCut();
		if (!cut.camera && !cameraView.safe) return null;
		return { ...cameraOf(cut, canvasSize), safe: cameraView.safe };
	});

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
	/** how much the rendered frames may take (RGBA): views multiply them */
	const RENDERED_BYTES = 384 * 1024 * 1024;
	let playTimer = 0;

	/** What a frame looks like depends on: its drawings (and their changes), the layers. */
	function frameKey(pairs: [number, number][], shrink: number, hidden: number[] = []) {
		const layers = (status?.layers ?? []).map((l) => [
			l.id,
			l.visible,
			l.opacity,
			l.blend,
			l.clip,
			l.mask,
			l.group,
			l.adjust,
			l.style,
			l.drawing ? 0 : l.empty
		]);
		return JSON.stringify([
			pairs.map(([l, d]) => [l, d, cache.version(d)]),
			layers,
			status?.groups ?? [],
			shrink,
			hidden
		]);
	}

	/** Draw a rendered frame: what the cut's camera sees, as big as fits. */
	function drawPlayer(bmp: ImageBitmap | undefined, whole = false) {
		const el = playerEl;
		if (!el || !bmp) return;
		const dpr = window.devicePixelRatio || 1;
		const [w, h] = [Math.round(el.clientWidth * dpr), Math.round(el.clientHeight * dpr)];
		if (el.width !== w || el.height !== h) [el.width, el.height] = [w, h];
		const g = el.getContext('2d');
		if (!g) return;
		// the camera, in the rendered frame's pixels
		const cam = whole
			? { x: 0, y: 0, width: canvasSize.width, height: canvasSize.height }
			: cameraOf(theCut(), canvasSize);
		const s = bmp.width / (whole ? bmp.width : canvasSize.width);
		const [sx, sy, sw, sh] = whole
			? [0, 0, bmp.width, bmp.height]
			: [cam.x * s, cam.y * s, cam.width * s, cam.height * s];
		const k = Math.min(w / sw, h / sh);
		const [dw, dh] = [sw * k, sh * k];
		g.fillStyle = getComputedStyle(el).backgroundColor || '#e8e4da';
		g.fillRect(0, 0, w, h);
		g.imageSmoothingQuality = 'high';
		g.drawImage(bmp, sx, sy, sw, sh, (w - dw) / 2, (h - dh) / 2, dw, dh);
	}

	/**
	 * Render frames of this cut for the player (each distinct picture once, kept for next
	 * time); frame → its picture's key. Null if playing stopped meanwhile.
	 */
	async function renderFrames(todo: number[]): Promise<Map<number, string> | null> {
		if (!canvas || !status) return null;
		const cut = theCut();
		const shrink = previewShrink(canvasSize.width);
		// only the view's layers (the others' drawings aren't needed)
		const layerIds = viewLayerIds();
		const hidden = inView.hidden;
		const t0 = performance.now();
		let made = 0;
		if (renderedFor !== canvasKey) {
			for (const b of rendered.values()) b.close();
			rendered = new Map();
			renderedFor = canvasKey;
		}
		// eslint-disable-next-line svelte/prefer-svelte-reactivity -- a local lookup, not state
		const keys = new Map<number, string>();
		preparing = { done: 0, total: todo.length };
		try {
			for (const f of todo) {
				if (!playing || !canvas) return null;
				const pairs = showing(cut, layerIds, f, timeline.statics);
				const key = frameKey(pairs, shrink, hidden);
				keys.set(f, key);
				if (!rendered.has(key)) {
					await cache.ensure(pairs.map(([, d]) => d));
					const r = canvas?.renderFrame(pairs, shrink, false, hidden);
					if (!r) {
						stopPlay();
						return null;
					}
					rendered.set(key, await createImageBitmap(new ImageData(r.rgba, r.width, r.height)));
					made++;
					cache.trim();
				}
				preparing = { done: preparing!.done + 1, total: todo.length };
			}
		} catch (e) {
			note(`Couldn’t play: ${e}`);
			stopPlay();
			return null;
		} finally {
			preparing = null;
		}
		// (how long a frame takes, for the performance budget: see the console)
		if (made)
			console.debug(
				`renderFrames: ${made} distinct frames in ${(performance.now() - t0).toFixed(0)} ms ` +
					`(${((performance.now() - t0) / made).toFixed(1)} ms a frame; view hides ${hidden.length} layers)`
			);
		// at most RENDERED_BYTES of frames stay rendered (each view has its own)
		let bytes = 0;
		for (const b of rendered.values()) bytes += b.width * b.height * 4;
		while (bytes > RENDERED_BYTES && rendered.size) {
			const [k, b] = rendered.entries().next().value!;
			bytes -= b.width * b.height * 4;
			b.close();
			rendered.delete(k);
		}
		return keys;
	}

	// ---- flipping: hold a key, the drawings roll by; let go, you're back ----
	let flipping: { origin: number; timer: ReturnType<typeof setInterval> } | null = null;

	/** Start flipping: the drawings around this frame, or the cut's keys and breakdowns. */
	async function startFlip(kind: 'roll' | 'keys') {
		if (flipping || playing || !status) return;
		await showQueue;
		const cut = theCut();
		const layers = viewLayerIds();
		const list =
			kind === 'roll'
				? rollFrames(drawingStarts(cut, [status.active], timeline.statics), frame)
				: keyFrames(cut, layers, timeline.kinds ?? {}, timeline.statics);
		if (list.length < 2) {
			note(
				kind === 'roll'
					? 'Nothing to flip: draw a drawing or two more.'
					: 'Mark keys or breakdowns first (Shift+K) to flip through them.'
			);
			return;
		}
		const origin = frame;
		flipping = { origin, timer: 0 as unknown as ReturnType<typeof setInterval> };
		playing = true;
		const keys = await renderFrames(list);
		if (!keys || !flipping) return;
		let i = 0;
		const show = () => {
			const f = list[flipIndex(i++, list.length)];
			frame = f;
			drawPlayer(rendered.get(keys.get(f)!));
		};
		show();
		flipping.timer = setInterval(show, 1000 / FLIP_FPS);
	}

	/** Mark the active layer's drawing on this frame: key → breakdown → in-between → none. */
	function markKind() {
		if (!status) return;
		const d = drawingAt(theCut().tracks[status.active], frame);
		if (!d) return note('No drawing here to mark.');
		const k = nextKind(timeline.kinds?.[d]);
		setKind(timeline, d, k);
		tlLabel = k ? `Mark ${k === 'inbetween' ? 'in-between' : k}` : 'Unmark drawing';
		docVersion++;
		note(k ? `Marked as ${k === 'inbetween' ? 'an in-between' : `a ${k}`}.` : 'Unmarked.', 1500);
	}

	// ---- production stages: each layer's (Timeline.stages) ----
	const STAGE_SHORT: Record<Stage, string> = {
		board: 'Board',
		rough: 'Rough',
		tiedown: 'Tie-down',
		clean: 'Clean',
		tone: 'Tone',
		flats: 'Colour',
		shade: 'Shade',
		comp: 'Comp'
	};
	const STAGE_CHOICES = STAGES.map((id) => ({
		id,
		label: STAGE_LABELS[id],
		short: STAGE_SHORT[id]
	}));

	// ---- viewing by stage: a view of the canvas (never the document, never an undo step) ----
	let stageView = $state<StageView>(VIEW_ALL);
	/** the layers the view shows and leaves out */
	const inView = $derived(layersInView(status?.layers ?? [], timeline.stages, stageView));
	/** what the view leaves out, as a key (frames rendered for one view aren't another's) */
	const viewKey = $derived(inView.hidden.join(','));
	/** the paint layers the view shows (adjustment layers have no drawings) */
	function viewLayerIds(): number[] {
		const out = new Set(inView.hidden);
		return (status?.layers ?? []).filter((l) => !l.adjust && !out.has(l.id)).map((l) => l.id);
	}
	// the canvas shows what the view does
	$effect(() => {
		const hidden = viewKey ? viewKey.split(',').map(Number) : [];
		if (status) canvas?.setViewHidden(hidden);
	});
	// a new canvas or project: every layer (an opened project then shows its own view)
	$effect(() => {
		void canvasKey;
		untrack(() => {
			stageView = VIEW_ALL;
			lightTable = LIGHT_TABLE_OFF;
		});
	});

	// ---- light table: another stage's drawings on this frame, faint, under the view ----
	// (a view as well: never exported, never an undo step, never shared; not in playback,
	// which plays the film as it will look)
	let lightTable = $state<LightTable>(LIGHT_TABLE_OFF);
	/** the layers it shows (its stage's, not the view's own) */
	const underLayers = $derived(
		lightTableLayers(
			(status?.layers ?? []).map((l) => ({
				id: l.id,
				adjust: l.adjust,
				visible: l.visible && (status?.groups.find((g) => g.id === l.group)?.visible ?? true)
			})),
			timeline.stages,
			lightTable,
			inView.shown
		)
	);
	/** why it shows nothing */
	const lightTableNote = $derived.by(() => {
		const st = lightTable.stage;
		if (!st || underLayers.length) return '';
		const tagged = (status?.layers ?? []).some((l) => timeline.stages?.[l.id] === st);
		return !tagged
			? `No layers are in ${STAGE_LABELS[st]}.`
			: inView.shown.some((id) => timeline.stages?.[id] === st)
				? 'The view shows this stage already.'
				: 'Its layers are hidden.';
	});
	let underImage = $state.raw<ImageBitmap | null>(null);
	let underKey = '';
	/** the drawings it shows now */
	let underNow = new Set<number>();
	let underToken = 0;
	/** bumps when a light table drawing may have changed (not when another layer is drawn on) */
	let underTick = $state(0);
	/** bumps when the canvas shows another frame */
	let underShown = $state(0);
	/** pictures made, by what they show: stepping back and forth doesn't make them again */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- a cache, not state
	const underMade = new Map<string, ImageBitmap>();

	/** Make (or find) the light table for the frame the canvas shows. */
	async function updateUnder(pairs: [number, number][]) {
		const token = ++underToken;
		const c = canvas;
		const lt = lightTable;
		underNow = new Set(pairs.map(([, d]) => d).filter((d) => d));
		if (!c || !pairs.some(([, d]) => d)) {
			underKey = '';
			underImage = null;
			return;
		}
		const k = Math.max(1, Math.ceil(canvasSize.width / 1920));
		const key = JSON.stringify([
			pairs,
			lt.tint,
			lt.opacity,
			k,
			canvasSize.width,
			canvasSize.height,
			canvasKey,
			underTick
		]);
		if (key === underKey) return;
		let bmp = underMade.get(key);
		if (!bmp) {
			await cache.ensure(pairs.map(([, d]) => d));
			if (token !== underToken || !canvas) return;
			if (lt.tint) {
				const img = canvas.onionRgba(
					pairs.map(([, d]) => ({ drawing: d, colour: lt.tint!, opacity: lt.opacity })),
					k
				);
				if (!img) return;
				bmp = await createImageBitmap(img);
			} else {
				// its own colours: the frame of just these layers, on transparency, faint
				const keep = new Set(pairs.map(([l]) => l));
				const others = (status?.layers ?? []).map((l) => l.id).filter((id) => !keep.has(id));
				const r = canvas.renderFrame(pairs, k, true, others);
				if (!r) return;
				const src = await createImageBitmap(new ImageData(r.rgba, r.width, r.height));
				const out = new OffscreenCanvas(r.width, r.height);
				const g = out.getContext('2d')!;
				g.globalAlpha = lt.opacity;
				g.drawImage(src, 0, 0);
				src.close();
				bmp = out.transferToImageBitmap();
			}
			if (token !== underToken) return bmp.close();
			underMade.set(key, bmp);
			// a few frames' worth stay made (oldest go first; never the one showing)
			for (const [old, b] of underMade) {
				if (underMade.size <= 12) break;
				if (b === underImage) continue;
				b.close();
				underMade.delete(old);
			}
		} else {
			// (most recently used last)
			underMade.delete(key);
			underMade.set(key, bmp);
		}
		if (token !== underToken) return;
		underKey = key;
		underImage = bmp;
	}
	// follows the frame, the settings, the stages and the layers (not every stroke)
	$effect(() => {
		void underShown;
		const cut = timeline.scenes[onCanvas.scene]?.cuts[onCanvas.cut];
		const pairs =
			cut && timelineFor === canvasKey && underLayers.length
				? showing(cut, underLayers, onCanvas.frame, timeline.statics)
				: [];
		void [lightTable.tint, lightTable.opacity, underTick, canvasSize.width, canvasSize.height];
		untrack(() => void updateUnder(pairs));
	});
	// a new canvas: what was made was another's
	$effect(() => {
		void canvasKey;
		untrack(() => {
			for (const b of underMade.values()) b.close();
			underMade.clear();
			underKey = '';
			underImage = null;
		});
	});
	/**
	 * The canvas changed: the light table is made again unless it was drawing on another
	 * layer (or undoing that), which can't change it.
	 */
	function underChanged(s: CanvasStatus) {
		if (!underNow.size) return;
		if (underLayers.includes(s.active)) underTick++;
		else underTouched(canvas?.undoDrawing() ?? 0, canvas?.redoDrawing() ?? 0);
	}
	/** These drawings changed (someone drawing together, an undo): one of the light table's? */
	function underTouched(...ds: number[]) {
		if (ds.some((d) => d && underNow.has(d))) underTick++;
	}

	/** Show a view; drawing on a layer it leaves out moves to the top one it shows. */
	function setView(v: StageView) {
		stageView = v;
		if (playing) stopPlay();
		const now = layersInView(status?.layers ?? [], timeline.stages, v);
		if (!status || !now.hidden.includes(status.active)) return;
		const paint = new Set(status.layers.filter((l) => !l.adjust).map((l) => l.id));
		const top = now.shown.filter((id) => paint.has(id)).at(-1);
		if (top) canvas?.selectLayer(top);
	}

	/**
	 * Make a layer (`make`), then put the new layer(s) in `stage`: a single stage's view's,
	 * or a duplicated layer's own. With the new layer: one undo step.
	 */
	function inStage<T>(make: () => T, stage: Stage | null): T {
		const before = new Set((status?.layers ?? []).map((l) => l.id));
		const out = make();
		if (stage)
			void Promise.resolve(out)
				.then(() => tick())
				.then(() => {
					const fresh = (status?.layers ?? []).filter(
						(l) => !before.has(l.id) && !timeline.stages?.[l.id]
					);
					if (!fresh.length) return;
					for (const l of fresh) setStage(timeline, l.id, stage);
					tlJoin = true;
					tlLabel = 'Set stage';
					docVersion++;
				});
		return out;
	}
	/** the Layers panel's controls: new layers go in the view's stage, copies in their own */
	const panelControls = $derived.by(() => {
		const c = layerControls;
		if (!c) return c;
		return new Proxy(c, {
			get(target, prop, recv) {
				const f = Reflect.get(target, prop, recv);
				if (typeof f !== 'function') return f;
				if (prop === 'addLayer' || prop === 'addLineLayer' || prop === 'importImage')
					return (...a: unknown[]) => inStage(() => f.apply(target, a), singleStage(stageView));
				if (prop === 'duplicateLayer')
					return (id: number) => inStage(() => f.call(target, id), timeline.stages?.[id] ?? null);
				return f;
			}
		});
	});

	/** Put a layer in a stage (null: in none), one undo step. */
	function putStage(layer: number, stage: string | null) {
		const st = isStage(stage) ? stage : null;
		if ((timeline.stages?.[layer] ?? null) === st) return;
		setStage(timeline, layer, st);
		tlLabel = 'Set stage';
		docVersion++;
	}

	// ---- character palettes (Timeline.palettes) ----
	function putPalettes(next: Palette[], label: string) {
		timeline.palettes = next;
		tlLabel = label;
		docVersion++;
	}
	/** the reference picture's flat colours (a model sheet: its paper and lines left out) */
	async function referenceColours() {
		const pic = refShelf.pic;
		if (!pic) return null;
		const bmp = await createImageBitmap(new Blob([pic.bytes as BlobPart]));
		try {
			const k = Math.min(1, 480 / Math.max(bmp.width, bmp.height));
			const [w, h] = [
				Math.max(3, Math.round(bmp.width * k)),
				Math.max(3, Math.round(bmp.height * k))
			];
			const c = document.createElement('canvas');
			[c.width, c.height] = [w, h];
			const g = c.getContext('2d', { willReadFrequently: true })!;
			g.drawImage(bmp, 0, 0, w, h);
			return { name: pic.name, colours: flatColours(g.getImageData(0, 0, w, h).data, w, h) };
		} finally {
			bmp.close();
		}
	}

	/** Put each layer with no stage in the one its name suggests, one undo step. */
	function sortStages() {
		let n = 0;
		for (const l of status?.layers ?? []) {
			if (timeline.stages?.[l.id]) continue;
			const st = guessStage(l, timeline.statics.includes(l.id));
			if (!st) continue;
			setStage(timeline, l.id, st);
			n++;
		}
		if (!n) return note('No layer’s name suggests a stage: pick each one’s stage instead.', 3000);
		tlLabel = 'Sort layers into stages';
		docVersion++;
		note(`Put ${n} layer${n > 1 ? 's' : ''} in stages.`, 2000);
	}

	/** The stages a layer traced over layer `id` can go in (after its stage, or its name's). */
	function traceStagesOf(id: number): Stage[] {
		const l = status?.layers.find((x) => x.id === id);
		if (!l || l.adjust) return [];
		return traceStages(timeline.stages?.[id] ?? guessStage(l, timeline.statics.includes(id)));
	}

	/**
	 * New layer from this: a layer in `stage` just above layer `id`, named after it, with an
	 * empty drawing wherever `id` has a drawing, held as long, in every cut (static if it
	 * is): trace each in turn and the timing matches. One undo step; a view that would
	 * leave the new layer out switches to its stage.
	 */
	async function traceLayer(id: number, stage: string) {
		const c = canvas;
		const src = status?.layers.find((l) => l.id === id);
		if (!c || !src || src.adjust || !isStage(stage) || timelineFor !== canvasKey) return;
		await showQueue;
		const layers = () => status?.layers ?? [];
		const before = new Set(layers().map((l) => l.id));
		// (made on the canvas itself: drawing together, it's shared just below)
		c.selectLayer(id);
		c.addLayer(traceName(src.name, stage));
		await tick();
		const fresh = layers().find((l) => !before.has(l.id));
		if (!fresh) return note('No room for another layer.', 2500);
		const t = sharing ? together : null;
		if (t) await t.shareAbove(fresh.id, id);
		else {
			// just above it (in one step with the new layer)
			const i = layers().findIndex((l) => l.id === id);
			const k = layers().findIndex((l) => l.id === fresh.id);
			if (i >= 0 && k !== i + 1) {
				c.moveLayer(fresh.id, k < i ? i : i + 1);
				c.joinLastSteps();
			}
		}
		const made = followTiming(timeline, id, fresh.id, () => c.newDrawing());
		cache.touch(made);
		setStage(timeline, fresh.id, stage);
		tlJoin = true;
		tlLabel = `New ${STAGE_LABELS[stage]} layer`;
		docVersion++;
		c.selectLayer(fresh.id);
		if (layersInView(layers(), timeline.stages, stageView).hidden.includes(fresh.id))
			setView(oneStageView(stage));
		await showFrame();
		note(
			`${fresh.name}: ${made.length} empty drawing${made.length === 1 ? '' : 's'} timed as ${src.name}, to trace.`,
			3000
		);
	}

	/** Let go: back to the frame flipping started on. */
	function endFlip() {
		if (flipping) stopPlay();
	}

	// ---- playing the whole scene: every cut, each through its camera, with transitions ----
	let playScope = $state<'cut' | 'scene'>('cut');

	async function playScene() {
		if (!canvas || !status) return;
		await showQueue;
		const si = scene;
		const sc = theScene();
		const cuts = cutsIn(timeline, 'scene', { scene: si, cut: cutIndex }, null);
		const layerIds = viewLayerIds();
		const hidden = inView.hidden;
		const runs = plan(timeline, cuts, layerIds, fps, fps);
		const frames = outputFrames(
			runs,
			(src) => timeline.scenes[src.scene]?.cuts[src.cut]?.transition,
			fps
		);
		if (!frames.length) return;
		// a small picture of each distinct frame, through its cut's camera
		const cam0 = cameraOf(sc.cuts[0], canvasSize);
		const k0 = Math.min(1, 720 / Math.max(cam0.width, cam0.height));
		const out = {
			width: Math.max(2, Math.round(cam0.width * k0)),
			height: Math.max(2, Math.round(cam0.height * k0))
		};
		playing = true;
		// eslint-disable-next-line svelte/prefer-svelte-reactivity -- a local cache, not state
		const byKey = new Map<string, ImageBitmap>();
		const pics: ImageBitmap[] = [];
		preparing = { done: 0, total: runs.length };
		try {
			for (const run of runs) {
				if (!playing || !canvas) return;
				const cut = timeline.scenes[run.source.scene].cuts[run.source.cut];
				const cam = cameraOf(cut, canvasSize);
				const key = `${run.source.cut}:${JSON.stringify(run.pairs)}`;
				let pic = byKey.get(key);
				if (!pic) {
					const at = placeCamera(cam, out);
					const k = Math.max(1, Math.floor(1 / at.k));
					await cache.ensure(run.pairs.map(([, d]) => d));
					const r = canvas.renderFrame(run.pairs, k, false, hidden);
					if (!r) return stopPlay();
					const src = await createImageBitmap(new ImageData(r.rgba, r.width, r.height));
					const c = new OffscreenCanvas(out.width, out.height);
					const g = c.getContext('2d')!;
					g.fillStyle = '#fff';
					g.fillRect(0, 0, out.width, out.height);
					g.imageSmoothingQuality = 'high';
					g.drawImage(
						src,
						cam.x / k,
						cam.y / k,
						cam.width / k,
						cam.height / k,
						at.x,
						at.y,
						at.width,
						at.height
					);
					src.close();
					pic = c.transferToImageBitmap();
					byKey.set(key, pic);
					cache.trim();
				}
				pics.push(pic);
				preparing = { done: preparing!.done + 1, total: runs.length };
			}
		} catch (e) {
			note(`Couldn’t play: ${e}`);
			return stopPlay();
		} finally {
			preparing = null;
		}
		const view = new OffscreenCanvas(out.width, out.height);
		const vg = view.getContext('2d')!;
		const clips = sc.audio ?? [];
		let t0 = performance.now();
		let lastN = -1;
		if (clips.length) sounds.play(clips, 0, fps);
		const tick = () => {
			if (!playing) {
				for (const b of byKey.values()) b.close();
				return;
			}
			let n = Math.floor(((performance.now() - t0) * fps) / 1000);
			if (n >= frames.length) {
				if (playMode === 'once') return stopPlay();
				t0 = performance.now();
				n = 0;
				if (clips.length) sounds.play(clips, 0, fps);
			}
			if (n !== lastN) {
				lastN = n;
				const fr = frames[n];
				vg.globalAlpha = 1;
				vg.drawImage(pics[fr.run], 0, 0);
				if (fr.over) {
					vg.globalAlpha = fr.over.alpha;
					vg.drawImage(pics[fr.over.run], 0, 0);
					vg.globalAlpha = 1;
				}
				if (fr.dim !== undefined) {
					vg.fillStyle = `rgba(0, 0, 0, ${1 - fr.dim})`;
					vg.fillRect(0, 0, out.width, out.height);
				}
				if (fr.white) {
					vg.fillStyle = `rgba(255, 255, 255, ${fr.white})`;
					vg.fillRect(0, 0, out.width, out.height);
				}
				// the timeline follows along
				const src = runs[fr.run].source;
				if (cutIndex !== src.cut) cutIndex = src.cut;
				if (frame !== src.frame) frame = src.frame;
				const bmp = view.transferToImageBitmap();
				drawPlayer(bmp, true);
				bmp.close();
			}
			playTimer = requestAnimationFrame(tick);
		};
		tick();
	}

	/** Play the range (or the cut) from the start of it, or stop. */
	async function togglePlay() {
		if (playing) return stopPlay();
		if (!canvas || !status) return;
		if (playScope === 'scene') return playScene();
		await showQueue;
		const cut = theCut();
		const len = cutLength(cut, timeline.statics);
		const range = clampRange(playRange ?? { from: 0, to: len - 1 }, len);
		playing = true;
		const keys = await renderFrames(framesOf(range));
		if (!keys) return;
		const t0 = performance.now();
		// the scene's sound plays along (forwards only: a ping-pong's way back is silent)
		const clips = sceneClips();
		const at = sceneFrameOf(0);
		let lastF = -1;
		let heard = false;
		const tick = () => {
			if (!playing) return;
			const n = Math.floor(((performance.now() - t0) * fps) / 1000);
			const f = frameAt(n, range, playMode);
			if (f === null) return stopPlay();
			if (clips.length && f !== lastF) {
				if (lastF >= 0 && f < lastF) {
					if (playMode === 'pingpong') {
						sounds.stop();
						heard = false;
					} else {
						sounds.play(clips, at + f, fps);
						heard = true;
					}
				} else if (!heard) {
					sounds.play(clips, at + f, fps);
					heard = true;
				}
				lastF = f;
			}
			if (f !== frame) frame = f;
			drawPlayer(rendered.get(keys.get(f)!));
			playTimer = requestAnimationFrame(tick);
		};
		tick();
	}

	function stopPlay() {
		if (!playing) return;
		playing = false;
		if (flipping) {
			clearInterval(flipping.timer);
			frame = flipping.origin;
			flipping = null;
		}
		sounds.stop();
		preparing = null;
		cancelAnimationFrame(playTimer);
		// the canvas shows where playback stopped
		void showFrame();
	}

	/** Loop from (or to) the current frame. */
	function setRange(end: 'from' | 'to') {
		const len = cutLength(theCut(), timeline.statics);
		const r = playRange ?? { from: 0, to: len - 1 };
		playRange = clampRange(end === 'from' ? { ...r, from: frame } : { ...r, to: frame }, len);
		if (end === 'from' && playRange.to < frame) playRange = { from: frame, to: len - 1 };
	}

	// a new, empty canvas: a new timeline. Not while a project is opening: emptying the
	// frame store then would land after the open filled it, and its drawings would be gone
	// (a big project's open outlasts the canvas mounting)
	$effect(() => {
		if (!status || pending || busy === 'opening' || timelineFor === canvasKey) return;
		untrack(() => {
			timeline = newTimeline(status!.layers.map((l) => ({ id: l.id, drawing: l.drawing })));
			timelineFor = canvasKey;
			[scene, cutIndex, frame] = [0, 0, 0];
			onCanvas = { scene: 0, cut: 0, frame: 0 };
			void cache.clear().catch(() => {});
			sounds.clear();
		});
	});

	// ---- the tools' own options (the same as the website's studio) ----
	const toolOpts = new ToolOptions({
		canvas: () => canvas,
		status: () => status ?? BLANK_STATUS,
		color: () => color,
		setTool: (t) => (tool = t),
		onguides: (g) => keepLocal('pp.app.guides', { w: canvasSize.width, h: canvasSize.height, g }),
		size: () => canvasSize,
		saveFile: async (name, text) => {
			const target = await askCompositionPath(name, settings.projectFolder);
			if (!target) return null;
			await writeExport(target, new TextEncoder().encode(text));
			return target;
		}
	});
	/** a shading fill's tone lines (see toneLayersFor) */
	const fillTones = $derived(status ? toneLayersFor(status.active, toolOpts.fillReference) : []);
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
	/**
	 * The guides for a canvas: a saved project's own (where they were moved to), or for a
	 * new canvas the ones last switched on (grid, perspective, symmetry…), every point in
	 * the middle: a mirror line moved on one drawing never carries over to the next.
	 */
	let projectGuides: GuideSettings | null = null;
	function loadGuides(w: number, h: number) {
		const own = projectGuides;
		projectGuides = null;
		if (own) {
			toolOpts.guides = own;
			return;
		}
		const saved = readLocal<{ w: number; h: number; g: GuideSettings } | null>(
			'pp.app.guides',
			null
		);
		const last = readGuides(saved?.g);
		if (!last || !saved) {
			toolOpts.guides = defaultGuides(w, h);
			return;
		}
		const k = Math.min(w / saved.w, h / saved.h);
		toolOpts.guides = centredGuides(
			{ ...last, grid: { ...last.grid, size: Math.max(2, Math.round(last.grid.size * k)) } },
			w,
			h
		);
	}
	// a new canvas or an opened project (not a resize: Canvas size moves the guides itself)
	$effect(() => {
		void canvasKey;
		untrack(() => loadGuides(canvasSize.width, canvasSize.height));
	});
	toolOpts.smoothing = {
		brush: 0.2,
		eraser: 0.2,
		smudge: 0.2,
		...readLocal<Partial<Record<SmoothTool, number>>>('pp.app.smoothing', {})
	};
	$effect(() => keepLocal('pp.app.smoothing', $state.snapshot(toolOpts.smoothing)));
	/** the painting tool whose smoothing the slider shows */
	const smoothKind = $derived(smoothTool(tool, eraser));
	toolOpts.quickShape = readLocal('pp.app.quickShape', true);
	toolOpts.closeFill = readLocal('pp.app.closeFill', false);
	toolOpts.animated = true;
	toolOpts.setupColour = () => void setupColour();

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
		recordTimeline();
		const step = canvas?.undoNote() ?? 0;
		await reveal(canvas?.undoDrawing() ?? 0);
		await sizeStep(false, () => {
			canvas?.undo();
			if (step) putTimeline(tlSteps.get(step)?.before);
		});
	}
	/** The History panel: several steps, each with its drawing brought back first. */
	async function historyGo(steps: number) {
		for (let i = 0; i < Math.abs(steps); i++) await (steps < 0 ? undo() : redo());
	}
	async function redo() {
		// (a frame still coming from the store: let it arrive first)
		await showQueue;
		if (toolOpts.adjustOpen) toolOpts.closeAdjust(false);
		recordTimeline();
		const step = canvas?.redoNote() ?? 0;
		await reveal(canvas?.redoDrawing() ?? 0);
		await sizeStep(true, () => {
			canvas?.redo();
			if (step) putTimeline(tlSteps.get(step)?.after);
		});
	}

	/**
	 * Undo / redo (`go`): if the step is a canvas size, every drawing it changes is
	 * brought from the frame store first (it puts them all back as they were), and they
	 * all go back to the store after. If they can't all come, the step doesn't happen.
	 */
	async function sizeStep(redoing: boolean, go: () => void) {
		const ids = canvas?.canvasStepDrawings(redoing) ?? [];
		const swaps = canvas?.canvasStepSwaps(redoing) ?? [];
		if (!ids.length && !swaps.length) return go();
		// (the others' canvases would stay the size they are)
		if (sharing) return note('Canvas size can’t change while drawing together.', 3500);
		if (swaps.length) return swapSizeStep(redoing, swaps, go);
		switching = true;
		try {
			await cache.hold(ids);
		} catch (e) {
			// (not all of them came: better not to undo than to lose frames)
			await cache.release();
			note(`Couldn’t ${redoing ? 'redo' : 'undo'} the canvas size: ${e}`, 4000);
			return;
		} finally {
			switching = false;
		}
		go();
		await cache.release(ids).catch((e) => note(`Couldn’t keep the frames: ${e}`, 4000));
	}

	/** the drawings the layers show now */
	const showingNow = () => new Set((status?.layers ?? []).map((l) => l.drawing).filter(Boolean));

	/**
	 * Undo / redo of a canvas size whose frames wait in the frame store: each drawing
	 * trades places there with its other size (nothing is copied); only the drawings
	 * showing come into the engine for it.
	 */
	async function swapSizeStep(redoing: boolean, pairs: [number, number][], go: () => void) {
		const c = canvas;
		if (!c) return;
		const key = (p: [number, number][]) => JSON.stringify(p);
		switching = true;
		try {
			const ok = await cache.swapStep(pairs, showingNow(), () => {
				go();
				// (it happened: the step moved to the other side of the history)
				return key(c.canvasStepSwaps(!redoing)) === key(pairs);
			});
			if (!ok)
				note(
					`Couldn’t ${redoing ? 'redo' : 'undo'} the canvas size: finish what you’re doing first.`,
					4000
				);
		} catch (e) {
			note(`Couldn’t ${redoing ? 'redo' : 'undo'} the canvas size: ${e}`, 4000);
		} finally {
			switching = false;
		}
		cache.trim();
	}

	// ---- canvas size (Image → Canvas size…): every frame of every layer, one undo step ----
	let canvasSizeOpen = $state(false);
	/** about how much paint the project has, in 64×64 tiles (for the memory check) */
	let projectTiles = $state(0);

	async function openCanvasSize() {
		fileOpen = exportOpen = importOpen = false;
		if (!canvas || !status || busy) return;
		if (sharing) return note('Canvas size can’t change while drawing together.', 3500);
		// (what the engine holds: a bit over, never under. With the frame store, frames are
		// resized one batch at a time and their undo waits in the store, so they don't count)
		projectTiles = canvas.tileCount();
		const png = await canvas.thumbnail(240).catch(() => null);
		sizePicture = png
			? URL.createObjectURL(new Blob([png as BlobPart], { type: 'image/png' }))
			: null;
		canvasSizeOpen = true;
	}
	/** the drawing as it shows, for the dialog's preview */
	let sizePicture = $state<string | null>(null);
	function closeCanvasSize() {
		canvasSizeOpen = false;
		if (sizePicture) URL.revokeObjectURL(sizePicture);
		sizePicture = null;
	}

	/**
	 * Change the canvas size: every drawing of every layer and frame, the masks, text
	 * layers' boxes, the cuts' cameras and the guides, in one undo step. In the app the
	 * frames are resized a batch at a time through the frame store, and the step swaps
	 * each with its new size there (so a project of any size can change, with undo). In a
	 * browser the engine has every drawing and changes them all at once. Nothing changes
	 * if any of it can't.
	 */
	async function resizeAll(plan: ResizePlan) {
		if (!canvas || !status || busy) return;
		if (sharing) return note('Canvas size can’t change while drawing together.', 3500);
		if (playing) stopPlay();
		await showQueue;
		if (toolOpts.adjustOpen) toolOpts.closeAdjust(false);
		// (a text being typed is kept first: its box moves with the rest)
		if (toolOpts.text.spec) toolOpts.text.done();
		const c = canvas;
		busy = 'resizing';
		switching = true;
		let changed: number[] = [];
		try {
			c.ensureDrawings();
			// whatever the timeline has waiting becomes its own step first
			recordTimeline();
			const texts = (status?.layers ?? []).flatMap((l) => {
				const t = l.text ? moveText(l.text, plan) : null;
				return t ? [[l.id, t] as [number, string]] : [];
			});
			const guides = moveGuides($state.snapshot(toolOpts.guides), plan);
			const resize = (pairs: [number, number][] = []) =>
				c.resizeCanvas(plan.to.width, plan.to.height, plan.dx, plan.dy, plan.scale, texts, pairs);
			const busyCanvas = 'the canvas is busy (finish the transform or adjustment first)';
			if (cache.native) {
				// frame by frame through the store: each drawing's new size is made and sent
				// there in turn; the step swaps them (its undo waits in the store, not in memory)
				const showing = showingNow();
				const ids = [...drawingsIn(timeline), ...showing];
				// (the new sizes' ids: past every drawing there is)
				c.reserveDrawings(Math.max(0, ...ids));
				const { width, height } = plan.to;
				const pairs = await cache.resizeEach(
					ids,
					showing,
					(from, to) => c.resizeDrawingInto(from, to, width, height, plan.dx, plan.dy, plan.scale),
					() => c.newDrawing(),
					(done, total) => {
						if (total > 8) fileNote = `Resizing frames: ${done} of ${total}`;
					}
				);
				fileNote = '';
				let ok = false;
				try {
					ok = await cache.swapStep(pairs, showing, () => resize(pairs));
				} finally {
					if (!ok) cache.drop(pairs.map(([, to]) => to));
				}
				if (!ok) throw new Error(busyCanvas);
			} else {
				const held = drawingsIn(timeline);
				await cache.hold(held);
				if (!resize()) throw new Error(busyCanvas);
				// what the engine changed: every drawing it had, the held ones among them
				changed = [...new Set([...held, ...c.canvasStepDrawings(false)])];
			}
			canvasSize = { ...plan.to };
			// the guides stay on the drawing (kept for the new size, so they aren't rescaled)
			keepLocal('pp.app.guides', { w: plan.to.width, h: plan.to.height, g: guides });
			toolOpts.guides = guides;
			// the cameras frame the same part of the drawing: one step with the pixels
			timeline = moveCameras(timeline, plan);
			[tlLabel, tlJoin] = ['Canvas size', true];
			docVersion++;
			recordTimeline();
			await updateOnion();
			note(`Canvas: ${plan.to.width} × ${plan.to.height}`);
		} catch (e) {
			note(`Couldn’t change the canvas size: ${e}`, 4000);
		} finally {
			switching = false;
			busy = '';
			fileNote = '';
			await cache
				.release(changed)
				.catch((e) => note(`Couldn’t keep the frames in the store: ${e}`, 4000));
		}
	}

	/** Image → Crop to selection: the canvas becomes the box around the selection. */
	function cropToSelection() {
		fileOpen = false;
		const box = canvas?.selectionBounds();
		if (!box) return note('Select something first.');
		const plan = planCrop(canvasSize, box);
		const ok = checkResize(plan, 0, Infinity);
		if (!ok.ok) return note(`Can’t crop to that: ${ok.why}`, 3500);
		void resizeAll(plan);
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
	/** an unsaved document's name (a Photoshop file opened: its name) */
	let unsavedName = $state<string | null>(null);
	/** the project's name, as shown and offered when saving */
	const docName = $derived(path ? projectName(path) : (unsavedName ?? projectName(null)));
	let created = $state<string | undefined>(undefined);
	/** this document's recovery copy (a new one for each opened or new document) */
	let docId = newDocId();
	/** the canvas version when last saved or opened (anything newer is unsaved; -1: all of it) */
	let savedVersion = $state(1);
	/** the canvas version in the recovery copy */
	let recoveredVersion = 0;
	let busy = $state<'' | 'saving' | 'opening' | 'exporting' | 'importing' | 'resizing'>('');
	let fileNote = $state('');
	/** something is drawn, on this frame or another */
	const drawn = $derived(!!status && (!status.blank || drawingsIn(timeline).length > 0));
	const unsaved = $derived(
		!!status &&
			(savedVersion === -1 || ((drawn || refShelf.pics.length > 0) && docVersion !== savedVersion))
	);
	/** a document being loaded, waiting for its canvas (and whether it starts unsaved) */
	let pending = $state.raw<{ doc: ProjectDoc; unsaved: boolean } | null>(null);
	let exportOpen = $state(false);

	// ---- drawing together (S12-T03): peer to peer, in the app's Rust side ----
	let sharing = $state<ShareInfo | null>(null);
	let shareOpen = $state(false);
	/** Settings, as a window over the canvas */
	let settingsOpen = $state(false);
	/** from home: join with this code once the canvas is ready */
	let joinOnReady = $state<string | null>(null);
	/** from home: offer this project's session again once it's open */
	let rejoinOnOpen = $state(false);
	$effect(() => {
		if (!joinOnReady || !canvas || !status) return;
		const code = joinOnReady;
		joinOnReady = null;
		void shareStart(code);
	});
	$effect(() => {
		if (!rejoinOnOpen || !savedShare) return;
		rejoinOnOpen = false;
		shareOpen = true;
	});
	let shareBusy = $state(false);
	// chat while drawing together: only while a session is on, never saved
	let chatLines = $state<ChatLine[]>([]);
	let chatOpen = $state(false);
	let chatUnread = $state(0);
	let chatId = 0;
	function chatAdd(line: Omit<ChatLine, 'id'>) {
		chatLines = capLines([...chatLines, { ...line, id: ++chatId }]);
		if (!chatOpen && line.kind === 'message' && !line.mine) chatUnread++;
	}
	function chatSend(typed: string) {
		const body = chatBody(typed);
		const me = sharing?.me;
		if (!body || !me || !sharing) return;
		for (const m of sharing.members)
			if (m.id !== me.id) void sendDirect(m.id, body).catch(() => {});
		chatAdd({ kind: 'message', from: me.id, name: me.name, mine: true, text: body.text });
	}
	$effect(() => {
		if (!sharing) {
			chatLines = [];
			chatUnread = 0;
			chatOpen = false;
		}
	});
	$effect(() => {
		if (chatOpen) chatUnread = 0;
	});
	let shareError = $state('');
	const cursorsOf = new Cursors();
	let cursorTick = $state(0);
	const remoteCursors = $derived.by(() => {
		void cursorTick;
		return sharing ? cursorsOf.list(sharing.members) : [];
	});
	const cursorOut = new Throttle<{ x: number; y: number }>((c) => {
		void sendPresence(c).catch(() => {});
	});

	/** the drawing side: shared layers, access, tiles (lib/together.ts) */
	let together = $state.raw<Together | null>(null);
	/** this project's shared session, from its file or from leaving (for Rejoin) */
	let savedShare = $state.raw<SavedShare | null>(null);
	let togetherTick = $state(0);
	let togetherTimer: ReturnType<typeof setTimeout> | undefined;

	function canvasPort(c: PaintCanvas): CanvasPort {
		return {
			addLayer: (name, adjust) =>
				adjust ? c.addAdjustmentLayerQuiet(adjust, name) : c.addLayerQuiet(name),
			setAdjust: (id, spec) => c.setAdjustmentQuiet(id, spec),
			maskPacked: (id) => c.maskPacked(id),
			putMask: (id, packed) => c.putMaskQuiet(id, packed),
			setLayer: (id, patch) => c.setLayerQuiet(id, patch as Partial<LayerSettings>),
			drawingOf: (id) => c.layerDrawingOf(id),
			newDrawing: () => c.newDrawing(),
			layerOfDrawing: (d) =>
				status?.layers.find((l) => l.drawing === d)?.id ?? layerOfDrawing(timeline, d),
			isLineLayer: (id) => c.isLines(id),
			setLineLayer: (id, on) => c.setLinesLayerQuiet(id, on),
			lineStrokes: (d) => c.lineStrokes(d),
			setLineStrokes: (d, list, plain) => {
				underTouched(d);
				return c.setLinesQuiet(d, list, false, plain);
			},
			plainStrokes: (d) => c.plainStrokes(d),
			baseChanges: (d) => c.baseChanges(d),
			baseAll: (d) => c.baseAll(d),
			putBaseChanges: (d, bytes) => {
				// (their change may be to the light table's drawings)
				underTouched(d);
				return c.putBaseChanges(d, bytes);
			},
			drawingsOfLayer: (id) =>
				timeline.scenes.flatMap((sc) =>
					sc.cuts.flatMap((cut) => (cut.tracks[id] ?? []).map((e) => e.drawing).filter((d) => d))
				),
			// (a drawing far from the playhead may be in the frame store: fetched back first)
			packed: async (d) => {
				await cache.ensure([d]);
				return c.drawingTilesPacked(d);
			},
			put: (d, p) => {
				const ok = c.putDrawingTiles(d, p);
				if (ok) cache.touch([d]);
				if (ok) underTouched(d);
				return ok;
			},
			sound: (file) => {
				const snd = sounds.get(file);
				return snd ? { bytes: snd.bytes, name: snd.name } : null;
			},
			putSound: async (file, bytes, name) => {
				await sounds.put(file, bytes, name);
				soundTick++;
			},
			timeline: () => $state.snapshot(timeline) as Timeline,
			setTimeline: (tl) => putTimeline(JSON.stringify(tl)),
			select: (id) => c.selectLayer(id),
			removeLayer: (id) => c.removeLayerQuiet(id),
			moveNextTo: (id, ref, above) => c.moveLayerQuietNextTo(id, ref, above),
			place: (id, ref, above, group) => c.placeLayerQuiet(id, ref, above, group),
			addGroup: (id) => c.addGroupQuiet(id),
			setGroup: (id, s) => void c.setGroupQuiet(id, s),
			ungroup: (id) => c.ungroupQuiet(id),
			mergeDown: (id) => c.mergeDownQuiet(id),
			mergeGroup: (id) => c.mergeGroupQuiet(id),
			groups: () => c.groupsNow(),
			layers: () => c.sharedLayerList()
		};
	}

	function startTogether(info: ShareInfo, joining: boolean) {
		if (!canvas) return;
		together = new Together(
			info.me.id,
			info.me.name,
			canvasPort(canvas),
			{
				edit: (body) => void sendEdit(body).catch(() => {}),
				direct: (to, body) => void sendDirect(to, body).catch(() => {})
			},
			() => togetherTick++
		);
		together.onnote = (text) => note(text, 4000);
		together.ontiles = (from) => {
			// their real pixels arrived: the live preview can go (unless they're mid-stroke)
			if (!liveStrokes.get(from)) canvas?.remoteClear(`live:${from}`);
		};
		const host = info.members[0]?.id;
		if (joining && host) together.startJoin(host);
		else void together.startHost();
		togetherTick++;
	}

	// ---- live strokes: others' strokes drawn as they draw them, until the real pixels come
	/** member → their preview layer here */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- bookkeeping, never rendered
	const livePreview = new Map<string, number>();
	/** member → strokes they have going */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- bookkeeping, never rendered
	const liveStrokes = new Map<string, number>();

	/** this app's stroke, as it's drawn: to the others (fast and lossy, like cursors) */
	function onLive(ev: LiveEvent) {
		const t = together;
		if (!t || !status || !t.synced) return;
		const lid = t.lidOf(status.active);
		if (!lid || !t.shared.canDraw(lid, t.me)) return;
		if (ev.type === 'points' && ev.pts.length > 96) {
			// datagrams stay small: long batches go in pieces (4 numbers a point)
			for (let i = 0; i < ev.pts.length; i += 96)
				void sendPresence({ live: { ...ev, pts: ev.pts.slice(i, i + 96) } }).catch(() => {});
		} else void sendPresence({ live: ev }).catch(() => {});
	}

	function takeLive(from: string, ev: LiveEvent) {
		const c = canvas;
		if (!c || !together?.synced) return;
		const key = `live:${from}`;
		const name = sharing?.members.find((m) => m.id === from)?.name ?? 'Someone';
		if (ev.type === 'begin') {
			if (!livePreview.has(from)) {
				const id = c.remoteJoin(key, `${name} (drawing now)`);
				if (!id) return;
				together.exclude(id);
				c.setLayerQuiet(id, { locked: true });
				livePreview.set(from, id);
			}
			liveStrokes.set(from, (liveStrokes.get(from) ?? 0) + 1);
			c.remoteBegin(key, ev.k, ev.style, name);
		} else if (ev.type === 'points') c.remotePoints(key, ev.k, ev.pts);
		else {
			c.remoteEnd(key, ev.k);
			liveStrokes.set(from, Math.max(0, (liveStrokes.get(from) ?? 1) - 1));
			if (ev.type === 'cancel') c.remoteClear(key);
		}
	}

	function dropLive() {
		if (canvas)
			for (const [from, id] of livePreview) {
				canvas.removeLayerQuiet(id);
				canvas.remoteForget(`live:${from}`);
			}
		livePreview.clear();
		liveStrokes.clear();
	}

	/** everyone who's been in the project (newest addresses first), to find one later */
	const everyone = (now: ShareInfo['members']) => [
		...now,
		...(savedShare?.members ?? []).filter((m) => !now.some((x) => x.id === m.id))
	];

	/** after the session: the layers stay (everyone keeps a full copy), unlocked */
	function stopTogether(info: ShareInfo | null = sharing) {
		clearTimeout(togetherTimer);
		if (together && info) savedShare = together.save(info.invite, everyone(info.members));
		together?.close();
		dropLive();
		if (together && canvas)
			for (const id of together.ids.values()) canvas.setLayerQuiet(id, { locked: false });
		together = null;
		togetherTick++;
	}

	/**
	 * the canvas changed: send this app's changes a moment later (one batch per pause; a
	 * stroke's tiles go about 40 ms after it ends, well within the 100 ms others wait for
	 * them; a send already going takes in what changes meanwhile)
	 */
	function togetherChanged() {
		if (!together) return;
		clearTimeout(togetherTimer);
		togetherTimer = setTimeout(() => void together?.localChanged(), 40);
	}

	/** Back to this project's shared session: join who's online, or host it again. */
	async function shareRejoin(): Promise<boolean> {
		const saved = savedShare;
		if (!saved || !canvas) return false;
		shareBusy = true;
		shareError = '';
		try {
			const name = settings.shareName.trim();
			void saveSettings().catch(() => {});
			const info = await rejoinSession(
				saved.code,
				saved.members.map((m) => m.addr).filter(Boolean),
				name,
				cleanRelay(settings.shareRelay)
			);
			sharing = info;
			together = new Together(
				info.me.id,
				info.me.name,
				canvasPort(canvas),
				{
					edit: (body) => void sendEdit(body).catch(() => {}),
					direct: (to, body) => void sendDirect(to, body).catch(() => {})
				},
				() => togetherTick++
			);
			together.onnote = (text) => note(text, 4000);
			together.ontiles = (from) => {
				if (!liveStrokes.get(from)) canvas?.remoteClear(`live:${from}`);
			};
			together.restore(saved);
			const host = info.members[0]?.id;
			if (info.host || !host) await together.resumeHost();
			else together.rejoin(host);
			togetherTick++;
			return true;
		} catch (e) {
			shareError = String(e);
			return false;
		} finally {
			shareBusy = false;
		}
	}

	/**
	 * The session dropped (not by leaving): back in by itself, a few times, then the
	 * dialog with Rejoin. Not when the host closed it (rejoining would host it again).
	 */
	async function reconnect(reason: string) {
		const can = !!savedShare && !!settings.shareName.trim() && !/host left/i.test(reason);
		if (can)
			for (const wait of [2000, 5000, 12000]) {
				note('Connection lost: reconnecting…', wait + 3000);
				await new Promise((r) => setTimeout(r, wait));
				// (rejoined by hand meanwhile, or the project changed)
				if (sharing || !savedShare) return;
				if (await shareRejoin()) {
					note('Reconnected.', 2500);
					return;
				}
			}
		shareError = can ? `${reason} Couldn’t reconnect by itself: press Rejoin.` : reason;
		shareOpen = true;
	}

	async function shareStart(join?: string) {
		shareBusy = true;
		shareError = '';
		try {
			const name = settings.shareName.trim();
			void saveSettings().catch(() => {});
			const relay = cleanRelay(settings.shareRelay);
			sharing = join ? await joinSession(join, name, relay) : await hostSession(name, relay);
			startTogether(sharing, !!join);
		} catch (e) {
			shareError = String(e);
		} finally {
			shareBusy = false;
		}
	}

	async function shareLeave() {
		shareBusy = true;
		cursorOut.stop();
		const was = sharing;
		await leaveSession().catch(() => {});
		sharing = null;
		stopTogether(was);
		cursorsOf.clear();
		cursorTick++;
		shareBusy = false;
	}

	/** a change to someone else's group, from its slider, to ask for when it's let go */
	let groupWish: { id: number; patch: Partial<GroupSettings> } | null = null;
	const memberName = (id: string) => sharing?.members.find((m) => m.id === id)?.name ?? 'someone';

	/** what the layers panel does while drawing together */
	const layerControls = $derived.by(() => {
		void togetherTick;
		const c = canvas;
		if (!c || !sharing || !together) return c;
		const t = together;
		return new Proxy(c, {
			get(target, prop, recv) {
				if (prop === 'addLayer') return (name?: string) => t.addOwnLayer(name);
				if (prop === 'addLineLayer') return (name?: string) => t.addOwnLayer(name, true);
				const own = (id: number) => {
					const lid = t.lidOf(id);
					if (lid && t.shared.layers.get(lid)?.owner === t.me) return lid;
					note('Only the person who made a shared layer can move it.');
					return null;
				};
				if (prop === 'moveLayer')
					return (id: number, to: number) => {
						const lid = own(id);
						// it goes where the layer now at `to` is, in the shared order
						const there = status?.layers[to]?.id;
						const at = there ? t.shared.order.indexOf(t.lidOf(there) ?? '') : -1;
						if (lid && at >= 0) t.moveOwn(lid, at);
					};
				/** what a change that needs others' leave did: asked them, or went ahead */
				const asked = (owners: string[] | null) => {
					if (owners === null)
						note('Only shared layers and groups can change while drawing together.');
					else if (owners.length) {
						const who = owners.map(memberName).join(' and ');
						note(`Asked ${who}: it happens when they say yes (Together).`, 4000);
					}
				};
				if (prop === 'mergeDown') return (id: number) => asked(t.mergeDown(id));
				if (prop === 'deleteLayer') return (id: number) => asked(t.deleteLayer(id));
				if (prop === 'addGroup')
					return (id: number) => {
						asked(t.addGroup(id));
						return 0;
					};
				if (prop === 'moveLayerTo')
					return (id: number, to: number, group: number) => {
						const there = status?.layers[to]?.id;
						const at = there ? t.shared.order.indexOf(t.lidOf(there) ?? '') : -1;
						if (at < 0) return;
						asked(t.moveLayerTo(id, at, group));
					};
				if (prop === 'ungroup') return (id: number) => asked(t.ungroup(id));
				if (prop === 'deleteGroup') return (id: number) => asked(t.deleteGroup(id));
				if (prop === 'mergeGroup') return (id: number) => asked(t.mergeGroup(id));
				if (prop === 'moveGroup') return (id: number, up: boolean) => asked(t.moveGroup(id, up));
				const mayChangeGroup = (id: number) => {
					const gid = t.gidOf(id);
					return !gid || t.shared.canDraw(gid, t.me);
				};
				if (prop === 'setGroup')
					return (id: number, patch: Partial<GroupSettings>) => {
						if (mayChangeGroup(id)) target.setGroup(id, patch);
						else asked(t.changeGroup(id, () => target.setGroup(id, patch)));
					};
				if (prop === 'previewGroup')
					return (id: number, patch: Partial<GroupSettings>) => {
						if (mayChangeGroup(id)) target.previewGroup(id, patch);
						else groupWish = { id, patch: { ...groupWish?.patch, ...patch } };
					};
				if (prop === 'commitGroup')
					return () => {
						const w = groupWish;
						groupWish = null;
						if (!w) return target.commitGroup();
						asked(t.changeGroup(w.id, () => target.setGroup(w.id, w.patch)));
					};
				if (prop === 'setLayer')
					return (id: number, patch: Partial<LayerSettings>) => {
						target.setLayer(id, patch);
						const lid = t.lidOf(id);
						if (lid && patch.name && t.shared.canDraw(lid, t.me))
							void sendEdit({ k: 'rename', lid, name: patch.name }).catch(() => {});
					};
				return Reflect.get(target, prop, recv);
			}
		});
	});

	/** the project has arrived (joining); refreshed with the session's changes */
	const togetherSynced = $derived.by(() => {
		void togetherTick;
		return !!together?.synced;
	});

	/** shared layers, for the Draw together dialog */
	const sharedLayers = $derived.by(() => {
		void togetherTick;
		const t = together;
		if (!t || !sharing) return [];
		const name = (id: string) => sharing?.members.find((m) => m.id === id)?.name ?? 'Someone';
		const nameOf = (lid: string | undefined) =>
			(lid && (t.shared.layers.get(lid) ?? t.shared.groups.get(lid))?.name) || 'a layer';
		/** what someone asks for, in words */
		const what = (a: Ask, group: boolean) => {
			if (a.why === 'merge')
				return a.own
					? `asks for it, to merge it into “${nameOf(a.with)}”`
					: group
						? 'asks to merge this group into one layer'
						: `asks to merge “${nameOf(a.with)}” into it`;
			if (a.why === 'delete') return 'asks for it, to delete it';
			if (a.why === 'group')
				return a.own
					? 'asks for this group, to take it apart'
					: group
						? 'asks to change this group'
						: 'asks to put it in a group';
			return 'asks to draw here';
		};
		const row = (lid: string, l: SharedLayer | SharedGroup, group: boolean) => ({
			lid,
			name: group ? `Group: ${l.name}` : l.name,
			group,
			owner: name(l.owner),
			mine: l.owner === t.me,
			canDraw: l.writers.includes(t.me),
			asked: t.shared.asks.some((a) => a.lid === lid && a.from === t.me),
			writers: l.writers.filter((w) => w !== l.owner).map((id) => ({ id, name: name(id) })),
			asks: t.shared.asks
				.filter((a) => a.lid === lid && l.owner === t.me)
				.map((a) => ({
					id: a.from,
					name: name(a.from),
					text: what(a, group),
					own: !!a.own,
					draw: (a.why ?? 'draw') === 'draw'
				}))
		});
		return [
			...t.shared.order.map((lid) => row(lid, t.shared.layers.get(lid)!, false)),
			...[...t.shared.groups.values()].map((g) => row(g.gid, g, true))
		];
	});
	const asksForMe = $derived(sharedLayers.reduce((n, l) => n + l.asks.length, 0));
	/** a new ask for this person: said, so it isn't missed */
	let asksSeen = 0;
	$effect(() => {
		if (asksForMe > asksSeen) note('Someone is asking you about a layer: see Together.', 4000);
		asksSeen = asksForMe;
	});

	function onCursor(at: [number, number] | null) {
		if (sharing && at) cursorOut.push({ x: Math.round(at[0]), y: Math.round(at[1]) });
	}

	onMount(() => {
		if (!isTauri()) return;
		let stop: (() => void) | undefined;
		let gone = false;
		void sessionStatus()
			.then((s) => (sharing = s))
			.catch(() => {});
		void onShareEvent((e) => {
			if (!sharing) return;
			if (e.kind === 'presence') {
				const live = (e.body as { live?: LiveEvent } | null)?.live;
				if (live && typeof live === 'object' && typeof live.type === 'string')
					takeLive(e.from, live);
				else if (isCursor(e.body) && cursorsOf.set(e.from, e.body.x, e.body.y, performance.now()))
					cursorTick++;
			} else if (e.kind === 'joined') {
				if (!sharing.members.some((m) => m.id === e.member.id))
					chatAdd({ kind: 'system', text: `${e.member.name || 'Someone'} joined` });
				sharing.members = [...sharing.members.filter((m) => m.id !== e.member.id), e.member];
			} else if (e.kind === 'left') {
				const who = sharing.members.find((m) => m.id === e.id);
				if (who) chatAdd({ kind: 'system', text: `${who.name || 'Someone'} left` });
				sharing.members = sharing.members.filter((m) => m.id !== e.id);
				if (cursorsOf.remove(e.id)) cursorTick++;
			} else if (e.kind === 'host_changed') {
				sharing = { ...sharing, members: e.members, host: e.you, invite: e.invite };
				for (const id of cursorsOf.list([]).map((c) => c.id))
					if (!e.members.some((m) => m.id === id)) cursorsOf.remove(id);
				cursorTick++;
			} else if (e.kind === 'edit') {
				void together?.onEdit(e.seq, e.from, e.body);
			} else if (e.kind === 'direct') {
				const said = chatText(e.body);
				if (said !== null) {
					const name = sharing.members.find((m) => m.id === e.from)?.name || 'Someone';
					chatAdd({ kind: 'message', from: e.from, name, text: said });
				} else void together?.onDirect(e.from, e.body);
			} else if (e.kind === 'closed') {
				const was = sharing;
				sharing = null;
				stopTogether(was);
				cursorsOf.clear();
				cursorTick++;
				void reconnect(e.reason);
			}
		}).then((un) => {
			if (gone) un();
			else stop = un;
		});
		const prune = setInterval(() => {
			if (cursorsOf.prune(performance.now())) cursorTick++;
		}, 1000);
		return () => {
			gone = true;
			stop?.();
			clearInterval(prune);
			cursorOut.stop();
		};
	});

	/** Everything the window knows, as a project document. */
	/** (with `timelapse`: its pictures too; recovery copies leave them out, to stay light) */
	async function currentDoc(timelapse = false): Promise<ProjectDoc> {
		if (!canvas) throw new Error('the canvas isn’t ready');
		canvas.ensureDrawings();
		// the store gets what changed; then the file is made from it, no pixels sent
		await cache.push();
		// (an animation's layer pixels are its drawings, in the store: not copied out)
		const snap = canvas.snapshotLayers({ pixels: !cache.native });
		if (!snap) throw new Error('the canvas isn’t ready');
		const layerIds = snap.layers.filter((l) => !l.adjust).map((l) => l.id);
		const c = canvas;
		const drawings = drawingsIn(timeline).flatMap((id) => {
			const layer = layerOfDrawing(timeline, id);
			if (layer === null || !layerIds.includes(layer)) return [];
			// editable strokes: a line layer's, and a plain layer's (with the pixels under them)
			const lines = c.isLines(layer) || c.plainStrokes(id) ? c.lineData(id) : null;
			const withLines = lines ? { lines } : {};
			if (cache.native) return [{ id, layer, ...withLines }];
			// (no store: the pixels go in the packet; an empty drawing as no bytes)
			const rgba = c.drawingEmpty(id) ? new Uint8Array(0) : c.drawingRgba(id);
			return [{ id, layer, rgba, ...withLines }];
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
			extra: {
				brushId,
				color,
				size,
				opacity,
				scene,
				cut: cutIndex,
				frame,
				// this project's guides, where they were moved to
				guides: $state.snapshot(toolOpts.guides),
				// what it showed (a view by stage)
				...(stageView.id !== 'all' ? { view: $state.snapshot(stageView) } : {}),
				// and the stage on its light table
				...(JSON.stringify(lightTable) !== JSON.stringify(LIGHT_TABLE_OFF)
					? { lightTable: $state.snapshot(lightTable) }
					: {}),
				// a shared project keeps its session, to rejoin later
				...(together && sharing
					? { share: together.save(sharing.invite, everyone(sharing.members)) }
					: savedShare
						? { share: savedShare }
						: {})
			},
			created,
			thumbnail: await canvas.thumbnail(),
			scenes: toFile(timeline, layerIds),
			statics: staticsToFile(timeline, layerIds),
			mouths: mouthsToFile(timeline, layerIds),
			kinds: kindsToFile(
				timeline,
				drawings.map((d) => d.id)
			),
			stages: stagesToFile(
				timeline,
				snap.layers.map((l) => l.id)
			),
			palettes: timeline.palettes ?? [],
			audio: [
				...new Set(timeline.scenes.flatMap((s) => (s.audio ?? []).map((a) => a.file)))
			].flatMap((file) => {
				const snd = sounds.get(file);
				return snd ? [{ file, bytes: snd.bytes }] : [];
			}),
			...(timelapse ? { timelapse: await c.timelapse().saved() } : {}),
			fonts: fontsUsed(snap.layers.map((l) => parseText(l.text))),
			references: refShelf.toFile(),
			drawings
		};
	}

	async function save(as = false) {
		if (!canvas || busy) return;
		const target = as || !path ? await askSavePath(docName, settings.projectFolder) : path;
		if (!target) return;
		busy = 'saving';
		fileNote = '';
		try {
			const versionAtSave = docVersion;
			path = await saveProject(target, await currentDoc(true));
			created ??= new Date().toISOString();
			savedVersion = versionAtSave;
			// saved for real: the recovery copy has done its job
			await discardRecovery(docId).catch(() => {});
			recoveredVersion = 0;
			if (together || savedShare)
				fileNote =
					'Saved your copy of this shared project. Open it any time and press Rejoin (Draw together) to carry on with the others.';
		} catch (e) {
			fileNote = `Couldn’t save: ${e}`;
		} finally {
			busy = '';
		}
	}

	/** the recovery copy kept when "Save before…?" wasn't asked (it stays for home to offer) */
	let keptCopy = '';
	/** Let go of a document's recovery copy, unless it was kept on purpose. */
	const dropRecovery = (id: string) =>
		id === keptCopy ? Promise.resolve() : discardRecovery(id).catch(() => {});

	/** "Save before…?": the question showing, and how to answer it */
	let saveAsk = $state<{
		title: string;
		answer: (choice: 'yes' | 'no' | 'cancel') => void;
	} | null>(null);
	const SAVE_BEFORE = {
		quit: 'Save before quitting?',
		open: 'Save before opening another project?',
		version: 'Save before opening an earlier version?',
		leave: 'Save before leaving?'
	};

	/**
	 * Unsaved changes would be lost: "Save before …?" Yes saves first (going ahead only if
	 * it did), No goes ahead without saving, Cancel stays. True to go ahead.
	 */
	async function okToDrop(why: keyof typeof SAVE_BEFORE = 'leave'): Promise<boolean> {
		if (!unsaved) return true;
		if (!settings.askToSave) {
			// not asking (Settings): go ahead, keeping a recovery copy so nothing is lost
			try {
				await writeRecovery(docId, path, await currentDoc());
				keptCopy = docId;
			} catch {
				return false;
			}
			return true;
		}
		const choice = await new Promise<'yes' | 'no' | 'cancel'>(
			(answer) => (saveAsk = { title: SAVE_BEFORE[why], answer })
		);
		saveAsk = null;
		if (choice === 'cancel') return false;
		if (choice === 'no') return true;
		await save(false);
		return !unsaved;
	}

	// ---- the File menu ----
	let fileOpen = $state(false);

	/** Put a document on a fresh canvas of its size. */
	function load(
		doc: ProjectDoc,
		opts: { path: string | null; unsaved: boolean; id?: string; name?: string }
	) {
		path = opts.path;
		unsavedName = opts.name ?? null;
		created = doc.created;
		fps = doc.fps;
		docId = opts.id ?? newDocId();
		recoveredVersion = 0;
		const x = doc.extra;
		if (typeof x.brushId === 'string') brushId = x.brushId;
		if (typeof x.color === 'string') color = x.color;
		if (typeof x.size === 'number') size = x.size;
		if (typeof x.opacity === 'number') opacity = x.opacity;
		projectGuides = readGuides(x.guides);
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
		if (busy || !(await okToDrop('open'))) return;
		const target = await askOpenPath(settings.projectFolder);
		if (!target) return;
		const old = docId;
		await run('opening', async () => {
			load(await openProject(target), { path: target, unsaved: false });
			await dropRecovery(old);
		});
	}

	/** A Photoshop file as a new, unsaved project (File → Open a Photoshop file…). */
	let psdInput = $state<HTMLInputElement>();
	async function openPsd(file: File) {
		if (busy || !(await okToDrop('open'))) return;
		const old = docId;
		busy = 'opening';
		fileNote = '';
		try {
			const { readPsd } = await import('@pencilpilot/paint/psd-read');
			const r = await readPsd(new Uint8Array(await file.arrayBuffer()));
			const big = tooBig(r.width, r.height);
			if (big) throw new Error(big);
			load(psdToDoc(r, settings.newFps), { path: null, unsaved: true, name: psdStem(file.name) });
			await dropRecovery(old);
			await tick();
			if (r.notes.length) fileNote = `Opened ${file.name}. ${r.notes.join(' ')}`;
		} catch (e) {
			fileNote = `Couldn’t open ${file.name}: ${e instanceof Error ? e.message : e}`;
			busy = '';
		}
	}

	async function openOldVersion(id: string) {
		if (!path || busy || !(await okToDrop('version'))) return;
		const file = path;
		await run('opening', async () =>
			load(await openVersion(file, id), { path: file, unsaved: true })
		);
	}

	async function exportAs(kind: 'png' | 'psd') {
		if (!canvas || busy) return;
		const target = await askExportPath(docName, kind, settings.projectFolder);
		if (!target) return;
		busy = 'exporting';
		fileNote = '';
		try {
			let bytes: Uint8Array;
			if (kind === 'png') {
				const d = canvas.exportDrawing();
				if (!d) throw new Error('nothing to export');
				bytes = tagPng(new Uint8Array(await (await fetch(d.png)).arrayBuffer()), exportMeta());
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

	// ---- films of the drawing (S3-T05): the timelapse, or this frame drawn live ----
	let film = $state<{
		video: string[];
		pictures: number;
		strokes: Stroke[];
		drawnSeconds: number;
		progress: { done: number; total: number } | null;
	} | null>(null);
	let filmSettings = $state<FilmSettings>({
		...FILM_DEFAULTS,
		...readLocal<Partial<FilmSettings>>('pp.app.film', {})
	});
	$effect(() => keepLocal('pp.app.film', $state.snapshot(filmSettings)));
	/** a canvas made for drawing a film on (not the document's) */
	let filmStage = $state<PaintCanvas>();
	let staging = $state(false);
	let filmStop = false;

	/** The layers and drawings showing on this frame. */
	const framePairs = () =>
		showing(
			theCut(),
			(status?.layers ?? []).filter((l) => !l.adjust && l.visible).map((l) => l.id),
			frame,
			timeline.statics
		);

	async function openFilm() {
		exportOpen = false;
		if (!canvas || !status) return;
		if (playing) stopPlay();
		const { invoke } = await import('@tauri-apps/api/core');
		const tools = await invoke<{ video: string[] }>('export_tools').catch(() => ({ video: [] }));
		await canvas.timelapse().settled();
		const drawingsOn = (layer: number) =>
			drawingsIn(timeline).filter((d) => layerOfDrawing(timeline, d) === layer).length;
		const strokes = strokesOfFrame(canvas.loggedStrokes(), framePairs(), drawingsOn);
		film = {
			video: tools.video,
			pictures: canvas.timelapse().length,
			strokes,
			drawnSeconds: liveSchedule(strokes).duration,
			progress: null
		};
	}

	/** The film's size: the timelapse's pictures, or the cut's camera at the scale. */
	function filmSize(): { width: number; height: number } {
		const video = filmSettings.format !== 'gif';
		if (filmSettings.kind === 'live')
			return outputSize(cameraOf(theCut(), canvasSize), filmSettings.scale, video);
		const last = canvas?.timelapse().frames.at(-1);
		return outputSize({ x: 0, y: 0, width: last?.width ?? 2, height: last?.height ?? 2 }, 1, video);
	}

	async function runFilm() {
		const f = film;
		if (!f || !canvas || busy) return;
		const s = $state.snapshot(filmSettings) as FilmSettings;
		const target = await askExportPath(
			`${docName} ${s.kind === 'live' ? 'drawn live' : 'timelapse'}`,
			s.format,
			settings.projectFolder
		);
		if (!target) return;
		const { invoke } = await import('@tauri-apps/api/core');
		const out = filmSize();
		busy = 'exporting';
		fileNote = '';
		filmStop = false;
		const off = new OffscreenCanvas(out.width, out.height);
		const g = off.getContext('2d', { willReadFrequently: true })!;
		g.imageSmoothingQuality = 'high';
		/** pictures as [bitmap or null (the last one again), frames held] */
		let sent = 0;
		let total = 0;
		const send = async (repeat: number) => {
			const px = g.getImageData(0, 0, out.width, out.height).data;
			await invoke('export_frame', new Uint8Array(px.buffer, px.byteOffset, px.byteLength), {
				headers: { repeat: String(repeat) }
			});
			sent += repeat;
			if (film) film.progress = { done: sent, total };
			if (filmStop) throw new Error('cancelled');
		};
		const paper = () => {
			g.globalCompositeOperation = 'source-over';
			g.globalAlpha = 1;
			g.fillStyle = '#fff';
			g.fillRect(0, 0, out.width, out.height);
		};
		try {
			if (s.kind === 'timelapse') {
				const pics = canvas.timelapse().frames.slice();
				const plan = runsOf(filmPlan(pics.length, FILM_FPS, s.seconds));
				total = plan.reduce((n, [, r]) => n + r, 0);
				await invoke('export_begin', {
					spec: {
						format: s.format,
						path: target,
						width: out.width,
						height: out.height,
						fps: FILM_FPS,
						transparent: false,
						frames: plan.length,
						total,
						once: false,
						quality: settings.exportQuality,
						meta: exportMeta()
					}
				});
				for (const [i, repeat] of plan) {
					const b = pics[i];
					const at = placeCamera({ x: 0, y: 0, width: b.width, height: b.height }, out);
					const pic = await b.picture();
					paper();
					g.drawImage(pic, at.x, at.y, at.width, at.height);
					await send(repeat);
				}
			} else {
				const cam = cameraOf(theCut(), canvasSize);
				const at = placeCamera(cam, out);
				const k = Math.max(1, Math.floor(1 / at.k));
				const draw = async (
					r: { rgba: Uint8ClampedArray<ArrayBuffer>; width: number; height: number },
					alpha = 1
				) => {
					const bmp = await createImageBitmap(new ImageData(r.rgba, r.width, r.height));
					g.globalAlpha = alpha;
					g.drawImage(
						bmp,
						cam.x / k,
						cam.y / k,
						cam.width / k,
						cam.height / k,
						at.x,
						at.y,
						at.width,
						at.height
					);
					g.globalAlpha = 1;
					bmp.close();
				};
				// the finished frame, as the document shows it
				const pairs = framePairs();
				await cache.ensure(pairs.map(([, d]) => d));
				const done = canvas.renderFrame(pairs, k);
				if (!done) throw new Error('the canvas went away');
				const finished = { rgba: done.rgba.slice(), width: done.width, height: done.height };
				// a canvas of the same size to draw the strokes on again
				staging = true;
				let player: Awaited<ReturnType<PaintCanvas['strokePlayer']>> = null;
				for (let tries = 0; tries < 200 && !player; tries++) {
					await new Promise((r) => setTimeout(r, 50));
					const layers = (status?.layers ?? []).filter((l) => pairs.some(([id]) => id === l.id));
					player =
						(await filmStage?.strokePlayer(
							f.strokes,
							layers.map((l) => ({ id: l.id, name: l.name }))
						)) ?? null;
				}
				if (!player || !filmStage) throw new Error('the film canvas didn’t start');
				const { events, duration } = liveSchedule(f.strokes, s.speed);
				const drawFrames = Math.max(1, Math.ceil(duration * FILM_FPS));
				const pause = Math.round(0.5 * FILM_FPS);
				const fade = Math.round(0.6 * FILM_FPS);
				const hold = Math.round(1.5 * FILM_FPS);
				total = drawFrames + pause + fade + hold;
				// (frames where nothing new is drawn are sent once, held)
				const steps = liveSteps(
					events.map((v) => v.t),
					drawFrames,
					FILM_FPS
				);
				let e = 0;
				await invoke('export_begin', {
					spec: {
						format: s.format,
						path: target,
						width: out.width,
						height: out.height,
						fps: FILM_FPS,
						transparent: false,
						// pictures sent (each held some frames): the drawing, the lines
						// (held for the pause), the fade, the finished frame (held)
						frames: steps.length + 1 + fade + 1,
						total,
						once: false,
						quality: settings.exportQuality,
						meta: exportMeta()
					}
				});
				for (const step of steps) {
					while (e < step.upTo) {
						player.drawTo(events[e].stroke, events[e].points);
						e++;
					}
					const r = filmStage.renderFrame([], k);
					if (!r) throw new Error('the film canvas went away');
					paper();
					await draw(r);
					await send(step.repeat);
				}
				while (e < events.length) player.drawTo(events[e].stroke, events[e++].points);
				const r = filmStage.renderFrame([], k);
				if (!r) throw new Error('the film canvas went away');
				const lines = { rgba: r.rgba.slice(), width: r.width, height: r.height };
				paper();
				await draw(lines);
				await send(pause);
				for (let i = 1; i <= fade; i++) {
					paper();
					await draw(lines);
					await draw(finished, ease(i / fade));
					await send(1);
				}
				paper();
				await draw(finished);
				await send(hold);
			}
			const where = await invoke<string>('export_end');
			fileNote = `Exported to ${where}`;
			film = null;
		} catch (e) {
			await invoke('export_cancel').catch(() => {});
			fileNote = String(e).includes('cancelled') ? 'Export cancelled.' : `Couldn’t export: ${e}`;
			if (film) film.progress = null;
		} finally {
			staging = false;
			busy = '';
		}
	}

	// ---- exporting the animation (src-tauri/src/export.rs writes the file) ----
	let animExport = $state<{
		video: string[];
		ffmpeg: string | null;
		progress: { done: number; total: number } | null;
	} | null>(null);
	let exportSettings = $state<ExportSettings>({
		...EXPORT_DEFAULTS,
		...readLocal<Partial<ExportSettings>>('pp.app.export', {})
	});
	$effect(() => keepLocal('pp.app.export', $state.snapshot(exportSettings)));
	let exportStop = false;

	/** What exports say about the work (Settings can switch it off): see png-meta.ts. */
	function exportMeta() {
		return exportText({
			on: settings.exportCredits,
			author: settings.shareName,
			with: (sharing?.members ?? []).map((m) => m.name)
		});
	}

	async function openAnimExport() {
		exportOpen = false;
		if (playing) stopPlay();
		// it starts on the format Settings names
		exportSettings.format = settings.exportFormat;
		// a project with stages: the finished film (or what this project last used)
		exportSettings.show = !timeline.stages
			? 'all'
			: exportShow.key === canvasKey
				? exportShow.show
				: 'final';
		const { invoke } = await import('@tauri-apps/api/core');
		const tools = await invoke<{ ffmpeg: string | null; video: string[] }>('export_tools').catch(
			() => ({ ffmpeg: null, video: [] })
		);
		animExport = { ...tools, progress: null };
	}

	/** the export's Show this project last used (a project with stages starts on Final) */
	let exportShow = { key: -1, show: 'final' as ExportShow };

	/** Render the frames and send them to be written: once, or once a stage. */
	async function runAnimExport() {
		if (!canvas || !status || !animExport || busy) return;
		const s = $state.snapshot(exportSettings) as ExportSettings;
		const kind = s.format === 'png' ? 'png-seq' : s.format === 'psd' ? 'psd-seq' : s.format;
		const target = await askExportPath(docName, kind, settings.projectFolder);
		if (!target) return;
		await showQueue;
		if (timeline.stages) exportShow = { key: canvasKey, show: s.show };
		// which layers, by stage: one pass, or one per stage (a file each, named after it)
		const passes = exportPasses(timeline.stages ? s.show : 'all', timeline.stages, stageView);
		let last = '';
		for (const pass of passes) {
			const hidden = layersInView(status?.layers ?? [], timeline.stages, pass.view).hidden;
			const where = await exportPass(
				s,
				pass.label ? stageFile(target, pass.label) : target,
				hidden
			);
			// (cancelled or failed: the note says so)
			if (!where) return;
			last = where;
		}
		if (passes.length > 1) fileNote = `Exported ${passes.length} stages (the last: ${last})`;
		animExport = null;
	}

	/** One export: the layers the view `hidden` leaves out aren't in it. Where it went, or null. */
	async function exportPass(
		s: ExportSettings,
		target: string,
		hidden: number[]
	): Promise<string | null> {
		if (!canvas || !status || !animExport) return null;
		const f = FORMATS.find((x) => x.id === s.format)!;
		const outFps = s.fps || fps;
		const cuts = cutsIn(timeline, s.range, { scene, cut: cutIndex }, playRange);
		const left = new Set(hidden);
		const layerIds = status.layers.filter((l) => !l.adjust && !left.has(l.id)).map((l) => l.id);
		const runs = plan(timeline, cuts, layerIds, fps, outFps);
		const first = timeline.scenes[cuts[0].scene].cuts[cuts[0].cut];
		const out = outputSize(cameraOf(first, canvasSize), s.scale, f.video);
		const transparent = s.transparent && f.alpha;
		const { invoke } = await import('@tauri-apps/api/core');
		busy = 'exporting';
		fileNote = '';
		exportStop = false;
		animExport.progress = { done: 0, total: runs.length };
		if (f.files) return writeFrameFiles(s.format, target, runs, out, transparent, hidden);
		try {
			// a video gets the sound of the same frames
			if (f.video) {
				const mix = await sounds.mixdown(
					cuts.map((c) => {
						const sc = timeline.scenes[c.scene];
						return {
							clips: sc.audio ?? [],
							from: cutStart(sc, c.cut, timeline.statics) + c.from,
							frames: c.to - c.from + 1
						};
					}),
					fps
				);
				if (mix) await invoke('export_audio', wav(mix));
			}
			await invoke('export_begin', {
				spec: {
					format: s.format,
					path: target,
					width: out.width,
					height: out.height,
					fps: outFps,
					transparent,
					frames: joinFrames(
						outputFrames(
							runs,
							(src) => timeline.scenes[src.scene]?.cuts[src.cut]?.transition,
							outFps
						)
					).length,
					total: runs.reduce((n, r) => n + r.repeat, 0),
					once: s.once,
					quality: settings.exportQuality,
					meta: exportMeta()
				}
			});
			const off = new OffscreenCanvas(out.width, out.height);
			const g = off.getContext('2d', { willReadFrequently: true })!;
			g.imageSmoothingQuality = 'high';
			/** a run's picture at the output size (the last few kept: a dissolve needs two) */
			// eslint-disable-next-line svelte/prefer-svelte-reactivity -- a local cache, not state
			const pictures = new Map<number, ImageBitmap>();
			const picture = async (i: number) => {
				const have = pictures.get(i);
				if (have) return have;
				const run = runs[i];
				const cut = timeline.scenes[run.source.scene].cuts[run.source.cut];
				const cam = cameraOf(cut, canvasSize);
				const at = placeCamera(cam, out);
				// (render smaller first when the output is much smaller: faster, and smoother)
				const k = Math.max(1, Math.floor(1 / at.k));
				await cache.ensure(run.pairs.map(([, d]) => d));
				const r = canvas!.renderFrame(run.pairs, k, transparent, hidden);
				if (!r) throw new Error('the canvas went away');
				const src = await createImageBitmap(new ImageData(r.rgba, r.width, r.height));
				const pic = new OffscreenCanvas(out.width, out.height);
				const pg = pic.getContext('2d')!;
				pg.imageSmoothingQuality = 'high';
				if (!transparent) {
					pg.fillStyle = '#fff';
					pg.fillRect(0, 0, out.width, out.height);
				}
				pg.drawImage(
					src,
					cam.x / k,
					cam.y / k,
					cam.width / k,
					cam.height / k,
					at.x,
					at.y,
					at.width,
					at.height
				);
				src.close();
				const bmp = pic.transferToImageBitmap();
				pictures.set(i, bmp);
				for (const [key, old] of pictures)
					if (key < i - 6) {
						old.close();
						pictures.delete(key);
					}
				cache.trim();
				return bmp;
			};
			// the frames, with the transitions between cuts laid over them
			const frames = joinFrames(
				outputFrames(runs, (src) => timeline.scenes[src.scene]?.cuts[src.cut]?.transition, outFps)
			);
			animExport.progress = { done: 0, total: frames.length };
			for (const [i, [fr, repeat]] of frames.entries()) {
				if (exportStop) throw new Error('cancelled');
				g.globalAlpha = 1;
				g.clearRect(0, 0, out.width, out.height);
				g.drawImage(await picture(fr.run), 0, 0);
				if (fr.over) {
					g.globalAlpha = fr.over.alpha;
					g.drawImage(await picture(fr.over.run), 0, 0);
					g.globalAlpha = 1;
				}
				if (fr.dim !== undefined) {
					// darker, where there's picture
					g.globalCompositeOperation = 'source-atop';
					g.fillStyle = `rgba(0, 0, 0, ${1 - fr.dim})`;
					g.fillRect(0, 0, out.width, out.height);
					g.globalCompositeOperation = 'source-over';
				}
				if (fr.white) {
					g.fillStyle = `rgba(255, 255, 255, ${fr.white})`;
					g.fillRect(0, 0, out.width, out.height);
				}
				const px = g.getImageData(0, 0, out.width, out.height).data;
				await invoke('export_frame', new Uint8Array(px.buffer, px.byteOffset, px.byteLength), {
					headers: { repeat: String(repeat) }
				});
				if (animExport) animExport.progress = { done: i + 1, total: frames.length };
			}
			for (const b of pictures.values()) b.close();
			const where = await invoke<string>('export_end');
			fileNote = `Exported to ${where}`;
			return where;
		} catch (e) {
			await invoke('export_cancel').catch(() => {});
			fileNote = String(e).includes('cancelled') ? 'Export cancelled.' : `Couldn’t export: ${e}`;
			if (animExport) animExport.progress = null;
			return null;
		} finally {
			busy = '';
		}
	}

	/**
	 * PSD per frame, or a PNG sequence per layer: the window makes each file (cropped to
	 * the cut's camera and scaled like the other exports) and the app writes it.
	 */
	async function writeFrameFiles(
		format: ExportFormat,
		target: string,
		runs: Run[],
		out: { width: number; height: number },
		transparent: boolean,
		/** layers left out (a stage's pass) */
		hidden: number[] = []
	): Promise<string | null> {
		const c = canvas;
		if (!c || !status) {
			busy = '';
			return null;
		}
		const stem = target.replace(/\.(psd|png)$/i, '');
		const total = runs.reduce((n, r) => n + r.repeat, 0);
		const paintLayers = status.layers.filter((l) => !l.adjust && !hidden.includes(l.id));
		// (layers with nothing on any exported frame are left out of the per-layer files)
		const used = new Set(runs.flatMap((r) => r.pairs.filter(([, d]) => d).map(([l]) => l)));
		const off = new OffscreenCanvas(out.width, out.height);
		const g = off.getContext('2d', { willReadFrequently: true })!;
		g.imageSmoothingQuality = 'high';
		const { width: W, height: H } = canvasSize;
		/** a canvas-size picture, cropped to the camera and scaled into the output */
		const fit = async (rgba: Uint8Array | Uint8ClampedArray, cam: Camera) => {
			const at = placeCamera(cam, out);
			const px = new Uint8ClampedArray(rgba.buffer as ArrayBuffer, rgba.byteOffset, rgba.length);
			const bmp = await createImageBitmap(new ImageData(px, W, H), { premultiplyAlpha: 'none' });
			g.clearRect(0, 0, out.width, out.height);
			g.drawImage(bmp, cam.x, cam.y, cam.width, cam.height, at.x, at.y, at.width, at.height);
			bmp.close();
			return g.getImageData(0, 0, out.width, out.height).data;
		};
		const text = exportMeta();
		const png = async () =>
			tagPng(
				new Uint8Array(await (await off.convertToBlob({ type: 'image/png' })).arrayBuffer()),
				text
			);
		const { writePsd } = await import('@pencilpilot/paint/psd');
		let n = 0;
		let last = '';
		try {
			for (const [i, run] of runs.entries()) {
				if (exportStop) throw new Error('cancelled');
				const cam = cameraOf(timeline.scenes[run.source.scene].cuts[run.source.cut], canvasSize);
				await cache.ensure(run.pairs.map(([, d]) => d));
				const files: { part: string; ext: string; bytes: Uint8Array }[] = [];
				if (format === 'psd') {
					const doc = c.exportLayered({ pairs: run.pairs, transparent, hidden });
					if (!doc) throw new Error('the canvas went away');
					const items = [];
					for (const it of doc.items) {
						if (it.kind === 'layer') items.push({ ...it, rgba: await fit(it.rgba, cam) });
						else {
							const children = [];
							for (const ch of it.children) children.push({ ...ch, rgba: await fit(ch.rgba, cam) });
							items.push({ ...it, children });
						}
					}
					const flat = await fit(doc.composite, cam);
					if (!transparent) {
						// (bars around a cut of another shape are paper, as in the other exports)
						g.globalCompositeOperation = 'destination-over';
						g.fillStyle = '#fff';
						g.fillRect(0, 0, out.width, out.height);
						g.globalCompositeOperation = 'source-over';
					}
					const composite = transparent ? flat : g.getImageData(0, 0, out.width, out.height).data;
					files.push({
						part: '',
						ext: 'psd',
						bytes: writePsd({ width: out.width, height: out.height, composite, items })
					});
				} else {
					for (const l of paintLayers) {
						if (!used.has(l.id)) continue;
						const d = run.pairs.find(([id]) => id === l.id)?.[1] ?? 0;
						g.clearRect(0, 0, out.width, out.height);
						if (d) await fit(c.drawingRgba(d), cam);
						files.push({ part: l.name, ext: 'png', bytes: await png() });
					}
				}
				for (let k = 0; k < run.repeat; k++) {
					n++;
					for (const f of files) {
						last = frameFile(stem, n, total, f.ext, f.part);
						await writeExport(last, f.bytes);
					}
				}
				cache.trim();
				if (animExport) animExport.progress = { done: i + 1, total: runs.length };
			}
			fileNote = `Exported ${n} frame${n === 1 ? '' : 's'} (the last: ${last})`;
			return last;
		} catch (e) {
			fileNote = String(e).includes('cancelled')
				? `Export cancelled after ${n} frame${n === 1 ? '' : 's'}.`
				: `Couldn’t export: ${e}`;
			if (animExport) animExport.progress = null;
			return null;
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
		// its reference pictures
		void refShelf.load(doc.references);
		// its text layers' fonts (their pixels are there already: needed to change them)
		if (doc.fonts?.length)
			void Promise.all(doc.fonts.map((f) => addFont(f.family, f.file, f.bytes))).then(() =>
				toolOpts.text.listFonts()
			);
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
			// a shared project: its session, with this canvas's layer ids
			const share = readSavedShare(doc.extra.share);
			if (share)
				share.ids = share.ids
					.map(([lid, id]) => [lid, ids.get(id) ?? 0] as [string, number])
					.filter(([, id]) => id > 0);
			savedShare = share;
			const at = (v: unknown) => (typeof v === 'number' && v >= 0 ? Math.floor(v) : 0);
			if (anim) {
				// (drawings in the frame store come when their frames show)
				for (const d of doc.drawings ?? []) if (d.rgba) c.putDrawing(d.id, d.rgba);
				// line layers' strokes, editable again
				for (const d of doc.drawings ?? []) if (d.lines) c.putLineData(d.id, d.lines);
				// the store has this project's drawings; others there (an earlier project's, a
				// canvas size's other sizes for an undo that's gone) aren't needed any more
				void cache
					.opened(
						(doc.drawings ?? []).filter((d) => !d.rgba).map((d) => d.id),
						(doc.drawings ?? []).map((d) => d.id)
					)
					.catch(() => {});
				// (new drawings mustn't take the ids of ones waiting in the store)
				c.reserveDrawings(Math.max(0, ...(doc.drawings ?? []).map((d) => d.id)));
				timeline = fromFile(doc.scenes!, ids, doc.statics, doc.mouths);
				const kinds = kindsFromFile(doc.kinds);
				if (Object.keys(kinds).length) timeline.kinds = kinds;
				sounds.clear();
				void Promise.all(
					(doc.audio ?? []).map((a) =>
						sounds.put(a.file, a.bytes).catch(() => {
							fileNote = 'A sound in this project couldn’t be played here.';
						})
					)
				).then(() => soundTick++);
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
			const stages = stagesFromFile(doc.stages, ids);
			if (Object.keys(stages).length) timeline.stages = stages;
			const palettes = readPalettes(doc.palettes);
			if (palettes.length) timeline.palettes = palettes;
			stageView = readView(doc.extra.view);
			lightTable = readLightTable(doc.extra.lightTable);
			timelineFor = canvasKey;
			onCanvas = { scene, cut: cutIndex, frame };
			if (anim) {
				onCanvas = { scene: -1, cut: -1, frame: -1 };
				showFrame();
			}
			const active = ids.get(doc.activeLayer);
			if (active) c.selectLayer(active);
			// the timelapse so far (this session's frames follow it)
			if (doc.timelapse?.length) void c.timelapse().load(doc.timelapse);
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
			await dropRecovery(docId);
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

	// ---- preferences (Settings) the studio follows ----

	/** how the canvas shows and handles things (cursor, zoom, rulers, selection, guides) */
	const viewPrefs: ViewPrefs = $derived({
		cursor: settings.cursor,
		cursorWhilePainting: settings.cursorWhilePainting,
		crosshairWhenSmall: settings.crosshairWhenSmall,
		zoomToPointer: settings.zoomToPointer,
		wheel: settings.wheel,
		zoomSpeed: settings.zoomSpeed,
		smoothZoom: settings.smoothZoom,
		brushFollowsZoom: settings.brushFollowsZoom,
		maxBrushSize: settings.maxBrushSize,
		rulers: settings.rulers,
		units: settings.units,
		ppi: settings.ppi,
		selectionTint: settings.selectionTint,
		marchingAnts: settings.marchingAnts,
		gridColour: settings.gridColour,
		guideColour: settings.guideColour
	});

	// a new canvas starts as Settings says: its brush, its grid, and (the studio opened on
	// its own, not from a template) its size and frame rate
	onMount(() => {
		void loadSettings().then(() => {
			if (start.kind !== 'new') return;
			if (settings.defaultBrush) brushId = settings.defaultBrush;
			if (settings.gridSize && !readLocal('pp.app.guides', null))
				toolOpts.guides = {
					...toolOpts.guides,
					grid: { ...toolOpts.guides.grid, size: settings.gridSize }
				};
			if (page.url.searchParams.get('new')) return;
			fps = settings.newFps;
			const { newWidth: width, newHeight: height } = settings;
			if (width !== canvasSize.width || height !== canvasSize.height) {
				status = null;
				canvasSize = { width, height };
				canvasKey++;
			}
		});
	});

	// ---- workspaces: layouts of this one studio (see lib/workspaces.ts) ----
	const workspace = $derived(
		settings.workspaces.find((w) => w.id === settings.workspace) ?? settings.workspaces[0]
	);
	let workspacesOpen = $state(false);
	/** the side panel opened on the workspace's tab once the settings were read */
	let workspaceTab = false;
	// the layout and the tab in use go to the workspace in use, so it comes back as it was left
	$effect(() => {
		const w = workspace;
		const layout = $state.snapshot(settings.layout);
		const t = tab;
		if (!w || !settingsLoaded()) return;
		untrack(() => {
			if (!workspaceTab) {
				workspaceTab = true;
				tab = w.tab;
				return;
			}
			if (JSON.stringify($state.snapshot(w.layout)) !== JSON.stringify(layout)) w.layout = layout;
			if (w.tab !== t) w.tab = t;
		});
	});
	/** Switch workspace: its bars, panels, tab and tools (the canvas stays as it is). */
	function useWorkspace(id: string) {
		const w = settings.workspaces.find((x) => x.id === id);
		if (!w) return;
		settings.workspace = id;
		settings.layout = $state.snapshot(w.layout);
		tab = w.tab;
		// a tool the strip no longer shows: back to the brush (its key still picks it)
		if (!(w.tools as string[]).includes(tool) && tool !== 'eraser') tool = 'brush';
	}

	// the largest brush size: the slider stops there, and a bigger size comes down to it
	$effect(() => {
		if (size > settings.maxBrushSize) size = settings.maxBrushSize;
	});

	// each brush remembers its own size and opacity (when Settings says so)
	let brushWas = untrack(() => brushId);
	$effect(() => {
		const id = brushId;
		untrack(() => {
			if (id === brushWas) return;
			if (settings.perBrushSize) {
				const kept = readLocal<Record<string, { size: number; opacity: number }>>(
					'pp.app.brushSizes',
					{}
				);
				kept[brushWas] = { size, opacity };
				const mine = kept[id];
				if (mine) [size, opacity] = [mine.size, mine.opacity];
				keepLocal('pp.app.brushSizes', kept);
			}
			brushWas = id;
		});
	});

	// the memory limit for frames: how many recent drawings the engine keeps (the app's
	// store takes the rest of the limit; see memoryPlan)
	$effect(() => {
		cache.limit(settings.frameMemoryMb * 2 ** 20, canvasSize.width, canvasSize.height);
	});

	onMount(() => {
		void loadSettings();
		if (!isTauri()) return;
		// home's Draw together: host, join or rejoin (the dialog shows how it goes)
		const ask = takeTogether();
		if (ask) shareOpen = true;
		if (ask?.what === 'join') joinOnReady = ask.code;
		if (ask?.what === 'rejoin') {
			// (it opens once the project's session is read from its file)
			shareOpen = false;
			rejoinOnOpen = true;
		}
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
					if (unsaved && !(await okToDrop('quit'))) return e.preventDefault();
					await dropRecovery(docId);
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
		{ id: 'xsheet', label: 'X‑sheet', meta: theCut().name },
		{ id: 'history', label: 'History', meta: status?.history.undo.at(-1) }
	]);

	/** tools whose key switches to them (and back to the brush) */
	const TOOL_ACTIONS: Partial<Record<ActionId, string>> = {
		picker: 'picker',
		fill: 'fill',
		'lasso-fill': 'lasso-fill',
		measure: 'measure',
		smudge: 'smudge',
		clone: 'clone',
		pen: 'pen',
		transform: 'transform',
		liquify: 'liquify',
		'select-rect': 'select-rect',
		'select-lasso': 'select-lasso',
		'select-wand': 'select-wand',
		gradient: 'gradient',
		text: 'text',
		lines: 'lines'
	};

	/** what's in use: a tool, or 'eraser' (the brush erasing) */
	const modeNow = () => (tool === 'brush' ? (eraser ? 'eraser' : 'brush') : tool);
	function useMode(m: string) {
		if (m === 'brush' || m === 'eraser') {
			eraser = m === 'eraser';
			tool = 'brush';
		} else tool = m;
	}
	/** where each tool was come to from (its key pressed again goes back there) */
	const cameFrom: Record<string, string> = {};
	let modeIs = 'brush';
	/** going back: the tool gone back to keeps where it was come to from */
	let goingBack = false;
	$effect(() => {
		const m = modeNow();
		untrack(() => {
			if (m === modeIs) return;
			if (!goingBack) cameFrom[m] = modeIs;
			goingBack = false;
			modeIs = m;
		});
	});
	/** a tool's key: that tool; pressed again, back to the one it was come to from (not B) */
	function toggleMode(m: string) {
		// (the brush is home, where the others come back to: B is always the brush)
		if (modeNow() !== m || m === 'brush') return useMode(m);
		const back = cameFrom[m];
		const to = back && back !== m ? back : 'brush';
		if (to === m) return;
		goingBack = true;
		useMode(to);
	}

	function onkey(e: KeyboardEvent) {
		// Settings over the canvas has the keys (it may be choosing a new one)
		if (settingsOpen) return;
		if ((e.target as HTMLElement).closest('input, textarea, select')) return;
		// keys that belong to the tool in use (a stroke picked with Move or the brush first)
		const picked = !!canvas?.strokePicked();
		if (tool === 'transform' && !picked && e.key === 'Enter') return canvas?.transformCommit();
		if (tool === 'transform' && !picked && e.key === 'Escape') return canvas?.transformCancel();
		if (tool === 'pen' && e.key === 'Enter') {
			if (canvas?.penStroke(toolOpts.penTaper)) canvas.penClear();
			return;
		}
		if (tool === 'pen' && e.key === 'Escape') return canvas?.penClear();
		if (tool === 'text' && e.key === 'Escape') return toolOpts.text.cancel();
		if ((tool === 'lines' || picked) && canvas?.linesSelected()) {
			const k = e.key;
			if (k === 'Delete' || k === 'Backspace') {
				e.preventDefault();
				return canvas.deleteLines();
			}
			if (k === 'Escape' || (picked && k === 'Enter')) return canvas.deselectLines();
			const step = e.shiftKey ? 10 : 1;
			const arrows: Record<string, [number, number]> = {
				ArrowLeft: [-step, 0],
				ArrowRight: [step, 0],
				ArrowUp: [0, -step],
				ArrowDown: [0, step]
			};
			if (arrows[k] && !e.ctrlKey && !e.metaKey && !e.altKey) {
				e.preventDefault();
				return canvas.nudgeLines(...arrows[k]);
			}
		}
		if (tool === 'pen' && e.key === 'Backspace') {
			e.preventDefault();
			return canvas?.penBack();
		}
		if (e.key === 'Escape' && toolOpts.adjustOpen) return toolOpts.closeAdjust(false);
		if (e.key === 'Escape' && status?.selection) return canvas?.deselect();
		const combo = comboOf(e);
		const action = combo ? actionFor(settings.keys, combo) : null;
		// flipping: the key held down repeats (it keeps flipping)
		if ((action === 'flip' || action === 'flip-keys') && (e.repeat || flipping)) {
			e.preventDefault();
			return;
		}
		// while playing: Esc or any other shortcut stops first
		if (playing && (e.key === 'Escape' || (action && action !== 'play' && action !== 'add-marker')))
			stopPlay();
		if (!action) return;
		e.preventDefault();
		const toolFor =
			TOOL_ACTIONS[action] ?? (action === 'brush' || action === 'eraser' ? action : undefined);
		// a tool's key: that tool; pressed again, back to the one it was come to from
		if (toolFor) toggleMode(toolFor);
		else if (action === 'smaller') size = Math.max(0.25, +(size / 1.25).toFixed(2));
		else if (action === 'bigger') size = Math.min(settings.maxBrushSize, +(size * 1.25).toFixed(2));
		else if (action === 'undo') undo();
		else if (action === 'redo') redo();
		else if (action === 'prev-frame') timelinePanel?.step(-1);
		else if (action === 'next-frame') timelinePanel?.step(1);
		else if (action === 'new-drawing') timelinePanel?.addDrawing();
		else if (action === 'add-marker') markFrame();
		else if (action === 'flip') void startFlip('roll');
		else if (action === 'flip-keys') void startFlip('keys');
		else if (action === 'mark-kind') markKind();
		else if (action === 'copy-frames') timelinePanel?.copyRange();
		else if (action === 'paste-frames') timelinePanel?.pasteRange();
		else if (action === 'duplicate-frames') void timelinePanel?.duplicateRange();
		else if (action === 'play') void togglePlay();
		else if (action === 'set-in') setRange('from');
		else if (action === 'set-out') setRange('to');
		else if (action === 'onion') onion.on = !onion.on;
		else if (action === 'pin-frame') pinFrame();
		else if (action === 'adjust') toolOpts.openAdjust();
		else if (action === 'copy' || action === 'cut') toolOpts.copyCut(action === 'cut');
		else if (action === 'copy-to-layer') {
			if (canvas?.copySelection(false, false)) toolOpts.pasteCopied();
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
		else if (action === 'reference') showReference();
		else if (action === 'save') void save(false);
		else if (action === 'save-as') void save(true);
		else if (action === 'open') void open();
	}
</script>

<svelte:window
	onpointerdowncapture={(e) => {
		// a press anywhere outside the open menu (the canvas too) closes it
		if (!(fileOpen || exportOpen || importOpen)) return;
		// pressing inside the open one (or its button) is left to it
		if ((e.target as Element | null)?.closest?.('.export')?.querySelector('.menu')) return;
		fileOpen = exportOpen = importOpen = false;
		// only closes it: no dot on the canvas (a click on another button still goes through)
		e.stopPropagation();
	}}
	onkeydowncapture={(e) => {
		if (e.key === 'Escape' && (fileOpen || exportOpen || importOpen)) {
			fileOpen = exportOpen = importOpen = false;
			e.stopPropagation();
		}
	}}
	onkeydown={onkey}
	onkeyup={(e) => {
		// letting go of the flip key: back where flipping started
		const a = flipping ? actionFor(settings.keys, comboOf(e) ?? '') : null;
		if (a === 'flip' || a === 'flip-keys' || e.key === 'Shift') endFlip();
	}}
	onblur={endFlip}
/>

{#snippet reference(where: 'corner' | 'side' | 'float')}
	<ReferencePanel
		shelf={refShelf}
		{where}
		bind:layout={settings.reference}
		gridLines={refGrid()}
		{canvasSize}
		onpick={refPicked}
		onchange={() => docVersion++}
		onpen={refPen}
		mirror={refMirror}
	/>
{/snippet}
{#snippet refPane()}
	{@render reference('side')}
{/snippet}

<div class="app">
	<Studio
		backHref={resolve('/')}
		tabs={TABS}
		bind:tab
		bind:dockOpen
		bind:layout={settings.layout}
		leftHanded={settings.leftHanded}
	>
		{#snippet title()}
			<div class="brand">
				<Logo /><span data-testid="project-name">{docName}</span>
				{#if unsaved}<span class="dot" title="Not saved yet" aria-label="not saved">●</span>{/if}
			</div>
		{/snippet}
		{#snippet actions()}
			{#if fileNote}<span class="note" role="alert">{fileNote}</span>{/if}
			<div class="workspace">
				<label>
					<span class="visually-hidden">Workspace</span>
					<select
						value={workspace?.id}
						title="Workspace: a layout of the studio, with its own panels and tools"
						onchange={(e) => useWorkspace(e.currentTarget.value)}
					>
						{#each settings.workspaces as w (w.id)}
							<option value={w.id}>{w.name}</option>
						{/each}
					</select>
				</label>
				<button
					type="button"
					class="ws-edit"
					aria-label="Customise workspaces"
					title="Customise workspaces"
					onclick={() => (workspacesOpen = true)}
				>
					<svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true">
						<path d="M3 3h8v8H3ZM13 3h8v5h-8ZM13 10h8v11h-8ZM3 13h8v8H3Z" />
					</svg>
				</button>
			</div>
		{/snippet}
		{#snippet menu()}
			<div class="export">
				<button
					type="button"
					class="act"
					aria-expanded={fileOpen}
					disabled={!!busy}
					onclick={() => {
						fileOpen = !fileOpen;
						exportOpen = false;
						importOpen = false;
					}}
				>
					{busy === 'opening' ? 'Opening…' : busy === 'saving' ? 'Saving…' : 'File'}
				</button>
				{#if fileOpen}
					<div class="menu" role="group" aria-label="File">
						<button
							type="button"
							onclick={() => {
								fileOpen = false;
								void open();
							}}
						>
							<b>Open…</b><span>A project ({keyOf('open')})</span>
						</button>
						<button
							type="button"
							onclick={() => {
								fileOpen = false;
								psdInput?.click();
							}}
						>
							<b>Open a Photoshop file…</b><span>Its layers, groups and masks (.psd, .psb)</span>
						</button>
						<button
							type="button"
							onclick={() => {
								fileOpen = false;
								void save(false);
							}}
						>
							<b>Save</b><span>This project ({keyOf('save')})</span>
						</button>
						<button
							type="button"
							onclick={() => {
								fileOpen = false;
								void save(true);
							}}
						>
							<b>Save as…</b><span>A copy under a new name ({keyOf('save-as')})</span>
						</button>
						<p class="menu-head">Image</p>
						<button
							type="button"
							disabled={!status}
							title={sharing ? 'Canvas size can’t change while drawing together' : undefined}
							onclick={() => void openCanvasSize()}
						>
							<b>Canvas size…</b><span>
								{sharing
									? 'Not while drawing together'
									: `${canvasSize.width} × ${canvasSize.height}: every frame changes with it`}
							</span>
						</button>
						<button
							type="button"
							disabled={!status?.selection || !!sharing}
							onclick={cropToSelection}
						>
							<b>Crop to selection</b><span>The canvas becomes the box around it</span>
						</button>
						<p class="menu-head">Export as</p>
						<button
							type="button"
							disabled={!status || status.blank}
							onclick={() => {
								fileOpen = false;
								void exportAs('png');
							}}
						>
							<b>PNG picture…</b><span>One flat image</span>
						</button>
						<button
							type="button"
							disabled={!status || status.blank}
							onclick={() => {
								fileOpen = false;
								void exportAs('psd');
							}}
						>
							<b>Photoshop file (.psd)…</b><span>Keeps layers, groups and masks</span>
						</button>
						<button
							type="button"
							disabled={!status || status.blank}
							onclick={() => {
								fileOpen = false;
								void openAnimExport();
							}}
						>
							<b>Animation…</b><span>MP4, ProRes, WebM, GIF, animated PNG, PNG sequence</span>
						</button>
						<button
							type="button"
							disabled={!status || status.blank}
							onclick={() => {
								fileOpen = false;
								void openFilm();
							}}
						>
							<b>Film of the drawing…</b><span>Timelapse, or this frame drawn live</span>
						</button>
					</div>
				{/if}
			</div>
			<VersionsMenu {path} disabled={!!busy} onopen={openOldVersion} />
			<div class="export">
				<button
					type="button"
					class="act"
					aria-expanded={importOpen}
					disabled={!!busy || !status}
					onclick={() => {
						importOpen = !importOpen;
						exportOpen = false;
					}}
				>
					{busy === 'importing' ? 'Importing…' : 'Import'}
				</button>
				{#if busy === 'importing'}
					<button type="button" class="act" onclick={() => (importStop = true)}>Stop</button>
				{/if}
				{#if importOpen}
					<div class="menu" role="group" aria-label="Import">
						<button
							type="button"
							onclick={() => {
								importOpen = false;
								sequenceInput?.click();
							}}
						>
							<b>Image sequence…</b><span>Numbered pictures, a drawing per frame</span>
						</button>
						<button
							type="button"
							onclick={() => {
								importOpen = false;
								void importVideo();
							}}
						>
							<b>Video frames…</b><span>A frame per frame at {fps} fps, to trace over</span>
						</button>
						<button
							type="button"
							onclick={() => {
								importOpen = false;
								refInput?.click();
							}}
						>
							<b>Reference pictures…</b><span>
								Your own pictures to draw from, beside the canvas
							</span>
						</button>
					</div>
				{/if}
				<input
					bind:this={psdInput}
					type="file"
					accept=".psd,.psb,image/vnd.adobe.photoshop"
					hidden
					aria-label="Photoshop file"
					onchange={(e) => {
						const f = e.currentTarget.files?.[0];
						e.currentTarget.value = '';
						if (f) void openPsd(f);
					}}
				/>
				<input
					bind:this={refInput}
					type="file"
					accept="image/*"
					multiple
					hidden
					aria-label="Reference picture files"
					onchange={(e) => {
						const files = [...(e.currentTarget.files ?? [])];
						e.currentTarget.value = '';
						void importReferences(files);
					}}
				/>
				<input
					bind:this={sequenceInput}
					type="file"
					accept="image/*"
					multiple
					hidden
					aria-label="Image sequence files"
					onchange={(e) => {
						const files = [...(e.currentTarget.files ?? [])];
						e.currentTarget.value = '';
						if (files.length) void importSequence(files);
					}}
				/>
			</div>
			<button
				type="button"
				class="act"
				aria-pressed={settings.reference.shown}
				data-testid="reference-toggle"
				title="Your reference pictures ({keyOf('reference')})"
				onclick={() => showReference()}
			>
				Reference
			</button>
			{#if isTauri()}
				<button
					type="button"
					class="act"
					class:live={!!sharing}
					onclick={() => (shareOpen = true)}
					title="Draw with others, live"
				>
					{sharing
						? `Together (${sharing.members.length})${asksForMe ? ' • asks' : ''}`
						: 'Draw together'}
				</button>
				{#if !sharing && savedShare}
					<!-- out of this project's session (dropped, or left): one click back in -->
					<button
						type="button"
						class="act"
						disabled={shareBusy}
						title="Back into this project’s Draw together session"
						onclick={() => {
							if (!settings.shareName.trim()) shareOpen = true;
							else void shareRejoin().then((ok) => (shareOpen = !ok));
						}}
					>
						Rejoin
					</button>
				{/if}
			{/if}
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
						<button type="button" onclick={() => void openAnimExport()}>
							<b>Animation…</b><span>MP4, ProRes, WebM, GIF, animated PNG, PNG sequence</span>
						</button>
						<button type="button" onclick={() => void openFilm()}>
							<b>Film of the drawing…</b><span>Timelapse, or this frame drawn live</span>
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
			<button
				type="button"
				class="act"
				aria-haspopup="dialog"
				onclick={() => {
					fileOpen = exportOpen = importOpen = false;
					settingsOpen = true;
				}}
			>
				Settings
			</button>
		{/snippet}
		{#snippet rail()}
			<ToolRail
				{brushPx}
				tools={workspace?.tools ?? [...ALL_TOOLS, 'text', 'lines']}
				bind:tool
				bind:eraser
				bind:size
				bind:opacity
				keys={railKeys}
				maxSize={settings.maxBrushSize}
				adjustOpen={toolOpts.adjustOpen}
				onadjust={() => toolOpts.openAdjust()}
			/>
		{/snippet}
		{#snippet options()}
			<span class="brush-name">{manifest ? brushLabel(manifest, brushId) : ''}</span>
			<Swatches bind:color picker />
			{#if smoothKind}
				<label class="slider">
					<span>Smoothing <b>{Math.round(toolOpts.smoothing[smoothKind] * 100)}%</b></span>
					<input
						type="range"
						min="0"
						max="1"
						step="0.05"
						bind:value={toolOpts.smoothing[smoothKind]}
						title="Steadies wobbly lines. The brush, eraser and smudge each keep their own."
					/>
				</label>
			{/if}
			<button
				type="button"
				class="opt"
				aria-pressed={drawingGuide(toolOpts.guides)}
				aria-expanded={toolOpts.guidesOpen}
				title="Guides: grid, perspective, ruler and symmetry"
				onclick={() => {
					toolOpts.guidesOpen = !toolOpts.guidesOpen;
					if (toolOpts.guidesOpen) toolOpts.compositionOpen = false;
				}}
			>
				Guides
			</button>
			<button
				type="button"
				class="opt"
				aria-pressed={compositionOn(toolOpts.guides)}
				aria-expanded={toolOpts.compositionOpen}
				title="Composition: thirds, golden ratio, spiral and more"
				onclick={() => {
					toolOpts.compositionOpen = !toolOpts.compositionOpen;
					if (toolOpts.compositionOpen) toolOpts.guidesOpen = false;
				}}
			>
				Composition
			</button>
			<button
				type="button"
				class="opt"
				aria-pressed={toolOpts.quickShape}
				title="Snap shapes: hold at the end of a stroke to snap it to a line, circle or rectangle"
				onclick={() => {
					toolOpts.quickShape = !toolOpts.quickShape;
					keepLocal('pp.app.quickShape', toolOpts.quickShape);
				}}
			>
				Snap shapes
			</button>
			<button
				type="button"
				class="opt"
				aria-pressed={toolOpts.closeFill}
				title="Close and fill: a stroke that ends where it began fills inside with its colour"
				onclick={() => {
					toolOpts.closeFill = !toolOpts.closeFill;
					keepLocal('pp.app.closeFill', toolOpts.closeFill);
				}}
			>
				Close &amp; fill
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
				<ToolOptionsBar
					opts={toolOpts}
					bind:tool
					{status}
					{canvas}
					{eraser}
					askToDelete={settings.askToDelete}
				/>
			{/if}
		{/snippet}
		{#snippet stage()}
			<RefSplit
				bind:split={settings.reference.split}
				pane={settings.reference.shown && settings.reference.place === 'side' ? refPane : null}
				leftHanded={settings.leftHanded}
			>
				{#key canvasKey}
					<PaintCanvas
						onbrushdiameter={(px) => (brushPx = px)}
						bind:this={canvas}
						cursors={remoteCursors}
						oncursor={onCursor}
						onlive={sharing ? onLive : undefined}
						onfill={(f) => void carryFill(f)}
						oncolourlayer={(id) => {
							// (with the fill: one undo step)
							setStage(timeline, id, 'flats');
							tlJoin = true;
							tlLabel = 'Fill';
							docVersion++;
						}}
						fill
						width={canvasSize.width}
						height={canvasSize.height}
						oncanvassize={(s) => (canvasSize = s)}
						{brushId}
						{eraser}
						{size}
						{opacity}
						{color}
						tool={tool as never}
						smoothing={toolOpts.smoothingFor(tool, eraser)}
						penPressure={settings.penPressure}
						fingers={settings.fingers}
						tapGestures={settings.tapGestures}
						view={viewPrefs}
						undoLimit={settings.undoSteps}
						editableStrokes={settings.editableStrokes}
						holdToPick={settings.holdToPick}
						brushTexture={texture}
						allowImport
						onpick={(c) => (color = c)}
						onsize={(v) => (size = v)}
						fillTolerance={toolOpts.fillTolerance}
						fillAllLayers={toolOpts.fillAllLayers}
						fillReference={toolOpts.fillReference}
						fillAlso={fillTones}
						distort={toolOpts.distort}
						warp={toolOpts.warp}
						quickShape={toolOpts.quickShape}
						closeFill={toolOpts.closeFill}
						inkSnap={toolOpts.inkSnap}
						selectMode={toolOpts.selectMode}
						selectFill={toolOpts.selectFill}
						selectShape={toolOpts.selectShape}
						wandTolerance={toolOpts.wandTolerance}
						wandContiguous={toolOpts.wandContiguous}
						wandAllLayers={toolOpts.wandAllLayers}
						lassoErase={toolOpts.lassoMode === 'erase'}
						lassoSmoothing={toolOpts.lassoSmoothing}
						lassoHard={toolOpts.lassoHard}
						cloneAligned={toolOpts.cloneAligned}
						cloneAllLayers={toolOpts.cloneAllLayers}
						cloneHeal={toolOpts.cloneHeal}
						liquifySize={toolOpts.liquifySize}
						liquifyStrength={toolOpts.liquifyStrength}
						smudgeStrength={toolOpts.smudgeStrength}
						gradientKind={toolOpts.gradientKind}
						gradientEnd={toolOpts.gradientEndValue}
						guides={toolOpts.guides}
						onguides={toolOpts.onguides}
						onpenpath={toolOpts.onpenpath}
						onimport={toolOpts.onimport}
						onshape={(kind) =>
							note(`Snapped to ${/^[aeiou]/.test(kind) ? 'an' : 'a'} ${kind}`, 1800)}
						ontransformend={(r) => {
							tool = 'brush';
							if (r === 'empty') note('Nothing to move on this layer (or it’s locked).');
						}}
						onblocked={(why) => {
							const lid = together && status ? together.lidOf(status.active) : null;
							const l = lid ? sharedLayers.find((x) => x.lid === lid) : null;
							note(
								l && !l.canDraw
									? `This is ${l.owner}’s layer. Ask to draw on it in Draw together, or pick your own.`
									: why === 'hidden'
										? 'This layer is hidden. Show it or pick another layer to draw.'
										: why === 'busy'
											? 'Finish the move or adjustment first (Done or Cancel), then draw.'
											: 'This layer is locked. Unlock it or pick another layer to draw.'
							);
						}}
						ontext={toolOpts.text.ontext}
						textFrame={toolOpts.text.frame}
						onrasterize={toolOpts.text.onrasterize}
						lineEraser={toolOpts.lineEraser}
						onquickmenu={(x, y) => (quickMenu = { x, y })}
						onchange={canvasChanged}
						locked={switching}
						onionImage={playing ? null : onionImage}
						underImage={playing ? null : underImage}
						camera={cameraOverlay}
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
			</RefSplit>
		{/snippet}
		{#snippet dockTop()}
			{#if settings.reference.shown && settings.reference.place === 'corner'}
				{@render reference('corner')}
			{/if}
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
					bind:playScope
					bind:shift
					bind:playRange
					{onion}
					bind:cameraView
					{canvasSize}
					bind:pinned
					onpin={pinFrame}
					onflip={(kind, down) => (down ? void startFlip(kind) : endFlip())}
					onmark={markKind}
					onplay={togglePlay}
					onrange={setRange}
					{stageView}
					onview={setView}
					viewHidden={inView.hidden}
					{lightTable}
					onlighttable={(t) => (lightTable = t)}
					{lightTableNote}
					layers={status.layers}
					active={status.active}
					newDrawing={() => canvas?.newDrawing() ?? 0}
					drawings={frameDrawings}
					audio={sounds}
					{soundTick}
					onsound={(files) => void addSounds(files)}
					onlipsync={(id) => void openLipSync(id)}
					onmarker={markFrame}
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
					controls={panelControls}
					lineLayers
					stages={STAGE_CHOICES}
					stageOf={(id) => timeline.stages?.[id] ?? null}
					onstage={(id, st) => putStage(id, st)}
					onsortstages={sortStages}
					{traceStagesOf}
					ontrace={(id, st) => void traceLayer(id, st)}
					aspect={canvasSize.width / canvasSize.height}
					onadjustment={(id) => toolOpts.adjustmentLayer(id)}
					ontext={(id) => {
						tool = 'text';
						toolOpts.text.open(id);
					}}
				/>
			{:else if t === 'brushes'}
				{#if manifest}
					<BrushPicker {manifest} bind:selected={brushId} askToDelete={settings.askToDelete} />
					<BrushTexturePanel {brushId} />
				{:else}
					<p>Loading brushes…</p>
				{/if}
			{:else if t === 'xsheet' && status}
				<XSheet
					{timeline}
					{scene}
					cut={cutIndex}
					bind:frame
					{fps}
					layers={status.layers}
					active={status.active}
					newDrawing={() => canvas?.newDrawing() ?? 0}
					onlayer={(id) => canvas?.selectLayer(id)}
					onedit={(changed) => {
						if (playing) stopPlay();
						timelineEdited(changed);
					}}
				/>
			{:else if t === 'history' && status}
				<HistoryPanel history={status.history} ongo={historyGo} />
			{:else if t === 'colour'}
				<PalettePanel
					palettes={timeline.palettes ?? []}
					{color}
					onchange={putPalettes}
					onuse={(hex) => {
						color = hex;
						eraser = false;
					}}
					fromReference={referenceColours}
				/>
				<ColorPanel bind:color onclose={() => (tab = 'layers')} />
			{/if}
		{/snippet}
		{#snippet after()}
			{#if settings.reference.shown && settings.reference.place === 'float'}
				{@render reference('float')}
			{/if}
			{#if quickMenu}
				<QuickMenu
					x={quickMenu.x}
					y={quickMenu.y}
					actions={quickActions()}
					onclose={() => (quickMenu = null)}
				/>
			{/if}
			{#if animExport}
				<ExportDialog
					bind:settings={exportSettings}
					video={animExport.video}
					ffmpeg={animExport.ffmpeg}
					{fps}
					hasLoop={!!playRange}
					camera={cameraOf(theCut(), canvasSize)}
					progress={animExport.progress}
					staged={!!timeline.stages}
					viewName={viewLabel(stageView)}
					onstart={() => void runAnimExport()}
					oncancel={() => (exportStop = true)}
					onclose={() => (animExport = null)}
				/>
			{/if}
			{#if saveAsk}
				<SaveChangesDialog
					title={saveAsk.title}
					name={docName}
					onanswer={(c) => saveAsk?.answer(c)}
				/>
			{/if}
			{#if settingsOpen}
				<SettingsPanel onclose={() => (settingsOpen = false)} />
			{/if}
			{#if workspacesOpen}
				<WorkspaceDialog bind:tab onuse={useWorkspace} onclose={() => (workspacesOpen = false)} />
			{/if}
			{#if canvasSizeOpen}
				<CanvasSizeDialog
					size={canvasSize}
					tiles={projectTiles}
					limitBytes={settings.frameMemoryMb * 2 ** 20}
					picture={sizePicture}
					onclose={closeCanvasSize}
					onapply={(plan) => {
						closeCanvasSize();
						void resizeAll(plan);
					}}
				/>
			{/if}
			{#if shareOpen}
				<ShareDialog
					info={sharing}
					canRejoin={!!savedShare}
					onrejoin={() => void shareRejoin()}
					layers={sharedLayers}
					synced={togetherSynced}
					onask={(lid) => together?.ask(lid)}
					ongrant={(lid, to, handover) => together?.grant(lid, to, handover)}
					ondeny={(lid, to) => together?.deny(lid, to)}
					onrevoke={(lid, to) => together?.revoke(lid, to)}
					bind:name={settings.shareName}
					busy={shareBusy}
					error={shareError}
					onhost={() => void shareStart()}
					onjoin={(code) => void shareStart(code)}
					onleave={() => void shareLeave()}
					onclose={() => {
						shareOpen = false;
						shareError = '';
					}}
				/>
			{/if}
			{#if sharing}
				<TogetherChat
					lines={chatLines}
					unread={chatUnread}
					bind:open={chatOpen}
					onsend={chatSend}
				/>
			{/if}
			{#if film}
				<FilmDialog
					bind:settings={filmSettings}
					video={film.video}
					pictures={film.pictures}
					strokes={film.strokes.length}
					drawnSeconds={film.drawnSeconds}
					size={filmSize()}
					progress={film.progress}
					onstart={() => void runFilm()}
					oncancel={() => (filmStop = true)}
					onclose={() => (film = null)}
				/>
			{/if}
			{#if staging}
				<div class="stage" aria-hidden="true">
					<PaintCanvas
						bind:this={filmStage}
						width={canvasSize.width}
						height={canvasSize.height}
						brushId={DEFAULT_BRUSH}
						viewControls={false}
					/>
				</div>
			{/if}
			{#if lipSync}
				{@const ls = lipSync}
				{@const r = lipRange(ls.clip, ls.toEnd)}
				{@const chart = timeline.mouths?.find((m) => m.layer === ls.layer) ?? {
					layer: ls.layer,
					shapes: {}
				}}
				<LipSyncDialog
					name={r?.clip.name ?? ''}
					range={{ from: r?.from ?? 0, frames: r?.frames ?? 0 }}
					longer={!!r?.longer}
					bind:toEnd={ls.toEnd}
					layers={mouthLayers()}
					bind:layer={ls.layer}
					{chart}
					choices={drawingChoices(ls.layer)}
					thumb={(d) => drawingThumb(ls.layer, d)}
					bind:settings={lipSettings}
					bind:dialog={ls.dialog}
					rhubarb={ls.rhubarb}
					progress={ls.progress}
					error={ls.error}
					onchart={(k: MouthShape, d: number) => {
						const c = chartOf(ls.layer);
						if (d) c.shapes[k] = d;
						else delete c.shapes[k];
						docVersion++;
					}}
					onauto={() => {
						autoChart(
							chartOf(ls.layer),
							drawingChoices(ls.layer).map((c) => c.id)
						);
						docVersion++;
					}}
					onstart={() => void runLipSync()}
					oncancel={() => void cancelLipSync()}
					onclose={() => (lipSync = null)}
				/>
			{/if}
			{#if timelapseOpen && canvas}
				<TimelapsePlayer
					film={canvas.timelapse()}
					onclose={() => (timelapseOpen = false)}
					onsave={(fps, frames) => {
						// saved through the film export (the app's own encoder: the window's
						// web view can't record video), as long as the speed chosen here
						const want = frames / fps;
						filmSettings.kind = 'timelapse';
						filmSettings.seconds = [10, 15, 30, 60].reduce((a, b) =>
							Math.abs(b - want) < Math.abs(a - want) ? b : a
						);
						timelapseOpen = false;
						void openFilm();
					}}
				/>
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
	.act.live {
		background: #2a9d8f;
		color: #fff;
	}
	.export {
		position: relative;
	}
	/* the canvas a "drawn live" film is drawn on: out of sight */
	.stage {
		position: fixed;
		top: 0;
		left: -100000px;
		width: 640px;
		height: 480px;
		overflow: hidden;
		pointer-events: none;
	}
	.menu {
		position: absolute;
		z-index: 30;
		top: calc(100% + 8px);
		left: 0;
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
	.menu button:disabled {
		opacity: 0.45;
		cursor: default;
	}
	.menu-head {
		margin: 6px 12px 0;
		color: var(--pp-muted);
		font: 600 12px var(--pp-font-body);
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}
	.menu span {
		color: var(--pp-muted);
		font-size: 13px;
	}
	.dot {
		color: var(--pp-accent);
		font-size: 12px;
	}
	.workspace {
		display: flex;
		align-items: center;
		gap: 4px;
	}
	.workspace select {
		height: 32px;
		max-width: 160px;
		padding: 0 8px;
		border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
		border-radius: 10px;
		background: transparent;
		color: inherit;
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.workspace select option {
		color: #16161a;
	}
	.ws-edit {
		display: grid;
		place-items: center;
		width: 32px;
		height: 32px;
		border: 0;
		border-radius: 10px;
		background: transparent;
		color: inherit;
		cursor: pointer;
	}
	.ws-edit:hover {
		background: color-mix(in srgb, currentColor 12%, transparent);
	}
	.ws-edit svg {
		fill: none;
		stroke: currentColor;
		stroke-width: 2;
		stroke-linejoin: round;
	}
	.workspace .visually-hidden {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
		white-space: nowrap;
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
