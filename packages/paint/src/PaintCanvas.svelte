<script lang="ts" module>
	export type BlendMode =
		'normal' | 'multiply' | 'screen' | 'overlay' | 'darken' | 'lighten' | 'add';

	export interface LayerInfo {
		id: number;
		name: string;
		visible: boolean;
		/** 0–1 */
		opacity: number;
		blend: BlendMode;
		locked: boolean;
		alphaLock: boolean;
		/** clipping mask: only shows where the layer below has paint */
		clip: boolean;
		/** has a layer mask */
		mask: boolean;
		/** the tools paint on the mask */
		editingMask: boolean;
		/** the layer's group (0 = none) */
		group: number;
		/** holds an imported picture (or paint merged with one) */
		imported: boolean;
		empty: boolean;
		/** the drawing (animation cel) it shows; 0 = none yet (drawing on it starts one) */
		drawing: number;
		/** an adjustment layer's settings (JSON, see tone.ts); null for paint layers */
		adjust: string | null;
	}
	export type LayerSettings = Omit<
		LayerInfo,
		'id' | 'empty' | 'mask' | 'editingMask' | 'group' | 'imported' | 'adjust' | 'drawing'
	>;

	/** What happened to a picture brought in with `importImage`. */
	export type ImportResult = 'ok' | 'too-many' | 'unreadable' | 'busy' | 'too-big';

	/** Pictures bigger than this (bytes) aren't read. */
	export const MAX_IMPORT_BYTES = 30 * 1024 * 1024;

	/** A layer group: its eye and opacity apply to each of its layers. */
	export interface GroupInfo {
		id: number;
		name: string;
		visible: boolean;
		opacity: number;
	}
	export type GroupSettings = Omit<GroupInfo, 'id'>;

	export interface CanvasStatus {
		canUndo: boolean;
		canRedo: boolean;
		blank: boolean;
		strokes: number;
		/** bottom → top */
		layers: LayerInfo[];
		groups: GroupInfo[];
		active: number;
		/** bumps on every change (for refreshing layer thumbnails) */
		version: number;
		/** something is selected */
		selection: boolean;
		/** the history panel: step names, oldest → newest, and the redo steps (next first) */
		history: { undo: string[]; redo: string[] };
	}

	/** A local stroke as it happens, for shared canvases. Points are [x, y, pressure, dtMs, …]. */
	export type LiveEvent =
		| {
				type: 'begin';
				k: number;
				style: { brushId: string; color: string; size: number; opacity: number; eraser: boolean };
		  }
		| { type: 'points'; k: number; pts: number[] }
		| { type: 'end'; k: number }
		/** the stroke was abandoned (a pinch): others should drop it */
		| { type: 'cancel'; k: number };

	/** Status before the engine has loaded. */
	export const BLANK_STATUS: CanvasStatus = {
		canUndo: false,
		canRedo: false,
		blank: true,
		strokes: 0,
		layers: [],
		groups: [],
		active: 0,
		version: 0,
		selection: false,
		history: { undo: [], redo: [] }
	};

	export interface DrawingExport {
		width: number;
		height: number;
		/** PNG data URL of the drawing */
		png: string;
		/** grayscale bytes, width*height, for the grader */
		gray: Uint8Array;
		strokes: Stroke[];
		/** the exported layers, bottom → top (for replays) */
		layers: LayerInfo[];
	}
</script>

<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import { loadBrush, loadEngine, PaintCanvas, type PaintBrush } from './brushes';
	import { StrokeLog, type Stroke } from './strokes';
	import { recognise, resample } from './quickshape';
	import type { PsdDoc, PsdGroup, PsdLayer } from './psd';
	import { applyCurve } from './pressure';
	import { customBrush, isCustom, onCustomBrushes } from './custom-brushes';
	/** other players don't have "My brushes": they see the brush it's based on */
	const sharedBrushId = (id: string) => (isCustom(id) ? (customBrush(id)?.base ?? id) : id);
	import {
		closestDirection,
		gridLines,
		perspectiveRays,
		project,
		rulerLine,
		snapDirections,
		symmetryAxes,
		SYMMETRY_MODES,
		type GuideSettings,
		type Pt
	} from './guides';
	import { toGray } from './pixels';
	import { boxToQuad, usableQuad, type Quad } from './homography';
	import { meshLines, meshNodes, WARP_N } from './warp';
	import { editable, moveHandle, shapeHandles, shapePath, type EditShape } from './shape-edit';
	import { smudgeSource } from './smudge';
	import { textureById, type BrushTexture } from './brush-textures';
	import { Timelapse } from './timelapse';
	import { mapTable, parseSpec, specName, toneTables, type AdjustSpec } from './tone';
	import {
		corner,
		dragPart,
		EMPTY_PATH,
		hitPath,
		pathD,
		samplePath,
		type PenPath
	} from './pen-path';

	let {
		width = 800,
		height = 600,
		brushId,
		eraser = false,
		size = 1,
		opacity = 1,
		color = '#16161a',
		onchange,
		onblocked,
		overlaySrc = null,
		overlayOpacity = 0.3,
		overlayTransform = { x: 0, y: 0, scale: 1, rotate: 0 },
		adjustOverlay = false,
		onoverlaymove,
		locked = false,
		onlive,
		tool = 'brush',
		smoothing = 0,
		onpick,
		ongesture,
		fingers = 'auto',
		tapGestures = true,
		fillTolerance = 0.12,
		fillAllLayers = true,
		fillReference = 0,
		grayView = $bindable(false),
		distort = false,
		warp = false,
		onsize,
		notan = $bindable(0),
		squint = $bindable(false),
		measureUnit = 0,
		angleGuide = null,
		quickShape = true,
		onshape,
		selectMode = 'replace',
		wandTolerance = 0.15,
		ontransformend,
		penPressure = true,
		guides = null,
		onguides,
		liquifySize = 60,
		liquifyStrength = 0.6,
		allowImport = false,
		onimport,
		gradientKind = 'linear',
		gradientEnd = 'transparent',
		maxHeight = '78vh',
		viewControls = true,
		fill = false,
		smudgeStrength = 0.6,
		brushTexture = null,
		wandContiguous = true,
		oncanvassize,
		onquickmenu,
		onpenpath
	}: {
		width?: number;
		height?: number;
		brushId: string;
		eraser?: boolean;
		/** multiplier on the brush's designed size */
		size?: number;
		opacity?: number;
		color?: string;
		onchange?: (s: CanvasStatus) => void;
		/** a stroke was refused because the active layer is locked or hidden */
		onblocked?: (why: 'locked' | 'hidden') => void;
		/** a picture shown over the canvas (fitted like the grader fits references); never part of the drawing */
		overlaySrc?: string | null;
		overlayOpacity?: number;
		/** where the overlay sits: offset (fraction of the canvas), scale and rotation (degrees) */
		overlayTransform?: { x: number; y: number; scale: number; rotate: number };
		/** what a press on the canvas does */
		tool?:
			| 'brush'
			| 'picker'
			| 'fill'
			| 'select-rect'
			| 'select-lasso'
			| 'select-wand'
			| 'transform'
			| 'liquify'
			| 'guides'
			| 'gradient'
			| 'lasso-fill'
			| 'measure'
			| 'smudge'
			| 'pen';
		/** pictures can be dropped onto the canvas (as a new layer) */
		allowImport?: boolean;
		/** a dropped picture was imported (or why not) */
		onimport?: (r: ImportResult) => void;
		/** gradient: straight or round */
		gradientKind?: 'linear' | 'radial';
		/** gradient: fades from the brush colour to this colour, or to transparent */
		gradientEnd?: string | 'transparent';
		/** grid, perspective and ruler guides (null = none) */
		guides?: GuideSettings | null;
		/** guides moved (vanishing points or the ruler dragged with the 'guides' tool) */
		onguides?: (g: GuideSettings) => void;
		/** liquify: brush radius in canvas px, and how hard it pushes (0–1) */
		liquifySize?: number;
		liquifyStrength?: number;
		/** use pen pressure and tilt (off = every stroke at a steady medium pressure, like a mouse) */
		penPressure?: boolean;
		/** a transform finished (kept or cancelled), or had nothing to move */
		ontransformend?: (result: 'done' | 'cancelled' | 'empty') => void;
		/** how a new selection combines with the current one (Shift = add, Alt = subtract) */
		selectMode?: 'replace' | 'add' | 'subtract' | 'intersect';
		/** magic wand: colour tolerance (0–1) */
		wandTolerance?: number;
		/** fill: how different a colour may be and still be filled (0–1) */
		fillTolerance?: number;
		/** fill: sample what you see (true) or the current layer only */
		fillAllLayers?: boolean;
		/** fill: find the shape on this layer instead (a line-art "reference" layer; 0 = off) */
		fillReference?: number;
		/** show the canvas in grey (values only); the drawing itself keeps its colours */
		grayView?: boolean;
		/** transform: dragging a corner moves just that corner (distort / perspective) */
		distort?: boolean;
		/** transform: bend with a 4×4 grid of points (warp) */
		warp?: boolean;
		/** the brush size was changed on the canvas (Ctrl+Alt+drag) */
		onsize?: (size: number) => void;
		/** notan view: the canvas in 2 or 3 flat tones (0 = off) */
		notan?: 0 | 2 | 3;
		/** squint view: blurred, to compare big shapes */
		squint?: boolean;
		/** the measuring tool's unit, in canvas px (from the reference; 0 = plain pixels) */
		measureUnit?: number;
		/** an angle taken from the reference: a guide line through `p` at `deg` (0 = flat, counter-clockwise) */
		angleGuide?: { p: [number, number]; deg: number } | null;
		/** hold at the end of a stroke to snap it to a clean shape */
		quickShape?: boolean;
		/** a stroke snapped to a shape */
		onshape?: (kind: string) => void;
		/** 0–1: steadies strokes (lazy brush, like Procreate's StreamLine) */
		smoothing?: number;
		/** a colour was picked (eyedropper tool, Alt+click, or a finger held still) */
		onpick?: (hex: string) => void;
		/** two-finger tap = undo, three-finger tap = redo; defaults to this canvas's own */
		ongesture?: (g: 'undo' | 'redo') => void;
		/** what one finger does: draw, move the view, or 'auto' (draw until a pen is used, then move) */
		fingers?: 'auto' | 'draw' | 'move';
		/** two-finger tap = undo, three-finger tap = redo */
		tapGestures?: boolean;
		/** local strokes as they happen (shared canvases) */
		onlive?: (e: LiveEvent) => void;
		/** no more strokes (e.g. time is up); a stroke in progress is finished */
		locked?: boolean;
		/** dragging moves the overlay instead of drawing */
		adjustOverlay?: boolean;
		/** overlay dragged by (dx, dy), as fractions of the canvas width / height */
		onoverlaymove?: (dx: number, dy: number) => void;
		/** tallest the canvas area may get */
		maxHeight?: string;
		/** show the zoom / rotate / mirror bar */
		viewControls?: boolean;
		/** fill the parent's height (the sheet is fitted and centred) instead of keeping the canvas's shape */
		fill?: boolean;
		/** the Smudge tool: how far it drags the paint (0–1) */
		smudgeStrength?: number;
		/** grain, picture tip and dual brush for the current brush (null = none) */
		brushTexture?: BrushTexture | null;
		/** magic wand: only the joined-up area (false: every similar colour, like Color Range) */
		wandContiguous?: boolean;
		/** the canvas changed size (resized, or that undone): set `width` / `height` to this */
		oncanvassize?: (s: { width: number; height: number }) => void;
		/** right-click / long-press on the canvas (screen coordinates) */
		onquickmenu?: (x: number, y: number) => void;
		/** the pen tool's path changed: how many points, and whether it's closed */
		onpenpath?: (s: { points: number; closed: boolean }) => void;
	} = $props();

	let el: HTMLCanvasElement;
	let ctx: CanvasRenderingContext2D | null = null;
	let memory: WebAssembly.Memory | undefined;
	let paint: PaintCanvas | undefined;
	let brush: PaintBrush | undefined;
	let ready = $state(false);
	let error = $state('');

	const log = new StrokeLog();
	const t0 = performance.now();
	let current: Stroke | null = null;
	let lastT = 0;
	let raf = 0;

	onMount(() => {
		let dead = false;
		loadEngine()
			.then((mem) => {
				if (dead) return;
				memory = mem;
				paint = new PaintCanvas(width, height);
				ctx = el.getContext('2d', { willReadFrequently: true });
				if (ctx) {
					ctx.fillStyle = '#fff';
					ctx.fillRect(0, 0, width, height);
				}
				ready = true;
				notify();
			})
			.catch((e) => (error = `Could not start the paint engine: ${e}`));
		return () => {
			dead = true;
			cancelAnimationFrame(raf);
			paint?.free();
			brush?.free();
		};
	});

	// "My brushes" edited elsewhere (the brush editor): reload the current one
	let brushVersion = $state(0);
	$effect(() => onCustomBrushes(() => brushVersion++));

	// Load the selected brush.
	$effect(() => {
		const id = brushId;
		void (isCustom(id) && brushVersion);
		const smudging = tool === 'smudge';
		const strength = smudgeStrength;
		if (!ready) return;
		let cancelled = false;
		loadBrush(id, smudging ? (src) => smudgeSource(src, strength) : undefined)
			.then((b) => {
				if (cancelled) return b.free();
				brush?.free();
				brush = b;
				apply();
			})
			.catch((e) => (error = `Could not load brush "${id}": ${e}`));
		return () => {
			cancelled = true;
		};
	});

	// Re-apply size / opacity / colour / eraser whenever they change.
	$effect(() => {
		apply();
	});

	function apply() {
		const [s, o, c, er] = [size, opacity, color, eraser]; // read props so the effect tracks them
		if (!brush) return;
		brush.setSizeScale(s);
		brush.setOpacity(o);
		const n = parseInt(c.slice(1), 16);
		brush.setColor((n >> 16) & 255, (n >> 8) & 255, n & 255);
		brush.setEraser(er && tool !== 'smudge');
		brushR = brush.radius();
		editShape = null;
	}

	/** Ctrl+Alt+drag resizing the brush */
	let sizeDrag: { id: number; x: number; size: number; at: [number, number] } | null = null;
	/** the last QuickShape, still editable (its points can be dragged) */
	let editShape = $state<EditShape | null>(null);
	let shapeDrag: { id: number; index: number } | null = null;
	let hoverHandle = $state(false);
	/** the measuring tool's line (canvas px) and whether it's being dragged */
	let measureSeg = $state<[[number, number], [number, number]] | null>(null);
	let measuring: number | null = null;
	/** ids for this canvas's notan filters */
	const fid = `pp-notan-${Math.random().toString(36).slice(2, 9)}`;
	const viewFilter = $derived(
		[
			grayView && !notan ? 'grayscale(1)' : '',
			notan ? `url(#${fid}-${notan})` : '',
			squint ? 'blur(5px)' : ''
		]
			.filter(Boolean)
			.join(' ') || null
	);

	/** Redraw the editable shape in its new form: one step, replacing the last. */
	function redrawShape(sh: EditShape) {
		if (!paint || !brush) return;
		if (paint.undo()) log.undo();
		if (!paint.beginStroke()) return;
		const stroke: Stroke = {
			brushId,
			eraser,
			layer: paint.activeLayer,
			color,
			size,
			opacity,
			points: []
		};
		const t = Math.round(performance.now() - t0);
		for (const [x, y] of resample(shapePath(sh), 3)) {
			paint.strokeTo(brush, x, y, sh.pressure, 0, 0, 0.008);
			stroke.points.push({
				x: Math.round(x * 10) / 10,
				y: Math.round(y * 10) / 10,
				pressure: sh.pressure,
				t
			});
		}
		paint.endStroke(brush);
		log.add(stroke);
		log.labelLast('Shape');
		schedule();
	}

	/** the brush's radius in canvas px, for the outline cursor */
	let brushR = $state(0);
	/** where the pointer hovers (canvas px), for the brush outline cursor */
	let cursorAt = $state<[number, number] | null>(null);

	let version = 0;

	function layerList(): LayerInfo[] {
		return paint ? (JSON.parse(paint.layersJson()) as LayerInfo[]) : [];
	}

	function groupList(): GroupInfo[] {
		return paint ? (JSON.parse(paint.groupsJson()) as GroupInfo[]) : [];
	}

	/** layers as they show on the canvas: their group's eye and opacity folded in */
	function shownLayers(): LayerInfo[] {
		const groups = new Map(groupList().map((g) => [g.id, g]));
		return layerList().map((l) => {
			const g = groups.get(l.group);
			return g ? { ...l, visible: l.visible && g.visible, opacity: l.opacity * g.opacity } : l;
		});
	}

	function visibleLayerIds(): Set<number> {
		return new Set(
			shownLayers()
				.filter((l) => l.visible && l.opacity > 0)
				.map((l) => l.id)
		);
	}

	function notify() {
		if (!paint) return;
		if (paint.width !== width || paint.height !== height)
			oncanvassize?.({ width: paint.width, height: paint.height });
		const layers = layerList();
		onchange?.({
			canUndo: paint.canUndo(),
			canRedo: paint.canRedo(),
			blank: paint.isBlank(),
			strokes: log.on(visibleLayerIds()).length,
			layers,
			groups: groupList(),
			active: paint.activeLayer,
			version: ++version,
			selection: paint.hasSelection(),
			history: log.history()
		});
	}

	/** the stroke in progress paints a layer mask (not shared, not in the stroke log) */
	let maskStroke = false;
	const sendLive = () => !!onlive && !maskStroke;

	/** run an engine change, keep the history in step, then redraw */
	function changed(recorded: boolean, logIt: () => void = () => log.layerChange(), label?: string) {
		if (recorded) {
			logIt();
			if (label) log.labelLast(label);
			editShape = null;
		}
		schedule();
		notify();
		return recorded;
	}

	function schedule() {
		if (!raf) raf = requestAnimationFrame(render);
	}

	// ---- shared canvases ----

	let liveK = 0;
	let livePts: number[] = [];
	function flushLive() {
		if (livePts.length && sendLive()) onlive?.({ type: 'points', k: liveK, pts: livePts });
		livePts = [];
	}

	/** other people's layers and brushes */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- engine bookkeeping, never rendered
	const remoteLayers = new Map<string, number>();
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- engine bookkeeping, never rendered
	const remoteBrushes = new Map<string, Promise<PaintBrush>>();
	type Style = Extract<LiveEvent, { type: 'begin' }>['style'];
	/** strokes in progress from others: key → engine stroke key, style, queued work */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- engine bookkeeping, never rendered
	const remoteLive = new Map<
		string,
		{ key: number; style: Style; brush: PaintBrush | null; queue: number[][]; ended: boolean }
	>();
	let remoteKeys = 0;

	function remoteBrush(id: string) {
		let b = remoteBrushes.get(id);
		if (!b) {
			b = loadBrush(id).catch(() => loadBrush('deevad-2b-pencil'));
			remoteBrushes.set(id, b);
		}
		return b;
	}

	function styled(b: PaintBrush, st: Style) {
		b.setSizeScale(st.size);
		b.setOpacity(st.opacity);
		const n = parseInt(st.color.slice(1), 16) || 0;
		b.setColor((n >> 16) & 255, (n >> 8) & 255, n & 255);
		b.setEraser(st.eraser);
		return b;
	}

	/** A layer for another person (no undo step). Returns its id. */
	export function remoteJoin(playerId: string, name: string): number {
		if (!paint) return 0;
		let id = remoteLayers.get(playerId);
		if (!id) {
			id = paint.addLayerSilent(name);
			remoteLayers.set(playerId, id);
			notify();
		}
		return id;
	}

	function runRemote(r: NonNullable<ReturnType<typeof remoteLive.get>>) {
		if (!paint || !r.brush) return;
		const b = styled(r.brush, r.style);
		for (const pts of r.queue)
			for (let i = 0; i + 3 < pts.length; i += 4)
				paint.remoteTo(r.key, b, pts[i], pts[i + 1], pts[i + 2], pts[i + 3] / 1000);
		r.queue = [];
		if (r.ended) paint.remoteEnd(r.key, b);
		schedule();
	}

	export function remoteBegin(playerId: string, k: number, style: Style, name = '') {
		if (!paint) return;
		const layer = remoteJoin(playerId, name);
		const key = ++remoteKeys;
		paint.remoteBegin(key, layer);
		const r = {
			key,
			style,
			brush: null as PaintBrush | null,
			queue: [] as number[][],
			ended: false
		};
		remoteLive.set(`${playerId}:${k}`, r);
		remoteBrush(style.brushId).then((b) => {
			r.brush = b;
			runRemote(r);
		});
	}

	export function remotePoints(playerId: string, k: number, pts: number[]) {
		const r = remoteLive.get(`${playerId}:${k}`);
		if (!r) return;
		r.queue.push(pts);
		runRemote(r);
	}

	export function remoteEnd(playerId: string, k: number) {
		const id = `${playerId}:${k}`;
		const r = remoteLive.get(id);
		if (!r) return;
		r.ended = true;
		remoteLive.delete(id);
		runRemote(r);
	}

	/** Redraw someone's whole layer from their strokes (after their undo / clear, or on joining). */
	export async function remoteRedraw(
		playerId: string,
		strokes: { style: Style; pts: number[] }[],
		name = ''
	) {
		if (!paint) return;
		const layer = remoteJoin(playerId, name);
		const brushes = await Promise.all(strokes.map((s) => remoteBrush(s.style.brushId)));
		paint.clearLayerSilent(layer);
		strokes.forEach((s, i) => {
			const key = ++remoteKeys;
			paint!.remoteBegin(key, layer);
			const b = styled(brushes[i], s.style);
			for (let j = 0; j + 3 < s.pts.length; j += 4)
				paint!.remoteTo(key, b, s.pts[j], s.pts[j + 1], s.pts[j + 2], s.pts[j + 3] / 1000);
			paint!.remoteEnd(key, b);
		});
		schedule();
	}

	/** Name the local layer (e.g. after the player). */
	export function nameOwnLayer(name: string) {
		const id = paint?.activeLayer;
		if (paint && id) {
			const s = settings(id);
			if (s) setRaw(id, { ...s, name }, false);
			notify();
		}
	}

	function render() {
		raf = 0;
		flushLive();
		if (!paint || !ctx || !memory) return;
		// a new canvas size waits for the page to pass it in (the <canvas> resizes then)
		if (paint.width !== width || paint.height !== height || el.width !== width) return;
		const r = paint.flush();
		if (r.length === 4) {
			const data = new Uint8ClampedArray(memory.buffer, paint.rgbaPtr(), width * height * 4);
			ctx.putImageData(new ImageData(data, width, height), 0, 0, r[0], r[1], r[2], r[3]);
			filmDirty = true;
		}
		// a frame once each change has settled (not in the middle of a stroke or a drag)
		if (filmDirty && !current && !liq && !grad && !paint.isTransforming() && !paint.isAdjusting()) {
			filmDirty = false;
			film.capture(el);
		}
	}

	// ---- timelapse: a frame after every change ----

	const film = new Timelapse();
	/** the canvas changed since the last timelapse frame */
	let filmDirty = false;
	/** The timelapse of this drawing so far (frames after each change). */
	export function timelapse(): Timelapse {
		return film;
	}
	onMount(() => () => film.clear());

	// the canvas got its new size: draw all of it again
	$effect(() => {
		const [w, h] = [width, height];
		if (!paint || !ctx || paint.width !== w || paint.height !== h) return;
		untrack(() => {
			ctx!.fillStyle = '#fff';
			ctx!.fillRect(0, 0, w, h);
			paint!.flush();
			// a view of the engine's memory only after the last engine call (a call can
			// grow the memory, which leaves earlier views empty)
			const data = new Uint8ClampedArray(memory!.buffer, paint!.rgbaPtr(), w * h * 4);
			ctx!.putImageData(new ImageData(data, w, h), 0, 0);
			refreshAnts();
		});
	});

	// ---- brush textures ----

	/** texture ids given to the engine: app id → engine id */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- engine bookkeeping, never rendered
	const texIds = new Map<string, number>();
	function engineTexture(id: string | null): number {
		if (!id || !paint) return 0;
		const known = texIds.get(id);
		if (known) return known;
		const t = textureById(id);
		if (!t) return 0;
		const n = texIds.size + 1;
		if (!paint.setTexture(n, t.w, t.h, t.px)) return 0;
		texIds.set(id, n);
		return n;
	}
	$effect(() => {
		const t = brushTexture;
		if (!ready || !paint) return;
		untrack(() => {
			if (!t) return paint!.setDabStyle(0, 1, 0, 0, 0, 0, 0, 1);
			paint!.setDabStyle(
				engineTexture(t.grain),
				t.grainScale,
				t.grainStrength,
				engineTexture(t.tip),
				engineTexture(t.dual),
				t.dualSize,
				t.dualScatter,
				t.dualCount
			);
		});
	});

	// ---- view: zoom, pan, rotate, mirror (only how it's shown, never the drawing) ----

	let vp: HTMLDivElement;
	let vpW = $state(0);
	let vpH = $state(0);
	let zoom = $state(1);
	let rot = $state(0);
	let flipped = $state(false);
	let panX = $state(0);
	let panY = $state(0);
	const ZOOM_MIN = 0.25;
	const ZOOM_MAX = 16;
	/** CSS px per canvas px at zoom 1 (the canvas fitted inside the viewport) */
	const fit = $derived(vpW && vpH ? Math.min(vpW / width, vpH / height) : 0);
	const baseW = $derived(width * fit);
	const baseH = $derived(height * fit);
	const sheetTransform = $derived(
		`translate(-50%, -50%) translate(${panX}px, ${panY}px) rotate(${rot}deg) scale(${zoom * (flipped ? -1 : 1)}, ${zoom})`
	);

	$effect(() => {
		const ro = new ResizeObserver(([e]) => {
			vpW = e.contentRect.width;
			vpH = e.contentRect.height;
		});
		ro.observe(vp);
		return () => ro.disconnect();
	});

	/** client (screen) position → canvas pixels, undoing pan, rotation, zoom and mirror */
	function toCanvas(clientX: number, clientY: number): [number, number] {
		const r = vp.getBoundingClientRect();
		const dx = clientX - (r.left + r.width / 2) - panX;
		const dy = clientY - (r.top + r.height / 2) - panY;
		const a = (-rot * Math.PI) / 180;
		const ux = dx * Math.cos(a) - dy * Math.sin(a);
		const uy = dx * Math.sin(a) + dy * Math.cos(a);
		const lx = ux / (zoom * (flipped ? -1 : 1));
		const ly = uy / zoom;
		return [(lx + baseW / 2) / fit, (ly + baseH / 2) / fit];
	}

	function clampPan() {
		const mx = vpW / 2 + (baseW * zoom) / 2 - 48;
		const my = vpH / 2 + (baseH * zoom) / 2 - 48;
		panX = Math.max(-mx, Math.min(mx, panX));
		panY = Math.max(-my, Math.min(my, panY));
	}

	/** zoom by `f`, keeping the point under (clientX, clientY) still (default: the centre) */
	function zoomAt(f: number, clientX?: number, clientY?: number) {
		const z = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, zoom * f));
		const k = z / zoom;
		if (clientX !== undefined && clientY !== undefined) {
			const r = vp.getBoundingClientRect();
			const sx = clientX - (r.left + r.width / 2);
			const sy = clientY - (r.top + r.height / 2);
			panX = sx - k * (sx - panX);
			panY = sy - k * (sy - panY);
		} else {
			panX *= k;
			panY *= k;
		}
		zoom = z;
		clampPan();
	}

	// Smooth zoom: steps add up to a target the view glides to, anchored at the pointer.
	let zoomGoal: { log: number; x?: number; y?: number } | null = null;
	let zoomRaf = 0;
	function smoothZoom(logStep: number, clientX?: number, clientY?: number) {
		const from = zoomGoal?.log ?? Math.log(zoom);
		const log = Math.max(Math.log(ZOOM_MIN), Math.min(Math.log(ZOOM_MAX), from + logStep));
		zoomGoal = { log, x: clientX, y: clientY };
		if (!zoomRaf) zoomRaf = requestAnimationFrame(zoomFrame);
	}
	function zoomFrame() {
		zoomRaf = 0;
		if (!zoomGoal) return;
		const now = Math.log(zoom);
		const left = zoomGoal.log - now;
		const done = Math.abs(left) < 0.004;
		zoomAt(Math.exp(done ? left : left * 0.35), zoomGoal.x, zoomGoal.y);
		if (done) zoomGoal = null;
		else zoomRaf = requestAnimationFrame(zoomFrame);
	}
	export function zoomIn() {
		smoothZoom(Math.log(1.25));
	}
	export function zoomOut() {
		smoothZoom(-Math.log(1.25));
	}

	/** Ctrl + drag with the wheel (middle button) held: zoom, up = in, around where it started */
	let zoomDrag: { id: number; y: number; z: number; cx: number; cy: number } | null = null;
	/** back to the whole canvas, upright */
	export function resetView() {
		zoom = 1;
		rot = 0;
		panX = 0;
		panY = 0;
		flipped = false;
	}
	export function rotateBy(deg: number) {
		const a = (deg * Math.PI) / 180;
		[panX, panY] = [
			panX * Math.cos(a) - panY * Math.sin(a),
			panX * Math.sin(a) + panY * Math.cos(a)
		];
		rot = ((((rot + deg) % 360) + 540) % 360) - 180;
	}
	export function resetRotation() {
		rotateBy(-rot);
	}
	/** mirror the view (a classic way to spot proportion mistakes) */
	export function toggleMirror() {
		flipped = !flipped;
		panX = -panX;
	}

	// ---- input ----

	/** touch points currently down (for pinch / rotate / pan) */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- input bookkeeping, never rendered
	const touches = new Map<number, { x: number; y: number }>();
	let gesture: {
		mx: number;
		my: number;
		d: number;
		a: number;
		zoom: number;
		rot: number;
		panX: number;
		panY: number;
	} | null = null;
	/** after a pinch, remaining fingers do nothing until all are lifted */
	let gestureLatch = false;
	let panning: { id: number; x: number; y: number; panX: number; panY: number } | null = null;
	let moving: { id: number; cx: number; cy: number } | null = null;
	let spaceHeld = $state(false);
	/** the outline only belongs to the brush (it would linger after switching tools) */
	const showOutline = $derived(
		!!cursorAt && brushR > 0 && tool === 'brush' && !spaceHeld && !adjustOverlay && !locked
	);
	let hovering = false;
	/** once a pen has been used, fingers only move the view (palm rejection) */
	let penSeen = false;

	function twoTouches() {
		const [p, q] = [...touches.values()];
		return {
			mx: (p.x + q.x) / 2,
			my: (p.y + q.y) / 2,
			d: Math.hypot(q.x - p.x, q.y - p.y) || 1,
			a: Math.atan2(q.y - p.y, q.x - p.x)
		};
	}

	// ---- eyedropper ----

	/** The colour you see at a client position (all visible layers), as #rrggbb. */
	function colorAt(clientX: number, clientY: number): string | null {
		if (!paint || !memory) return null;
		render(); // composite anything pending (and show it) before reading
		const [cx, cy] = toCanvas(clientX, clientY);
		const data = new Uint8ClampedArray(memory.buffer, paint.rgbaPtr(), width * height * 4);
		let [r, g, b, n] = [0, 0, 0, 0];
		for (let dy = -1; dy <= 1; dy++)
			for (let dx = -1; dx <= 1; dx++) {
				const x = Math.round(cx) + dx;
				const y = Math.round(cy) + dy;
				if (x < 0 || y < 0 || x >= width || y >= height) continue;
				const i = (y * width + x) * 4;
				r += data[i];
				g += data[i + 1];
				b += data[i + 2];
				n++;
			}
		if (!n) return null;
		const h = (v: number) =>
			Math.round(v / n)
				.toString(16)
				.padStart(2, '0');
		return `#${h(r)}${h(g)}${h(b)}`;
	}

	function pick(e: PointerEvent) {
		const c = colorAt(e.clientX, e.clientY);
		if (c) onpick?.(c);
	}

	// ---- fill ----

	/** Fill at a client position with `hex` (default: the current colour). */
	function fillAt(clientX: number, clientY: number, hex = color) {
		const [cx, cy] = toCanvas(clientX, clientY);
		return fillAtCanvas(cx, cy, hex, fillTolerance);
	}

	function fillAtCanvas(cx: number, cy: number, hex: string, tolerance: number): boolean {
		if (!paint) return false;
		if (cx < 0 || cy < 0 || cx >= width || cy >= height) return false;
		const l = shownLayers().find((x) => x.id === paint!.activeLayer);
		if (l && (l.locked || !l.visible)) {
			onblocked?.(l.visible ? 'locked' : 'hidden');
			return false;
		}
		const n = parseInt(hex.slice(1), 16) || 0;
		const done = paint.fill(
			Math.floor(cx),
			Math.floor(cy),
			(n >> 16) & 255,
			(n >> 8) & 255,
			n & 255,
			opacity,
			tolerance,
			fillAllLayers,
			2,
			3,
			fillReference
		);
		return changed(done, undefined, 'Fill');
	}

	/**
	 * The last ColorDrop, still adjustable (Procreate: keep holding and slide):
	 * a slider sets how far the colour spreads, refilling as it moves.
	 */
	let dropFill = $state<{
		at: [number, number];
		screen: [number, number];
		hex: string;
		tol: number;
	} | null>(null);
	let dropTimer: ReturnType<typeof setTimeout> | undefined;
	function keepDropFill(ms = 6000) {
		clearTimeout(dropTimer);
		dropTimer = setTimeout(() => (dropFill = null), ms);
	}
	function refillDrop(tol: number) {
		if (!dropFill || !paint) return;
		if (paint.undo()) log.undo();
		fillAtCanvas(dropFill.at[0], dropFill.at[1], dropFill.hex, tol);
		dropFill = { ...dropFill, tol };
		keepDropFill();
	}

	// ---- imported pictures ----

	/**
	 * Bring a picture in as a new layer above the current one, fitted inside the
	 * canvas and centred (move or resize it afterwards with Transform). The
	 * picture never leaves the browser.
	 */
	export async function importImage(file: Blob, name = ''): Promise<ImportResult> {
		if (!paint) return 'busy';
		if (file.size > MAX_IMPORT_BYTES) return 'too-big';
		if (layerList().length >= 32) return 'too-many';
		let bmp: ImageBitmap;
		try {
			bmp = await createImageBitmap(file);
		} catch {
			return 'unreadable';
		}
		try {
			const k = Math.min(width / bmp.width, height / bmp.height);
			const [w, h] = [bmp.width * k, bmp.height * k];
			const c = document.createElement('canvas');
			[c.width, c.height] = [width, height];
			const g = c.getContext('2d')!;
			g.imageSmoothingQuality = 'high';
			g.drawImage(bmp, (width - w) / 2, (height - h) / 2, w, h);
			const rgba = g.getImageData(0, 0, width, height).data;
			const id = paint.addImageLayer(
				name.replace(/\.[a-z0-9]+$/i, '').slice(0, 60),
				new Uint8Array(rgba.buffer)
			);
			if (!id) return 'busy';
			changed(true, undefined, 'Import picture');
			return 'ok';
		} finally {
			bmp.close();
		}
	}

	// ColorDrop: drag a colour swatch onto the canvas to fill there; drop a picture to import it.
	function dragOver(e: DragEvent) {
		const types = e.dataTransfer?.types ?? [];
		if (types.includes('application/x-pp-color') || (allowImport && types.includes('Files')))
			e.preventDefault();
	}
	function drop(e: DragEvent) {
		const file = allowImport
			? [...(e.dataTransfer?.files ?? [])].find((f) => f.type.startsWith('image/'))
			: undefined;
		if (file) {
			e.preventDefault();
			if (!locked) void importImage(file, file.name).then((r) => onimport?.(r));
			return;
		}
		const hex = e.dataTransfer?.getData('application/x-pp-color');
		if (!hex || !/^#[0-9a-f]{6}$/i.test(hex) || locked) return;
		e.preventDefault();
		const at = toCanvas(e.clientX, e.clientY);
		if (fillAtCanvas(at[0], at[1], hex, fillTolerance)) {
			const r = vp.getBoundingClientRect();
			dropFill = { at, screen: [e.clientX - r.left, e.clientY - r.top], hex, tol: fillTolerance };
			keepDropFill();
		}
	}

	// ---- QuickShape: hold still at the end of a stroke ----

	const HOLD_MS = 550;
	let shapeTimer: ReturnType<typeof setTimeout> | undefined;
	let holdAnchor: { x: number; y: number } | null = null;
	let snapped = false;
	/** the shape the stroke in progress snapped to (editable once the pen lifts) */
	let snappedShape: EditShape | null = null;

	function armShape(e: PointerEvent) {
		if (!quickShape || snapped || !current) return;
		if (holdAnchor && Math.hypot(e.clientX - holdAnchor.x, e.clientY - holdAnchor.y) < 4) return;
		holdAnchor = { x: e.clientX, y: e.clientY };
		clearTimeout(shapeTimer);
		shapeTimer = setTimeout(snapShape, HOLD_MS);
	}

	function snapShape() {
		if (!paint || !brush || !current || snapped) return;
		const pts = current.points.map((p) => [p.x, p.y] as [number, number]);
		const shape = recognise(pts);
		if (!shape) return;
		const pressure = current.points.reduce((s, p) => s + p.pressure, 0) / current.points.length;
		snappedShape = editable(shape, pressure);
		const path = resample(shape.points, 3);
		// redraw: drop the hand-drawn stroke, draw the clean one in its place
		paint.cancelStroke();
		if (sendLive()) {
			flushLive();
			onlive?.({ type: 'cancel', k: liveK });
			liveK++;
			onlive?.({
				type: 'begin',
				k: liveK,
				style: { brushId: sharedBrushId(brushId), color, size, opacity, eraser }
			});
		}
		if (!paint.beginStroke()) return;
		const t = current.points.at(-1)?.t ?? 0;
		current.points = [];
		for (const [x, y] of path) {
			paint.strokeTo(brush, x, y, pressure, 0, 0, 0.008);
			current.points.push({ x: Math.round(x * 10) / 10, y: Math.round(y * 10) / 10, pressure, t });
			if (sendLive())
				livePts.push(
					Math.round(x * 10) / 10,
					Math.round(y * 10) / 10,
					Math.round(pressure * 1000) / 1000,
					8
				);
		}
		snapped = true;
		schedule();
		onshape?.(shape.kind);
	}

	// ---- selections ----

	/** a rectangle or lasso being dragged (canvas px), shown live */
	let marquee = $state<{ kind: 'rect' | 'lasso'; pts: [number, number][]; op: number } | null>(
		null
	);
	let antsEl: HTMLCanvasElement | undefined = $state();
	let antsEdges: Int32Array = new Int32Array(0);
	let antsBox = { x0: 0, y0: 0, x1: 0, y1: 0 };
	let antsPhase = 0;

	/** 0 new, 1 add (Shift), 2 subtract (Alt), 3 intersect (Shift+Alt) */
	const opOf = (e: PointerEvent | MouseEvent) =>
		e.shiftKey && e.altKey
			? 3
			: e.shiftKey
				? 1
				: e.altKey
					? 2
					: { replace: 0, add: 1, subtract: 2, intersect: 3 }[selectMode];

	/** Recompute the marching-ants outline from the engine's selection. */
	function refreshAnts() {
		if (!paint) return;
		const m = paint.selectionMask();
		const edges: number[] = [];
		let [x0, y0, x1, y1] = [width, height, -1, -1];
		if (m.length) {
			for (let y = 0; y < height; y++)
				for (let x = 0; x < width; x++) {
					const i = y * width + x;
					// a feathered selection's outline is where it's half selected
					if (m[i] < 128) continue;
					if (
						x === 0 ||
						y === 0 ||
						x === width - 1 ||
						y === height - 1 ||
						m[i - 1] < 128 ||
						m[i + 1] < 128 ||
						m[i - width] < 128 ||
						m[i + width] < 128
					) {
						edges.push(i);
						x0 = Math.min(x0, x);
						y0 = Math.min(y0, y);
						x1 = Math.max(x1, x);
						y1 = Math.max(y1, y);
					}
				}
		}
		antsEdges = Int32Array.from(edges);
		antsBox = { x0, y0, x1, y1 };
		drawAnts();
		notify();
	}

	function drawAnts() {
		const c = antsEl?.getContext('2d');
		if (!c) return;
		c.clearRect(0, 0, width, height);
		if (!antsEdges.length) return;
		const { x0, y0, x1, y1 } = antsBox;
		const bw = x1 - x0 + 1;
		const img = c.createImageData(bw, y1 - y0 + 1);
		for (const i of antsEdges) {
			const x = i % width;
			const y = (i / width) | 0;
			const o = ((y - y0) * bw + (x - x0)) * 4;
			const v = ((x + y + antsPhase) >> 2) & 1 ? 255 : 0;
			img.data[o] = img.data[o + 1] = img.data[o + 2] = v;
			img.data[o + 3] = 255;
		}
		c.putImageData(img, x0, y0);
	}

	$effect(() => {
		const id = setInterval(() => {
			if (!antsEdges.length) return;
			antsPhase = (antsPhase + 1) % 8;
			drawAnts();
		}, 140);
		return () => clearInterval(id);
	});

	/** rasterise a rectangle / lasso to a canvas-sized mask and combine it */
	// ---- lasso fill: draw a shape, it fills solid with the brush colour ----

	let lassoFill = $state<[number, number][] | null>(null);

	function commitLassoFill() {
		if (!paint || !lassoFill) return;
		const pts = lassoFill;
		lassoFill = null;
		fillPolygon(pts, 'Lasso fill');
	}

	/** Fill a closed shape with the brush colour (one undo step). */
	function fillPolygon(pts: [number, number][], label: string) {
		if (!paint || pts.length < 3) return false;
		const off = new OffscreenCanvas(width, height);
		const c = off.getContext('2d')!;
		c.fillStyle = '#fff';
		c.beginPath();
		c.moveTo(...pts[0]);
		for (const p of pts.slice(1)) c.lineTo(...p);
		c.closePath();
		c.fill();
		const rgba = c.getImageData(0, 0, width, height).data;
		const coverage = new Uint8Array(width * height);
		for (let i = 0; i < coverage.length; i++) coverage[i] = rgba[i * 4 + 3];
		const n = parseInt(color.slice(1), 16) || 0;
		return changed(
			paint.fillMask(coverage, (n >> 16) & 255, (n >> 8) & 255, n & 255, opacity),
			undefined,
			label
		);
	}

	function commitMarquee() {
		if (!paint || !marquee) return;
		const { kind, pts, op } = marquee;
		marquee = null;
		const area =
			kind === 'rect' ? Math.abs((pts[1][0] - pts[0][0]) * (pts[1][1] - pts[0][1])) : pts.length;
		if (area < 4) {
			// a click without a drag clears the selection (replace mode)
			if (op === 0) paint.deselect();
			return refreshAnts();
		}
		const off = new OffscreenCanvas(width, height);
		const c = off.getContext('2d')!;
		c.fillStyle = '#fff';
		c.beginPath();
		if (kind === 'rect') {
			const [[ax, ay], [bx, by]] = pts;
			c.rect(Math.min(ax, bx), Math.min(ay, by), Math.abs(bx - ax), Math.abs(by - ay));
		} else {
			c.moveTo(...pts[0]);
			for (const p of pts.slice(1)) c.lineTo(...p);
			c.closePath();
		}
		c.fill();
		const rgba = c.getImageData(0, 0, width, height).data;
		const mask = new Uint8Array(width * height);
		for (let i = 0; i < mask.length; i++) mask[i] = rgba[i * 4 + 3];
		paint.setSelection(mask, op);
		refreshAnts();
	}

	// ---- copy / cut / paste (the selection, or the whole layer) ----

	/** what was copied: a full-canvas picture (straight RGBA), transparent outside the selection */
	let clip: Uint8Array | null = null;

	/** Copy the selected part of the layer (all of it with no selection); `cut` also clears it. */
	export function copySelection(cut = false): boolean {
		if (!paint || xf) return false;
		const rgba = paint.layerRgba(paint.activeLayer);
		if (!rgba.length) return false;
		const sel = paint.selectionMask();
		let any = false;
		for (let i = 0; i < width * height; i++) {
			// a feathered edge copies partly
			if (sel.length) rgba[i * 4 + 3] = Math.round((rgba[i * 4 + 3] * sel[i]) / 255);
			any ||= rgba[i * 4 + 3] > 0;
		}
		if (!any) return false;
		clip = rgba;
		if (cut) {
			const layer = paint.activeLayer;
			changed(paint.clear(), () => log.clear(layer), 'Cut');
		}
		return true;
	}

	/** Paste what was copied as a new layer, in the same place; false if nothing was copied. */
	export function paste(): boolean {
		if (!paint || !clip || xf) return false;
		const id = paint.addImageLayer('Pasted', clip);
		if (!id) return false;
		paint.deselect();
		refreshAnts();
		changed(true, undefined, 'Paste');
		return true;
	}

	/** Something has been copied (Ctrl+V pastes it). */
	export const hasClip = () => !!clip;

	/** Soften the selection's edge over about `radius` canvas px. */
	export function featherSelection(radius: number) {
		if (paint?.featherSelection(radius)) refreshAnts();
	}

	export function selectAll() {
		paint?.selectAll();
		refreshAnts();
	}
	export function deselect() {
		paint?.deselect();
		refreshAnts();
	}
	export function invertSelection() {
		paint?.invertSelection();
		refreshAnts();
	}

	// ---- transform: move / scale / rotate / flip the selection or the layer ----

	interface Xf {
		box: [number, number, number, number];
		tx: number;
		ty: number;
		sx: number;
		sy: number;
		rot: number;
		fx: 1 | -1;
		fy: 1 | -1;
		/** distort: the box's corners (tl, tr, br, bl) pinned here instead; overrides the rest */
		quad?: Quad | null;
		/** warp: 4×4 control points, row by row (a bicubic patch); overrides the rest */
		mesh?: Pt[] | null;
	}
	let xf = $state<Xf | null>(null);
	let xfDrag: {
		mode: 'move' | 'rotate' | 'corner' | 'edge-x' | 'edge-y' | 'node';
		start: [number, number];
		from: Xf;
		/** which corner / edge was grabbed (distort moves just that one) */
		index: number;
	} | null = null;
	let xfRaf = 0;

	const xfCenter = (x: Xf): [number, number] => [
		(x.box[0] + x.box[2]) / 2,
		(x.box[1] + x.box[3]) / 2
	];

	/** dest = M · src, as [a, b, c, d, e, f] */
	function xfMatrix(x: Xf): [number, number, number, number, number, number] {
		const [cx, cy] = xfCenter(x);
		const r = (x.rot * Math.PI) / 180;
		const [cs, sn] = [Math.cos(r), Math.sin(r)];
		const [kx, ky] = [x.sx * x.fx, x.sy * x.fy];
		const a = cs * kx;
		const b = sn * kx;
		const c = -sn * ky;
		const d = cs * ky;
		return [a, b, c, d, cx + x.tx - (a * cx + c * cy), cy + x.ty - (b * cx + d * cy)];
	}

	const xfApply = (m: number[], p: [number, number]): [number, number] => [
		m[0] * p[0] + m[2] * p[1] + m[4],
		m[1] * p[0] + m[3] * p[1] + m[5]
	];

	/** the box's corners (tl, tr, br, bl) after the transform, in canvas px */
	function xfCorners(x: Xf): Quad {
		if (x.mesh) return [x.mesh[0], x.mesh[3], x.mesh[15], x.mesh[12]];
		if (x.quad) return x.quad;
		const m = xfMatrix(x);
		const [x0, y0, x1, y1] = x.box;
		return [xfApply(m, [x0, y0]), xfApply(m, [x1, y0]), xfApply(m, [x1, y1]), xfApply(m, [x0, y1])];
	}

	/** the transformed box corners (tl, tr, br, bl) and handles, in canvas px */
	const xfShape = $derived.by(() => {
		if (!xf) return null;
		const corners = xfCorners(xf);
		const mid = (p: [number, number], q: [number, number]): [number, number] => [
			(p[0] + q[0]) / 2,
			(p[1] + q[1]) / 2
		];
		const top = mid(corners[0], corners[1]);
		const centre = mid(corners[0], corners[2]);
		// the rotate handle sits outside the top edge, ~28 screen px away
		const away = 28 / Math.max(0.01, fit * zoom);
		const dir = [top[0] - centre[0], top[1] - centre[1]];
		const len = Math.hypot(dir[0], dir[1]) || 1;
		const rotH: [number, number] = [top[0] + (dir[0] / len) * away, top[1] + (dir[1] / len) * away];
		return {
			corners,
			edges: [
				top,
				mid(corners[1], corners[2]),
				mid(corners[2], corners[3]),
				mid(corners[3], corners[0])
			],
			rotH,
			top
		};
	});

	function xfShow() {
		if (!paint || !xf || xfRaf) return;
		xfRaf = requestAnimationFrame(() => {
			xfRaf = 0;
			if (!paint || !xf) return;
			if (xf.mesh) paint.transformPreviewMesh(meshNodes(xf.mesh, WARP_N), WARP_N, WARP_N);
			else if (xf.quad) paint.transformPreviewH(new Float32Array(boxToQuad(xf.box, xf.quad)));
			else paint.transformPreview(...xfMatrix(xf));
			render();
		});
	}

	// entering the tool lifts the pixels; leaving it keeps the result
	$effect(() => {
		if (!ready || !paint) return;
		if (tool === 'transform' && !xf && !paint.isTransforming()) {
			const b = paint.transformBegin();
			if (b.length !== 4) return void ontransformend?.('empty');
			xf = { box: [b[0], b[1], b[2], b[3]], tx: 0, ty: 0, sx: 1, sy: 1, rot: 0, fx: 1, fy: 1 };
			if (untrack(() => warp)) xf = { ...xf, mesh: startMesh(xf) };
			schedule();
		} else if (tool !== 'transform' && xf) {
			transformCommit();
		}
	});

	export function transformCommit() {
		if (!paint || !xf) return;
		const label = xf.mesh ? 'Warp' : xf.quad ? 'Distort' : 'Transform';
		xf = null;
		changed(paint.transformCommit(), undefined, label);
		refreshAnts();
		ontransformend?.('done');
	}
	export function transformCancel() {
		if (!paint || !xf) return;
		xf = null;
		paint.transformCancel();
		schedule();
		notify();
		refreshAnts();
		ontransformend?.('cancelled');
	}
	/** the middle of a quad (average of its corners) */
	const quadMid = (q: Quad): Pt => [
		(q[0][0] + q[1][0] + q[2][0] + q[3][0]) / 4,
		(q[0][1] + q[1][1] + q[2][1] + q[3][1]) / 4
	];
	const mapQuad = (q: Quad, f: (p: Pt) => Pt): Quad => [f(q[0]), f(q[1]), f(q[2]), f(q[3])];
	function rotatePts(ps: Pt[], deg: number, about: Pt = meshMid(ps)): Pt[] {
		const [cx, cy] = about;
		const r = (deg * Math.PI) / 180;
		const [cs, sn] = [Math.cos(r), Math.sin(r)];
		return ps.map(([x, y]) => [
			cx + (x - cx) * cs - (y - cy) * sn,
			cy + (x - cx) * sn + (y - cy) * cs
		]);
	}
	const rotateQuad = (q: Quad, deg: number) => rotatePts(q, deg, quadMid(q)) as Quad;

	/** the 4×4 warp grid laid evenly over the current corners */
	function startMesh(x: Xf): Pt[] {
		const [tl, tr, br, bl] = xfCorners(x);
		const lerp = (a: Pt, b: Pt, t: number): Pt => [
			a[0] + (b[0] - a[0]) * t,
			a[1] + (b[1] - a[1]) * t
		];
		const out: Pt[] = [];
		for (let r = 0; r < 4; r++)
			for (let c = 0; c < 4; c++) out.push(lerp(lerp(tl, tr, c / 3), lerp(bl, br, c / 3), r / 3));
		return out;
	}
	// a new tool ends shape editing and the measuring line
	$effect(() => {
		if (tool !== 'brush') editShape = null;
		if (tool !== 'measure') measureSeg = null;
	});

	// ---- pen tool: Bézier paths, stroked with the brush or filled ----

	let penPath = $state<PenPath>(EMPTY_PATH);
	let penDrag: { id: number; index: number; part: 'p' | 'hin' | 'hout'; broken: boolean } | null =
		null;
	$effect(() => {
		const p = penPath;
		untrack(() => onpenpath?.({ points: p.anchors.length, closed: p.closed }));
	});

	function penDown(e: PointerEvent) {
		const p = toCanvas(e.clientX, e.clientY);
		const r = 10 / Math.max(0.01, fit * zoom);
		const hit = hitPath(penPath, p, r);
		capture(e);
		if (hit) {
			// the first point again closes the path
			if (hit.index === 0 && hit.part === 'p' && !penPath.closed && penPath.anchors.length > 2) {
				penPath = { ...penPath, closed: true };
				penDrag = { id: e.pointerId, index: 0, part: 'hout', broken: false };
				return;
			}
			penDrag = { id: e.pointerId, ...hit, broken: e.altKey };
			return;
		}
		if (penPath.closed) penPath = EMPTY_PATH;
		penPath = { ...penPath, anchors: [...penPath.anchors, corner(p)] };
		// dragging out of a new point pulls its handles (a smooth point)
		penDrag = { id: e.pointerId, index: penPath.anchors.length - 1, part: 'hout', broken: false };
	}

	function penMove(e: PointerEvent) {
		if (!penDrag || penDrag.id !== e.pointerId) return false;
		const to = toCanvas(e.clientX, e.clientY);
		penPath = dragPart(penPath, penDrag.index, penDrag.part, to, penDrag.broken || e.altKey);
		return true;
	}

	/** Draw along the path with the current brush (one stroke, one undo step). */
	export function penStroke(taper = true) {
		if (!paint || !brush || penPath.anchors.length < 2) return false;
		if (!paint.beginStroke()) {
			const l = shownLayers().find((x) => x.id === paint!.activeLayer);
			onblocked?.(l && !l.visible ? 'hidden' : 'locked');
			return false;
		}
		const pts = samplePath(penPath, 2);
		const stroke: Stroke = {
			brushId,
			eraser,
			layer: paint.activeLayer,
			color,
			size,
			opacity,
			points: []
		};
		const t = Math.round(performance.now() - t0);
		pts.forEach(([x, y], i) => {
			// a light touch at the ends, like a hand-drawn line
			const k = i / Math.max(1, pts.length - 1);
			const pr = taper && !penPath.closed ? 0.25 + 0.6 * Math.sin(Math.PI * k) : 0.7;
			paint!.strokeTo(brush!, x, y, pr, 0, 0, 0.008);
			stroke.points.push({
				x: Math.round(x * 10) / 10,
				y: Math.round(y * 10) / 10,
				pressure: pr,
				t
			});
		});
		paint.endStroke(brush);
		changed(true, () => log.add(stroke), 'Pen path');
		return true;
	}

	/** Fill inside the path with the brush colour. */
	export function penFill() {
		if (penPath.anchors.length < 3) return false;
		return fillPolygon(samplePath({ ...penPath, closed: true }, 1.5), 'Pen fill');
	}

	export function penClear() {
		penPath = EMPTY_PATH;
	}

	/** Take back the last point. */
	export function penBack() {
		penPath = penPath.closed
			? { ...penPath, closed: false }
			: { ...penPath, anchors: penPath.anchors.slice(0, -1) };
	}

	/** warp mode was switched on or off while transforming */
	$effect(() => {
		const on = warp;
		const x = untrack(() => xf);
		if (!x) return;
		if (on && !x.mesh) xf = { ...x, mesh: startMesh(x), quad: null };
		else if (!on && x.mesh) xf = { ...x, quad: xfCorners(x), mesh: null };
		untrack(xfShow);
	});
	const meshMid = (m: Pt[]): Pt => [
		m.reduce((s, p) => s + p[0], 0) / m.length,
		m.reduce((s, p) => s + p[1], 0) / m.length
	];

	export function transformFlip(axis: 'x' | 'y') {
		if (!xf) return;
		if (xf.mesh) {
			const [cx, cy] = meshMid(xf.mesh);
			xf = {
				...xf,
				mesh: xf.mesh.map(([x, y]) => (axis === 'x' ? [2 * cx - x, y] : [x, 2 * cy - y]))
			};
		} else if (xf.quad) {
			// mirror the pinned corners across the middle: the picture flips with them
			const [cx, cy] = quadMid(xf.quad);
			xf = {
				...xf,
				quad: mapQuad(xf.quad, ([x, y]) => (axis === 'x' ? [2 * cx - x, y] : [x, 2 * cy - y]))
			};
		} else
			xf = axis === 'x' ? { ...xf, fx: xf.fx === 1 ? -1 : 1 } : { ...xf, fy: xf.fy === 1 ? -1 : 1 };
		xfShow();
	}
	export function transformRotate(deg: number) {
		if (!xf) return;
		if (xf.mesh) xf = { ...xf, mesh: rotatePts(xf.mesh, deg) };
		else if (xf.quad) xf = { ...xf, quad: rotateQuad(xf.quad, deg) };
		else xf = { ...xf, rot: (((xf.rot + deg) % 360) + 360) % 360 };
		xfShow();
	}
	export function transformReset() {
		if (!xf) return;
		xf = { ...xf, tx: 0, ty: 0, sx: 1, sy: 1, rot: 0, fx: 1, fy: 1, quad: null, mesh: null };
		if (warp) xf = { ...xf, mesh: startMesh(xf) };
		xfShow();
	}

	/** what a press at canvas point p grabs (and which corner / edge) */
	function xfHit(
		p: [number, number]
	): { mode: NonNullable<typeof xfDrag>['mode']; index: number } | null {
		if (!xfShape) return null;
		const r = 14 / Math.max(0.01, fit * zoom);
		const near = (q: [number, number]) => Math.hypot(p[0] - q[0], p[1] - q[1]) <= r;
		if (xf?.mesh) {
			const ni = xf.mesh.findIndex(near);
			if (ni >= 0) return { mode: 'node', index: ni };
		}
		if (near(xfShape.rotH)) return { mode: 'rotate', index: 0 };
		const ci = xfShape.corners.findIndex(near);
		if (ci >= 0) return { mode: 'corner', index: ci };
		const e = xfShape.edges.findIndex(near);
		if (e >= 0) return { mode: e % 2 === 0 ? 'edge-y' : 'edge-x', index: e };
		// inside the (possibly rotated) box?
		const c = xfShape.corners;
		let inside = false;
		for (let i = 0, j = 3; i < 4; j = i++)
			if (
				c[i][1] > p[1] !== c[j][1] > p[1] &&
				p[0] < ((c[j][0] - c[i][0]) * (p[1] - c[i][1])) / (c[j][1] - c[i][1]) + c[i][0]
			)
				inside = !inside;
		return inside ? { mode: 'move', index: 0 } : null;
	}

	/** Distort: drag corners / edges freely; move and rotate the pinned quad. */
	function quadMove(e: PointerEvent, p: Pt, f: Xf & { quad: Quad }) {
		if (!xfDrag) return;
		const [dx, dy] = [p[0] - xfDrag.start[0], p[1] - xfDrag.start[1]];
		const q = f.quad;
		let next: Quad;
		if (xfDrag.mode === 'move') next = mapQuad(q, ([x, y]) => [x + dx, y + dy]);
		else if (xfDrag.mode === 'rotate') {
			const [cx, cy] = quadMid(q);
			const a0 = Math.atan2(xfDrag.start[1] - cy, xfDrag.start[0] - cx);
			const a1 = Math.atan2(p[1] - cy, p[0] - cx);
			let deg = ((a1 - a0) * 180) / Math.PI;
			if (e.shiftKey) deg = Math.round(deg / 15) * 15;
			next = rotateQuad(q, deg);
		} else if (xfDrag.mode === 'corner') {
			next = [...q] as Quad;
			next[xfDrag.index] = [q[xfDrag.index][0] + dx, q[xfDrag.index][1] + dy];
		} else {
			// an edge: both of its corners move together
			const [i, j] = [xfDrag.index, (xfDrag.index + 1) % 4];
			next = [...q] as Quad;
			next[i] = [q[i][0] + dx, q[i][1] + dy];
			next[j] = [q[j][0] + dx, q[j][1] + dy];
		}
		// a folded or flat shape can't be shown: keep the last good one
		if (usableQuad(next)) xf = { ...f, quad: next };
	}

	function xfMove(e: PointerEvent) {
		if (!xfDrag) return;
		const p = toCanvas(e.clientX, e.clientY);
		const f = xfDrag.from;
		if (f.mesh) {
			const [dx, dy] = [p[0] - xfDrag.start[0], p[1] - xfDrag.start[1]];
			const m = f.mesh;
			let next: Pt[];
			if (xfDrag.mode === 'node')
				next = m.map((q, i) => (i === xfDrag!.index ? [q[0] + dx, q[1] + dy] : q));
			else if (xfDrag.mode === 'rotate') {
				const [cx, cy] = meshMid(m);
				const a0 = Math.atan2(xfDrag.start[1] - cy, xfDrag.start[0] - cx);
				const a1 = Math.atan2(p[1] - cy, p[0] - cx);
				next = rotatePts(m, ((a1 - a0) * 180) / Math.PI);
			} else next = m.map(([x, y]) => [x + dx, y + dy]);
			xf = { ...f, mesh: next };
			return xfShow();
		}
		if (f.quad) {
			quadMove(e, p, f as Xf & { quad: Quad });
			return xfShow();
		}
		const [cx, cy] = xfCenter(f);
		const centre: [number, number] = [cx + f.tx, cy + f.ty];
		if (xfDrag.mode === 'move') {
			xf = { ...f, tx: f.tx + p[0] - xfDrag.start[0], ty: f.ty + p[1] - xfDrag.start[1] };
		} else if (xfDrag.mode === 'rotate') {
			const a0 = Math.atan2(xfDrag.start[1] - centre[1], xfDrag.start[0] - centre[0]);
			const a1 = Math.atan2(p[1] - centre[1], p[0] - centre[0]);
			let rot = f.rot + ((a1 - a0) * 180) / Math.PI;
			if (e.shiftKey) rot = Math.round(rot / 15) * 15;
			xf = { ...f, rot: ((rot % 360) + 360) % 360 };
		} else {
			// work in the box's own (unrotated) frame
			const r = (-f.rot * Math.PI) / 180;
			const local = (q: [number, number]): [number, number] => {
				const [dx, dy] = [q[0] - centre[0], q[1] - centre[1]];
				return [dx * Math.cos(r) - dy * Math.sin(r), dx * Math.sin(r) + dy * Math.cos(r)];
			};
			const [s0, s1] = [local(xfDrag.start), local(p)];
			const k = (a: number, b: number) =>
				Math.abs(a) < 1 ? 1 : Math.max(0.02, Math.abs(b) / Math.abs(a));
			if (xfDrag.mode === 'corner') {
				const g = k(Math.hypot(...s0), Math.hypot(...s1));
				xf = { ...f, sx: f.sx * g, sy: f.sy * g };
			} else if (xfDrag.mode === 'edge-x') xf = { ...f, sx: f.sx * k(s0[0], s1[0]) };
			else xf = { ...f, sy: f.sy * k(s0[1], s1[1]) };
		}
		xfShow();
	}

	// ---- guides: assist while drawing, and dragging them with the 'guides' tool ----

	/** the line a stroke is held to (ruler, or a perspective direction) */
	let snapLine: { origin: Pt; u: Pt } | null = null;
	/** perspective: directions on offer until the stroke has shown which way it goes */
	let snapPending: { start: Pt; dirs: Pt[] } | null = null;
	let guideDrag: {
		what: 'vp' | 'a' | 'b' | 'ruler' | 'sym';
		i: number;
		start: Pt;
		from: GuideSettings;
	} | null = null;

	function startSnap(p: Pt) {
		snapLine = null;
		snapPending = null;
		heldBack = [];
		if (!guides) return;
		const reach = 24 / Math.max(0.01, fit * zoom);
		const r = rulerLine(guides, p, reach);
		if (r) {
			snapLine = r;
			return;
		}
		const dirs = snapDirections(guides, p);
		if (dirs.length) snapPending = { start: p, dirs };
	}

	/** where a raw input point goes once guides have had their say */
	function guided(p: Pt): Pt {
		if (snapPending) {
			const d: Pt = [p[0] - snapPending.start[0], p[1] - snapPending.start[1]];
			if (Math.hypot(d[0], d[1]) > 20 / Math.max(0.01, fit * zoom)) {
				const u = closestDirection(snapPending.dirs, d);
				if (u) snapLine = { origin: snapPending.start, u };
				snapPending = null;
			}
		}
		return snapLine ? project(snapLine.origin, snapLine.u, p) : p;
	}

	function guideHit(p: Pt): typeof guideDrag {
		if (!guides) return null;
		const r = 16 / Math.max(0.01, fit * zoom);
		const near = (q: Pt) => Math.hypot(p[0] - q[0], p[1] - q[1]) <= r;
		if (guides.symmetry?.mode !== 'off' && guides.symmetry && near(guides.symmetry.c))
			return { what: 'sym', i: 0, start: p, from: $state.snapshot(guides) as GuideSettings };
		const n = guides.perspective.points;
		for (let i = 0; i < n; i++)
			if (near(guides.perspective.vps[i]))
				return { what: 'vp', i, start: p, from: $state.snapshot(guides) as GuideSettings };
		if (guides.ruler.on) {
			if (near(guides.ruler.a))
				return { what: 'a', i: 0, start: p, from: $state.snapshot(guides) as GuideSettings };
			if (near(guides.ruler.b))
				return { what: 'b', i: 0, start: p, from: $state.snapshot(guides) as GuideSettings };
			if (rulerLine(guides, p, r))
				return { what: 'ruler', i: 0, start: p, from: $state.snapshot(guides) as GuideSettings };
		}
		return null;
	}

	function guideMove(p: Pt) {
		if (!guideDrag) return;
		const g = structuredClone(guideDrag.from);
		const [dx, dy] = [p[0] - guideDrag.start[0], p[1] - guideDrag.start[1]];
		const shift = (q: Pt): Pt => [q[0] + dx, q[1] + dy];
		if (guideDrag.what === 'vp')
			g.perspective.vps[guideDrag.i] = shift(g.perspective.vps[guideDrag.i]);
		else if (guideDrag.what === 'a') g.ruler.a = shift(g.ruler.a);
		else if (guideDrag.what === 'b') g.ruler.b = shift(g.ruler.b);
		else if (guideDrag.what === 'sym') g.symmetry.c = shift(g.symmetry.c);
		else g.ruler = { ...g.ruler, a: shift(g.ruler.a), b: shift(g.ruler.b) };
		onguides?.(g);
	}

	const guideAxes = $derived(guides ? symmetryAxes(guides, width, height) : []);

	// tell the engine about symmetry whenever it changes
	$effect(() => {
		if (!ready || !paint) return;
		const sym = guides?.symmetry;
		const mode = sym ? SYMMETRY_MODES.indexOf(sym.mode) : 0;
		paint.setSymmetry(Math.max(0, mode), sym?.n ?? 6, sym?.c[0] ?? 0, sym?.c[1] ?? 0);
	});

	const guideGrid = $derived(guides?.grid.on ? gridLines(guides, width, height) : []);
	const guideRays = $derived(
		guides && guides.perspective.points
			? guides.perspective.vps
					.slice(0, guides.perspective.points)
					.flatMap((vp) => perspectiveRays(vp, width, height))
			: []
	);

	// ---- liquify ----

	let liq: { id: number; x: number; y: number } | null = null;

	// ---- gradient: drag from start to end, previewed live ----
	let grad = $state<{ id: number; from: Pt; to: Pt } | null>(null);

	const unit = (hex: string): [number, number, number] => {
		const n = parseInt(hex.slice(1), 16) || 0;
		return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
	};

	function gradientShow() {
		if (!paint || !grad) return;
		const c0 = unit(color);
		const end = gradientEnd === 'transparent' ? null : unit(gradientEnd);
		const c1 = end ?? c0;
		paint.gradientPreview(
			gradientKind === 'radial',
			grad.from[0],
			grad.from[1],
			grad.to[0],
			grad.to[1],
			...c0,
			opacity,
			...c1,
			end ? opacity : 0
		);
		schedule();
	}

	// ---- adjustments (the draw page's Adjust panel) ----

	/** Start adjusting the active layer; false if it's locked or hidden. */
	export function adjustBegin(): boolean {
		return !!paint?.adjustBegin();
	}

	export interface Adjustments {
		hue: number;
		saturation: number;
		brightness: number;
		contrast: number;
		blur: number;
		/** paint the layer this colour (keeps its transparency) */
		recolour: string | null;
		/** curves, levels and gradient map */
		curves?: AdjustSpec['curves'];
		levels?: AdjustSpec['levels'];
		map?: AdjustSpec['map'];
	}

	export function adjustPreview(a: Adjustments) {
		if (!paint?.isAdjusting()) return;
		const [r, g, b] = a.recolour ? unit(a.recolour) : [0, 0, 0];
		paint.adjustPreview(
			a.hue,
			a.saturation,
			a.brightness,
			a.contrast,
			a.blur,
			!!a.recolour,
			r,
			g,
			b,
			toneTables({ curves: a.curves ?? null, levels: a.levels ?? null }),
			mapTable(a.map ?? null)
		);
		schedule();
	}

	// ---- adjustment layers (Photoshop's): they change what's below, and stay editable ----

	/** Add an adjustment layer above the current one; its id (0 = not possible). */
	export function addAdjustmentLayer(spec: AdjustSpec, name: string): number {
		if (!paint) return 0;
		const id = paint.addAdjustmentLayer(
			name,
			JSON.stringify(spec),
			spec.hue,
			spec.saturation,
			spec.brightness,
			spec.contrast,
			toneTables(spec),
			mapTable(spec.map)
		);
		changed(id > 0, undefined, 'Adjustment layer');
		return id;
	}

	/** Change an adjustment layer live (`adjustmentCommit` makes it one undo step). */
	export function adjustmentPreview(id: number, spec: AdjustSpec) {
		if (!paint) return;
		paint.adjustmentPreview(
			id,
			JSON.stringify(spec),
			spec.hue,
			spec.saturation,
			spec.brightness,
			spec.contrast,
			toneTables(spec),
			mapTable(spec.map)
		);
		schedule();
	}

	export function adjustmentCommit() {
		if (paint) changed(paint.adjustmentCommit(), undefined, 'Adjustment layer');
	}

	/**
	 * A new canvas size, keeping the drawing (one undo step): moved by (dx, dy)
	 * canvas px (a crop, or more room), or with `scale` resized to fit. The page
	 * hears the new size through `oncanvassize`.
	 */
	export function resizeCanvas(w: number, h: number, dx: number, dy: number, scale: boolean) {
		if (!paint) return false;
		if (xf) transformCancel();
		penPath = EMPTY_PATH;
		return changed(
			paint.resizeCanvas(w, h, Math.round(dx), Math.round(dy), scale),
			undefined,
			'Canvas size'
		);
	}

	/** keep the adjustment (one undo step) */
	export function adjustCommit() {
		if (paint?.isAdjusting()) changed(paint.adjustCommit(), undefined, 'Adjustment');
	}

	export function adjustCancel() {
		if (!paint?.isAdjusting()) return;
		paint.adjustCancel();
		schedule();
	}
	/** where the liquify brush circle is drawn (canvas px) */
	let hoverAt = $state<[number, number] | null>(null);

	// ---- smoothing (lazy brush): the pen point follows the pointer on a short leash ----

	let smooth: { x: number; y: number; p: number } | null = null;
	let lastRaw: { x: number; y: number; p: number; t: number } | null = null;

	/** at the end of a smoothed stroke, carry the line on to where the pen actually stopped */
	function catchUp() {
		if (!paint || !brush || !current || !smooth || !lastRaw || smoothing <= 0) return;
		const steps = 6;
		const from = { ...smooth };
		for (let k = 1; k <= steps; k++) {
			const t = k / steps;
			const x = from.x + (lastRaw.x - from.x) * t;
			const y = from.y + (lastRaw.y - from.y) * t;
			const p = from.p + (lastRaw.p - from.p) * t;
			paint.strokeTo(brush, x, y, p, 0, 0, 0.004);
			if (sendLive())
				livePts.push(
					Math.round(x * 10) / 10,
					Math.round(y * 10) / 10,
					Math.round(p * 1000) / 1000,
					4
				);
			current.points.push({
				x: Math.round(x * 10) / 10,
				y: Math.round(y * 10) / 10,
				pressure: Math.round(p * 1000) / 1000,
				t: Math.round(lastRaw.t - t0)
			});
		}
		smooth = { x: lastRaw.x, y: lastRaw.y, p: lastRaw.p };
	}

	// ---- touch taps: two fingers = undo, three = redo ----

	let tap: {
		t0: number;
		max: number;
		moved: boolean;
		starts: Map<number, { x: number; y: number }>;
		view: { zoom: number; rot: number; panX: number; panY: number };
	} | null = null;
	let holdTimer: ReturnType<typeof setTimeout> | undefined;
	/** follow factor per sample: 1 = raw input, small = very steady */
	const follow = () => 1 - Math.min(0.92, Math.max(0, smoothing) * 0.92);

	interface RawPoint {
		x: number;
		y: number;
		p: number;
		tx: number;
		ty: number;
		time: number;
	}
	/** points held back while perspective assist works out which way the stroke goes */
	let heldBack: RawPoint[] = [];

	// ---- Shift+click: a straight line from where the last stroke ended (as in Photoshop) ----

	const D = Math.SQRT1_2;
	/** Shift + drag: horizontal, vertical and the two diagonals */
	const STRAIGHT_DIRS: Pt[] = [
		[1, 0],
		[0, 1],
		[D, D],
		[D, -D]
	];
	/** the stroke in progress began with a Shift+click line */
	let lineStroke = false;

	/** the end of the last brush stroke, canvas px */
	let lastEnd = $state<{ x: number; y: number; p: number } | null>(null);
	let shiftHeld = $state(false);
	/** where the pointer is (canvas px) while Shift is held, for the line preview */
	let lineHover = $state<[number, number] | null>(null);

	/** Draw the straight part of a Shift+click stroke: from the last stroke's end to `e`. */
	function lineFromLast(e: PointerEvent) {
		if (!paint || !brush || !current || !lastEnd) return;
		const [bx, by] = toCanvas(e.clientX, e.clientY);
		// a pen's first contact is often very light: keep the last stroke's pressure then
		const raw = e.pointerType === 'mouse' || !penPressure ? 0.5 : applyCurve(e.pressure);
		const pb = raw < 0.05 ? lastEnd.p : raw;
		const len = Math.hypot(bx - lastEnd.x, by - lastEnd.y);
		const n = Math.max(1, Math.ceil(len / 2));
		const t = Math.round(e.timeStamp - t0);
		for (let k = 0; k <= n; k++) {
			const f = k / n;
			const x = lastEnd.x + (bx - lastEnd.x) * f;
			const y = lastEnd.y + (by - lastEnd.y) * f;
			const p = lastEnd.p + (pb - lastEnd.p) * f;
			paint.strokeTo(brush, x, y, p, 0, 0, 0.004);
			if (sendLive())
				livePts.push(
					Math.round(x * 10) / 10,
					Math.round(y * 10) / 10,
					Math.round(p * 1000) / 1000,
					4
				);
			current.points.push({
				x: Math.round(x * 10) / 10,
				y: Math.round(y * 10) / 10,
				pressure: Math.round(p * 1000) / 1000,
				t
			});
		}
		// carry on from the click as a normal stroke (drag on, or Shift+click again)
		smooth = { x: bx, y: by, p: pb };
		lastRaw = { x: bx, y: by, p: pb, t: e.timeStamp };
		lastT = e.timeStamp;
	}

	function feed(e: PointerEvent) {
		if (!paint || !brush || !current) return;
		const [cx, cy] = toCanvas(e.clientX, e.clientY);
		const raw: RawPoint = {
			x: cx,
			y: cy,
			p: e.pointerType === 'mouse' || !penPressure ? 0.5 : applyCurve(e.pressure),
			tx: penPressure ? (e.tiltX || 0) / 90 : 0,
			ty: penPressure ? (e.tiltY || 0) / 90 : 0,
			time: e.timeStamp
		};
		if (snapPending) {
			heldBack.push(raw);
			guided([cx, cy]); // decides once the stroke has gone far enough
			if (snapPending) return;
			releaseHeld();
			return;
		}
		feedPoint(raw);
	}

	/** draw the points that were held back (now that the direction is known, or the stroke ended) */
	function releaseHeld() {
		const pts = heldBack;
		heldBack = [];
		snapPending = null;
		for (const r of pts) feedPoint(r);
	}

	function feedPoint(r: RawPoint) {
		if (!paint || !brush || !current) return;
		const [rx, ry] = guided([r.x, r.y]);
		const rp = r.p;
		const a = follow();
		smooth = smooth
			? {
					x: smooth.x + (rx - smooth.x) * a,
					y: smooth.y + (ry - smooth.y) * a,
					p: smooth.p + (rp - smooth.p) * a
				}
			: { x: rx, y: ry, p: rp };
		lastRaw = { x: rx, y: ry, p: rp, t: r.time };
		const { x, y, p: pressure } = smooth;
		const dt = Math.max(0, (r.time - lastT) / 1000);
		lastT = r.time;
		paint.strokeTo(brush, x, y, pressure, r.tx, r.ty, dt);
		if (sendLive())
			livePts.push(
				Math.round(x * 10) / 10,
				Math.round(y * 10) / 10,
				Math.round(pressure * 1000) / 1000,
				Math.round(dt * 1000)
			);
		current.points.push({
			x: Math.round(x * 10) / 10,
			y: Math.round(y * 10) / 10,
			pressure: Math.round(pressure * 1000) / 1000,
			t: Math.round(r.time - t0)
		});
	}

	/** capture the pointer (some synthetic / stylus edge cases can't be captured: then just don't) */
	function capture(e: PointerEvent) {
		try {
			vp.setPointerCapture(e.pointerId);
		} catch {
			/* not capturable */
		}
	}

	function startPan(e: PointerEvent) {
		capture(e);
		panning = { id: e.pointerId, x: e.clientX, y: e.clientY, panX, panY };
	}

	function down(e: PointerEvent) {
		if (!paint || !brush) return;
		if ((e.target as HTMLElement).closest('.drop-tol')) return;
		dropFill = null;
		// the view toolbar sits on top of the canvas area: its buttons aren't strokes
		if ((e.target as HTMLElement).closest('.view')) return;
		if (e.pointerType === 'pen') penSeen = true;
		if (e.pointerType === 'touch') {
			touches.set(e.pointerId, { x: e.clientX, y: e.clientY });
			if (!tap)
				tap = {
					t0: e.timeStamp,
					max: 0,
					moved: false,
					starts: new Map(),
					view: { zoom, rot, panX, panY }
				};
			tap.starts.set(e.pointerId, { x: e.clientX, y: e.clientY });
			tap.max = Math.max(tap.max, touches.size);
			if (touches.size === 2) {
				// a second finger: this is a pinch, not a stroke
				if (current) {
					paint.cancelStroke();
					current = null;
					schedule();
					if (sendLive()) {
						flushLive();
						onlive?.({ type: 'cancel', k: liveK });
					}
				}
				panning = null;
				const g = twoTouches();
				gesture = { ...g, zoom, rot, panX, panY };
				gestureLatch = true;
				return;
			}
			if (touches.size > 2 || gestureLatch) return;
			if (fingers === 'move' || (fingers === 'auto' && penSeen)) {
				// with a pen around, a finger moves the view, or picks a colour when held still
				clearTimeout(holdTimer);
				const id = e.pointerId;
				holdTimer = setTimeout(() => {
					if (touches.size === 1 && touches.has(id) && !tap?.moved) {
						panning = null;
						const t = touches.get(id)!;
						const c = colorAt(t.x, t.y);
						if (c) onpick?.(c);
					}
				}, 450);
				return startPan(e);
			}
		}
		if (e.pointerType === 'mouse' && e.button === 1 && (e.ctrlKey || e.metaKey)) {
			e.preventDefault();
			capture(e);
			zoomGoal = null;
			zoomDrag = { id: e.pointerId, y: e.clientY, z: zoom, cx: e.clientX, cy: e.clientY };
			return;
		}
		if (e.pointerType === 'mouse' && e.button === 1) return startPan(e);
		if (spaceHeld) return startPan(e);
		if (e.pointerType === 'mouse' && e.button !== 0) return;
		// Ctrl+Alt + drag: resize the brush, watching its outline (as in Photoshop)
		if (tool === 'brush' && e.altKey && (e.ctrlKey || e.metaKey) && e.pointerType !== 'touch') {
			capture(e);
			sizeDrag = { id: e.pointerId, x: e.clientX, size, at: toCanvas(e.clientX, e.clientY) };
			return;
		}
		// editable shape: grab one of its points
		if (tool === 'brush' && editShape && !locked) {
			const p = toCanvas(e.clientX, e.clientY);
			const r = 14 / Math.max(0.01, fit * zoom);
			const i = shapeHandles(editShape).findIndex((q) => Math.hypot(p[0] - q[0], p[1] - q[1]) <= r);
			if (i >= 0) {
				capture(e);
				shapeDrag = { id: e.pointerId, index: i };
				return;
			}
		}
		// eyedropper: the tool, or Alt+click with the brush
		if (tool === 'picker' || (e.altKey && e.pointerType !== 'touch')) {
			capture(e);
			picking = e.pointerId;
			pick(e);
			return;
		}
		if (locked) return;
		if (tool === 'transform') {
			if (!xf) return;
			const p = toCanvas(e.clientX, e.clientY);
			const hit: { mode: NonNullable<typeof xfDrag>['mode']; index: number } = xfHit(p) ?? {
				mode: 'move',
				index: 0
			};
			// Distort (or Ctrl/Cmd + a corner, as in Photoshop): corners move on their own
			if (!xf.quad && !xf.mesh && (distort || e.ctrlKey || e.metaKey) && hit.mode !== 'rotate')
				xf = { ...xf, quad: xfCorners(xf) };
			capture(e);
			xfDrag = { ...hit, start: p, from: { ...xf } };
			return;
		}
		if (tool === 'guides') {
			guideDrag = guideHit(toCanvas(e.clientX, e.clientY));
			if (guideDrag) capture(e);
			return;
		}
		if (tool === 'liquify') {
			if (!paint.liquifyBegin()) {
				const l = shownLayers().find((x) => x.id === paint!.activeLayer);
				return onblocked?.(l && !l.visible ? 'hidden' : 'locked');
			}
			capture(e);
			const [x, y] = toCanvas(e.clientX, e.clientY);
			liq = { id: e.pointerId, x, y };
			return;
		}
		if (tool === 'gradient') {
			if (!paint.adjustBegin()) {
				const l = shownLayers().find((x) => x.id === paint!.activeLayer);
				return onblocked?.(l && !l.visible ? 'hidden' : 'locked');
			}
			capture(e);
			const p = toCanvas(e.clientX, e.clientY);
			grad = { id: e.pointerId, from: p, to: p };
			return;
		}
		if (tool === 'fill') return fillAt(e.clientX, e.clientY);
		if (tool === 'pen') return penDown(e);
		if (tool === 'select-wand') {
			const [cx, cy] = toCanvas(e.clientX, e.clientY);
			if (cx >= 0 && cy >= 0 && cx < width && cy < height) {
				const [x, y] = [Math.floor(cx), Math.floor(cy)];
				if (wandContiguous) paint.selectWand(x, y, wandTolerance, true, opOf(e));
				else paint.selectColourRange(x, y, wandTolerance, true, opOf(e));
			}
			return refreshAnts();
		}
		if (tool === 'measure') {
			capture(e);
			const p = toCanvas(e.clientX, e.clientY);
			measureSeg = [p, p];
			measuring = e.pointerId;
			return;
		}
		if (tool === 'lasso-fill') {
			const l = shownLayers().find((x) => x.id === paint!.activeLayer);
			if (l && (l.locked || !l.visible)) return onblocked?.(l.visible ? 'locked' : 'hidden');
			capture(e);
			lassoFill = [toCanvas(e.clientX, e.clientY)];
			return;
		}
		if (tool === 'select-rect' || tool === 'select-lasso') {
			capture(e);
			const p = toCanvas(e.clientX, e.clientY);
			marquee = { kind: tool === 'select-rect' ? 'rect' : 'lasso', pts: [p, p], op: opOf(e) };
			return;
		}
		if (adjustOverlay && overlaySrc) {
			capture(e);
			const [cx, cy] = toCanvas(e.clientX, e.clientY);
			moving = { id: e.pointerId, cx, cy };
			return;
		}
		if (!paint.beginStroke()) {
			const l = shownLayers().find((x) => x.id === paint!.activeLayer);
			onblocked?.(l && !l.visible ? 'hidden' : 'locked');
			return;
		}
		capture(e);
		editShape = null;
		current = { brushId, eraser, layer: paint.activeLayer, color, size, opacity, points: [] };
		lastT = e.timeStamp;
		smooth = null;
		lastRaw = null;
		startSnap(toCanvas(e.clientX, e.clientY));
		snapped = false;
		holdAnchor = null;
		clearTimeout(shapeTimer);
		maskStroke = paint.isEditingMask();
		if (sendLive()) {
			liveK++;
			onlive?.({
				type: 'begin',
				k: liveK,
				style: { brushId: sharedBrushId(brushId), color, size, opacity, eraser }
			});
		}
		lineStroke = false;
		if (e.shiftKey) {
			const p = toCanvas(e.clientX, e.clientY);
			if (lastEnd) {
				lineFromLast(e);
				lineHover = null;
				lineStroke = true;
			}
			// Shift + drag: the stroke goes straight, across, down or at 45° (as in Photoshop)
			snapLine = null;
			heldBack = [];
			snapPending = { start: p, dirs: STRAIGHT_DIRS };
		}
		feed(e);
		schedule();
	}

	let picking: number | null = null;

	function move(e: PointerEvent) {
		if (penMove(e)) return;
		if (sizeDrag && e.pointerId === sizeDrag.id) {
			// right = bigger; the slider's range is ¼× to about 6×
			const s = sizeDrag.size * Math.exp((e.clientX - sizeDrag.x) / 120);
			onsize?.(Math.round(Math.min(6.06, Math.max(0.25, s)) * 100) / 100);
			cursorAt = sizeDrag.at;
			return;
		}
		if (shapeDrag && e.pointerId === shapeDrag.id && editShape) {
			editShape = moveHandle(editShape, shapeDrag.index, toCanvas(e.clientX, e.clientY));
			redrawShape(editShape);
			return;
		}
		if (zoomDrag && e.pointerId === zoomDrag.id) {
			const want = zoomDrag.z * Math.exp((zoomDrag.y - e.clientY) * 0.006);
			zoomAt(want / zoom, zoomDrag.cx, zoomDrag.cy);
			return;
		}
		if (guideDrag) return guideMove(toCanvas(e.clientX, e.clientY));
		if (tool === 'liquify') hoverAt = toCanvas(e.clientX, e.clientY);
		lineHover =
			e.shiftKey && !current && lastEnd && tool === 'brush' && e.pointerType !== 'touch'
				? toCanvas(e.clientX, e.clientY)
				: null;
		cursorAt =
			tool === 'brush' && e.pointerType !== 'touch' && !adjustOverlay
				? toCanvas(e.clientX, e.clientY)
				: null;
		hoverHandle =
			editShape && cursorAt
				? shapeHandles(editShape).some(
						(q) =>
							Math.hypot(cursorAt![0] - q[0], cursorAt![1] - q[1]) <=
							14 / Math.max(0.01, fit * zoom)
					)
				: false;
		if (grad && e.pointerId === grad.id) {
			grad = { ...grad, to: toCanvas(e.clientX, e.clientY) };
			gradientShow();
			return;
		}
		if (liq && e.pointerId === liq.id && paint) {
			const events = e.getCoalescedEvents?.() ?? [];
			for (const ev of events.length ? events : [e]) {
				const [x, y] = toCanvas(ev.clientX, ev.clientY);
				paint.liquifyPush(liq.x, liq.y, x - liq.x, y - liq.y, liquifySize, liquifyStrength);
				liq = { ...liq, x, y };
			}
			schedule();
			return;
		}
		if (picking === e.pointerId) return pick(e);
		if (xfDrag) return xfMove(e);
		if (lassoFill) {
			lassoFill = [...lassoFill, toCanvas(e.clientX, e.clientY)];
			return;
		}
		if (measuring === e.pointerId && measureSeg) {
			measureSeg = [measureSeg[0], toCanvas(e.clientX, e.clientY)];
			return;
		}
		if (marquee) {
			const p = toCanvas(e.clientX, e.clientY);
			marquee =
				marquee.kind === 'rect'
					? { ...marquee, pts: [marquee.pts[0], p] }
					: { ...marquee, pts: [...marquee.pts, p] };
			return;
		}
		if (e.pointerType === 'touch' && touches.has(e.pointerId)) {
			touches.set(e.pointerId, { x: e.clientX, y: e.clientY });
			const st = tap?.starts.get(e.pointerId);
			if (tap && st && Math.hypot(e.clientX - st.x, e.clientY - st.y) > 12) {
				tap.moved = true;
				clearTimeout(holdTimer);
			}
			if (gesture && touches.size >= 2) {
				const g = twoTouches();
				const r = vp.getBoundingClientRect();
				const [cx, cy] = [r.left + r.width / 2, r.top + r.height / 2];
				zoom = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, (gesture.zoom * g.d) / gesture.d));
				const da = g.a - gesture.a;
				rot = gesture.rot + (da * 180) / Math.PI;
				// keep the canvas point under the first midpoint under the current one
				const f = zoom / gesture.zoom;
				const [ox, oy] = [gesture.mx - cx - gesture.panX, gesture.my - cy - gesture.panY];
				panX = g.mx - cx - f * (ox * Math.cos(da) - oy * Math.sin(da));
				panY = g.my - cy - f * (ox * Math.sin(da) + oy * Math.cos(da));
				return;
			}
		}
		if (moving && e.pointerId === moving.id) {
			const [cx, cy] = toCanvas(e.clientX, e.clientY);
			onoverlaymove?.((cx - moving.cx) / width, (cy - moving.cy) / height);
			moving = { ...moving, cx, cy };
			return;
		}
		if (panning && e.pointerId === panning.id) {
			panX = panning.panX + e.clientX - panning.x;
			panY = panning.panY + e.clientY - panning.y;
			clampPan();
			return;
		}
		if (!current) return;
		if (snapped) return; // the shape is set; lift the pen to finish
		const events = e.getCoalescedEvents?.() ?? [];
		for (const ev of events.length ? events : [e]) feed(ev);
		armShape(e);
		schedule();
	}

	function up(e: PointerEvent) {
		if (penDrag && e.pointerId === penDrag.id) {
			penDrag = null;
			return;
		}
		if (sizeDrag && e.pointerId === sizeDrag.id) {
			sizeDrag = null;
			return;
		}
		if (shapeDrag && e.pointerId === shapeDrag.id) {
			shapeDrag = null;
			notify();
			return;
		}
		if (zoomDrag && e.pointerId === zoomDrag.id) {
			zoomDrag = null;
			return;
		}
		if (guideDrag) {
			guideDrag = null;
			return;
		}
		if (liq && e.pointerId === liq.id && paint) {
			liq = null;
			changed(paint.liquifyEnd(), undefined, 'Liquify');
			return;
		}
		if (grad && e.pointerId === grad.id && paint) {
			const { from, to } = grad;
			grad = null;
			// a tap (no drag) paints nothing
			if (Math.hypot(to[0] - from[0], to[1] - from[1]) * fit * zoom < 4) paint.adjustCancel();
			else changed(paint.adjustCommit(), undefined, 'Gradient');
			schedule();
			return;
		}
		if (xfDrag) {
			xfDrag = null;
			return;
		}
		if (lassoFill) return commitLassoFill();
		if (measuring === e.pointerId) {
			measuring = null;
			return;
		}
		if (marquee) return commitMarquee();
		if (picking === e.pointerId) {
			picking = null;
			return;
		}
		if (e.pointerType === 'touch') {
			touches.delete(e.pointerId);
			clearTimeout(holdTimer);
			if (touches.size < 2) gesture = null;
			if (touches.size === 0) {
				gestureLatch = false;
				// a quick tap with two or three fingers that didn't move: undo / redo
				if (tapGestures && tap && !tap.moved && tap.max >= 2 && e.timeStamp - tap.t0 < 350) {
					({ zoom, rot, panX, panY } = tap.view);
					const g = tap.max === 2 ? 'undo' : 'redo';
					if (ongesture) ongesture(g);
					else if (g === 'undo') undo();
					else redo();
				}
				tap = null;
			}
		}
		if (panning && e.pointerId === panning.id) panning = null;
		if (moving && e.pointerId === moving.id) moving = null;
		clearTimeout(shapeTimer);
		if (!current || !paint || !brush) return;
		if (heldBack.length) releaseHeld(); // a short stroke that never picked a direction
		if (!snapped) catchUp();
		// a snapped shape stays editable (not on shared canvases: others already have it)
		editShape = snapped && !onlive && !maskStroke ? snappedShape : null;
		snapped = false;
		snappedShape = null;
		paint.endStroke(brush);
		const end = current.points.at(-1);
		if (end) lastEnd = { x: end.x, y: end.y, p: end.pressure };
		if (maskStroke) log.layerChange();
		else {
			log.add(current);
			if (lineStroke) log.labelLast('Straight line');
		}
		current = null;
		if (sendLive()) {
			flushLive();
			onlive?.({ type: 'end', k: liveK });
		}
		schedule();
		notify();
	}

	// Time's up mid-stroke: keep what was drawn so far.
	$effect(() => {
		if (locked && current && paint && brush) {
			paint.endStroke(brush);
			if (maskStroke) log.layerChange();
			else log.add(current);
			current = null;
			schedule();
			notify();
		}
	});

	// Ctrl/⌘ + wheel (and trackpad pinch) zooms at the pointer; the plain wheel pans once zoomed in.
	$effect(() => {
		const onWheel = (e: WheelEvent) => {
			if (e.ctrlKey || e.metaKey) {
				e.preventDefault();
				// lines / pages → pixels; a mouse wheel notch is ~100 px, a trackpad pinch a few px
				const px = e.deltaY * (e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1);
				const notch = Math.abs(px) >= 40 && Number.isInteger(px);
				const step = notch ? -Math.sign(px) * Math.log(1.12) : -px * 0.008;
				smoothZoom(step, e.clientX, e.clientY);
			} else if (zoom > 1.01) {
				e.preventDefault();
				panX -= e.deltaX;
				panY -= e.deltaY;
				clampPan();
			}
		};
		vp.addEventListener('wheel', onWheel, { passive: false });
		return () => vp.removeEventListener('wheel', onWheel);
	});

	function typing(t: EventTarget | null) {
		return t instanceof HTMLElement && !!t.closest('input, textarea, select, [contenteditable]');
	}
	function keydown(e: KeyboardEvent) {
		if (e.key === 'Shift') shiftHeld = true;
		if (e.code === 'Space' && hovering && !typing(e.target)) {
			e.preventDefault();
			spaceHeld = true;
		}
	}
	function keyup(e: KeyboardEvent) {
		if (e.key === 'Shift') {
			shiftHeld = false;
			lineHover = null;
		}
		if (e.code === 'Space') spaceHeld = false;
	}

	/** The history panel: go `steps` back (negative) or forward (positive) in one go. */
	export function historyGo(steps: number) {
		if (!paint) return;
		editShape = null;
		if (xf) transformCancel();
		for (let i = 0; i < Math.abs(steps); i++) {
			const ok = steps < 0 ? paint.undo() : paint.redo();
			if (!ok) break;
			if (steps < 0) log.undo();
			else log.redo();
		}
		schedule();
		notify();
	}

	export function undo() {
		editShape = null;
		if (xf) return transformCancel();
		if (paint?.undo()) log.undo();
		schedule();
		notify();
	}

	export function redo() {
		if (paint?.redo()) log.redo();
		schedule();
		notify();
	}

	// ---- drawings (animation cels) ----
	// Each layer shows one drawing; the others are kept by the engine. A timeline
	// says which drawing each layer shows on each frame (see the app's timeline).

	/** Show other drawings: [layer id, drawing id] pairs (drawing 0 = an empty cel). */
	export function showDrawings(pairs: [number, number][]): boolean {
		if (!paint) return false;
		editShape = null;
		const ok = paint.showDrawings(new Uint32Array(pairs.flat()));
		schedule();
		notify();
		return ok;
	}
	/** Give layers with paint but no drawing (a pasted picture) one of their own. */
	export function ensureDrawings() {
		if (!paint) return;
		const before = paint.layersJson();
		paint.ensureDrawings();
		if (paint.layersJson() !== before) notify();
	}
	/** A new, empty drawing's id. */
	export const newDrawing = () => paint?.newDrawing() ?? 0;
	/** A copy of a drawing; returns the copy's id. */
	export const copyDrawing = (drawing: number) => paint?.copyDrawing(drawing) ?? 0;
	/** A drawing's pixels (straight-alpha RGBA, canvas size). */
	export const drawingRgba = (drawing: number) =>
		paint?.drawingRgba(drawing) ?? new Uint8Array(width * height * 4);
	export const drawingEmpty = (drawing: number) => paint?.drawingEmpty(drawing) ?? true;
	/** Load a drawing's pixels (opening a project). */
	export function putDrawing(drawing: number, rgba: Uint8Array): boolean {
		const ok = paint?.putDrawing(drawing, rgba) ?? false;
		schedule();
		return ok;
	}
	/** Forget kept drawings not in `keep` (and not needed by undo). */
	export const dropDrawingsExcept = (keep: number[]) =>
		paint?.dropDrawingsExcept(new Uint32Array(keep));
	/** A drawing as packed file tiles (u32 count; per tile i32 x, i32 y, 64×64 RGBA8). */
	export const drawingTilesPacked = (drawing: number) =>
		paint?.drawingTilesPacked(drawing) ?? new Uint8Array(4);
	/** Load a drawing from packed file tiles (from the frame store). */
	export function putDrawingTiles(drawing: number, packed: Uint8Array): boolean {
		const ok = paint?.putDrawingTiles(drawing, packed) ?? false;
		schedule();
		return ok;
	}
	/** The engine holds this drawing's pixels (showing or kept). */
	export const hasDrawing = (drawing: number) => paint?.hasDrawing(drawing) ?? false;
	/** Drawings kept in the engine that aren't showing. */
	export const keptDrawings = (): number[] => [...(paint?.keptDrawings() ?? [])];
	/** Drawings the undo history needs (they stay in the engine). */
	export const historyDrawings = (): number[] => [...(paint?.historyDrawings() ?? [])];
	/** Let go of a kept drawing (the frame store has it). */
	export const forgetDrawing = (drawing: number) => paint?.forgetDrawing(drawing) ?? false;
	/**
	 * An animation frame as it would look, without changing the canvas: [layer, drawing]
	 * pairs (layers not named show what they show), made `shrink` times smaller.
	 */
	export function renderFrame(
		pairs: [number, number][],
		shrink = 1
	): { rgba: Uint8ClampedArray<ArrayBuffer>; width: number; height: number } | null {
		if (!paint) return null;
		const k = Math.max(1, Math.round(shrink));
		const rgba = paint.renderFrame(new Uint32Array(pairs.flat()), k);
		return {
			rgba: new Uint8ClampedArray(rgba.buffer as ArrayBuffer, rgba.byteOffset, rgba.length),
			width: Math.ceil(width / k),
			height: Math.ceil(height / k)
		};
	}
	/** The drawing the next undo / redo changes (0: none, or not a drawing). */
	export const undoDrawing = () => paint?.undoDrawing() ?? 0;
	export const redoDrawing = () => paint?.redoDrawing() ?? 0;

	/** Erase the active layer. */
	export function clear() {
		if (!paint) return;
		const layer = paint.activeLayer;
		const mask = paint.isEditingMask();
		changed(paint.clear(), () => (mask ? log.layerChange() : log.clear(layer)));
	}

	// ---- layers ----

	export function selectLayer(id: number) {
		if (paint?.setActiveLayer(id)) notify();
	}

	export function addLayer(name = '') {
		if (paint) changed(paint.addLayer(name) !== 0, undefined, 'New layer');
	}

	export function duplicateLayer(id: number) {
		if (paint) changed(paint.duplicateLayer(id) !== 0, undefined, 'Duplicate layer');
	}

	export function deleteLayer(id: number) {
		if (paint) changed(paint.deleteLayer(id), undefined, 'Delete layer');
	}

	/** `to`: 0 = bottom */
	export function moveLayer(id: number, to: number) {
		if (paint) changed(paint.moveLayer(id, to));
	}

	/** move a layer to `to` (0 = bottom) and into `group` (0 = none), one undo step */
	export function moveLayerTo(id: number, to: number, group: number) {
		if (paint) changed(paint.moveLayerTo(id, to, group));
	}

	// ---- layer groups ----

	export function addGroup(layer: number): number {
		if (!paint) return 0;
		const gid = paint.addGroup(layer);
		changed(gid !== 0);
		return gid;
	}

	function groupSettings(id: number): GroupSettings | null {
		const g = groupList().find((x) => x.id === id);
		return g ? { name: g.name, visible: g.visible, opacity: g.opacity } : null;
	}

	export function setGroup(id: number, patch: Partial<GroupSettings>) {
		const cur = paint && groupSettings(id);
		if (!cur) return;
		const s = { ...cur, ...patch };
		changed(paint!.setGroup(id, s.name, s.visible, s.opacity, true));
	}

	let previewingGroup: { id: number; before: GroupSettings } | null = null;

	/** live preview (the group opacity slider); finish with `commitGroup` */
	export function previewGroup(id: number, patch: Partial<GroupSettings>) {
		const cur = paint && groupSettings(id);
		if (!cur) return;
		if (previewingGroup?.id !== id) previewingGroup = { id, before: cur };
		const s = { ...cur, ...patch };
		paint!.setGroup(id, s.name, s.visible, s.opacity, false);
		schedule();
		notify();
	}

	export function commitGroup() {
		if (!paint || !previewingGroup) return;
		const { id, before: b } = previewingGroup;
		previewingGroup = null;
		changed(paint.commitGroup(id, b.name, b.visible, b.opacity));
	}

	export function ungroup(id: number) {
		if (paint) changed(paint.ungroup(id));
	}

	export function deleteGroup(id: number) {
		if (!paint) return;
		// strokes on deleted layers drop out at export (like deleting a layer)
		changed(paint.deleteGroup(id));
	}

	export function mergeGroup(id: number) {
		if (!paint) return;
		const members = layerList().filter((l) => l.group === id);
		const into = paint.mergeGroup(id);
		changed(into !== 0, () =>
			log.batch(() => {
				for (const l of members.slice(1).reverse()) log.merge(l.id, into);
			})
		);
	}

	export function moveGroup(id: number, up: boolean) {
		if (paint) changed(paint.moveGroup(id, up));
	}

	export function mergeDown(id: number) {
		if (!paint) return;
		const layers = layerList();
		const i = layers.findIndex((l) => l.id === id);
		const below = layers[i - 1]?.id;
		if (below === undefined) return;
		changed(paint.mergeDown(id), () => log.merge(id, below));
	}

	function settings(id: number): LayerSettings | null {
		const l = layerList().find((x) => x.id === id);
		if (!l) return null;
		const { name, visible, opacity, blend, locked, alphaLock, clip } = l;
		return { name, visible, opacity, blend, locked, alphaLock, clip: !!clip };
	}

	function setRaw(id: number, s: LayerSettings, record: boolean) {
		return paint!.setLayer(
			id,
			s.name,
			s.visible,
			s.opacity,
			s.blend,
			s.locked,
			s.alphaLock,
			s.clip,
			record
		);
	}

	/** Change layer settings as one undo step. */
	export function setLayer(id: number, patch: Partial<LayerSettings>) {
		const cur = paint && settings(id);
		if (!cur) return;
		changed(setRaw(id, { ...cur, ...patch }, true));
	}

	let previewing: { id: number; before: LayerSettings } | null = null;

	/** Live preview (e.g. while dragging the opacity slider); finish with `commitLayer`. */
	export function previewLayer(id: number, patch: Partial<LayerSettings>) {
		const cur = paint && settings(id);
		if (!cur) return;
		if (previewing?.id !== id) previewing = { id, before: cur };
		setRaw(id, { ...cur, ...patch }, false);
		schedule();
		notify();
	}

	export function commitLayer() {
		if (!paint || !previewing) return;
		const { id, before } = previewing;
		previewing = null;
		const b = before;
		changed(
			paint.commitLayer(id, b.name, b.visible, b.opacity, b.blend, b.locked, b.alphaLock, b.clip)
		);
	}

	// ---- layer masks (white shows, black hides) ----

	export function addMask(id: number) {
		if (paint) changed(paint.addMask(id));
	}
	export function deleteMask(id: number) {
		if (paint) changed(paint.deleteMask(id));
	}
	/** bake the mask into the layer */
	export function applyMask(id: number) {
		if (paint) changed(paint.applyMask(id));
	}
	/** paint on the mask (true) or the layer's colour; selects the layer */
	export function editMask(id: number, on: boolean) {
		if (!paint) return;
		paint.setMaskEditing(id, on);
		notify();
	}
	export function maskThumbnail(id: number, w: number, h: number): ImageData | null {
		if (!paint) return null;
		const px = paint.maskThumbnail(id, w, h);
		return px.length ? new ImageData(new Uint8ClampedArray(px), w, h) : null;
	}

	/** Straight-alpha preview of one layer. */
	export function layerThumbnail(id: number, w: number, h: number): ImageData | null {
		if (!paint) return null;
		return new ImageData(new Uint8ClampedArray(paint.layerThumbnail(id, w, h)), w, h);
	}

	/**
	 * The drawing as shown, minus the `exclude` layers (e.g. sketch-mode
	 * construction lines), with the strokes on the layers that remain.
	 */
	/**
	 * Everything needed for a layered file (Photoshop .psd): each layer's own
	 * pixels, masks, groups and settings, plus the flattened picture.
	 */
	/** One layer as the drawing history keeps it. */
	export interface LayerSnapshot {
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
		/** the layer's own pixels (straight-alpha RGBA, canvas size) */
		rgba: Uint8Array;
		/** greyscale mask, one byte per pixel (255 = shown) */
		mask?: Uint8Array;
		/** an adjustment layer's settings (JSON, see tone.ts) */
		adjust?: string | null;
	}

	/** A small PNG of the drawing as it shows (at most `max` px on its long side). */
	export async function thumbnail(max = 512): Promise<Uint8Array | null> {
		if (!ctx || !paint) return null;
		render();
		const k = Math.min(1, max / Math.max(width, height));
		const c = new OffscreenCanvas(
			Math.max(1, Math.round(width * k)),
			Math.max(1, Math.round(height * k))
		);
		const g = c.getContext('2d');
		if (!g) return null;
		g.drawImage(el, 0, 0, c.width, c.height);
		const blob = await c.convertToBlob({ type: 'image/png' });
		return new Uint8Array(await blob.arrayBuffer());
	}

	/** Everything needed to bring this drawing back later (bottom → top). */
	export function snapshotLayers(): {
		layers: LayerSnapshot[];
		groups: GroupInfo[];
		strokes: Stroke[];
	} | null {
		if (!paint) return null;
		const layers = layerList().map((l) => {
			let mask: Uint8Array | undefined;
			if (l.mask) {
				const m = paint!.maskThumbnail(l.id, width, height);
				mask = new Uint8Array(width * height);
				for (let i = 0; i < mask.length; i++) mask[i] = m[i * 4];
			}
			return {
				id: l.id,
				name: l.name,
				opacity: l.opacity,
				blend: l.blend,
				visible: l.visible,
				locked: l.locked,
				alphaLock: l.alphaLock,
				clip: !!l.clip,
				imported: !!l.imported,
				group: l.group,
				rgba: paint!.layerRgba(l.id),
				mask,
				adjust: l.adjust
			};
		});
		return { layers, groups: groupList(), strokes: structuredClone(log.strokes) as Stroke[] };
	}

	/**
	 * Replace this canvas's layers with saved ones (the drawing history): pixels,
	 * settings, masks, groups and the strokes, with nothing left to undo.
	 */
	export function restoreLayers(saved: LayerSnapshot[], groups: GroupInfo[], strokes: Stroke[]) {
		if (!paint || !saved.length) return false;
		const before = layerList().map((l) => l.id);
		paint.setActiveLayer(before.at(-1)!);
		// eslint-disable-next-line svelte/prefer-svelte-reactivity -- a local lookup, not state
		const ids = new Map<number, number>();
		for (const l of saved) {
			let id: number;
			if (l.adjust) {
				const spec = parseSpec(l.adjust);
				id = paint.addAdjustmentLayer(
					l.name,
					l.adjust,
					spec.hue,
					spec.saturation,
					spec.brightness,
					spec.contrast,
					toneTables(spec),
					mapTable(spec.map)
				);
			} else id = paint.restoreLayer(l.name, l.rgba, l.imported);
			if (!id) continue;
			ids.set(l.id, id);
			setRaw(
				id,
				{
					name: l.name,
					visible: l.visible,
					opacity: l.opacity,
					blend: l.blend as BlendMode,
					locked: l.locked,
					alphaLock: l.alphaLock,
					clip: l.clip
				},
				false
			);
			if (l.mask) paint.restoreMask(id, l.mask);
		}
		// groups: the first layer of each starts it, the rest join
		// eslint-disable-next-line svelte/prefer-svelte-reactivity -- a local lookup, not state
		const gids = new Map<number, number>();
		for (const l of saved) {
			const id = ids.get(l.id);
			if (!id || !l.group) continue;
			const g = groups.find((x) => x.id === l.group);
			const known = gids.get(l.group);
			if (known === undefined) {
				const gid = paint.addGroup(id);
				if (!gid) continue;
				gids.set(l.group, gid);
				if (g) paint.setGroup(gid, g.name, g.visible, g.opacity, false);
			} else {
				const at = layerList().findIndex((x) => x.id === id);
				paint.moveLayerTo(id, at, known);
			}
		}
		for (const id of before) paint.deleteLayer(id);
		log.reset(
			strokes
				.filter((st) => st.layer === undefined || ids.has(st.layer))
				.map((st) => (st.layer === undefined ? st : { ...st, layer: ids.get(st.layer)! }))
		);
		paint.clearHistory();
		schedule();
		notify();
		return true;
	}

	export function exportLayered(): PsdDoc | null {
		if (!paint || !ctx) return null;
		render();
		const groups = new Map(groupList().map((g) => [g.id, g]));
		const toLayer = (l: LayerInfo): PsdLayer => {
			const maskRgba = l.mask ? paint!.maskThumbnail(l.id, width, height) : null;
			let mask: Uint8Array | undefined;
			if (maskRgba && maskRgba.length) {
				mask = new Uint8Array(width * height);
				for (let i = 0; i < mask.length; i++) mask[i] = maskRgba[i * 4];
			}
			// Photoshop's own adjustment layers aren't written: the file's flat picture
			// shows the change, and the layer is kept (hidden, empty) under its name
			if (l.adjust)
				return {
					kind: 'layer',
					name: `${l.name} (${specName(parseSpec(l.adjust))}, not editable)`.slice(0, 60),
					rgba: new Uint8Array(width * height * 4),
					opacity: l.opacity,
					blend: l.blend,
					visible: false,
					clip: l.clip,
					mask
				};
			return {
				kind: 'layer',
				name: l.name,
				rgba: paint!.layerRgba(l.id),
				opacity: l.opacity,
				blend: l.blend,
				visible: l.visible,
				clip: l.clip,
				mask
			};
		};
		// a group's layers sit next to each other, so consecutive layers of one group form a folder
		const items: PsdDoc['items'] = [];
		let open = null as { id: number; folder: PsdGroup } | null;
		for (const l of layerList()) {
			const g = l.group ? groups.get(l.group) : undefined;
			if (!g) {
				open = null;
				items.push(toLayer(l));
			} else if (open?.id === g.id) open.folder.children.push(toLayer(l));
			else {
				const folder: PsdGroup = {
					kind: 'group',
					name: g.name,
					opacity: g.opacity,
					visible: g.visible,
					children: [toLayer(l)]
				};
				open = { id: g.id, folder };
				items.push(folder);
			}
		}
		return {
			width,
			height,
			composite: ctx.getImageData(0, 0, width, height).data,
			items
		};
	}
	export function exportDrawing(opts: { exclude?: number[] } = {}): DrawingExport | null {
		if (!ctx || !paint) return null;
		const hide = (opts.exclude ?? []).filter((id) => layerList().some((l) => l.id === id));
		const restore: [number, LayerSettings][] = [];
		for (const id of hide) {
			const s = settings(id)!;
			restore.push([id, s]);
			setRaw(id, { ...s, visible: false }, false);
		}
		render();
		const ids = visibleLayerIds();
		const out = {
			width,
			height,
			png: el.toDataURL('image/png'),
			gray: toGray(ctx.getImageData(0, 0, width, height).data),
			strokes: structuredClone(log.on(ids)),
			layers: shownLayers().filter((l) => ids.has(l.id))
		};
		// put the hidden layers back (in the same task, so nothing flickers)
		for (const [id, s] of restore) setRaw(id, s, false);
		if (restore.length) render();
		return out;
	}

	/** Forget undo/redo (after setting up starter layers). */
	export function clearHistory() {
		paint?.clearHistory();
		log.forgetHistory();
		notify();
	}
</script>

<svelte:window
	onkeydown={keydown}
	onkeyup={keyup}
	onblur={() => {
		spaceHeld = false;
		shiftHeld = false;
		lineHover = null;
	}}
/>

<div
	class="viewport"
	class:grab={spaceHeld}
	class:moving-overlay={adjustOverlay && !!overlaySrc}
	class:picker={tool === 'picker'}
	class:filling={tool === 'fill'}
	class:outlined={showOutline && !hoverHandle}
	class:on-handle={hoverHandle}
	class:selecting={tool.startsWith('select')}
	class:transforming={tool === 'transform'}
	class:liquifying={tool === 'liquify'}
	class:editing-guides={tool === 'guides'}
	bind:this={vp}
	style={fill ? 'height: 100%' : `aspect-ratio: ${width} / ${height}; max-height: ${maxHeight}`}
	onpointerdown={down}
	onpointermove={move}
	onpointerup={up}
	onpointercancel={up}
	onlostpointercapture={up}
	ondragover={dragOver}
	ondrop={drop}
	oncontextmenu={(e) => {
		if (!onquickmenu) return;
		e.preventDefault();
		// a long press with a finger started a stroke: it was for the menu instead
		if (current && paint) {
			paint.cancelStroke();
			current = null;
			schedule();
		}
		onquickmenu(e.clientX, e.clientY);
	}}
	onpointerenter={() => (hovering = true)}
	onpointerleave={() => {
		hovering = false;
		hoverAt = null;
		lineHover = null;
		cursorAt = null;
	}}
	role="presentation"
>
	<div class="sheet" style="width: {baseW}px; height: {baseH}px; transform: {sheetTransform}">
		<canvas
			bind:this={el}
			{width}
			{height}
			aria-label="Drawing canvas, {width} by {height} pixels"
			data-ready={ready}
			style:filter={viewFilter}
		></canvas>
		<canvas class="ants" bind:this={antsEl} {width} {height} aria-hidden="true"></canvas>
		{#if guides && (guideGrid.length || guideRays.length || guides.ruler.on || guideAxes.length)}
			{@const hr = 8 / Math.max(0.01, fit * zoom)}
			<svg
				class="guides"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
			>
				{#each guideGrid as [x1, y1, x2, y2], i (i)}
					<line class="grid" {x1} {y1} {x2} {y2} />
				{/each}
				{#each guideRays as [x1, y1, x2, y2], i (i)}
					<line class="ray" {x1} {y1} {x2} {y2} />
				{/each}
				{#if guides.perspective.points >= 1 && guides.perspective.points <= 2}
					{@const v = guides.perspective.vps}
					{@const [ax, ay] = v[0]}
					{@const [bx, by] = guides.perspective.points === 2 ? v[1] : [v[0][0] + 1, v[0][1]]}
					{@const k = (by - ay) / (bx - ax || 1e-6)}
					<line
						class="horizon"
						x1={-width}
						y1={ay - k * (ax + width)}
						x2={2 * width}
						y2={ay + k * (2 * width - ax)}
					/>
				{/if}
				{#each guideAxes as [x1, y1, x2, y2], i (i)}
					<line class="axis" {x1} {y1} {x2} {y2} />
				{/each}
				{#if guides.ruler.on}
					<line
						class="ruler"
						x1={guides.ruler.a[0]}
						y1={guides.ruler.a[1]}
						x2={guides.ruler.b[0]}
						y2={guides.ruler.b[1]}
					/>
				{/if}
				{#if tool === 'guides'}
					{#if guideAxes.length}
						<circle class="handle sym" cx={guides.symmetry.c[0]} cy={guides.symmetry.c[1]} r={hr} />
					{/if}
					{#each guides.perspective.vps.slice(0, guides.perspective.points) as [x, y], i (i)}
						<circle class="handle" cx={x} cy={y} r={hr} />
					{/each}
					{#if guides.ruler.on}
						<circle class="handle" cx={guides.ruler.a[0]} cy={guides.ruler.a[1]} r={hr} />
						<circle class="handle" cx={guides.ruler.b[0]} cy={guides.ruler.b[1]} r={hr} />
					{/if}
				{/if}
			</svg>
		{/if}
		{#if grad}
			{@const hr = 6 / Math.max(0.01, fit * zoom)}
			<svg class="xf" viewBox="0 0 {width} {height}" preserveAspectRatio="none" aria-hidden="true">
				<line class="grad" x1={grad.from[0]} y1={grad.from[1]} x2={grad.to[0]} y2={grad.to[1]} />
				<circle class="grad" cx={grad.from[0]} cy={grad.from[1]} r={hr} />
				<circle class="grad" cx={grad.to[0]} cy={grad.to[1]} r={hr} />
			</svg>
		{/if}
		{#if editShape && tool === 'brush'}
			{@const hr = 6 / Math.max(0.01, fit * zoom)}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="shape-handles"
			>
				{#each shapeHandles(editShape) as [x, y], i (i)}
					<circle class="shape-h" cx={x} cy={y} r={hr} />
				{/each}
			</svg>
		{/if}
		{#if xf?.mesh}
			{@const hr = 6 / Math.max(0.01, fit * zoom)}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="warp-grid"
			>
				{#each meshLines(xf.mesh) as line, i (i)}
					<polyline class="warp-line" points={line.map((p) => p.join(',')).join(' ')} />
				{/each}
				{#each xf.mesh as [x, y], i (i)}
					<circle class="shape-h" cx={x} cy={y} r={hr} />
				{/each}
			</svg>
		{/if}
		{#if angleGuide}
			{@const far = 2 * Math.hypot(width, height)}
			{@const r = (-angleGuide.deg * Math.PI) / 180}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="angle-guide"
			>
				<line
					class="angle-guide"
					x1={angleGuide.p[0] - Math.cos(r) * far}
					y1={angleGuide.p[1] - Math.sin(r) * far}
					x2={angleGuide.p[0] + Math.cos(r) * far}
					y2={angleGuide.p[1] + Math.sin(r) * far}
				/>
			</svg>
		{/if}
		{#if tool === 'pen' && penPath.anchors.length}
			{@const k = 1 / Math.max(0.01, fit * zoom)}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="pen-path"
				data-points={penPath.anchors.length}
				data-closed={penPath.closed}
			>
				<path d={pathD(penPath)} fill="none" stroke="#fff" stroke-width={4 * k} />
				<path d={pathD(penPath)} fill="none" stroke="#2f6bff" stroke-width={2 * k} />
				{#each penPath.anchors as a, i (i)}
					{#each [a.hin, a.hout] as hnd, j (j)}
						{#if hnd[0] !== a.p[0] || hnd[1] !== a.p[1]}
							<line
								x1={a.p[0]}
								y1={a.p[1]}
								x2={hnd[0]}
								y2={hnd[1]}
								stroke="#2f6bff"
								stroke-width={1.2 * k}
							/>
							<circle cx={hnd[0]} cy={hnd[1]} r={4 * k} fill="#2f6bff" />
						{/if}
					{/each}
					<rect
						x={a.p[0] - 5 * k}
						y={a.p[1] - 5 * k}
						width={10 * k}
						height={10 * k}
						fill={i === 0 && !penPath.closed ? '#2f6bff' : '#fff'}
						stroke="#2f6bff"
						stroke-width={1.5 * k}
					/>
				{/each}
			</svg>
		{/if}
		{#if measureSeg && tool === 'measure'}
			{@const [[ax, ay], [bx, by]] = measureSeg}
			{@const len = Math.hypot(bx - ax, by - ay)}
			{@const k = 1 / Math.max(0.01, fit * zoom)}
			{@const [ux, uy] = len ? [(bx - ax) / len, (by - ay) / len] : [1, 0]}
			{@const deg = ((((Math.atan2(-(by - ay), bx - ax) * 180) / Math.PI) % 180) + 180) % 180}
			{@const ticks = measureUnit > 0 ? Math.min(200, Math.floor(len / measureUnit)) : 0}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="measure"
			>
				<line class="measure" x1={ax} y1={ay} x2={bx} y2={by} />
				{#each Array.from({ length: ticks + 1 }, (_, k) => k) as i (i)}
					{@const [tx, ty] = [ax + ux * measureUnit * i, ay + uy * measureUnit * i]}
					<line
						class="measure"
						x1={tx - uy * 7 * k}
						y1={ty + ux * 7 * k}
						x2={tx + uy * 7 * k}
						y2={ty - ux * 7 * k}
					/>
				{/each}
			</svg>
			<span
				class="measure-label"
				role="status"
				style="left: {(bx / width) * 100}%; top: {(by / height) * 100}%; transform: scale({(flipped
					? -1
					: 1) / zoom}, {1 / zoom}) translate(10px, 10px)"
			>
				{measureUnit > 0 ? `${(len / measureUnit).toFixed(1)} units` : `${Math.round(len)} px`} · {Math.round(
					deg
				)}°
			</span>
		{/if}
		{#if lassoFill && lassoFill.length > 1}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="lasso-fill-preview"
			>
				<polygon
					class="lasso-fill"
					points={lassoFill.map((p) => p.join(',')).join(' ')}
					style="fill: {color}"
				/>
			</svg>
		{/if}
		{#if showOutline && cursorAt}
			{@const small = brushR * fit * zoom < 3}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="brush-cursor"
				data-r={brushR.toFixed(1)}
			>
				{#if !small}
					<circle class="cur-out" cx={cursorAt[0]} cy={cursorAt[1]} r={brushR} />
					<circle class="cur-in" cx={cursorAt[0]} cy={cursorAt[1]} r={brushR} />
				{/if}
				<circle
					class="cur-dot"
					cx={cursorAt[0]}
					cy={cursorAt[1]}
					r={1.5 / Math.max(0.01, fit * zoom)}
				/>
			</svg>
		{/if}
		{#if shiftHeld && lineHover && lastEnd}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="line-preview"
			>
				<line
					class="shift-line"
					x1={lastEnd.x}
					y1={lastEnd.y}
					x2={lineHover[0]}
					y2={lineHover[1]}
				/>
			</svg>
		{/if}
		{#if tool === 'liquify' && hoverAt}
			<svg class="xf" viewBox="0 0 {width} {height}" preserveAspectRatio="none" aria-hidden="true">
				<circle class="liq" cx={hoverAt[0]} cy={hoverAt[1]} r={liquifySize} />
			</svg>
		{/if}
		{#if xfShape && !xf?.mesh}
			{@const hr = 7 / Math.max(0.01, fit * zoom)}
			<svg class="xf" viewBox="0 0 {width} {height}" preserveAspectRatio="none" aria-hidden="true">
				<polygon points={xfShape.corners.map((p) => p.join(',')).join(' ')} />
				<line x1={xfShape.top[0]} y1={xfShape.top[1]} x2={xfShape.rotH[0]} y2={xfShape.rotH[1]} />
				{#each [...xfShape.corners, ...xfShape.edges] as [x, y], i (i)}
					<rect x={x - hr} y={y - hr} width={hr * 2} height={hr * 2} />
				{/each}
				<circle cx={xfShape.rotH[0]} cy={xfShape.rotH[1]} r={hr * 1.1} />
			</svg>
		{/if}
		{#if marquee}
			<svg
				class="marquee"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
			>
				{#if marquee.kind === 'rect'}
					{@const [[ax, ay], [bx, by]] = marquee.pts}
					<rect
						x={Math.min(ax, bx)}
						y={Math.min(ay, by)}
						width={Math.abs(bx - ax)}
						height={Math.abs(by - ay)}
					/>
				{:else}
					<polygon points={marquee.pts.map((p) => p.join(',')).join(' ')} />
				{/if}
			</svg>
		{/if}
		{#if overlaySrc}
			<div class="overlay-clip">
				<img
					class="overlay"
					src={overlaySrc}
					alt=""
					style="opacity: {overlayOpacity}; transform: translate({overlayTransform.x *
						100}%, {overlayTransform.y *
						100}%) rotate({overlayTransform.rotate}deg) scale({overlayTransform.scale})"
				/>
			</div>
		{/if}
	</div>
	{#if dropFill}
		<div
			class="drop-tol"
			role="group"
			aria-label="ColorDrop threshold"
			data-testid="colordrop-threshold"
			style="left: {dropFill.screen[0]}px; top: {dropFill.screen[1]}px"
		>
			<label>
				<span>Spread <b>{Math.round(dropFill.tol * 100)}%</b></span>
				<input
					type="range"
					min="0"
					max="0.8"
					step="0.02"
					value={dropFill.tol}
					onchange={(e) => refillDrop(+e.currentTarget.value)}
				/>
			</label>
			<button type="button" aria-label="Done" onclick={() => (dropFill = null)}>✓</button>
		</div>
	{/if}
	{#if viewControls && ready}
		<div class="view" role="toolbar" aria-label="View">
			<button type="button" aria-label="Zoom out" title="Zoom out (−)" onclick={zoomOut}>
				<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14" /></svg>
			</button>
			<button
				type="button"
				class="pct"
				title="Fit the whole canvas (0)"
				aria-label="Zoom {Math.round(zoom * 100)}%, reset view"
				onclick={resetView}
			>
				{Math.round(zoom * 100)}%
			</button>
			<button type="button" aria-label="Zoom in" title="Zoom in (+)" onclick={zoomIn}>
				<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
			</button>
			<button
				type="button"
				aria-label="Rotate left"
				title="Rotate left (4)"
				onclick={() => rotateBy(-15)}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path d="M4 4v6h6" /><path d="M4.5 15a8 8 0 1 0 1.9-8.3L4 10" />
				</svg>
			</button>
			<button
				type="button"
				aria-label="Rotate right"
				title="Rotate right (6)"
				onclick={() => rotateBy(15)}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path d="M20 4v6h-6" /><path d="M19.5 15a8 8 0 1 1-1.9-8.3L20 10" />
				</svg>
			</button>
			<button
				type="button"
				aria-label="Mirror view"
				aria-pressed={flipped}
				title="Mirror the view (M): fresh eyes for proportions"
				onclick={toggleMirror}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path d="M12 3v18M8 7 3 12l5 5V7ZM16 7l5 5-5 5V7Z" />
				</svg>
			</button>
			<button
				type="button"
				aria-label="Notan view, {notan ? `${notan} tones` : 'off'}"
				aria-pressed={!!notan}
				title="Notan: just 2 or 3 flat tones, to see the big light and dark shapes"
				onclick={() => (notan = notan === 0 ? 2 : notan === 2 ? 3 : 0)}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<rect x="4" y="4" width="16" height="16" rx="3" /><path
						d="M4 14 10 9l4 4 6-5v9a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3Z"
						class="half"
					/>
				</svg>
				{#if notan}<span class="tones">{notan}</span>{/if}
			</button>
			<button
				type="button"
				aria-label="Squint view"
				aria-pressed={squint}
				title="Squint: blur the canvas and the reference to compare the big shapes"
				onclick={() => (squint = !squint)}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path d="M2 12s4-6 10-6 10 6 10 6" /><path
						d="M4 15l-1.5 2M9 17l-.5 2.5M15 17l.5 2.5M20 15l1.5 2"
					/>
				</svg>
			</button>
			<button
				type="button"
				aria-label="Grayscale view"
				aria-pressed={grayView}
				title="Grayscale view: check light and dark without the colours"
				onclick={() => (grayView = !grayView)}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<circle cx="12" cy="12" r="8" /><path d="M12 4a8 8 0 0 1 0 16Z" class="half" />
				</svg>
			</button>
			{#if rot !== 0}
				<span class="angle" aria-live="polite">{Math.round(rot)}°</span>
			{/if}
		</div>
	{/if}
	<svg class="filters" aria-hidden="true">
		{#each [2, 3] as n (n)}
			<filter id="{fid}-{n}" color-interpolation-filters="sRGB">
				<feColorMatrix type="saturate" values="0" />
				<feComponentTransfer>
					<feFuncR type="discrete" tableValues={n === 2 ? '0 1' : '0 0.55 1'} />
					<feFuncG type="discrete" tableValues={n === 2 ? '0 1' : '0 0.55 1'} />
					<feFuncB type="discrete" tableValues={n === 2 ? '0 1' : '0 0.55 1'} />
				</feComponentTransfer>
			</filter>
		{/each}
	</svg>
	{#if !ready && !error}<p class="status">Warming up the brushes…</p>{/if}
	{#if error}<p class="status error" role="alert">{error}</p>{/if}
</div>

<style>
	.viewport {
		position: relative;
		width: 100%;
		overflow: hidden;
		touch-action: none;
		cursor: crosshair;
		border-radius: inherit;
		background: var(--pp-surface-sunk);
		user-select: none;
		-webkit-user-select: none;
	}
	.viewport.grab {
		cursor: grab;
	}
	.viewport.picker {
		cursor:
			url('data:image/svg+xml;utf8,<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="white" stroke="black" stroke-width="1.6"><path d="m2 22 1-4 9-9 3 3-9 9Z"/><path d="m14 7 3-3a2.1 2.1 0 0 1 3 3l-3 3"/></svg>')
				2 22,
			crosshair;
	}
	/* the brush outline (drawn in the canvas) replaces the pointer */
	.viewport.outlined {
		cursor: none;
	}
	.viewport.on-handle {
		cursor: grab;
	}
	.viewport.selecting {
		cursor: crosshair;
	}
	.ants,
	.marquee {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		pointer-events: none;
	}
	/* an overlay: it must never hide the drawing (the plain canvas rule paints paper white) */
	canvas.ants {
		background: transparent;
	}
	.marquee {
		fill: rgba(61, 107, 255, 0.12);
		stroke: #16161a;
		stroke-width: 1.5;
		stroke-dasharray: 6 4;
		vector-effect: non-scaling-stroke;
	}
	.viewport.editing-guides {
		cursor: grab;
	}
	.viewport.liquifying {
		cursor: none;
	}
	.viewport.transforming {
		cursor: move;
	}
	.xf {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		pointer-events: none;
		overflow: visible;
	}
	.xf polygon,
	.xf line {
		fill: none;
		stroke: #3d6bff;
		stroke-width: 1.5;
		vector-effect: non-scaling-stroke;
	}
	.xf line.shift-line {
		stroke-dasharray: 6 4;
	}

	.lasso-fill {
		opacity: 0.45;
		stroke: #16161a;
		stroke-width: 1.5;
		stroke-dasharray: 6 4;
		vector-effect: non-scaling-stroke;
	}
	.xf .cur-out,
	.xf .cur-in {
		fill: none;
		vector-effect: non-scaling-stroke;
	}
	.xf .cur-out {
		stroke: rgba(255, 255, 255, 0.9);
		stroke-width: 3;
	}
	.xf .cur-in {
		stroke: rgba(22, 22, 26, 0.85);
		stroke-width: 1.2;
	}
	.xf .cur-dot {
		fill: rgba(22, 22, 26, 0.85);
		stroke: #fff;
		stroke-width: 1;
		vector-effect: non-scaling-stroke;
	}
	.view svg .half {
		fill: currentColor;
	}
	.view .tones {
		font: 700 11px var(--pp-font-body);
		margin-left: 2px;
	}
	.filters {
		position: absolute;
		width: 0;
		height: 0;
	}
	.xf .shape-h {
		fill: #fff;
		stroke: #3d6bff;
		stroke-width: 2;
		vector-effect: non-scaling-stroke;
	}
	.xf .warp-line {
		fill: none;
		stroke: #3d6bff;
		stroke-width: 1;
		opacity: 0.8;
		vector-effect: non-scaling-stroke;
	}
	.xf .angle-guide {
		stroke: #e8492f;
		stroke-width: 1.5;
		stroke-dasharray: 8 5;
		vector-effect: non-scaling-stroke;
	}
	.xf .measure {
		stroke: #e8492f;
		stroke-width: 2;
		vector-effect: non-scaling-stroke;
	}
	.measure-label {
		position: absolute;
		transform-origin: 0 0;
		padding: 3px 8px;
		border-radius: 999px;
		background: #e8492f;
		color: #fff;
		font: 700 13px var(--pp-font-body);
		white-space: nowrap;
		pointer-events: none;
	}
	.guides {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		pointer-events: none;
		overflow: visible;
	}
	.guides line {
		vector-effect: non-scaling-stroke;
	}
	.guides .grid {
		stroke: rgba(61, 107, 255, 0.28);
		stroke-width: 1;
	}
	.guides .ray {
		stroke: rgba(232, 73, 47, 0.22);
		stroke-width: 1;
	}
	.guides .axis {
		stroke: rgba(139, 92, 246, 0.7);
		stroke-width: 1.5;
		stroke-dasharray: 8 6;
	}
	.guides .horizon {
		stroke: rgba(232, 73, 47, 0.6);
		stroke-width: 1.5;
	}
	.guides .ruler {
		stroke: rgba(22, 22, 26, 0.35);
		stroke-width: 10;
		stroke-linecap: round;
	}
	.guides .handle {
		fill: #fff;
		stroke: #e8492f;
		stroke-width: 2;
		vector-effect: non-scaling-stroke;
	}
	.xf .grad {
		fill: #fff;
		stroke: #16161a;
		stroke-width: 2;
		vector-effect: non-scaling-stroke;
	}
	.xf circle.liq {
		fill: none;
		stroke: #16161a;
		stroke-dasharray: 4 3;
	}
	.xf rect,
	.xf circle {
		fill: #fff;
		stroke: #3d6bff;
		stroke-width: 1.5;
		vector-effect: non-scaling-stroke;
	}
	.viewport.filling {
		cursor: cell;
	}
	.viewport.moving-overlay {
		cursor: move;
	}
	.sheet {
		overflow: visible;
	}
	.sheet {
		position: absolute;
		left: 50%;
		top: 50%;
		background: #fff;
		box-shadow: 0 1px 6px rgba(22, 22, 26, 0.12);
	}
	canvas {
		display: block;
		width: 100%;
		height: 100%;
		touch-action: none;
		background: #fff;
	}
	.overlay-clip {
		position: absolute;
		inset: 0;
		overflow: hidden;
		pointer-events: none;
	}
	.overlay {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		object-fit: contain;
		pointer-events: none;
		mix-blend-mode: multiply;
	}
	.drop-tol {
		position: absolute;
		z-index: 5;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 8px 10px;
		transform: translate(-50%, 16px);
		border-radius: 14px;
		background: #23232b;
		color: #fff;
		font: 600 13px var(--pp-font-body, sans-serif);
		box-shadow: 0 6px 18px rgba(0, 0, 0, 0.3);
	}
	.drop-tol label {
		display: grid;
		gap: 2px;
	}
	.drop-tol span {
		display: flex;
		justify-content: space-between;
		gap: 12px;
	}
	.drop-tol button {
		width: 36px;
		height: 36px;
		border: 0;
		border-radius: 50%;
		background: var(--pp-accent, #2f6bff);
		color: #fff;
		cursor: pointer;
	}
	.view {
		position: absolute;
		top: 10px;
		right: 10px;
		display: flex;
		align-items: center;
		gap: 2px;
		padding: 4px;
		border-radius: 999px;
		background: color-mix(in srgb, var(--pp-surface) 92%, transparent);
		box-shadow: 0 1px 4px rgba(22, 22, 26, 0.15);
	}
	.view button {
		display: grid;
		place-items: center;
		min-width: 32px;
		height: 32px;
		padding: 0 6px;
		border: 0;
		border-radius: 999px;
		background: transparent;
		color: var(--pp-ink);
		font: 600 12px var(--pp-font-body);
		cursor: pointer;
	}
	.view button[aria-pressed='true'] {
		background: var(--pp-ink);
		color: #fff;
	}
	.view svg {
		width: 16px;
		height: 16px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2.2;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.angle {
		padding: 0 8px 0 4px;
		font: 600 12px var(--pp-font-body);
		color: var(--pp-ink-2);
	}
	.status {
		position: absolute;
		inset: 0;
		margin: 0;
		display: grid;
		place-items: center;
		color: var(--pp-muted);
		font-weight: 600;
	}
	.error {
		color: var(--pp-accent-ink);
		padding: 24px;
		text-align: center;
	}
</style>
