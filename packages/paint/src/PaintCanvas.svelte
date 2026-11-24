<script lang="ts" module>
	import type { LayerStyle } from './layer-style';

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
		/** drawing assist: strokes on it snap to the guides (kept by the canvas, not the engine) */
		assist?: boolean;
		/** stroke, drop shadow and outer glow (layer-style.ts); null: none */
		style: LayerStyle | null;
		/** a text layer's text (JSON, see text.ts); null for other layers */
		text?: string | null;
		/** a line (vector) layer: its strokes stay editable (see lines.ts) */
		lines?: boolean;
	}
	export type LayerSettings = Omit<
		LayerInfo,
		| 'id'
		| 'empty'
		| 'mask'
		| 'editingMask'
		| 'group'
		| 'imported'
		| 'adjust'
		| 'drawing'
		| 'assist'
		| 'text'
		| 'lines'
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

	/** Someone else's pointer, drawing together (canvas pixels). */
	export interface RemoteCursor {
		id: string;
		name: string;
		/** CSS colour */
		color: string;
		x: number;
		y: number;
	}

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
	import { LineLayers, type LineEraser } from './line-layers.svelte';
	import {
		STRIDE,
		boilStrokes,
		bounds,
		decodeLineData,
		encodeLineData,
		encodeLines,
		newId,
		newSeed,
		linkedCopies,
		pickable,
		shownOf,
		transformStroke,
		type Affine,
		type LineStroke,
		type SymmetryMode
	} from './lines';
	import { recognise, resample } from './quickshape';
	import type { PsdDoc, PsdGroup, PsdLayer } from './psd';
	import { applyCurve } from './pressure';
	import { parseText, renderText, textJson, type TextSpec } from './text';
	import { customBrush, isCustom, onCustomBrushes } from './custom-brushes';
	/** other players don't have "My brushes": they see the brush it's based on */
	const sharedBrushId = (id: string) => (isCustom(id) ? (customBrush(id)?.base ?? id) : id);
	import {
		closestDirection,
		compositionFrame,
		compositionOn,
		compositionParts,
		edgePin,
		MAX_SCALE,
		MIN_SCALE,
		gridLines,
		perspectiveRays,
		project,
		rulerLine,
		shiftOffset,
		shiftPoint,
		snapAlong,
		snapAngle,
		snapDirections,
		snapToGrid,
		STRAIGHT_DIRS,
		symmetryAxes,
		SYMMETRY_MODES,
		type GuideSettings,
		type Pt
	} from './guides';
	import { toGray } from './pixels';
	import { boxToQuad, usableQuad, type Quad } from './homography';
	import { meshLines, meshNodes, WARP_N } from './warp';
	import {
		curvable,
		editable,
		moveHandle,
		perfect,
		shapeHandles,
		shapeName,
		shapePath,
		type EditShape
	} from './shape-edit';
	import { smudgeSource } from './smudge';
	import { INK_SNAP_PX, nearestInk, nearestPoint, strokeEnds } from './ink-snap';
	import { Stabilizer, stringLength } from './stabilizer';
	import { cloneBrushSource, cloneOffset } from './clone';
	import { textureById, type BrushTexture } from './brush-textures';
	import { Timelapse } from './timelapse';
	import Rulers from './Rulers.svelte';
	import CompositionShapes from './CompositionShapes.svelte';
	import {
		panAfterZoom,
		paintSize,
		showLength,
		VIEW_DEFAULTS,
		wheelZoomStep,
		type ViewPrefs
	} from './view-prefs';
	import { mapTable, parseSpec, specName, toneTables, type AdjustSpec } from './tone';
	import { hasStyle, styleNumbers } from './layer-style';
	import type { FilterSpec } from './filters';
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
		size: chosenSize = 1,
		opacity = 1,
		color = '#16161a',
		onchange,
		onblocked,
		overlaySrc = null,
		overlayOpacity = 0.3,
		onionImage = null,
		camera = null,
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
		closeFill = false,
		inkSnap = false,
		onshape,
		selectMode = 'replace',
		selectFill = false,
		selectShape = 'rect',
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
		wandAllLayers = true,
		cloneAligned = true,
		cloneAllLayers = false,
		cloneHeal = false,
		oncanvassize,
		onquickmenu,
		onpenpath,
		onfill,
		cursors = [],
		oncursor,
		ontext,
		textFrame = null,
		onrasterize,
		view,
		undoLimit = 60,
		lineEraser = 'normal',
		onlines,
		editableStrokes = 0,
		holdToPick = false
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
		/** onion skin / light table: tinted drawings over the canvas (from `onionRgba`) */
		onionImage?: ImageBitmap | null;
		/**
		 * an animation cut's camera (canvas pixels): outside it is dimmed, and its safe
		 * areas (action 90%, title 80%) are marked when `safe` is on
		 */
		camera?: { x: number; y: number; width: number; height: number; safe?: boolean } | null;
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
			| 'pen'
			| 'clone'
			| 'text'
			| 'lines';
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
		/**
		 * colour fill (Procreate's Color Fill): each selection made fills with the colour;
		 * shapes added to it, or another colour picked, change that fill until you
		 * deselect or change tools
		 */
		selectFill?: boolean;
		/** the rectangle select tool's shape */
		selectShape?: 'rect' | 'ellipse';
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
		/**
		 * close and fill: a stroke that ends where it began fills inside with its colour
		 * (not on shared canvases yet: others only get the stroke)
		 */
		closeFill?: boolean;
		/**
		 * snap to ink: a brush stroke's start and end, a pen point and a dragged line point go
		 * onto ink close by on the active layer (a stroke's end first, else the nearest inked pixel)
		 */
		inkSnap?: boolean;
		/** a stroke snapped to a shape */
		onshape?: (kind: string) => void;
		/** 0–1: steadies strokes (a pulled string, like Procreate's StreamLine) */
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
		/** magic wand and Select colour: look at what you see (false: the current layer only) */
		wandAllLayers?: boolean;
		/** the Clone tool: later strokes keep the first one's offset (false: each starts at the source) */
		cloneAligned?: boolean;
		/** the Clone tool copies what you see (false: the current layer only) */
		cloneAllLayers?: boolean;
		/** the Clone tool blends what it copies into the colours where it lands (a healing brush) */
		cloneHeal?: boolean;
		/** the text tool: a press, drag and let-go on the canvas (canvas px) */
		ontext?: (x: number, y: number, phase: 'down' | 'move' | 'up') => void;
		/** the text being typed: its box's corners (canvas px), framed on the canvas */
		textFrame?: [number, number][] | null;
		/**
		 * a tool was about to paint on a text layer: ask to rasterize it first (as
		 * Procreate does). Without it, painting rasterizes the text by itself.
		 */
		onrasterize?: (layer: number) => void;
		/** the canvas changed size (resized, or that undone): set `width` / `height` to this */
		oncanvassize?: (s: { width: number; height: number }) => void;
		/** right-click / long-press on the canvas (screen coordinates) */
		onquickmenu?: (x: number, y: number) => void;
		/** the pen tool's path changed: how many points, and whether it's closed */
		onpenpath?: (s: { points: number; closed: boolean }) => void;
		/** a fill was made with the fill tool, at canvas point x, y (for carrying it on) */
		onfill?: (f: { x: number; y: number; hex: string; tolerance: number }) => void;
		/** other people's cursors (drawing together), in canvas pixels */
		cursors?: RemoteCursor[];
		/** this person's pointer over the canvas, in canvas pixels (null: it left) */
		oncursor?: (at: [number, number] | null) => void;
		/** the app's preferences for the cursor, zoom, rulers, selection and guides */
		view?: Partial<ViewPrefs>;
		/** undo steps kept (10–500) */
		undoLimit?: number;
		/** the eraser on a line layer: removes whole strokes, or cuts them where it passes */
		lineEraser?: LineEraser;
		/** a drawing's line-layer strokes changed (drawing together shares them) */
		onlines?: (drawing: number) => void;
		/**
		 * strokes on a plain layer stay editable, this many per drawing (0: none): older
		 * ones become plain pixels (the studio sets it; elsewhere strokes stay plain)
		 */
		editableStrokes?: number;
		/** holding the brush still on a stroke for a moment picks it to edit */
		holdToPick?: boolean;
	} = $props();

	const prefs = $derived<ViewPrefs>({ ...VIEW_DEFAULTS, ...view });
	/** the size the brush paints at: capped, and smaller when zoomed in if it follows the zoom */
	const size = $derived.by(() =>
		paintSize(chosenSize, zoom, prefs.brushFollowsZoom, prefs.maxBrushSize)
	);

	let el: HTMLCanvasElement;
	let ctx: CanvasRenderingContext2D | null = null;
	let memory: WebAssembly.Memory | undefined;
	let paint: PaintCanvas | undefined;
	let brush: PaintBrush | undefined;
	let ready = $state(false);
	let error = $state('');

	const log = new StrokeLog();
	// (each stroke notes the drawing its layer showed: an animation's frame, for replays)
	log.tag = (st) => {
		if (paint && st.layer !== undefined && st.drawing === undefined) {
			const d = paint.layerDrawing(st.layer);
			if (d) st.drawing = d;
		}
	};
	const t0 = performance.now();

	// ---- line layers (see line-layers.svelte.ts): strokes that stay editable ----

	/** brushes for redrawing line strokes, by id (loaded as needed) */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- engine bookkeeping, never rendered
	const lineBrushes = new Map<string, PaintBrush>();
	const lines = new LineLayers({
		engine: () => paint,
		width: () => width,
		height: () => height,
		isLineLayer: (id) => isLineLayer(id),
		symmetryOn: () => !!guides?.symmetry && guides.symmetry.mode !== 'off',
		layerShowing: (d) => (d ? (layerList().find((l) => l.drawing === d)?.id ?? 0) : 0),
		brush: (id) => lineBrushes.get(id) ?? null,
		loadBrushes: async (ids) => {
			await Promise.all(
				ids.map(async (id) => {
					if (!lineBrushes.has(id)) lineBrushes.set(id, await remoteBrush(id));
				})
			);
		},
		dabStyle: (t) => setDabStyle(t ?? null),
		restoreDabStyle: () => setDabStyle(brushTexture),
		step: (label, undo, redo) => {
			log.layerChange();
			log.labelLast(label);
			log.attachLast({ undo, redo });
		},
		attach: (effect) => log.attachLast(effect),
		schedule: () => schedule(),
		notify: () => notify()
	});
	lines.onlines = (d) => onlines?.(d);

	/** a stroke's path for the selection overlay (SVG points) */
	function linePoints(s: LineStroke) {
		let out = '';
		// (a cut piece: the part it shows)
		const [from, to] = shownOf(s);
		for (let i = from * STRIDE; i + 1 < to * STRIDE; i += STRIDE)
			out += `${s.pts[i]},${s.pts[i + 1]} `;
		return out;
	}

	/** whether layer `id` is a line layer */
	function isLineLayer(id: number) {
		return !!layerList().find((l) => l.id === id)?.lines;
	}

	/** how many strokes stay editable on a plain layer (0: none) */
	const strokeLimit = () => Math.max(0, Math.min(2000, Math.round(editableStrokes)));

	/**
	 * Whether a brush stroke starting now on the active layer is kept editable: on a line
	 * layer always; on a plain one when the limit allows and what it paints can be drawn
	 * again exactly (not the eraser, nor clipped by a selection or alpha lock, nor a mask).
	 */
	function keepsStroke(): boolean {
		if (!paint || paint.isEditingMask()) return false;
		const l = layerList().find((x) => x.id === paint!.activeLayer);
		if (!l || l.adjust || l.text) return false;
		// the eraser over editable strokes: kept as a stroke too, so it cuts them exactly
		// (see `lines.erased`); with a selection or alpha lock it erases pixels as ever
		if (eraser)
			return (
				lineEraser === 'normal' &&
				hasStrokes(l.id) &&
				!l.alphaLock &&
				!paint.hasSelection() &&
				(l.lines || strokeLimit() > 0)
			);
		if (l.lines) return true;
		return strokeLimit() > 0 && !l.alphaLock && !paint.hasSelection();
	}

	/** the layer has strokes the Select line tool (or a pick) can take */
	const hasStrokes = (layer: number) => pickable(lines.strokesOf(layer)).length > 0;

	/** a stroke picked to edit from another tool (Transform, a held brush): it acts as Select line */
	let picking2 = $state(false);
	/** bumps when the active layer, the selection or mask editing change (see `bakeTainted`) */
	let strokeTick = $state(0);
	/** the Select line tool, or a stroke picked from another tool */
	const lineTool = $derived(tool === 'lines' || picking2);

	/** Edit memory: what editable strokes on plain layers take (points, and pixels under them). */
	export function strokeMemory() {
		return lines.memory();
	}

	// paint over editable strokes made them plain pixels: their part goes with that step
	let bakeQueued = false;
	/** what Move's stroke outlines depend on, as last seen (see `strokeTick`) */
	let strokeKey = '';
	function bakeTainted() {
		bakeQueued = false;
		// (outside any effect, and only when it changed: no loops)
		if (paint) {
			const key = `${paint.activeLayer}|${paint.hasSelection()}|${paint.isEditingMask()}`;
			if (key !== strokeKey) {
				strokeKey = key;
				strokeTick++;
			}
		}
		if (!paint || lines.busy) return;
		const t = paint.takeTainted();
		if (!t.length) return;
		const effect = lines.tainted(t);
		if (effect) log.attachLast(effect);
	}

	/** the live stroke on a line layer: its points as painted (`STRIDE` each) and its seed */
	let linePts: number[] | null = null;
	let lineSeed = 0;
	/** the QuickShape being edited is this line stroke (else: the last raster stroke) */
	let shapeOf: { layer: number; id: string } | null = null;
	/** a press of the Select line tool or the line eraser */
	let linePointer: number | null = null;
	/** where the Select line tool's drag is, waiting for the next frame */
	let lineMove: { layer: number; x: number; y: number; shift: boolean } | null = null;
	let lineMoveRaf = 0;
	function lineMoveNow() {
		cancelAnimationFrame(lineMoveRaf);
		lineMoveRaf = 0;
		const m = lineMove;
		lineMove = null;
		if (m) lines.move(m.layer, m.x, m.y, m.shift, guides ?? null, inkSnap ? inkReach() : 0);
	}

	/** one point of a stroke into the engine (and, on a line layer, into its points) */
	function strokeTo(x: number, y: number, p: number, tx: number, ty: number, dt: number) {
		if (linePts) {
			// exactly as the engine gets them (f32), so redrawing paints the same pixels
			[x, y, p, tx, ty, dt] = [x, y, p, tx, ty, dt].map(Math.fround);
			linePts.push(x, y, p, tx, ty, dt);
		}
		paint!.strokeTo(brush!, x, y, p, tx, ty, dt);
	}

	/** start the stroke again (a QuickShape snapped): a line stroke keeps its seed */
	function restartStroke(): boolean {
		if (!paint) return false;
		if (linePts) {
			linePts = [];
			return paint.beginLineStroke(lineSeed);
		}
		return paint.beginStroke();
	}

	/** What a stroke keeps of its brush's reach: how far it paints, and whether it smudges. */
	function brushReach(b: PaintBrush): Pick<LineStroke, 'reach' | 'smudge'> {
		const reach = Math.round(b.paintReach() * 100) / 100;
		return {
			...(reach > 2.5 ? { reach } : {}),
			...(b.smudgeReach() > 0 ? { smudge: true } : {})
		};
	}

	/** A line stroke ending: it and its symmetry copies, to join the drawing (`lines.added`). */
	function finishLineStroke(shape: EditShape | null) {
		const pts = linePts;
		linePts = null;
		if (!pts?.length || !brush) return null;
		const stroke: LineStroke = {
			id: newId(),
			pts: new Float32Array(pts),
			brush: brushId,
			color,
			size,
			opacity,
			radius: brush.radius(),
			seed: lineSeed,
			// how far it paints or picks paint up (redraws take that in)
			...brushReach(brush),
			...(brushTexture ? { texture: $state.snapshot(brushTexture) as typeof brushTexture } : {}),
			...(shape ? { shape } : {})
		};
		// the eraser kept as a stroke: an erase mark (never a shape)
		if (current?.eraser) {
			stroke.erase = true;
			delete stroke.shape;
		}
		const sym = guides?.symmetry;
		// with symmetry, the copies are linked: picking one picks them all, edits mirror
		const all =
			sym && sym.mode !== 'off'
				? linkedCopies(stroke, sym.mode as SymmetryMode, sym.n ?? 6, sym.c[0], sym.c[1])
				: [stroke];
		if (!lineBrushes.has(brushId)) void lines.ready([stroke]);
		return { strokes: all };
	}

	let current: Stroke | null = null;
	/** the pointer drawing `current` */
	let strokePointer = -1;
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
		const cloning = tool === 'clone';
		const strength = smudgeStrength;
		if (!ready) return;
		let cancelled = false;
		loadBrush(
			id,
			smudging ? (src) => smudgeSource(src, strength) : cloning ? cloneBrushSource : undefined
		)
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

	// undo steps kept, in the engine and the log alike
	$effect(() => {
		const n = Math.min(500, Math.max(10, Math.round(undoLimit)));
		if (!ready) return;
		untrack(() => {
			if (!paint) return;
			paint.setUndoLimit(n);
			log.setLimit(n);
			notify();
		});
	});

	function apply() {
		const [s, o, c, er] = [size, opacity, color, eraser]; // read props so the effect tracks them
		if (!brush) return;
		brush.setSizeScale(s);
		brush.setOpacity(o);
		const n = parseInt(c.slice(1), 16);
		brush.setColor((n >> 16) & 255, (n >> 8) & 255, n & 255);
		brush.setEraser(er && tool !== 'smudge' && tool !== 'clone');
		brushR = brush.radius();
		editShape = null;
	}

	/** Ctrl+Alt+drag resizing the brush */
	let sizeDrag: { id: number; x: number; size: number; at: [number, number] } | null = null;
	/** the last QuickShape, still editable (its points can be dragged) */
	let editShape = $state<EditShape | null>(null);
	let shapeDrag: { id: number; index: number; from: Pt } | null = null;
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
		// a line stroke's shape: the stroke is drawn along it (live while a handle drags)
		if (shapeOf) return lines.reshape(shapeOf.layer, shapeOf.id, sh, !!shapeDrag);
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
	/** a stroke is being drawn (the cursor can hide meanwhile) */
	let strokeDown = $state(false);

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
		// (snap to ink reads the layer again: it changed)
		if (!current) inkCache = null;
		if (!bakeQueued) {
			bakeQueued = true;
			queueMicrotask(bakeTainted);
		}
		if (paint.width !== width || paint.height !== height)
			oncanvassize?.({ width: paint.width, height: paint.height });
		const layers = layerList().map((l) => ({ ...l, assist: assistLayers.has(l.id) }));
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

	/** layers with drawing assist on (Procreate keeps it per layer) */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- the status carries it out
	const assistLayers = new Set<number>();

	/** Drawing assist on a layer: its strokes snap to the guides. Not an undo step (a drawing aid). */
	export function setLayerAssist(id: number, on: boolean) {
		if (on) assistLayers.add(id);
		else assistLayers.delete(id);
		notify();
	}

	/** the stroke in progress paints a layer mask (not shared, not in the stroke log) */
	let maskStroke = false;
	/** the stroke in progress is a clone stroke (others get its pixels, not a live preview) */
	let cloneStroke = false;
	const sendLive = () => !!onlive && !maskStroke && !cloneStroke;

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

	/**
	 * "Drawn live" films: logged strokes drawn again, a piece at a time, on silent layers
	 * of this canvas (one per original layer, made bottom → top in the order given; no
	 * undo). Use it on a canvas made for the film, not the document.
	 */
	export async function strokePlayer(strokes: Stroke[], layers: { id: number; name: string }[]) {
		if (!paint) return null;
		const engine = paint;
		// eslint-disable-next-line svelte/prefer-svelte-reactivity -- engine bookkeeping, never rendered
		const ids = new Map<number, number>();
		for (const l of layers) ids.set(l.id, engine.addLayerSilent(l.name));
		const fallback = layers.length ? ids.get(layers[0].id)! : engine.activeLayer;
		const brushes = await Promise.all(strokes.map((st) => remoteBrush(st.brushId)));
		// eslint-disable-next-line svelte/prefer-svelte-reactivity -- engine bookkeeping, never rendered
		const keys = new Map<number, number>();
		/** how far each stroke has been drawn (points) */
		const done = strokes.map(() => 0);
		const style = (st: Stroke): Style => ({
			brushId: st.brushId,
			color: st.color ?? '#16161a',
			size: st.size ?? 1,
			opacity: st.opacity ?? 1,
			eraser: st.eraser
		});
		return {
			/** Draw stroke `i` up to (not including) point `n`; `n` past its end finishes it. */
			drawTo(i: number, n: number) {
				const st = strokes[i];
				// (a clone stroke copies the picture: it arrives with the finished frame)
				if (!st || done[i] < 0 || st.clone) return;
				const b = styled(brushes[i], style(st));
				let key = keys.get(i);
				if (key === undefined) {
					key = ++remoteKeys;
					keys.set(i, key);
					engine.remoteBegin(key, ids.get(st.layer ?? -1) ?? fallback);
				}
				const to = Math.min(n, st.points.length);
				for (let j = done[i]; j < to; j++) {
					const p = st.points[j];
					engine.remoteTo(key, b, p.x, p.y, p.pressure, p.t / 1000);
				}
				done[i] = to;
				if (n >= st.points.length) {
					engine.remoteEnd(key, b);
					done[i] = -1;
				}
			}
		};
	}

	/** Erase someone's layer of live strokes (their real pixels arrived elsewhere). */
	export function remoteClear(playerId: string) {
		const id = remoteLayers.get(playerId);
		if (!paint || !id) return;
		paint.clearLayerSilent(id);
		schedule();
	}
	/** Forget someone's layer of live strokes (it's gone from the canvas). */
	export function remoteForget(playerId: string) {
		remoteLayers.delete(playerId);
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

	// ---- onion skin: the picture the page made, drawn over the canvas ----
	let onionEl = $state<HTMLCanvasElement>();
	$effect(() => {
		const img = onionImage;
		const el = onionEl;
		if (!img || !el) return;
		[el.width, el.height] = [img.width, img.height];
		const g = el.getContext('2d');
		g?.clearRect(0, 0, img.width, img.height);
		g?.drawImage(img, 0, 0);
	});

	// ---- timelapse: a frame after every change ----

	const film = new Timelapse();
	/** the canvas changed since the last timelapse frame */
	let filmDirty = false;
	/** The timelapse of this drawing so far (frames after each change). */
	export function timelapse(): Timelapse {
		return film;
	}
	/** Every stroke the drawing has now (a copy), oldest first. */
	export const loggedStrokes = (): Stroke[] => structuredClone([...log.strokes]);
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
	/** how the engine's next strokes are textured (null: plain) */
	function setDabStyle(t: BrushTexture | null) {
		if (!paint) return;
		if (!t) return paint.setDabStyle(0, 1, 0, 0, 0, 0, 0, 1);
		paint.setDabStyle(
			engineTexture(t.grain),
			t.grainScale,
			t.grainStrength,
			engineTexture(t.tip),
			engineTexture(t.dual),
			t.dualSize,
			t.dualScatter,
			t.dualCount
		);
	}
	$effect(() => {
		const t = brushTexture;
		if (!ready || !paint) return;
		untrack(() => setDabStyle(t));
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
			panX = panAfterZoom(panX, k, clientX - (r.left + r.width / 2));
			panY = panAfterZoom(panY, k, clientY - (r.top + r.height / 2));
		} else {
			panX = panAfterZoom(panX, k);
			panY = panAfterZoom(panY, k);
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
		// a fifth of the way each frame: an even glide, not a jump
		zoomAt(Math.exp(done ? left : left * 0.2), zoomGoal.x, zoomGoal.y);
		if (done) zoomGoal = null;
		else zoomRaf = requestAnimationFrame(zoomFrame);
	}
	/** Zoom by a step (a change of log(zoom)): gliding or at once, as the preferences say. */
	function zoomBy(logStep: number, clientX?: number, clientY?: number) {
		if (prefs.smoothZoom) smoothZoom(logStep, clientX, clientY);
		else {
			zoomGoal = null;
			zoomAt(Math.exp(logStep), clientX, clientY);
		}
	}
	export function zoomIn() {
		zoomBy(Math.log(1.2));
	}
	export function zoomOut() {
		zoomBy(-Math.log(1.2));
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
	/** Ctrl (⌘) held over the canvas: a drag moves the view */
	let ctrlHeld = $state(false);
	/** the outline only belongs to the brush (it would linger after switching tools) */
	const showOutline = $derived(
		!!cursorAt &&
			brushR > 0 &&
			(tool === 'brush' || tool === 'clone') &&
			!spaceHeld &&
			!adjustOverlay &&
			!locked
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
		const done = fillAtCanvas(cx, cy, hex, fillTolerance);
		if (done) onfill?.({ x: Math.floor(cx), y: Math.floor(cy), hex, tolerance: fillTolerance });
		return done;
	}

	/** Fill at canvas point x, y on the active layer, as the fill tool does (an undo step). */
	export const fillPoint = (x: number, y: number, hex: string, tolerance = fillTolerance) =>
		fillAtCanvas(x, y, hex, tolerance);

	/** Make the last two undo steps one (a fill carried across frames, say). */
	export function joinLastSteps() {
		if (!paint?.joinLastEdits()) return false;
		log.joinLast();
		notify();
		return true;
	}

	function fillAtCanvas(cx: number, cy: number, hex: string, tolerance: number): boolean {
		if (!paint) return false;
		if (cx < 0 || cy < 0 || cx >= width || cy >= height) return false;
		const l = shownLayers().find((x) => x.id === paint!.activeLayer);
		// a line layer keeps only lines: the colour goes on the layer under it, inside its lines
		if (l?.lines && !l.editingMask) return fillUnderLines(l, cx, cy, hex, tolerance);
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
	 * Fill on a line layer (as Clip Studio fills with the lines as reference): the shape
	 * is found on the line layer, the colour goes on the plain layer right under it (a
	 * new "Colour" layer if there's none), and the line layer stays the one drawn on.
	 * One undo step, the new layer included.
	 */
	function fillUnderLines(
		line: LayerInfo,
		cx: number,
		cy: number,
		hex: string,
		tolerance: number
	): boolean {
		if (!paint) return false;
		const all = shownLayers();
		const i = all.findIndex((x) => x.id === line.id);
		const below = all[i - 1];
		const usable =
			below &&
			below.group === line.group &&
			!below.lines &&
			!below.text &&
			!below.adjust &&
			!below.locked &&
			below.visible;
		let target = usable ? below.id : 0;
		let made = 0;
		if (!target) {
			made = paint.addLayer('Colour');
			if (!made) return false;
			log.layerChange();
			// (made above the lines: moved under them)
			if (paint.moveLayer(made, i)) {
				log.layerChange();
				if (paint.joinLastEdits()) log.joinLast();
			}
			target = made;
		}
		paint.setActiveLayer(target);
		const n = parseInt(hex.slice(1), 16) || 0;
		const done = paint.fill(
			Math.floor(cx),
			Math.floor(cy),
			(n >> 16) & 255,
			(n >> 8) & 255,
			n & 255,
			opacity,
			tolerance,
			false,
			2,
			3,
			line.id
		);
		paint.setActiveLayer(line.id);
		if (done) {
			log.layerChange();
			if (made && paint.joinLastEdits()) log.joinLast();
			log.labelLast('Fill');
		} else if (made && paint.undo()) log.undo();
		return changed(done, () => {}, undefined);
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
	/**
	 * ColorDrop at a client position: into the selection (dropped inside it) or a
	 * fill there, as the Fill tool does (its reference layer too), with the spread
	 * slider to follow.
	 */
	function dropAt(clientX: number, clientY: number, hex: string) {
		if (!paint) return;
		const at = toCanvas(clientX, clientY);
		const [x, y] = [Math.floor(at[0]), Math.floor(at[1])];
		if (x < 0 || y < 0 || x >= width || y >= height) return;
		const sel = paint.selectionMask();
		if (sel.length && sel[y * width + x] >= 128) {
			const l = shownLayers().find((q) => q.id === paint!.activeLayer);
			if (l && (l.locked || !l.visible)) return onblocked?.(l.visible ? 'locked' : 'hidden');
			const [r, g, b] = rgbOf(hex);
			changed(paint.fillSelection(r, g, b, opacity), undefined, 'Fill selection');
			return;
		}
		if (fillAtCanvas(at[0], at[1], hex, fillTolerance)) {
			const r = vp.getBoundingClientRect();
			dropFill = { at, screen: [clientX - r.left, clientY - r.top], hex, tol: fillTolerance };
			keepDropFill();
		}
	}

	/** after a ColorDrop, "Continue filling": taps fill with this colour until another tool */
	let dropContinue = $state<string | null>(null);
	$effect(() => {
		void tool;
		untrack(() => (dropContinue = null));
	});

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
		dropAt(e.clientX, e.clientY, hex);
	}

	// ---- QuickShape: hold still at the end of a stroke ----

	const HOLD_MS = 550;
	let shapeTimer: ReturnType<typeof setTimeout> | undefined;
	let holdAnchor: { x: number; y: number } | null = null;
	let snapped = false;
	/** the shape the stroke in progress snapped to (editable once the pen lifts) */
	let snappedShape: EditShape | null = null;

	function armShape(e: PointerEvent) {
		if (!quickShape || snapped || !current || current.clone) return;
		if (holdAnchor && Math.hypot(e.clientX - holdAnchor.x, e.clientY - holdAnchor.y) < 4) return;
		holdAnchor = { x: e.clientX, y: e.clientY };
		clearTimeout(shapeTimer);
		shapeTimer = setTimeout(snapShape, HOLD_MS);
	}

	function snapShape() {
		if (!paint || !brush || !current || snapped) return;
		snapHint = null;
		// a held pen: a smoothed line catches up first, so the shape reaches the pen
		catchUp();
		const pts = current.points.map((p) => [p.x, p.y] as [number, number]);
		const shape = recognise(pts);
		if (!shape) return;
		const pressure = current.points.reduce((s, p) => s + p.pressure, 0) / current.points.length;
		snappedShape = editable(shape, pressure);
		// curved last time: shapes with corners come out curved again
		if (curveShapes && curvable(snappedShape) && snappedShape.kind === 'poly')
			snappedShape = { ...snappedShape, smooth: true };
		const path = resample(shapePath(snappedShape), 3);
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
		if (!restartStroke()) return;
		const t = current.points.at(-1)?.t ?? 0;
		current.points = [];
		for (const [x, y] of path) {
			strokeTo(x, y, pressure, 0, 0, 0.008);
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
		// Shift already down when it snaps: perfect straight away
		if (shiftHeld) perfectSnapped();
	}

	/**
	 * Still holding after a snap: another finger's tap (or Shift) makes it perfect, as
	 * in Procreate: a circle, a square, an even triangle, a line at a 15° step.
	 */
	function perfectSnapped() {
		if (!paint || !brush || !current || !snapped || !snappedShape) return;
		const sh = perfect(snappedShape);
		snappedShape = sh;
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
		if (!restartStroke()) return;
		const t = current.points.at(-1)?.t ?? 0;
		current.points = [];
		for (const [x, y] of resample(shapePath(sh), 3)) {
			strokeTo(x, y, sh.pressure, 0, 0, 0.008);
			current.points.push({
				x: Math.round(x * 10) / 10,
				y: Math.round(y * 10) / 10,
				pressure: sh.pressure,
				t
			});
			if (sendLive())
				livePts.push(
					Math.round(x * 10) / 10,
					Math.round(y * 10) / 10,
					Math.round(sh.pressure * 1000) / 1000,
					8
				);
		}
		schedule();
		onshape?.(shapeName(sh));
	}

	/** The Edit shape bar, moved out of the way by its grip (kept); 0, 0: top centre. */
	let shapeBarOff = $state(readBarOff());
	let shapeBarEl: HTMLDivElement | undefined = $state();
	function readBarOff(): { dx: number; dy: number } {
		try {
			const v = JSON.parse(localStorage.getItem('pp.shapeBarAt') ?? 'null');
			if (v && Number.isFinite(v.dx) && Number.isFinite(v.dy)) return { dx: v.dx, dy: v.dy };
		} catch {
			/* none kept */
		}
		return { dx: 0, dy: 0 };
	}
	/** Move the bar by an offset from its home, kept inside the canvas area. */
	function placeShapeBar(dx: number, dy: number, keep = true) {
		const el = shapeBarEl;
		const box = el?.offsetParent as HTMLElement | null | undefined;
		if (el && box) {
			const b = box.getBoundingClientRect();
			const r = el.getBoundingClientRect();
			// where it would be at home (no offset)
			const left = r.left - shapeBarOff.dx;
			const top = r.top - shapeBarOff.dy;
			dx = Math.min(Math.max(dx, b.left + 4 - left), b.right - 4 - (left + r.width));
			dy = Math.min(Math.max(dy, b.top + 4 - top), b.bottom - 4 - (top + r.height));
		}
		shapeBarOff = { dx: Math.round(dx), dy: Math.round(dy) };
		if (keep)
			try {
				localStorage.setItem('pp.shapeBarAt', JSON.stringify(shapeBarOff));
			} catch {
				/* storage blocked: kept for this visit */
			}
	}
	function dragShapeBar(e: PointerEvent) {
		if (e.button !== 0) return;
		e.preventDefault();
		e.stopPropagation();
		const grip = e.currentTarget as HTMLElement;
		grip.setPointerCapture(e.pointerId);
		const from = { x: e.clientX, y: e.clientY, ...shapeBarOff };
		const move = (m: PointerEvent) =>
			placeShapeBar(from.dx + m.clientX - from.x, from.dy + m.clientY - from.y, false);
		const up = () => {
			grip.removeEventListener('pointermove', move);
			grip.removeEventListener('pointerup', up);
			grip.removeEventListener('pointercancel', up);
			placeShapeBar(shapeBarOff.dx, shapeBarOff.dy);
		};
		grip.addEventListener('pointermove', move);
		grip.addEventListener('pointerup', up);
		grip.addEventListener('pointercancel', up);
	}
	function keyShapeBar(e: KeyboardEvent) {
		const step = e.shiftKey ? 48 : 12;
		const d = {
			ArrowLeft: [-step, 0],
			ArrowRight: [step, 0],
			ArrowUp: [0, -step],
			ArrowDown: [0, step]
		}[e.key];
		if (!d) return;
		e.preventDefault();
		e.stopPropagation();
		placeShapeBar(shapeBarOff.dx + d[0], shapeBarOff.dy + d[1]);
	}

	/** Snapped shapes with corners: straight lines between them, or a smooth curve (kept). */
	let curveShapes = $state(readCurve());
	function readCurve() {
		try {
			return localStorage.getItem('pp.shapeCurve') === '1';
		} catch {
			return false;
		}
	}
	export function setShapeCurve(on: boolean) {
		curveShapes = on;
		try {
			localStorage.setItem('pp.shapeCurve', on ? '1' : '0');
		} catch {
			/* storage blocked: kept for this visit */
		}
		const sh = editShape;
		if (!sh || sh.kind !== 'poly' || !curvable(sh) || !!sh.smooth === on) return;
		editShape = { ...sh, smooth: on };
		redrawShape(editShape);
		notify();
	}

	/** Edit Shape after the pen lifts: make the snapped shape perfect (or loose again). */
	export function setShapeForm(form: 'perfect' | 'free') {
		const sh = editShape;
		if (!sh) return;
		if (form === 'perfect') editShape = perfect(sh);
		else if (sh.kind === 'ellipse') editShape = { ...sh, circle: false };
		else return;
		if (editShape !== sh) redrawShape(editShape);
		notify();
	}

	// ---- selections ----

	/** a rectangle, ellipse or lasso being dragged (canvas px), shown live */
	let marquee = $state<{
		kind: 'rect' | 'ellipse' | 'lasso';
		pts: [number, number][];
		op: number;
		/** where the press was (a box's first corner before Shift puts it on the grid) */
		from: Pt;
		/** Shift shapes the box (not while the Shift that began it, adding, is still down) */
		shapes: boolean;
	} | null>(null);
	/** a lasso placed point by point (click, click…; click the first point or Enter to close) */
	let poly = $state<{ pts: [number, number][]; op: number; at: [number, number] | null } | null>(
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
		drawTint(m);
		notify();
	}

	let tintEl: HTMLCanvasElement | undefined = $state();
	/** The area outside the selection, tinted as strongly as the preferences say. */
	function drawTint(m = paint?.selectionMask()) {
		const c = tintEl?.getContext('2d');
		if (!c) return;
		c.clearRect(0, 0, width, height);
		const k = prefs.selectionTint;
		if (!m?.length || k <= 0) return;
		const img = c.createImageData(width, height);
		for (let i = 0; i < m.length; i++) {
			img.data[i * 4] = 22;
			img.data[i * 4 + 1] = 22;
			img.data[i * 4 + 2] = 26;
			img.data[i * 4 + 3] = Math.round((255 - m[i]) * k);
		}
		c.putImageData(img, 0, 0);
	}
	$effect(() => {
		void prefs.selectionTint;
		if (ready) untrack(() => drawTint());
	});

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
			if (!antsEdges.length || !prefs.marchingAnts) return;
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

	/**
	 * Close and fill: a stroke that ends near where it began (within ~24 screen pixels,
	 * and long enough to be a shape) fills inside with its colour, in the same undo step.
	 */
	function fillClosed(points: { x: number; y: number }[]): boolean {
		if (!paint || points.length < 8) return false;
		const near = 24 / Math.max(0.01, fit * zoom);
		const [a, b] = [points[0], points.at(-1)!];
		if (Math.hypot(b.x - a.x, b.y - a.y) > near) return false;
		let path = 0;
		for (let i = 1; i < points.length; i++)
			path += Math.hypot(points[i].x - points[i - 1].x, points[i].y - points[i - 1].y);
		if (path < near * 4) return false;
		const off = new OffscreenCanvas(width, height);
		const c = off.getContext('2d')!;
		c.fillStyle = '#fff';
		c.beginPath();
		c.moveTo(a.x, a.y);
		for (const p of points.slice(1)) c.lineTo(p.x, p.y);
		c.closePath();
		c.fill();
		const rgba = c.getImageData(0, 0, width, height).data;
		const coverage = new Uint8Array(width * height);
		for (let i = 0; i < coverage.length; i++) coverage[i] = rgba[i * 4 + 3];
		const [r, g, bl] = rgbOf(color);
		// (the stroke's own colour and opacity; one undo step with the stroke)
		if (!paint.fillMask(coverage, r, g, bl, opacity)) return false;
		paint.joinLastEdits();
		return true;
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

	/** A box marquee dragged to `p`; with Shift a square (circle), its corners on the grid when it's on. */
	function marqueeTo(p: Pt, shift: boolean) {
		if (!marquee) return;
		if (!shift) {
			marquee = { ...marquee, pts: [marquee.from, p] };
			return;
		}
		const a = snapToGrid(guides, marquee.from);
		const b = snapToGrid(guides, p);
		const [dx, dy] = [b[0] - a[0], b[1] - a[1]];
		const m = Math.max(Math.abs(dx), Math.abs(dy));
		marquee = {
			...marquee,
			pts: [a, [a[0] + (dx < 0 ? -m : m), a[1] + (dy < 0 ? -m : m)]]
		};
	}

	function commitMarquee() {
		if (!paint || !marquee) return;
		const { kind, pts, op } = marquee;
		marquee = null;
		const area =
			kind === 'lasso' ? pts.length : Math.abs((pts[1][0] - pts[0][0]) * (pts[1][1] - pts[0][1]));
		if (area < 4) {
			// a click on the lasso starts a shape placed point by point
			if (kind === 'lasso') {
				poly = { pts: [pts[0]], op, at: null };
				return;
			}
			// a click without a drag clears the selection (replace mode)
			if (op === 0) paint.deselect();
			return refreshAnts();
		}
		select(kind, pts, op);
	}

	/** where the next point of a lasso placed point by point goes (Shift: at 45° steps from the last) */
	function polyAt(e: PointerEvent): Pt {
		const p = toCanvas(e.clientX, e.clientY);
		const last = poly?.pts.at(-1);
		return e.shiftKey && last ? snapAngle(last, p, 45) : p;
	}

	/** A click while placing a lasso point by point: a point, or closing the shape. */
	function polyClick(p: [number, number]) {
		if (!poly) return;
		const [fx, fy] = poly.pts[0];
		const near = 12 / Math.max(0.01, fit * zoom);
		if (poly.pts.length >= 3 && Math.hypot(p[0] - fx, p[1] - fy) <= near) return closePoly();
		poly = { ...poly, pts: [...poly.pts, p] };
	}
	function closePoly() {
		const pl = poly;
		poly = null;
		if (pl && pl.pts.length >= 3) select('lasso', pl.pts, pl.op);
	}
	// another tool: the half-placed shape goes
	$effect(() => {
		if (tool !== 'select-lasso') untrack(() => (poly = null));
	});

	/** Rasterise a rectangle, ellipse or lasso to a mask, and combine it into the selection. */
	function select(kind: 'rect' | 'ellipse' | 'lasso', pts: [number, number][], op: number) {
		if (!paint) return;
		const off = new OffscreenCanvas(width, height);
		const c = off.getContext('2d')!;
		c.fillStyle = '#fff';
		c.beginPath();
		if (kind === 'lasso') {
			c.moveTo(...pts[0]);
			for (const p of pts.slice(1)) c.lineTo(...p);
			c.closePath();
		} else {
			const [[ax, ay], [bx, by]] = pts;
			const [x, y, w, h] = [
				Math.min(ax, bx),
				Math.min(ay, by),
				Math.abs(bx - ax),
				Math.abs(by - ay)
			];
			if (kind === 'rect') c.rect(x, y, w, h);
			else c.ellipse(x + w / 2, y + h / 2, w / 2, h / 2, 0, 0, Math.PI * 2);
		}
		c.fill();
		const rgba = c.getImageData(0, 0, width, height).data;
		const mask = new Uint8Array(width * height);
		for (let i = 0; i < mask.length; i++) mask[i] = rgba[i * 4 + 3];
		paint.setSelection(mask, op);
		selected(op);
	}

	// ---- colour fill: selections fill with the colour as they're made ----

	/** this session's fill can still change (a new colour, shapes added) */
	let fillLive = false;
	const rgbOf = (hex: string) => {
		const n = parseInt(hex.slice(1), 16) || 0;
		return [(n >> 16) & 255, (n >> 8) & 255, n & 255] as const;
	};

	/** The selection changed (`op` as made): with colour fill on, fill it. */
	function selected(op: number) {
		refreshAnts();
		if (!paint || !selectFill) return;
		const [r, g, b] = rgbOf(color);
		// added to, taken from, or crossed with the last fill's selection: that fill
		// changes; a new selection is a new fill (the last one stays)
		if (op !== 0 && fillLive && paint.selectionFillLive()) {
			changed(paint.refillSelection(r, g, b, opacity), () => {});
			return;
		}
		fillLive = changed(paint.fillSelection(r, g, b, opacity), undefined, 'Colour fill');
	}

	/** Fill the selection with the colour now (one undo step). */
	export function fillSelection(): boolean {
		if (!paint) return false;
		const [r, g, b] = rgbOf(color);
		const ok = changed(paint.fillSelection(r, g, b, opacity), undefined, 'Fill selection');
		fillLive = ok && selectFill;
		return ok;
	}

	// another colour while the fill can still change: it changes too
	$effect(() => {
		const [c, o] = [color, opacity];
		untrack(() => {
			if (!paint || !fillLive || !selectFill || !paint.selectionFillLive()) return;
			const [r, g, b] = rgbOf(c);
			changed(paint.refillSelection(r, g, b, o), () => {});
		});
	});
	// colour fill off, or another tool: the fill is kept as it is
	$effect(() => {
		if (!selectFill || !(tool.startsWith('select') || tool === 'picker')) fillLive = false;
	});

	// ---- copy / cut / paste (the selection, or the whole layer) ----

	/** what was copied: a full-canvas picture (straight RGBA), transparent outside the selection */
	let clip: Uint8Array | null = null;
	/** or (the last copied) editable strokes, picked when copied: pasted as strokes */
	let lineClip: LineStroke[] | null = null;

	/**
	 * Copy the selected part of the layer (all of it with no selection); `cut` also clears
	 * it. With editable strokes picked (Select line, or picked with Move or the brush),
	 * those strokes are copied (and cut) instead.
	 */
	export function copySelection(cut = false, merged = !cut): boolean {
		if (!paint || xf) return false;
		if (lineTool && lines.sel?.ids.length && !lines.busy) {
			const strokes = lines.copySelected();
			if (!strokes.length) return false;
			lineClip = strokes;
			clip = null;
			if (cut) {
				lines.deleteSelected('Cut lines');
				picking2 = false;
			}
			return true;
		}
		// a copy takes what shows in the selection, from every visible layer (not the
		// paper); a cut (and "copy to a new layer") takes the active layer's own pixels
		const shown = merged ? renderFrame([], 1, true)?.rgba : null;
		const rgba = shown
			? new Uint8Array(shown.buffer, shown.byteOffset, shown.length)
			: paint.layerRgba(paint.activeLayer);
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
		lineClip = null;
		if (cut) {
			const layer = paint.activeLayer;
			changed(paint.clear(), () => log.clear(layer), 'Cut');
		}
		return true;
	}

	/**
	 * Paste what was copied as a new layer, in the same place; false if nothing was
	 * copied. Strokes copied go on the active layer (a line layer or a plain one), where
	 * they were (a little further when pasted onto copies of them), editable and picked,
	 * in one undo step: 'lines' (it finishes on its own).
	 */
	export function paste(): boolean | 'lines' {
		if (!paint) return false;
		if (lineClip) {
			const layer = paint.activeLayer;
			const l = layerList().find((x) => x.id === layer);
			if (!l || l.adjust || l.text || paint.isEditingMask()) return false;
			if (l.locked || !l.visible) {
				onblocked?.(l.visible ? 'locked' : 'hidden');
				return false;
			}
			if (!l.lines && strokeLimit() <= 0) return false;
			lines.cancel();
			// (Move's layer box goes: the strokes come picked)
			if (xf) {
				picking2 = true;
				transformCommit();
			}
			const done = (ok: boolean) => {
				// the pasted strokes are picked, as Select line has them
				picking2 = ok && tool !== 'lines' && !!lines.sel;
				schedule();
				notify();
			};
			const r = lines.paste(layer, lineClip, l.lines ? Infinity : strokeLimit());
			if (typeof r === 'boolean') done(r);
			else void r.then(done);
			return 'lines';
		}
		if (!clip || xf) return false;
		const id = paint.addImageLayer('Pasted', clip);
		if (!id) return false;
		paint.deselect();
		refreshAnts();
		changed(true, undefined, 'Paste');
		return true;
	}

	/** Something has been copied (Ctrl+V pastes it). */
	export const hasClip = () => !!clip || !!lineClip;

	/** Soften the selection's edge over about `radius` canvas px. */
	export function featherSelection(radius: number) {
		if (paint?.featherSelection(radius)) refreshAnts();
	}

	export function selectAll() {
		paint?.selectAll();
		refreshAnts();
	}

	/** how new selections combine, from the selection mode */
	const modeOp = () => ({ replace: 0, add: 1, subtract: 2, intersect: 3 })[selectMode];

	/** Grow the selection by `px` canvas px, or shrink it with a negative `px`. */
	export function growSelection(px: number) {
		if (!paint) return;
		paint.growSelection(Math.round(px));
		refreshAnts();
	}

	/**
	 * Select a colour (the current one unless given) wherever it is, on this layer or
	 * in what you see, combined as the selection mode says (Photoshop's Color Range).
	 */
	export function selectColour(hex = color, tolerance = wandTolerance, allLayers = wandAllLayers) {
		if (!paint) return;
		const [r, g, b] = rgbOf(hex);
		const op = modeOp();
		paint.selectColour(r, g, b, tolerance, allLayers, op);
		selected(op);
	}

	/**
	 * Select what's on a layer (the current one unless given): soft where it's
	 * see-through. `op` as the selection mode says unless given (Ctrl+click a thumbnail).
	 */
	export function selectLayerContents(id = paint?.activeLayer ?? 0, op = modeOp()) {
		if (!paint) return;
		paint.selectLayer(id, op);
		selected(op);
	}

	// ---- the Clone tool's source ----

	/** where the Clone tool copies from (canvas px) */
	let cloneFrom = $state<[number, number] | null>(null);
	/** the offset aligned strokes keep (source − brush), once a stroke has set it */
	let cloneKept = $state<[number, number] | null>(null);
	/** the next press sets the source (touch has no Alt key) */
	let cloneSetting = $state(false);
	/** the source mark: where the brush copies from now */
	const cloneMark = $derived<[number, number] | null>(
		cloneKept && cursorAt ? [cursorAt[0] + cloneKept[0], cursorAt[1] + cloneKept[1]] : cloneFrom
	);

	/** The Clone tool: the next press on the canvas sets where to copy from. */
	export function cloneSetSource() {
		cloneSetting = true;
	}
	export function deselect() {
		paint?.deselect();
		fillLive = false;
		refreshAnts();
	}
	export function invertSelection() {
		paint?.invertSelection();
		refreshAnts();
	}
	/** The box around the selection, canvas px (null: nothing selected). */
	export function selectionBounds(): {
		x: number;
		y: number;
		width: number;
		height: number;
	} | null {
		const m = paint?.selectionMask();
		if (!m?.length) return null;
		let [x0, y0, x1, y1] = [width, height, -1, -1];
		for (let y = 0; y < height; y++) {
			const row = y * width;
			for (let x = 0; x < width; x++)
				if (m[row + x] >= 128) {
					if (x < x0) x0 = x;
					if (x > x1) x1 = x;
					if (y < y0) y0 = y;
					y1 = y;
				}
		}
		return x1 < 0 ? null : { x: x0, y: y0, width: x1 - x0 + 1, height: y1 - y0 + 1 };
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

	/**
	 * Move on a layer with editable strokes (and no selection): the strokes show as
	 * outlines to pick, and the layer's box only comes with a drag off them.
	 */
	const moveStrokes = $derived.by(() => {
		void strokeTick;
		void lines.version;
		if (tool !== 'transform' || picking2 || !ready || !paint) return false;
		if (paint.hasSelection() || paint.isEditingMask()) return false;
		return hasStrokes(paint.activeLayer);
	});
	/** the strokes Move shows to pick */
	const moveList = $derived.by(() =>
		moveStrokes && paint
			? pickable(lines.strokesOf(paint.activeLayer))
			: ([] as readonly LineStroke[])
	);
	/** the stroke under the pointer, with Move showing strokes (its id) */
	let hoverStroke = $state<string | null>(null);

	/** Lift the layer (or selection) for Transform; false if there's nothing to move. */
	function beginTransform(): boolean {
		if (!paint || xf || paint.isTransforming()) return !!xf;
		const b = paint.transformBegin();
		if (b.length !== 4) {
			ontransformend?.('empty');
			return false;
		}
		xf = { box: [b[0], b[1], b[2], b[3]], tx: 0, ty: 0, sx: 1, sy: 1, rot: 0, fx: 1, fy: 1 };
		if (untrack(() => warp)) xf = { ...xf, mesh: startMesh(xf) };
		schedule();
		notify();
		return true;
	}

	// entering the tool lifts the pixels (with editable strokes showing: at the first drag
	// off them); leaving it keeps the result
	$effect(() => {
		if (!ready || !paint) return;
		if (tool === 'transform' && !xf && !picking2 && !moveStrokes && !paint.isTransforming()) {
			untrack(beginTransform);
		} else if (tool !== 'transform' && xf) {
			transformCommit();
		}
	});

	export function transformCommit() {
		if (!paint || !xf) return;
		const label = xf.mesh ? 'Warp' : xf.quad ? 'Distort' : 'Transform';
		// move, scale, turn and flip take editable strokes along (they stay editable); a
		// distort or warp makes them plain pixels
		const m: Affine | null = !xf.mesh && !xf.quad ? xfMatrix(xf) : null;
		xf = null;
		const layer = paint.activeLayer;
		const follow = m ? strokesFollowing(layer) : null;
		const ok = paint.transformCommitKeeping(!!follow && isLineLayer(layer));
		changed(ok, undefined, label);
		if (ok && follow && m) {
			const effect = lines.followTransform(layer, m, follow, paint.takeTainted());
			if (effect) log.attachLast(effect);
			schedule();
			notify();
		}
		paint.xfForget();
		refreshAnts();
		ontransformend?.('done');
	}

	/**
	 * The editable strokes a transform of `layer` takes along (ids; null: none): every
	 * one, or with a selection those wholly inside it. A line layer's strokes go all
	 * together or not at all (then it turns into pixels, as before).
	 */
	function strokesFollowing(layer: number): Set<string> | null {
		if (!paint || paint.isEditingMask()) return null;
		const list = lines.strokesOf(layer);
		if (!list.length) return null;
		const sel = paint.hasSelection() ? paint.selectionMask() : null;
		const inside = (s: LineStroke) => {
			if (!sel) return true;
			const b = bounds(s);
			const [x0, y0] = [Math.floor(b.x0), Math.floor(b.y0)];
			const [x1, y1] = [Math.ceil(b.x1), Math.ceil(b.y1)];
			if (x0 < 0 || y0 < 0 || x1 >= width || y1 >= height) return false;
			for (let y = y0; y <= y1; y++)
				for (let x = x0; x <= x1; x++) if (sel[y * width + x] < 255) return false;
			return true;
		};
		const ids = new Set(list.filter(inside).map((s) => s.id));
		if (!ids.size || (isLineLayer(layer) && ids.size !== list.length)) return null;
		return ids;
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
		if (tool !== 'brush' && tool !== 'lines') editShape = null;
		if (tool !== 'measure') measureSeg = null;
		if (tool !== 'lines')
			untrack(() => {
				lines.deselect();
				picking2 = false;
			});
	});
	// a stroke picked from another tool: done when nothing is selected any more
	$effect(() => {
		if (picking2 && !lines.sel && !lines.dragging) picking2 = false;
	});
	// Select line: a QuickShape selected on its own shows its Edit shape handles
	$effect(() => {
		if (!lineTool) return;
		const sh = lines.selectedShape();
		const layer = lines.sel?.layer ?? 0;
		untrack(() => {
			if (shapeDrag) return;
			editShape = sh?.shape ?? null;
			shapeOf = sh ? { layer, id: sh.id } : null;
		});
	});
	// Select line: the colour, size, opacity and brush chosen go to the selected strokes
	let lineStyleWas = untrack(() => ({ color, chosenSize, opacity, brushId }));
	let lineStyleAt = 0;
	$effect(() => {
		const now = { color, chosenSize, opacity, brushId };
		untrack(() => {
			const was = lineStyleWas;
			lineStyleWas = now;
			if (!lineTool || !lines.sel || lines.busy) return;
			const patch: Partial<LineStroke> = {};
			if (now.color !== was.color) patch.color = color;
			if (now.chosenSize !== was.chosenSize) patch.size = size;
			if (now.opacity !== was.opacity) patch.opacity = opacity;
			if (now.brushId !== was.brushId) {
				patch.brush = brushId;
				patch.texture = brushTexture ? ($state.snapshot(brushTexture) as BrushTexture) : null;
			}
			if (!Object.keys(patch).length) return;
			const joinable = performance.now() - lineStyleAt < 900;
			void lines.restyle(patch, lineRadius).then((done) => {
				if (!done) return;
				// a slider dragged: one step
				if (joinable && log.history().undo.at(-2) === 'Change lines') joinLastSteps();
				lineStyleAt = performance.now();
			});
		});
	});
	/** a brush's radius (canvas px) at a size, for line strokes (null: not loaded) */
	function lineRadius(id: string, s: number) {
		const b = lineBrushes.get(id);
		if (!b) return null;
		b.setSizeScale(s);
		return b.radius();
	}

	// ---- pen tool: Bézier paths, stroked with the brush or filled ----

	let penPath = $state<PenPath>(EMPTY_PATH);
	let penDrag: {
		id: number;
		index: number;
		part: 'p' | 'hin' | 'hout';
		broken: boolean;
		/** where the anchor was when the drag began */
		from: Pt;
	} | null = null;
	$effect(() => {
		const p = penPath;
		untrack(() => onpenpath?.({ points: p.anchors.length, closed: p.closed }));
	});

	function penDown(e: PointerEvent) {
		let p = toCanvas(e.clientX, e.clientY);
		const r = 10 / Math.max(0.01, fit * zoom);
		const hit = hitPath(penPath, p, r);
		capture(e);
		const at = (i: number): Pt => penPath.anchors[i].p;
		if (hit) {
			// the first point again closes the path
			if (hit.index === 0 && hit.part === 'p' && !penPath.closed && penPath.anchors.length > 2) {
				penPath = { ...penPath, closed: true };
				penDrag = { id: e.pointerId, index: 0, part: 'hout', broken: false, from: at(0) };
				return;
			}
			penDrag = { id: e.pointerId, ...hit, broken: e.altKey, from: at(hit.index) };
			return;
		}
		if (penPath.closed) penPath = EMPTY_PATH;
		// Shift: on the grid when it's on, else at 15° steps from the point before
		const last = penPath.anchors.at(-1)?.p;
		if (e.shiftKey && guides?.grid.on) p = snapToGrid(guides, p);
		else if (e.shiftKey && last) p = snapAngle(last, p, 15);
		else {
			inkCache = null;
			p = inkTarget(p) ?? p;
		}
		penPath = { ...penPath, anchors: [...penPath.anchors, corner(p)] };
		// dragging out of a new point pulls its handles (a smooth point)
		penDrag = {
			id: e.pointerId,
			index: penPath.anchors.length - 1,
			part: 'hout',
			broken: false,
			from: p
		};
	}

	function penMove(e: PointerEvent) {
		if (!penDrag || penDrag.id !== e.pointerId) return false;
		let to = toCanvas(e.clientX, e.clientY);
		// Shift: an anchor moves along an axis (or onto the grid), a handle turns in 15° steps
		if (e.shiftKey) {
			const a = penPath.anchors[penDrag.index]?.p ?? penDrag.from;
			to = penDrag.part === 'p' ? shiftPoint(guides, penDrag.from, to) : snapAngle(a, to, 15);
		} else if (penDrag.part === 'p' && inkSnap) {
			// (not onto itself: the other points only)
			const others = penPath.anchors.filter((_, i) => i !== penDrag!.index).map((a) => a.p);
			const r = inkReach();
			const t = nearestPoint(others, to, r) ?? nearestInk(inkPixels(), width, height, to, r);
			inkMark = t;
			if (t) to = t;
		}
		penPath = dragPart(penPath, penDrag.index, penDrag.part, to, penDrag.broken || e.altKey);
		return true;
	}

	/** Draw along the path with the current brush (one stroke, one undo step). */
	export function penStroke(taper = true) {
		if (!paint || !brush || penPath.anchors.length < 2) return false;
		// on a line layer the path becomes a line (editable, as any stroke there)
		const layer = paint.activeLayer;
		const asLine = keepsStroke();
		if (asLine) lineSeed = newSeed();
		if (!(asLine ? paint.beginLineStroke(lineSeed) : paint.beginStroke())) {
			const l = shownLayers().find((x) => x.id === paint!.activeLayer);
			onblocked?.(l && !l.visible ? 'hidden' : 'locked');
			return false;
		}
		linePts = asLine ? [] : null;
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
			strokeTo(x, y, pr, 0, 0, 0.008);
			stroke.points.push({
				x: Math.round(x * 10) / 10,
				y: Math.round(y * 10) / 10,
				pressure: pr,
				t
			});
		});
		const line = asLine ? finishLineStroke(null) : null;
		paint.endStroke(brush);
		const effect = line ? lines.added(layer, line.strokes, strokeLimit()) : undefined;
		changed(
			true,
			() => {
				log.add(stroke);
				if (effect) log.attachLast(effect);
			},
			'Pen path'
		);
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
		let [dx, dy] = [p[0] - xfDrag.start[0], p[1] - xfDrag.start[1]];
		const q = f.quad;
		if (e.shiftKey && xfDrag.mode !== 'rotate') {
			// a corner goes along an axis (onto the grid when it's on); the rest moves by whole steps
			if (xfDrag.mode === 'corner') {
				const c = q[xfDrag.index];
				const to = shiftPoint(guides, c, [c[0] + dx, c[1] + dy]);
				[dx, dy] = [to[0] - c[0], to[1] - c[1]];
			} else [dx, dy] = shiftOffset(guides, [dx, dy]);
			xfHint(dx, dy);
		}
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

	/** the hint for a Shift-held transform drag: from where it began, by (dx, dy) */
	function xfHint(dx: number, dy: number) {
		if (!xfDrag) return;
		const a = xfDrag.start;
		snapHint = [a, [a[0] + dx, a[1] + dy]];
	}

	function xfMove(e: PointerEvent) {
		if (!xfDrag) return;
		snapHint = null;
		const p = toCanvas(e.clientX, e.clientY);
		const f = xfDrag.from;
		if (f.mesh) {
			let [dx, dy] = [p[0] - xfDrag.start[0], p[1] - xfDrag.start[1]];
			const m = f.mesh;
			if (e.shiftKey && xfDrag.mode !== 'rotate') {
				if (xfDrag.mode === 'node') {
					const c = m[xfDrag.index];
					const to = shiftPoint(guides, c, [c[0] + dx, c[1] + dy]);
					[dx, dy] = [to[0] - c[0], to[1] - c[1]];
				} else [dx, dy] = shiftOffset(guides, [dx, dy]);
				xfHint(dx, dy);
			}
			let next: Pt[];
			if (xfDrag.mode === 'node')
				next = m.map((q, i) => (i === xfDrag!.index ? [q[0] + dx, q[1] + dy] : q));
			else if (xfDrag.mode === 'rotate') {
				const [cx, cy] = meshMid(m);
				const a0 = Math.atan2(xfDrag.start[1] - cy, xfDrag.start[0] - cx);
				const a1 = Math.atan2(p[1] - cy, p[0] - cx);
				let deg = ((a1 - a0) * 180) / Math.PI;
				if (e.shiftKey) deg = Math.round(deg / 15) * 15;
				next = rotatePts(m, deg);
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
			let [dx, dy] = [p[0] - xfDrag.start[0], p[1] - xfDrag.start[1]];
			// Shift: across, down or at 45° from where the drag began (whole grid cells with the grid on)
			if (e.shiftKey) {
				[dx, dy] = shiftOffset(guides, [dx, dy]);
				xfHint(dx, dy);
			}
			xf = { ...f, tx: f.tx + dx, ty: f.ty + dy };
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
			// a corner keeps the proportions; with Shift it scales each way on its own
			if (xfDrag.mode === 'corner' && e.shiftKey)
				xf = { ...f, sx: f.sx * k(s0[0], s1[0]), sy: f.sy * k(s0[1], s1[1]) };
			else if (xfDrag.mode === 'corner') {
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
		/** comp: the composition guide moved; compScale: a corner, sizing it about its middle */
		what: 'vp' | 'a' | 'b' | 'ruler' | 'sym' | 'comp' | 'compScale';
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
		// drawing assist is the active layer's (as in Procreate)
		const dirs = assistLayers.has(paint?.activeLayer ?? 0) ? snapDirections(guides, p) : [];
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
		if (snapLine && shiftStroke) {
			const q = gridStroke
				? snapAlong(guides, snapLine.origin, snapLine.u, p)
				: project(snapLine.origin, snapLine.u, p);
			snapHint = [snapLine.origin, q];
			return q;
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
		for (let i = 0; i < n; i++) {
			// (a point off the canvas is picked up by its handle on the edge)
			const pin = edgePin(guides.perspective.vps[i], width, height, r);
			if (near(guides.perspective.vps[i]) || (pin && near(pin.at)))
				return { what: 'vp', i, start: p, from: $state.snapshot(guides) as GuideSettings };
		}
		if (compositionOn(guides)) {
			const [x, y, fw, fh] = compositionFrame(guides.composition, width, height);
			const corners: Pt[] = [
				[x, y],
				[x + fw, y],
				[x + fw, y + fh],
				[x, y + fh]
			];
			if (corners.some(near))
				return {
					what: 'compScale',
					i: 0,
					start: p,
					from: $state.snapshot(guides) as GuideSettings
				};
			if (near([x + fw / 2, y + fh / 2]))
				return { what: 'comp', i: 0, start: p, from: $state.snapshot(guides) as GuideSettings };
		}
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

	/**
	 * A guide's handle dragged to p. Shift: a ruler end turns the ruler in 15° steps about its
	 * other end; a vanishing point or the mirror's centre moves along an axis (or onto the grid);
	 * the whole ruler moves along an axis.
	 */
	function guideMove(p: Pt, shiftKey = false) {
		if (!guideDrag) return;
		const g = structuredClone(guideDrag.from);
		let [dx, dy] = [p[0] - guideDrag.start[0], p[1] - guideDrag.start[1]];
		if (shiftKey && guideDrag.what === 'ruler') [dx, dy] = shiftOffset(guides, [dx, dy]);
		const shift = (q: Pt): Pt => {
			const to: Pt = [q[0] + dx, q[1] + dy];
			return shiftKey && guideDrag?.what !== 'ruler' ? shiftPoint(guides, q, to) : to;
		};
		const turn = (q: Pt, about: Pt): Pt =>
			shiftKey ? snapAngle(about, [q[0] + dx, q[1] + dy], 15) : [q[0] + dx, q[1] + dy];
		if (guideDrag.what === 'vp')
			g.perspective.vps[guideDrag.i] = shift(g.perspective.vps[guideDrag.i]);
		else if (guideDrag.what === 'a') g.ruler.a = turn(g.ruler.a, g.ruler.b);
		else if (guideDrag.what === 'b') g.ruler.b = turn(g.ruler.b, g.ruler.a);
		else if (guideDrag.what === 'sym') g.symmetry.c = shift(g.symmetry.c);
		else if (guideDrag.what === 'comp') {
			const [ox, oy] = g.composition.offset ?? [0, 0];
			if (shiftKey) [dx, dy] = shiftOffset(null, [dx, dy]);
			const k = (v: number) => Math.max(-1, Math.min(1, v));
			g.composition.offset = [k(ox + dx / width), k(oy + dy / height)];
		} else if (guideDrag.what === 'compScale') {
			// sized about its middle, as far out as the corner is dragged
			const [x, y, fw, fh] = compositionFrame(g.composition, width, height);
			const [cx, cy] = [x + fw / 2, y + fh / 2];
			const was = Math.hypot(guideDrag.start[0] - cx, guideDrag.start[1] - cy);
			const now = Math.hypot(p[0] - cx, p[1] - cy);
			const s = (g.composition.scale ?? 1) * (was > 0 ? now / was : 1);
			g.composition.scale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, s));
		} else g.ruler = { ...g.ruler, a: shift(g.ruler.a), b: shift(g.ruler.b) };
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
	/** the composition overlay (thirds, the golden spiral…): only shown, never in the picture */
	const guideComp = $derived(
		compositionOn(guides) ? compositionParts(guides!.composition, width, height) : null
	);
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

	/** Preview a filter (sharpen, noise, glitch…) on the layer being adjusted. */
	export function filterPreview(f: FilterSpec) {
		if (!paint?.isAdjusting()) return;
		paint.filterPreview(f.kind, f.a, f.b, f.c, f.mono, f.seed);
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
	 * hears the new size through `oncanvassize`. Every drawing changes with it (an
	 * animation's other frames too, the ones the engine has), and every mask; text layers
	 * get the descriptions in `texts` ([layer id, text JSON]: their boxes, moved).
	 *
	 * `swaps` ([drawing, other id] pairs): drawings already resized one at a time with
	 * `resizeDrawingInto` (an animation in a frame store): the step swaps each with its
	 * resized copy instead of keeping both in memory (the app swaps them in its store).
	 */
	export function resizeCanvas(
		w: number,
		h: number,
		dx: number,
		dy: number,
		scale: boolean,
		texts: [number, string][] = [],
		swaps: [number, number][] = []
	) {
		if (!paint) return false;
		if (xf) transformCancel();
		penPath = EMPTY_PATH;
		lastEnd = null;
		// editable strokes (line layers' and plain ones') move or scale with their pixels: every
		// frame's, not only the ones showing (the engine resizes every drawing in the same step)
		const m: Affine = scale
			? [w / width, 0, 0, h / height, 0, 0]
			: [1, 0, 0, 1, Math.round(dx), Math.round(dy)];
		const drawings = lines.store.drawings();
		const ids = new Uint32Array(texts.map(([id]) => id));
		const specs = texts.map(([, t]) => t).join('\u0001');
		const [rdx, rdy] = [Math.round(dx), Math.round(dy)];
		const ok = swaps.length
			? paint.resizeCanvasSwapping(w, h, rdx, rdy, scale, ids, specs, new Uint32Array(swaps.flat()))
			: paint.resizeCanvas(w, h, rdx, rdy, scale, ids, specs);
		return changed(
			ok,
			() => {
				log.layerChange();
				if (drawings.length) log.attachLast(transformLinesStep(drawings, m));
				// replays (films of the drawing) show the strokes where the drawing is now
				log.attachLast(log.transformStep(m));
				// scaled texts are drawn again at their new size (their pixels would only be
				// stretched, and text can't stretch), in the same step
				if (scale) for (const [id, json] of texts) redrawText(id, json, w, h);
			},
			'Canvas size'
		);
	}

	/** Draw a text layer again from its description on a `w`×`h` canvas, joined to the last step. */
	function redrawText(id: number, json: string, w: number, h: number) {
		const spec = parseText(json);
		if (!paint || !spec) return;
		const r = renderText(spec, w, h);
		if (!paint.textRedraw(id, r.x, r.y, r.w, r.h, r.rgba)) return;
		log.layerChange();
		if (paint.joinLastEdits()) log.joinLast();
	}

	/**
	 * Kept drawing `to` becomes drawing `from` resized for a canvas size change (as
	 * `resizeCanvas` would resize it): not an undo step, for sending to a frame store.
	 */
	export function resizeDrawingInto(
		from: number,
		to: number,
		w: number,
		h: number,
		dx: number,
		dy: number,
		scale: boolean
	): boolean {
		return paint?.resizeDrawingInto(from, to, w, h, Math.round(dx), Math.round(dy), scale) ?? false;
	}

	/**
	 * Every stroke of these drawings' line layers moved / scaled by `m` (no redraw: their
	 * pixels were changed the same way). Returns the undo / redo to attach to the step that
	 * changed the pixels. For changing the canvas size across frames: call it with every
	 * line drawing and the same transform as the pixels get.
	 */
	export function transformLinesStep(drawings: number[], m: Affine) {
		const was = drawings.map((d) => [d, lines.store.get(d)] as const);
		const now = was.map(([d, list]) => [d, list.map((s) => transformStroke(s, m))] as const);
		for (const [d, list] of now) lines.store.set(d, list);
		lines.check();
		return {
			undo: () => was.forEach(([d, list]) => lines.store.set(d, list)),
			redo: () => now.forEach(([d, list]) => lines.store.set(d, list))
		};
	}

	/** The drawings the next undo (or redo) of a canvas size changes (none: it isn't one). */
	export const canvasStepDrawings = (redo = false): number[] => [
		...(paint?.canvasStepDrawings(redo) ?? [])
	];
	/** The [drawing, other id] pairs the next undo (or redo) of a canvas size swaps. */
	export function canvasStepSwaps(redo = false): [number, number][] {
		const flat = [...(paint?.canvasStepSwaps(redo) ?? [])];
		return flat.flatMap((v, i) => (i % 2 ? [] : [[v, flat[i + 1]] as [number, number]]));
	}
	/** Tiles (64×64) of paint the engine holds: layers, masks and kept drawings. */
	export const tileCount = () => paint?.tileCount() ?? 0;

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

	// ---- smoothing (pulled string): the brush tip trails the pointer on a string ----

	const stab = new Stabilizer();
	/** when the pointer last moved (for the catch-up's time) */
	let lastRawT = 0;

	/** one point of the stroke: painted, sent to others drawing along and logged */
	function paintAt(x: number, y: number, p: number, tx: number, ty: number, dt: number, t: number) {
		if (!paint || !brush || !current) return;
		strokeTo(x, y, p, tx, ty, dt);
		if (sendLive())
			livePts.push(
				Math.round(x * 10) / 10,
				Math.round(y * 10) / 10,
				Math.round(p * 1000) / 1000,
				Math.round(dt * 1000)
			);
		current.points.push({
			x: Math.round(x * 10) / 10,
			y: Math.round(y * 10) / 10,
			pressure: Math.round(p * 1000) / 1000,
			t: Math.round(t - t0)
		});
	}

	/** at the end of a smoothed stroke, carry the line on to where the pen actually stopped */
	function catchUp() {
		for (const q of stab.catchUp()) paintAt(q.x, q.y, q.p, 0, 0, 0.004, lastRawT);
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

	/** the stroke in progress began with a Shift+click line */
	let lineStroke = false;

	/** the end of the last brush stroke, canvas px */
	let lastEnd = $state<{ x: number; y: number; p: number } | null>(null);
	let shiftHeld = $state(false);
	/** where the pointer is (canvas px) while Shift is held, for the line preview */
	let lineHover = $state<[number, number] | null>(null);
	/** a Shift + drag stroke with the grid on: its points go from grid crossing to grid crossing */
	let gridStroke = false;
	/** the stroke in progress is a Shift + drag one (held straight) */
	let shiftStroke = false;
	/** a constraint at work (Shift): a light dashed line from where it holds to where it goes */
	let snapHint = $state<[Pt, Pt] | null>(null);

	// ---- snap to ink: strokes start and end on lines close by ----

	/** where a stroke would join ink (a small ring shows there) */
	let inkMark = $state<Pt | null>(null);
	/** the active layer's pixels as they were before the stroke (read again at each press) */
	let inkCache: { layer: number; rgba: Uint8Array } | null = null;
	/** how near ink must be, in canvas px (the same on screen at any zoom) */
	const inkReach = () => INK_SNAP_PX / Math.max(0.01, fit * zoom);
	function inkPixels() {
		if (!paint) return null;
		const l = paint.activeLayer;
		if (inkCache?.layer !== l) inkCache = { layer: l, rgba: paint.layerRgba(l) };
		return inkCache.rgba;
	}
	/** the ink near p to join (a line's end first, else the nearest inked pixel), if on */
	function inkTarget(p: Pt): Pt | null {
		if (!inkSnap || !paint || paint.isEditingMask()) return null;
		const r = inkReach();
		const ends = [
			...lines.strokesOf(paint.activeLayer).flatMap(strokeEnds),
			...penPath.anchors.map((a) => a.p)
		];
		return nearestPoint(ends, p, r) ?? nearestInk(inkPixels(), width, height, p, r);
	}
	/** this stroke joins ink at its ends (a plain brush stroke, not held straight with Shift) */
	const inkStroke = () =>
		inkSnap && !!current && !current.eraser && !current.clone && !shiftStroke && !lineStroke;

	/**
	 * The stroke's end onto ink at `t`: the points that ran on inside the reach go (no
	 * overshoot), and the line is drawn again ending exactly there (no gap).
	 */
	function endOnInk(t: Pt) {
		if (!paint || !current || !current.points.length) return;
		const r = inkReach();
		const pts = current.points;
		let n = pts.length;
		while (n > 1 && Math.hypot(pts[n - 1].x - t[0], pts[n - 1].y - t[1]) <= r) n--;
		const last = pts[n - 1];
		const keep = pts.slice(0, n);
		const len = Math.hypot(t[0] - last.x, t[1] - last.y);
		const steps = Math.max(1, Math.ceil(len / 2));
		for (let k = 1; k <= steps; k++)
			keep.push({
				x: Math.round((last.x + ((t[0] - last.x) * k) / steps) * 10) / 10,
				y: Math.round((last.y + ((t[1] - last.y) * k) / steps) * 10) / 10,
				pressure: last.pressure,
				t: last.t
			});
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
		if (!restartStroke()) return;
		current.points = [];
		for (const q of keep) {
			strokeTo(q.x, q.y, q.pressure, 0, 0, 0.008);
			current.points.push(q);
			if (sendLive()) livePts.push(q.x, q.y, q.pressure, 8);
		}
	}

	/** Draw the straight part of a Shift+click stroke: from the last stroke's end to `e`. */
	function lineFromLast(e: PointerEvent) {
		if (!paint || !brush || !current || !lastEnd) return;
		// (with the grid on, the line ends on a grid crossing)
		const [bx, by] = snapToGrid(guides, toCanvas(e.clientX, e.clientY));
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
			strokeTo(x, y, p, 0, 0, 0.004);
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
		stab.reset({ x: bx, y: by, p: pb });
		lastRawT = e.timeStamp;
		lastT = e.timeStamp;
	}

	function feed(e: PointerEvent, at?: Pt | null) {
		if (!paint || !brush || !current) return;
		const [cx, cy] = at ?? toCanvas(e.clientX, e.clientY);
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
		// the string is set in screen px, so it feels the same at any zoom
		stab.length = stringLength(smoothing) / Math.max(0.01, fit * zoom);
		lastRawT = r.time;
		const tip = stab.push(rx, ry, r.p);
		if (!tip) return; // the string is still slack
		const dt = Math.max(0, (r.time - lastT) / 1000);
		lastT = r.time;
		paintAt(tip.x, tip.y, tip.p, r.tx, r.ty, dt, r.time);
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
		lastPointer = e;
		if ((e.target as HTMLElement).closest('.drop-tol, .canvas-note')) return;
		dropFill = null;
		// the view toolbar sits on top of the canvas area: its buttons aren't strokes
		if ((e.target as HTMLElement).closest('.view, .shape-bar')) return;
		if (e.pointerType === 'pen') penSeen = true;
		// holding a snapped shape: another finger's tap makes it perfect (not a pinch)
		if (current && snapped && snappedShape && e.pointerId !== strokePointer) {
			perfectSnapped();
			return;
		}
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
				if (linePointer !== null) {
					lines.cancel();
					linePointer = null;
				}
				if (current) {
					paint.cancelStroke();
					linePts = null;
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
		// Ctrl (⌘) + drag moves the view, like Space + drag (Ctrl+Alt still sizes the brush,
		// and the Transform tool keeps Ctrl + drag for distorting)
		if ((e.ctrlKey || e.metaKey) && !e.altKey && e.pointerType !== 'touch' && tool !== 'transform')
			return startPan(e);
		// Ctrl+Alt + drag: resize the brush, watching its outline (as in Photoshop)
		if (tool === 'brush' && e.altKey && (e.ctrlKey || e.metaKey) && e.pointerType !== 'touch') {
			capture(e);
			sizeDrag = {
				id: e.pointerId,
				x: e.clientX,
				size: chosenSize,
				at: toCanvas(e.clientX, e.clientY)
			};
			return;
		}
		// editable shape: grab one of its points
		if ((tool === 'brush' || lineTool) && editShape && !locked) {
			const p = toCanvas(e.clientX, e.clientY);
			const r = 14 / Math.max(0.01, fit * zoom);
			const i = shapeHandles(editShape).findIndex((q) => Math.hypot(p[0] - q[0], p[1] - q[1]) <= r);
			if (i >= 0) {
				capture(e);
				shapeDrag = { id: e.pointerId, index: i, from: shapeHandles(editShape)[i] };
				return;
			}
		}
		// ColorDrop, carried on: each tap fills with the same colour
		if (dropContinue && !e.altKey) {
			dropAt(e.clientX, e.clientY, dropContinue);
			return;
		}
		// clone: Alt+click (or a tap after "Set source") picks where to copy from
		if (tool === 'clone' && (cloneSetting || (e.altKey && e.pointerType !== 'touch'))) {
			cloneFrom = toCanvas(e.clientX, e.clientY);
			cloneKept = null;
			cloneSetting = false;
			return;
		}
		// eyedropper: the tool, or Alt+click with the brush
		if (tool === 'picker' || (e.altKey && e.pointerType !== 'touch')) {
			capture(e);
			picking = e.pointerId;
			pick(e);
			return;
		}
		if (locked) return;
		if (tool === 'text') {
			capture(e);
			textPointer = e.pointerId;
			const [x, y] = toCanvas(e.clientX, e.clientY);
			ontext?.(x, y, 'down');
			return;
		}
		if (onrasterize && RASTERIZING.has(tool) && !paint.isEditingMask()) {
			const l = layerList().find((x) => x.id === paint!.activeLayer);
			if (l?.text) return onrasterize(l.id);
			// a line layer takes the brush (strokes), the eraser and fills; the rest paint pixels
			if (l?.lines && tool !== 'brush' && tool !== 'fill') return onrasterize(l.id);
		}
		// Select line (or a stroke picked from another tool), and the eraser on a line
		// layer (whole strokes, or cut where it passes) or, set to whole lines, on the
		// editable strokes of a plain one
		const onLines = !paint.isEditingMask() && isLineLayer(paint.activeLayer);
		const strokesHere = !paint.isEditingMask() && hasStrokes(paint.activeLayer);
		const wholeHere = strokesHere && lineEraser === 'whole';
		// (the normal eraser over editable strokes is drawn as a stroke: see `keepsStroke`)
		const exactErase = tool === 'brush' && eraser && !lineTool && keepsStroke();
		if (lineTool || ((onLines || wholeHere) && tool === 'brush' && eraser && !exactErase)) {
			const [x, y] = toCanvas(e.clientX, e.clientY);
			// a stroke picked from another tool: a press away from it puts that tool back
			if (picking2 && tool !== 'lines' && !lines.onSelection(x, y, fit * zoom)) {
				lines.deselect();
				picking2 = false;
				return;
			}
			if (!onLines && !strokesHere) return;
			const l = shownLayers().find((x) => x.id === paint!.activeLayer);
			if (l && (l.locked || !l.visible)) return onblocked?.(l.visible ? 'locked' : 'hidden');
			if (lines.busy) return;
			capture(e);
			linePointer = e.pointerId;
			if (lineTool) lines.down(paint.activeLayer, x, y, e.shiftKey, fit * zoom);
			else lines.eraseBegin(paint.activeLayer, x, y, brushR, onLines ? lineEraser : 'whole');
			return;
		}
		if (tool === 'transform') {
			const p = toCanvas(e.clientX, e.clientY);
			/** the box was there before this press (a tap away from it then accepts it) */
			const wasOpen = !!xf;
			if (!xf) {
				if (!moveStrokes) return;
				const l = shownLayers().find((x) => x.id === paint!.activeLayer);
				if (l && (l.locked || !l.visible)) return onblocked?.(l.visible ? 'locked' : 'hidden');
				// on a stroke: it's picked, and a drag moves it
				if (lines.hit(paint.activeLayer, p[0], p[1], fit * zoom) >= 0) {
					pickStroke(p);
					if (!picking2) return;
					capture(e);
					linePointer = e.pointerId;
					hoverStroke = null;
					lines.down(paint.activeLayer, p[0], p[1], false, fit * zoom);
					return;
				}
				// off them: the layer moves, as Move does
				if (!beginTransform() || !xf) return;
			}
			const handle = xfHit(p);
			// away from the box: a drag still moves it, a tap accepts it (as Done does)
			xfAway = !handle && wasOpen ? { x: e.clientX, y: e.clientY } : null;
			const hit: { mode: NonNullable<typeof xfDrag>['mode']; index: number } = handle ?? {
				mode: 'move',
				index: 0
			};
			// a tap (not a drag) on an editable stroke picks it to edit on its own
			xfTap =
				hit.mode === 'move' &&
				!e.shiftKey &&
				strokesHere &&
				lines.hit(paint.activeLayer, p[0], p[1], fit * zoom) >= 0
					? { x: e.clientX, y: e.clientY, p }
					: null;
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
				const op = opOf(e);
				if (wandContiguous) paint.selectWand(x, y, wandTolerance, wandAllLayers, op);
				else paint.selectColourRange(x, y, wandTolerance, wandAllLayers, op);
				return selected(op);
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
		if (tool === 'select-lasso' && poly) return polyClick(polyAt(e));
		if (tool === 'select-rect' || tool === 'select-lasso') {
			capture(e);
			const p = toCanvas(e.clientX, e.clientY);
			const kind =
				tool === 'select-lasso' ? 'lasso' : selectShape === 'ellipse' ? 'ellipse' : 'rect';
			// (Shift at the press adds to a selection; with none yet, it just shapes the box)
			const shapes = !e.shiftKey || !paint.hasSelection();
			marquee = { kind, pts: [p, p], op: opOf(e), from: p, shapes };
			if (e.shiftKey && shapes) marqueeTo(p, true);
			return;
		}
		if (adjustOverlay && overlaySrc) {
			capture(e);
			const [cx, cy] = toCanvas(e.clientX, e.clientY);
			moving = { id: e.pointerId, cx, cy };
			return;
		}
		// the Clone tool copies from its source (none yet: the hint says how to set one)
		const off =
			tool === 'clone'
				? cloneOffset(cloneFrom, toCanvas(e.clientX, e.clientY), cloneAligned, cloneKept)
				: null;
		if (tool === 'clone' && !off) return;
		// on a line layer the stroke is kept as a line (seeded: redrawn, it paints the same)
		// snap to ink: where it starts (read before this stroke marks the layer)
		inkCache = null;
		const inkStart =
			inkSnap && tool === 'brush' && !eraser && !off && !e.shiftKey
				? inkTarget(toCanvas(e.clientX, e.clientY))
				: null;
		const asLine = !off && tool === 'brush' && keepsStroke();
		if (asLine) lineSeed = newSeed();
		// (the eraser's brush redraws its mark: loaded before the stroke ends)
		if (asLine && eraser && !lineBrushes.has(brushId))
			void lines.ready([{ brush: brushId } as LineStroke]);
		const began = off
			? paint.beginCloneStroke(off[0], off[1], cloneAllLayers, cloneHeal)
			: asLine
				? paint.beginLineStroke(lineSeed)
				: paint.beginStroke();
		linePts = asLine && began ? [] : null;
		if (!began) {
			const l = shownLayers().find((x) => x.id === paint!.activeLayer);
			onblocked?.(l && !l.visible ? 'hidden' : 'locked');
			return;
		}
		capture(e);
		editShape = null;
		strokePointer = e.pointerId;
		current = { brushId, eraser, layer: paint.activeLayer, color, size, opacity, points: [] };
		cloneStroke = !!off;
		if (off) {
			cloneKept = off;
			current.eraser = false;
			current.clone = { dx: off[0], dy: off[1], heal: cloneHeal, all: cloneAllLayers };
		}
		lastT = e.timeStamp;
		stab.reset();
		// the brush held still on an editable stroke for a moment picks it
		if (holdToPick && tool === 'brush' && !eraser && !off && strokesHere && !e.shiftKey)
			startHoldPick(e);
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
		gridStroke = shiftStroke = false;
		if (e.shiftKey) {
			shiftStroke = true;
			// with the grid on, the stroke starts (and ends) on a grid crossing
			gridStroke = !!guides?.grid.on;
			const p = snapToGrid(guides, toCanvas(e.clientX, e.clientY));
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
		feed(e, inkStart);
		schedule();
	}

	// ---- picking a stroke to edit from another tool ----

	/** a Transform press that may be a tap on a stroke (screen px where it began) */
	let xfTap: { x: number; y: number; p: [number, number] } | null = null;
	/** a press away from the transform box (where it began, on screen) */
	let xfAway: { x: number; y: number } | null = null;
	/** a brush press held still, waiting to see whether it picks a stroke */
	let holdPick: { id: number; x: number; y: number; p: [number, number]; timer: number } | null =
		null;
	/** how long the brush must stay still on a stroke to pick it (ms) */
	const HOLD_PICK_MS = 500;

	function endHoldPick() {
		if (holdPick) clearTimeout(holdPick.timer);
		holdPick = null;
	}

	function startHoldPick(e: PointerEvent) {
		endHoldPick();
		const p = toCanvas(e.clientX, e.clientY);
		const id = e.pointerId;
		const timer = window.setTimeout(() => {
			const h = holdPick;
			holdPick = null;
			if (!h || h.id !== id || !current || !paint || snapped) return;
			if (lines.hit(paint.activeLayer, h.p[0], h.p[1], fit * zoom) < 0) return;
			// not a stroke after all: what it painted goes, and the line under it is picked
			paint.cancelStroke();
			linePts = null;
			current = null;
			strokeDown = false;
			clearTimeout(shapeTimer);
			if (sendLive()) {
				flushLive();
				onlive?.({ type: 'cancel', k: liveK });
			}
			pickStroke(h.p);
			// still pressed: moving on drags what was picked (its twins too), one step
			if (picking2 && paint) {
				linePointer = id;
				lines.down(paint.activeLayer, h.p[0], h.p[1], false, fit * zoom);
			}
		}, HOLD_PICK_MS);
		holdPick = { id, x: e.clientX, y: e.clientY, p, timer };
	}

	/** Pick the editable stroke at `p` (and its twins with symmetry on) to edit, from another tool. */
	function pickStroke(p: [number, number]) {
		if (!paint) return;
		if (xf) {
			xf = null;
			paint.transformCancel();
		}
		editShape = null;
		picking2 = lines.pick(paint.activeLayer, p[0], p[1], fit * zoom);
		schedule();
		notify();
	}

	let picking: number | null = null;
	/** the text tool's press, while it lasts */
	let textPointer: number | null = null;
	/**
	 * tools that paint on a press: on a text layer they ask first (the rest, a transform
	 * say, rasterize it as they finish; undo brings the text back)
	 */
	const RASTERIZING = new Set([
		'brush',
		'fill',
		'lasso-fill',
		'liquify',
		'gradient',
		'smudge',
		'clone'
	]);

	/** the last pointer event of a press (Shift pressed or let go replays it) */
	let lastPointer: PointerEvent | null = null;

	/** Shift went down or up mid-drag: drags it constrains follow at once, the pointer still. */
	function shiftChanged(shiftKey: boolean) {
		const e = lastPointer;
		if (!e) return;
		if (marquee && !shiftKey) marquee = { ...marquee, shapes: true };
		const constrained =
			xfDrag ||
			(marquee && marquee.kind !== 'lasso') ||
			shapeDrag ||
			guideDrag ||
			grad ||
			penDrag ||
			poly ||
			measuring !== null ||
			(linePointer !== null && !lines.erasing);
		if (!constrained) return;
		move(
			new PointerEvent('pointermove', {
				clientX: e.clientX,
				clientY: e.clientY,
				pointerId: e.pointerId,
				pointerType: e.pointerType,
				shiftKey,
				altKey: e.altKey,
				ctrlKey: e.ctrlKey,
				metaKey: e.metaKey
			})
		);
		if (linePointer !== null) lineMoveNow();
	}

	function move(e: PointerEvent) {
		lastPointer = e;
		if (oncursor) oncursor(toCanvas(e.clientX, e.clientY));
		if (moveStrokes && !xf && paint && linePointer === null) {
			const [x, y] = toCanvas(e.clientX, e.clientY);
			const list = lines.strokesOf(paint.activeLayer);
			const i = lines.hit(paint.activeLayer, x, y, fit * zoom);
			hoverStroke = i >= 0 ? list[i].id : null;
		}
		if (
			holdPick?.id === e.pointerId &&
			Math.hypot(e.clientX - holdPick.x, e.clientY - holdPick.y) > 3
		)
			endHoldPick();
		if (textPointer === e.pointerId) {
			const [x, y] = toCanvas(e.clientX, e.clientY);
			return ontext?.(x, y, 'move');
		}
		if (penMove(e)) return;
		if (sizeDrag && e.pointerId === sizeDrag.id) {
			// right = bigger; the slider's range is ¼× to the largest size (about 6×)
			const s = sizeDrag.size * Math.exp((e.clientX - sizeDrag.x) / 120);
			onsize?.(Math.round(Math.min(prefs.maxBrushSize, Math.max(0.25, s)) * 100) / 100);
			cursorAt = sizeDrag.at;
			return;
		}
		if (shapeDrag && e.pointerId === shapeDrag.id && editShape) {
			editShape = moveHandle(
				editShape,
				shapeDrag.index,
				toCanvas(e.clientX, e.clientY),
				e.shiftKey,
				guides ?? null,
				shapeDrag.from
			);
			redrawShape(editShape);
			return;
		}
		if (linePointer === e.pointerId && paint) {
			if (lines.erasing) {
				const events = e.getCoalescedEvents?.() ?? [];
				for (const ev of events.length ? events : [e]) {
					const [x, y] = toCanvas(ev.clientX, ev.clientY);
					lines.eraseTo(x, y);
				}
				cursorAt = toCanvas(e.clientX, e.clientY);
			} else {
				// (redrawn once a frame, however fast the pointer reports)
				const [x, y] = toCanvas(e.clientX, e.clientY);
				lineMove = { layer: paint.activeLayer, x, y, shift: e.shiftKey };
				if (!lineMoveRaf) lineMoveRaf = requestAnimationFrame(lineMoveNow);
			}
			return;
		}
		if (zoomDrag && e.pointerId === zoomDrag.id) {
			const want = zoomDrag.z * Math.exp((zoomDrag.y - e.clientY) * 0.006);
			zoomAt(want / zoom, zoomDrag.cx, zoomDrag.cy);
			return;
		}
		if (guideDrag) return guideMove(toCanvas(e.clientX, e.clientY), e.shiftKey);
		if (tool === 'liquify') hoverAt = toCanvas(e.clientX, e.clientY);
		lineHover =
			e.shiftKey && !current && lastEnd && tool === 'brush' && e.pointerType !== 'touch'
				? snapToGrid(guides, toCanvas(e.clientX, e.clientY))
				: null;
		cursorAt =
			(tool === 'brush' || tool === 'clone') && e.pointerType !== 'touch' && !adjustOverlay
				? toCanvas(e.clientX, e.clientY)
				: null;
		strokeDown = !!current;
		hoverHandle =
			editShape && cursorAt
				? shapeHandles(editShape).some(
						(q) =>
							Math.hypot(cursorAt![0] - q[0], cursorAt![1] - q[1]) <=
							14 / Math.max(0.01, fit * zoom)
					)
				: false;
		if (grad && e.pointerId === grad.id) {
			const to = toCanvas(e.clientX, e.clientY);
			grad = { ...grad, to: e.shiftKey ? snapAngle(grad.from, to, 45) : to };
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
			const to = toCanvas(e.clientX, e.clientY);
			measureSeg = [measureSeg[0], e.shiftKey ? snapAngle(measureSeg[0], to, 15) : to];
			return;
		}
		if (marquee) {
			const p = toCanvas(e.clientX, e.clientY);
			if (marquee.kind === 'lasso') marquee = { ...marquee, pts: [...marquee.pts, p] };
			else marqueeTo(p, e.shiftKey && marquee.shapes);
			return;
		}
		if (poly) poly = { ...poly, at: polyAt(e) };
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
		inkMark =
			inkSnap && tool === 'brush' && !eraser && (current ? inkStroke() : e.pointerType !== 'touch')
				? inkTarget(toCanvas(e.clientX, e.clientY))
				: null;
		if (!current) return;
		if (snapped) return; // the shape is set; lift the pen to finish
		const events = e.getCoalescedEvents?.() ?? [];
		for (const ev of events.length ? events : [e]) feed(ev);
		armShape(e);
		schedule();
	}

	function up(e: PointerEvent) {
		strokeDown = false;
		snapHint = null;
		if (holdPick?.id === e.pointerId) endHoldPick();
		if (textPointer === e.pointerId) {
			textPointer = null;
			const [x, y] = toCanvas(e.clientX, e.clientY);
			return ontext?.(x, y, 'up');
		}
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
			if (shapeOf) lines.reshaped();
			notify();
			return;
		}
		if (linePointer === e.pointerId) {
			linePointer = null;
			if (lines.erasing) lines.eraseEnd();
			else if (paint) {
				lineMoveNow();
				lines.up(paint.activeLayer);
			}
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
			const from = xfDrag.from;
			xfDrag = null;
			const t = xfTap;
			const away = xfAway;
			xfTap = xfAway = null;
			if (t && Math.hypot(e.clientX - t.x, e.clientY - t.y) < 4) {
				xf = from;
				pickStroke(t.p);
			} else if (
				away &&
				e.type !== 'pointercancel' &&
				Math.hypot(e.clientX - away.x, e.clientY - away.y) < 4
			) {
				// a tap away from the box: done (the tiny nudge of the tap left out)
				xf = from;
				transformCommit();
			}
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
		const inkEnd = !snapped && inkStroke() ? inkTarget(toCanvas(e.clientX, e.clientY)) : null;
		if (inkEnd) endOnInk(inkEnd);
		inkMark = null;
		inkCache = null;
		// a snapped shape stays editable (not on shared canvases: others already have it)
		editShape = snapped && !onlive && !maskStroke ? snappedShape : null;
		const lineLayer = current.layer ?? 0;
		const line = linePts ? finishLineStroke(snapped ? snappedShape : null) : null;
		paint.endStroke(brush);
		/** a plain layer's stroke kept editable: its part of the step */
		let kept: ReturnType<typeof lines.added> = undefined;
		if (line) {
			// its drawing is known now (a first stroke gives an empty cel one)
			const erase = !!line.strokes[0]?.erase;
			const effect = erase
				? lines.erased(lineLayer, line.strokes, strokeLimit())
				: lines.added(lineLayer, line.strokes, strokeLimit());
			shapeOf = editShape && effect && !erase ? { layer: lineLayer, id: line.strokes[0].id } : null;
			if (effect && !isLineLayer(lineLayer)) kept = effect;
			else if (effect) {
				log.add(current);
				if (!erase) log.labelLast(snapped ? 'Shape' : 'Line');
				log.attachLast(effect);
				current = null;
			}
		} else shapeOf = null;
		snapped = false;
		snappedShape = null;
		if (!current) {
			if (sendLive()) {
				flushLive();
				onlive?.({ type: 'end', k: liveK });
			}
			schedule();
			notify();
			return;
		}
		if (current.clone && !cloneAligned) cloneKept = null;
		const end = current.points.at(-1);
		if (end) lastEnd = { x: end.x, y: end.y, p: end.pressure };
		if (maskStroke) log.layerChange();
		else {
			log.add(current);
			if (kept) log.attachLast(kept);
			if (lineStroke) log.labelLast('Straight line');
			else if (
				closeFill &&
				!current.eraser &&
				!current.clone &&
				!onlive &&
				fillClosed(current.points)
			)
				log.labelLast('Close and fill');
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
			const layer = current.layer ?? 0;
			const line = linePts ? finishLineStroke(null) : null;
			paint.endStroke(brush);
			const effect = !line
				? null
				: line.strokes[0]?.erase
					? lines.erased(layer, line.strokes, strokeLimit())
					: lines.added(layer, line.strokes, strokeLimit());
			if (effect) {
				log.add(current);
				log.attachLast(effect);
				current = null;
				schedule();
				notify();
				return;
			}
			if (maskStroke) log.layerChange();
			else log.add(current);
			current = null;
			schedule();
			notify();
		}
	});

	// Ctrl/⌘ + wheel (and trackpad pinch) zooms at the pointer; the plain wheel pans once
	// zoomed in. The preferences can swap them (the plain wheel zooms, Ctrl/⌘ pans), zoom
	// towards the middle instead, change the speed, or jump instead of gliding.
	$effect(() => {
		const onWheel = (e: WheelEvent) => {
			const ctrl = e.ctrlKey || e.metaKey;
			// (a trackpad pinch comes as Ctrl + a fraction of a pixel: always a zoom)
			const pinch = e.ctrlKey && !Number.isInteger(e.deltaY);
			if (pinch || (prefs.wheel === 'zoom' ? !ctrl : ctrl)) {
				e.preventDefault();
				const step = wheelZoomStep(e.deltaY, e.deltaMode, prefs.zoomSpeed);
				if (prefs.zoomToPointer) zoomBy(step, e.clientX, e.clientY);
				else zoomBy(step);
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
		if (e.key === 'Shift' && !e.repeat && current && snapped) perfectSnapped();
		if (e.key === 'Shift' && !e.repeat) shiftChanged(true);
		if (poly && !typing(e.target) && (e.key === 'Enter' || e.key === 'Escape')) {
			if (e.key === 'Enter') closePoly();
			else poly = null;
		}
		if (e.code === 'Space' && hovering && !typing(e.target)) {
			e.preventDefault();
			spaceHeld = true;
		}
		if (e.key === 'Control' || e.key === 'Meta') ctrlHeld = hovering && !e.altKey;
		if (e.key === 'Alt') ctrlHeld = false;
	}
	function keyup(e: KeyboardEvent) {
		if (e.key === 'Shift') {
			shiftHeld = false;
			lineHover = null;
			shiftChanged(false);
		}
		if (e.code === 'Space') spaceHeld = false;
		if (e.key === 'Control' || e.key === 'Meta') ctrlHeld = false;
	}

	/** The history panel: go `steps` back (negative) or forward (positive) in one go. */
	export function historyGo(steps: number) {
		if (!paint) return;
		lines.cancel();
		editShape = null;
		if (xf) transformCancel();
		for (let i = 0; i < Math.abs(steps); i++) {
			const ok = steps < 0 ? paint.undo() : paint.redo();
			if (!ok) break;
			if (steps < 0) log.undo();
			else log.redo();
		}
		lines.check();
		schedule();
		notify();
	}

	export function undo() {
		editShape = null;
		if (xf) return transformCancel();
		lines.cancel();
		if (paint?.undo()) log.undo();
		lines.check();
		schedule();
		notify();
	}

	export function redo() {
		lines.cancel();
		if (paint?.redo()) log.redo();
		lines.check();
		schedule();
		notify();
	}

	/**
	 * A step of the app's own in the history (an animation timeline edit): undo and redo
	 * reach it in order with the drawing steps (`undoNote` / `redoNote` say when).
	 * `join`: it belongs with the step just before (a stroke that started a drawing).
	 */
	export function historyNote(id: number, label: string, join = false) {
		if (!paint?.pushNote(id)) return false;
		log.layerChange();
		log.labelLast(label);
		if (join && paint.joinLastEdits()) log.joinLast();
		notify();
		return true;
	}
	export const undoNote = () => paint?.undoNote() ?? 0;
	export const redoNote = () => paint?.redoNote() ?? 0;

	// ---- text layers (see text.ts): typed live on the layer, one undo step ----

	/** Start typing on text layer `id`, or (0) on a new layer; its id (0: locked, hidden…). */
	export function textBegin(id: number): number {
		if (!paint) return 0;
		editShape = null;
		return paint.textBegin(id, '');
	}

	/** Show the text on its layer as it is now; its box (measured), or null if not typing. */
	export function textPreview(spec: TextSpec) {
		if (!paint?.textLayer()) return null;
		const r = renderText(spec, width, height);
		paint.textPreview(textJson(spec), r.x, r.y, r.w, r.h, r.rgba);
		schedule();
		return r.layout;
	}

	/** Keep the text typed (one undo step). */
	export function textCommit(label = 'Text') {
		if (paint) changed(paint.textCommit(), undefined, label);
	}

	export function textCancel() {
		if (!paint) return;
		paint.textCancel();
		schedule();
		notify();
	}

	/** The text layer being typed on (0: none). */
	export const textLayer = () => paint?.textLayer() ?? 0;

	/** Turn a text or line layer into a plain one (one undo step). */
	export function rasterizeLayer(id: number) {
		if (!paint) return;
		if (isLineLayer(id)) {
			if (lines.sel?.layer === id) lines.deselect();
			changed(paint.rasterizeLines(id), undefined, 'Convert to pixels');
		} else changed(paint.rasterizeText(id), undefined, 'Rasterize text');
	}

	// ---- line layers (see lines.ts and line-layers.svelte.ts) ----

	/** A new line layer above the current one (one undo step). */
	export function addLineLayer(name = '') {
		if (paint) changed(paint.addLineLayer(name) !== 0, undefined, 'New line layer');
	}
	/** whether a layer is a line layer */
	export const isLines = (id: number) => isLineLayer(id);
	/** The selected line strokes go (one undo step). */
	export const deleteLines = () => lines.deleteSelected();
	export const deselectLines = () => lines.deselect();
	/** Move the selected line strokes (arrow keys). */
	export const nudgeLines = (dx: number, dy: number) => lines.nudge(dx, dy);
	/** how many line strokes are selected */
	export const linesSelected = () => lines.sel?.ids.length ?? 0;
	/** How many editable strokes a layer has (the drawing showing). */
	export const layerStrokes = (id: number) => pickable(lines.strokesOf(id)).length;
	/** A stroke picked with another tool (Move, a held brush) is being edited. */
	export const strokePicked = () => picking2;
	/** Move shows the active layer's editable strokes to pick (no layer box yet). */
	export const moveShowsStrokes = () => moveStrokes && !xf;
	/** a drawing's line strokes */
	export const lineStrokes = (drawing: number): readonly LineStroke[] => lines.store.get(drawing);
	/** the drawings that have line strokes */
	export const lineDrawings = () => lines.store.drawings();
	/** a drawing's line strokes as bytes, for the project file (null: none) */
	export function lineData(drawing: number): Uint8Array | null {
		const list = lines.store.get(drawing);
		if (!list.length) return null;
		// a plain layer's: with the pixels under its strokes
		return lines.store.isPlain(drawing)
			? encodeLineData(list, paint?.basePacked(drawing) ?? null)
			: encodeLines(list);
	}
	/** Put a drawing's line strokes back from the project file (no undo step; the pixels
	 * come with the drawing). False if the bytes aren't line data. */
	export function putLineData(drawing: number, bytes: Uint8Array): boolean {
		try {
			const { strokes: list, base } = decodeLineData(bytes);
			if (base && !paint?.putBasePacked(drawing, base)) return false;
			lines.store.setPlain(drawing, !!base);
			lines.store.set(drawing, list);
			void lines.ready(list);
			return true;
		} catch {
			return false;
		}
	}
	/** A plain layer drawing's base changes since last asked (drawing together sends them). */
	export const baseChanges = (drawing: number) => paint?.baseChangesPacked(drawing) ?? null;
	/** All of a plain layer drawing's base (someone joining). */
	export const baseAll = (drawing: number) => paint?.basePacked(drawing) ?? null;
	/** Someone else's base changes for a drawing (no undo step). */
	export function putBaseChanges(drawing: number, bytes: Uint8Array) {
		lines.store.setPlain(drawing);
		return paint?.putBaseChanges(drawing, bytes) ?? false;
	}
	/** Whether a drawing's strokes lie over a plain layer's pixels. */
	export const plainStrokes = (drawing: number) => lines.store.isPlain(drawing);
	/**
	 * Set a drawing's line strokes without an undo step (drawing together: someone else's
	 * change, whose pixels arrive as tiles). `redraw`: draw its pixels from them too, when
	 * it shows (they don't come with it).
	 */
	export function setLinesQuiet(
		drawing: number,
		list: readonly LineStroke[],
		redraw = false,
		plain = false
	) {
		if (plain) lines.store.setPlain(drawing);
		lines.store.set(drawing, list);
		void lines.ready(list);
		lines.check();
		if (redraw) {
			lines.markStale(drawing);
			const layer = layerList().find((l) => l.drawing === drawing && l.lines);
			if (layer) void lines.showing([layer.id]);
		}
	}
	/** Make a layer a line layer or a plain one without an undo step (drawing together). */
	export function setLinesLayerQuiet(id: number, on: boolean) {
		if (paint?.setLinesSilent(id, on)) notify();
	}

	// ---- drawings (animation cels) ----
	// Each layer shows one drawing; the others are kept by the engine. A timeline
	// says which drawing each layer shows on each frame (see the app's timeline).

	/** Show other drawings: [layer id, drawing id] pairs (drawing 0 = an empty cel). */
	export function showDrawings(pairs: [number, number][]): boolean {
		if (!paint) return false;
		editShape = null;
		lines.cancel();
		const ok = paint.showDrawings(new Uint32Array(pairs.flat()));
		lines.check();
		// drawings made as strokes (boiled) get their pixels as they first show
		if (ok) void lines.showing(pairs.map(([l]) => l));
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
	/** Drawings up to `id` are kept elsewhere (a frame store): new ones get ids past it. */
	export const reserveDrawings = (id: number) => paint?.reserveDrawings(id);
	/** A copy of a drawing (its line strokes too); returns the copy's id. */
	export function copyDrawing(drawing: number) {
		const id = paint?.copyDrawing(drawing) ?? 0;
		if (id) lines.store.copy(drawing, id);
		return id;
	}
	/**
	 * A boiled copy of a drawing: its lines nudged up to `amount` px, as if traced again.
	 * A line drawing's strokes wobble instead (they stay editable), drawn as it shows.
	 */
	export function boilDrawing(drawing: number, amount: number, seed: number) {
		if (!paint) return 0;
		const strokes = lines.store.get(drawing);
		// (a plain layer's strokes lie over pixels: those wobble, and the copy keeps none)
		if (!strokes.length || lines.store.isPlain(drawing))
			return paint.boilDrawing(drawing, amount, seed);
		const id = paint.copyDrawing(drawing);
		if (!id) return 0;
		lines.store.set(id, boilStrokes(strokes, amount, seed));
		lines.markStale(id);
		return id;
	}
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
		shrink = 1,
		/** on transparency (straight alpha), not on the paper: for exports with alpha */
		transparent = false
	): { rgba: Uint8ClampedArray<ArrayBuffer>; width: number; height: number } | null {
		if (!paint) return null;
		const k = Math.max(1, Math.round(shrink));
		const flat = new Uint32Array(pairs.flat());
		const rgba = transparent ? paint.renderFrameAlpha(flat, k) : paint.renderFrame(flat, k);
		return {
			rgba: new Uint8ClampedArray(rgba.buffer as ArrayBuffer, rgba.byteOffset, rgba.length),
			width: Math.ceil(width / k),
			height: Math.ceil(height / k)
		};
	}
	/**
	 * Onion skin: drawings as tinted lines on transparency, farthest first, made `shrink`
	 * times smaller. Pass the result to `onionImage` (as an ImageBitmap).
	 */
	export function onionRgba(
		entries: { drawing: number; colour: string; opacity: number }[],
		shrink = 1
	): ImageData | null {
		if (!paint) return null;
		const k = Math.max(1, Math.round(shrink));
		const flat = entries.flatMap((e) => [
			e.drawing,
			parseInt(e.colour.replace('#', ''), 16) || 0,
			Math.round(Math.min(1, Math.max(0, e.opacity)) * 255)
		]);
		const rgba = paint.onionRgba(new Uint32Array(flat), k);
		const w = Math.ceil(width / k);
		const h = Math.ceil(height / k);
		return new ImageData(
			new Uint8ClampedArray(rgba.buffer as ArrayBuffer, rgba.byteOffset, rgba.length),
			w,
			h
		);
	}

	/** The drawing the next undo / redo changes (0: none, or not a drawing). */
	export const undoDrawing = () => paint?.undoDrawing() ?? 0;
	export const redoDrawing = () => paint?.redoDrawing() ?? 0;

	/** Erase the active layer. */
	export function clear() {
		if (!paint) return;
		const layer = paint.activeLayer;
		const mask = paint.isEditingMask();
		// a line layer stays one: its strokes go
		if (!mask && isLineLayer(layer)) {
			lines.deselect();
			void lines.edit(layer, [], 'Clear layer');
			return;
		}
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
		if (!paint) return;
		const from = paint.layerDrawing(id);
		const copy = paint.duplicateLayer(id);
		if (copy && assistLayers.has(id)) assistLayers.add(copy);
		// a line layer's copy keeps its strokes (on its own drawing)
		if (copy && from) lines.store.copy(from, paint.layerDrawing(copy));
		changed(copy !== 0, undefined, 'Duplicate layer');
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
		const { name, visible, opacity, blend, locked, alphaLock, clip, style } = l;
		return { name, visible, opacity, blend, locked, alphaLock, clip: !!clip, style };
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
			record,
			styleNumbers(s.style)
		);
	}

	// ---- drawing together: silent changes (never undo steps) ----

	/** A new layer on top, not an undo step (a shared layer). Returns its id (0: no room). */
	export function addLayerQuiet(name: string): number {
		if (!paint) return 0;
		const id = paint.addLayerSilent(name);
		if (id) notify();
		return id;
	}
	/** Change layer settings without an undo step (locking someone else's layer). */
	export function setLayerQuiet(id: number, patch: Partial<LayerSettings>) {
		const cur = paint && settings(id);
		if (!cur) return;
		if (setRaw(id, { ...cur, ...patch }, false)) {
			schedule();
			notify();
		}
	}
	/** The drawing a layer shows, starting one if it has none yet. */
	export function layerDrawingOf(id: number): number {
		if (!paint) return 0;
		let d = paint.layerDrawing(id);
		if (!d) {
			d = paint.newDrawing();
			paint.showDrawings(new Uint32Array([id, d]));
		}
		return d;
	}
	/**
	 * Move a layer to `to` (0 = bottom) without an undo step: someone else moved a shared
	 * layer. Undo and redo are cleared (their steps place layers by position). False when
	 * busy (mid-stroke): try again.
	 */
	export function moveLayerQuiet(id: number, to: number): boolean {
		if (!paint?.moveLayerSilent(id, to)) return false;
		paint.clearHistory();
		log.forgetHistory();
		schedule();
		notify();
		return true;
	}
	/** Move a layer right above (or below) another, quietly, as above. */
	export function moveLayerQuietNextTo(id: number, ref: number, above: boolean): boolean {
		const list = layerList();
		const from = list.findIndex((l) => l.id === id);
		const r = list.findIndex((l) => l.id === ref);
		// a layer that isn't here: nothing to do
		if (from < 0 || r < 0 || from === r) return true;
		const to = above ? (r < from ? r + 1 : r) : r < from ? r : r - 1;
		return to === from || moveLayerQuiet(id, to);
	}
	/** Remove a layer without an undo step (someone else deleted a shared layer); as above. */
	export function removeLayerQuiet(id: number): boolean {
		if (!paint?.removeLayerSilent(id)) return false;
		paint.clearHistory();
		log.forgetHistory();
		schedule();
		notify();
		return true;
	}
	/** After a quiet change to the layers: undo and redo are cleared (as above). */
	function quietly(ok: boolean): boolean {
		if (!ok || !paint) return false;
		paint.clearHistory();
		log.forgetHistory();
		schedule();
		notify();
		return true;
	}
	/** A new group holding layer `id`, quietly; its id (0: busy or no such layer). */
	export function addGroupQuiet(id: number): number {
		const gid = paint?.addGroup(id) ?? 0;
		quietly(gid !== 0);
		return gid;
	}
	/** A group's settings, quietly (someone else changed them). */
	export function setGroupQuiet(id: number, patch: Partial<GroupSettings>): boolean {
		const cur = paint && groupSettings(id);
		if (!cur) return true;
		const s = { ...cur, ...patch };
		if (paint!.setGroup(id, s.name, s.visible, s.opacity, false)) {
			schedule();
			notify();
		}
		return true;
	}
	/** Move a layer right above (or below) `ref` (0: where it is) and into `group` (0 =
	 * none), quietly. False when busy. */
	export function placeLayerQuiet(id: number, ref: number, above: boolean, group: number): boolean {
		const list = layerList();
		const from = list.findIndex((l) => l.id === id);
		if (from < 0) return true;
		const r = ref ? list.findIndex((l) => l.id === ref) : from;
		if (r < 0) return true;
		const to = r === from ? from : above ? (r < from ? r + 1 : r) : r < from ? r : r - 1;
		if (to === from && list[from].group === group) return true;
		return quietly(!!paint?.moveLayerTo(id, to, group));
	}
	/** Take a group's layers out of it and remove it, quietly. */
	export function ungroupQuiet(id: number): boolean {
		if (!groupList().some((g) => g.id === id)) return true;
		return quietly(!!paint?.ungroup(id));
	}
	/** Merge a layer into the one below, quietly (drawing together: the merged pixels and
	 * the layer's removal go to the others as the session's edits). */
	export function mergeDownQuiet(id: number): boolean {
		if (!paint) return false;
		const layers = layerList();
		const below = layers[layers.findIndex((l) => l.id === id) - 1]?.id;
		if (below === undefined || !paint.mergeDown(id)) return false;
		log.merge(id, below);
		return quietly(true);
	}
	/** Flatten a group into its bottom layer, quietly (as above); that layer's id (0: no). */
	export function mergeGroupQuiet(id: number): number {
		if (!paint) return 0;
		const members = layerList().filter((l) => l.group === id);
		const into = paint.mergeGroup(id);
		if (!into) return 0;
		log.batch(() => {
			for (const l of members.slice(1).reverse()) log.merge(l.id, into);
		});
		quietly(true);
		return into;
	}
	/** The groups now (id, name, eye, opacity). */
	export const groupsNow = () => groupList();

	/** An adjustment layer on top, not an undo step (a shared one); `spec` is its JSON. */
	export function addAdjustmentLayerQuiet(spec: string, name: string): number {
		if (!paint) return 0;
		const a = parseSpec(spec);
		const id = paint.addAdjustmentLayerSilent(
			name,
			spec,
			a.hue,
			a.saturation,
			a.brightness,
			a.contrast,
			toneTables(a),
			mapTable(a.map)
		);
		if (id) {
			schedule();
			notify();
		}
		return id;
	}
	/** Change an adjustment layer, not an undo step (changed elsewhere). False when busy. */
	export function setAdjustmentQuiet(id: number, spec: string): boolean {
		if (!paint) return false;
		const a = parseSpec(spec);
		const ok = paint.setAdjustmentSilent(
			id,
			spec,
			a.hue,
			a.saturation,
			a.brightness,
			a.contrast,
			toneTables(a),
			mapTable(a.map)
		);
		if (ok) {
			schedule();
			notify();
		}
		return ok;
	}
	/** A layer's mask as packed tiles (empty: it has no mask). */
	export const maskPacked = (id: number) => paint?.maskTilesPacked(id) ?? new Uint8Array();
	/** Put a layer's mask (empty bytes remove it), not an undo step. False when busy. */
	export function putMaskQuiet(id: number, packed: Uint8Array): boolean {
		if (!paint?.putMaskTiles(id, packed)) return false;
		schedule();
		notify();
		return true;
	}
	/** Every layer now, bottom → top, with how it shows (and an adjustment layer's JSON). */
	export const sharedLayerList = () =>
		layerList().map((l) => ({
			id: l.id,
			name: l.name,
			adjust: l.adjust,
			group: l.group,
			look: {
				visible: l.visible,
				opacity: l.opacity,
				blend: l.blend as string,
				alphaLock: l.alphaLock,
				clip: !!l.clip,
				style: l.style
			}
		}));
	/** Paint layers now, bottom → top (not adjustment layers). */
	export const paintLayers = () =>
		layerList()
			.filter((l) => !l.adjust)
			.map((l) => ({
				id: l.id,
				name: l.name,
				look: {
					visible: l.visible,
					opacity: l.opacity,
					blend: l.blend as string,
					alphaLock: l.alphaLock,
					clip: !!l.clip,
					style: l.style
				}
			}));

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
			paint.commitLayer(
				id,
				b.name,
				b.visible,
				b.opacity,
				b.blend,
				b.locked,
				b.alphaLock,
				b.clip,
				styleNumbers(b.style)
			)
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
		/** drawing assist is on */
		assist?: boolean;
		/** its layer style (layer-style.ts) */
		style?: LayerStyle | null;
		/** a text layer's text (JSON, see text.ts) */
		text?: string | null;
		/** a line layer (its strokes are kept by drawing: `lineData`) */
		lines?: boolean;
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
				adjust: l.adjust,
				...(assistLayers.has(l.id) ? { assist: true } : {}),
				style: l.style,
				...(l.text ? { text: l.text } : {}),
				...(l.lines ? { lines: true } : {})
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
					clip: l.clip,
					style: l.style ?? null
				},
				false
			);
			if (l.mask) paint.restoreMask(id, l.mask);
			if (l.assist) assistLayers.add(id);
			if (l.text) paint.setTextSilent(id, l.text);
			if (l.lines) paint.setLinesSilent(id, true);
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
		for (const id of before) {
			paint.deleteLayer(id);
			assistLayers.delete(id);
		}
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

	/**
	 * The picture as a layered document (PSD). With `frame`, an animation frame: each
	 * layer shows the drawing the pairs give it (none: empty), and the flat picture is
	 * that frame (on transparency if asked); masks aren't included then.
	 */
	export function exportLayered(frame?: {
		pairs: [number, number][];
		transparent?: boolean;
	}): PsdDoc | null {
		if (!paint || !ctx) return null;
		render();
		const groups = new Map(groupList().map((g) => [g.id, g]));
		const shown = frame ? new Map(frame.pairs) : null;
		const pixels = (l: LayerInfo) => {
			// a style has no place in the file: drawn into the pixels (with the mask)
			if (hasStyle(l.style)) {
				const d = shown ? (shown.get(l.id) ?? 0) : 0;
				if (shown && !d) return new Uint8Array(width * height * 4);
				return paint!.layerStyledRgba(l.id, d);
			}
			if (!shown) return paint!.layerRgba(l.id);
			const d = shown.get(l.id) ?? 0;
			const rgba = d ? paint!.drawingRgba(d) : null;
			return rgba && rgba.length === width * height * 4 ? rgba : new Uint8Array(width * height * 4);
		};
		const toLayer = (l: LayerInfo): PsdLayer => {
			const maskRgba =
				l.mask && !frame && !hasStyle(l.style) ? paint!.maskThumbnail(l.id, width, height) : null;
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
				rgba: pixels(l),
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
		const flat = frame ? renderFrame(frame.pairs, 1, frame.transparent)?.rgba : null;
		return {
			width,
			height,
			composite: flat ?? ctx.getImageData(0, 0, width, height).data,
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
		ctrlHeld = false;
		shiftHeld = false;
		lineHover = null;
	}}
/>

<div
	class="viewport"
	class:grab={spaceHeld || (ctrlHeld && tool !== 'transform')}
	class:moving-overlay={adjustOverlay && !!overlaySrc}
	class:picker={tool === 'picker'}
	class:filling={tool === 'fill'}
	class:outlined={showOutline && !hoverHandle}
	class:on-handle={hoverHandle}
	class:selecting={tool.startsWith('select')}
	class:transforming={tool === 'transform'}
	class:liquifying={tool === 'liquify'}
	class:texting={tool === 'text'}
	class:line-select={lineTool}
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
		oncursor?.(null);
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
		<canvas class="sel-tint" bind:this={tintEl} {width} {height} aria-hidden="true"></canvas>
		<canvas class="ants" bind:this={antsEl} {width} {height} aria-hidden="true"></canvas>
		{#if guides && (guideGrid.length || guideRays.length || guides.ruler.on || guideAxes.length || guideComp)}
			{@const hr = 8 / Math.max(0.01, fit * zoom)}
			<svg
				class="guides"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				style="--grid: {prefs.gridColour}; --guide: {prefs.guideColour}"
			>
				{#each guideGrid as [x1, y1, x2, y2], i (i)}
					<line class="grid" {x1} {y1} {x2} {y2} />
				{/each}
				{#each guideRays as [x1, y1, x2, y2], i (i)}
					<line class="ray" {x1} {y1} {x2} {y2} />
				{/each}
				{#if guideComp}
					{@const own = guides.composition.custom}
					<g
						class="composition"
						data-testid="composition-guide"
						data-preset={guides.composition.preset}
						data-name={own?.name}
					>
						<CompositionShapes
							parts={guideComp}
							color={own?.color ?? 'var(--guide, #e8492f)'}
							opacity={own?.opacity ?? 0.6}
						/>
					</g>
				{/if}
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
						{@const pin = edgePin([x, y], width, height, 2 * hr)}
						{#if pin}
							<!-- off the canvas: its handle waits on the edge, an arrow pointing to it -->
							{@const [px, py] = pin.at}
							{@const [ux, uy] = [Math.cos(pin.angle), Math.sin(pin.angle)]}
							<g class="vp-pin" data-testid="vp-handle" data-off="true">
								<polygon
									class="arrow"
									points="{px + ux * hr * 1.9},{py + uy * hr * 1.9} {px + ux * hr - uy * hr},{py +
										uy * hr +
										ux * hr} {px + ux * hr + uy * hr},{py + uy * hr - ux * hr}"
								/>
								<circle class="handle" cx={px} cy={py} r={hr} />
							</g>
						{:else}
							<circle class="handle" data-testid="vp-handle" cx={x} cy={y} r={hr} />
						{/if}
					{/each}
					{#if guideComp}
						<!-- the composition guide: its frame, a handle in the middle to move it and
							one on each corner to size it -->
						{@const [x, y, fw, fh] = compositionFrame(guides.composition, width, height)}
						<rect class="comp-frame" {x} {y} width={fw} height={fh} />
						<circle
							class="handle"
							data-testid="composition-move"
							cx={x + fw / 2}
							cy={y + fh / 2}
							r={hr}
						/>
						{#each [[x, y], [x + fw, y], [x + fw, y + fh], [x, y + fh]] as [cx, cy], k (k)}
							<rect
								class="handle"
								data-testid="composition-size"
								x={cx - hr}
								y={cy - hr}
								width={2 * hr}
								height={2 * hr}
							/>
						{/each}
					{/if}
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
		{#if moveStrokes && !xf}
			{@const shown = moveList}
			<svg
				class="xf stroke-hints"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="stroke-outlines"
				data-count={shown.length}
				data-hover={hoverStroke ?? ''}
			>
				{#each shown as s (s.id)}
					<polyline class="hint" class:hover={s.id === hoverStroke} points={linePoints(s)} />
				{/each}
			</svg>
		{/if}
		{#if lineTool && (lines.sel || lines.marquee)}
			{@const k = Math.max(0.01, fit * zoom)}
			{@const hr = 5 / k}
			{@const th = lines.transformHandles(k)}
			{@const ph = editShape ? null : lines.pointHandles()}
			<svg
				class="xf line-sel"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="line-selection"
				data-count={lines.sel?.ids.length ?? 0}
			>
				{#each lines.selected() as s (s.id)}
					<polyline class="line-path-under" points={linePoints(s)} />
					<polyline class="line-path" points={linePoints(s)} />
				{/each}
				{#if th.length === 5}
					<rect
						class="line-box"
						x={th[0][0]}
						y={th[0][1]}
						width={th[2][0] - th[0][0]}
						height={th[2][1] - th[0][1]}
					/>
					<line class="line-box" x1={th[4][0]} y1={th[0][1]} x2={th[4][0]} y2={th[4][1]} />
					{#each th.slice(0, 4) as [x, y], i (i)}
						<rect class="line-h" x={x - hr} y={y - hr} width={hr * 2} height={hr * 2} />
					{/each}
					<circle class="line-h line-turn" cx={th[4][0]} cy={th[4][1]} r={hr * 1.1} />
				{/if}
				{#if ph}
					{#each ph.pts as [x, y], i (i)}
						<circle class="line-pt" cx={x} cy={y} r={hr * 0.85} data-testid="line-point" />
					{/each}
				{/if}
				{#if lines.marquee}
					{@const [a, b] = lines.marquee}
					<rect
						class="line-marquee"
						x={Math.min(a[0], b[0])}
						y={Math.min(a[1], b[1])}
						width={Math.abs(b[0] - a[0])}
						height={Math.abs(b[1] - a[1])}
					/>
				{/if}
			</svg>
		{/if}
		{#if editShape && (tool === 'brush' || lineTool)}
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
				{measureUnit > 0
					? `${(len / measureUnit).toFixed(1)} units`
					: showLength(len, prefs.units, prefs.ppi)} · {Math.round(deg)}°
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
		{#if showOutline && cursorAt && (prefs.cursorWhilePainting || !strokeDown)}
			{@const px = 1 / Math.max(0.01, fit * zoom)}
			{@const small = brushR * fit * zoom < 3}
			{@const look =
				prefs.cursor === 'outline' && small
					? prefs.crosshairWhenSmall
						? 'crosshair'
						: 'dot'
					: prefs.cursor}
			{@const [cx, cy] = cursorAt}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="brush-cursor"
				data-r={brushR.toFixed(1)}
				data-look={look}
			>
				{#if look === 'outline'}
					<circle class="cur-out" {cx} {cy} r={brushR} />
					<circle class="cur-in" {cx} {cy} r={brushR} />
				{/if}
				{#if look === 'crosshair'}
					{@const [g, l] = [3 * px, 10 * px]}
					{@const d = `M${cx - l} ${cy}H${cx - g}M${cx + g} ${cy}H${cx + l}M${cx} ${cy - l}V${cy - g}M${cx} ${cy + g}V${cy + l}`}
					<path class="cur-out" {d} />
					<path class="cur-in" {d} />
				{:else}
					<circle class="cur-dot" {cx} {cy} r={1.5 * px} />
				{/if}
			</svg>
		{/if}
		{#if tool === 'clone' && cloneMark}
			{@const k = 1 / Math.max(0.01, fit * zoom)}
			{@const [mx, my] = cloneMark}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="clone-source"
			>
				{#each ['cur-out', 'cur-in'] as c (c)}
					<path class={c} d="M{mx - 8 * k} {my}H{mx + 8 * k}M{mx} {my - 8 * k}V{my + 8 * k}" />
				{/each}
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
		{#if (inkMark || lines.inkMark) && inkSnap}
			{@const [mx, my] = (inkMark ?? lines.inkMark)!}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="ink-snap"
				data-x={mx.toFixed(1)}
				data-y={my.toFixed(1)}
			>
				<circle class="ink-mark" cx={mx} cy={my} r={5 / Math.max(0.01, fit * zoom)} />
			</svg>
		{/if}
		{#if snapHint || lines.hint}
			{@const [[hx1, hy1], [hx2, hy2]] = (snapHint ?? lines.hint)!}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="snap-hint"
			>
				<line class="shift-line" x1={hx1} y1={hy1} x2={hx2} y2={hy2} />
			</svg>
		{/if}
		{#if tool === 'liquify' && hoverAt}
			<svg class="xf" viewBox="0 0 {width} {height}" preserveAspectRatio="none" aria-hidden="true">
				<circle class="liq" cx={hoverAt[0]} cy={hoverAt[1]} r={liquifySize} />
			</svg>
		{/if}
		{#if xfShape && !xf?.mesh}
			{@const hr = 7 / Math.max(0.01, fit * zoom)}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="transform-box"
			>
				<polygon points={xfShape.corners.map((p) => p.join(',')).join(' ')} />
				<line x1={xfShape.top[0]} y1={xfShape.top[1]} x2={xfShape.rotH[0]} y2={xfShape.rotH[1]} />
				{#each [...xfShape.corners, ...xfShape.edges] as [x, y], i (i)}
					<rect x={x - hr} y={y - hr} width={hr * 2} height={hr * 2} />
				{/each}
				<circle cx={xfShape.rotH[0]} cy={xfShape.rotH[1]} r={hr * 1.1} />
			</svg>
		{/if}
		{#if textFrame && tool === 'text'}
			<svg
				class="xf"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="text-frame"
			>
				<polygon class="text-frame" points={textFrame.map((p) => p.join(',')).join(' ')} />
			</svg>
		{/if}
		{#if marquee}
			<svg
				class="marquee"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
			>
				{#if marquee.kind === 'lasso'}
					<polygon points={marquee.pts.map((p) => p.join(',')).join(' ')} />
				{:else}
					{@const [[ax, ay], [bx, by]] = marquee.pts}
					{#if marquee.kind === 'rect'}
						<rect
							x={Math.min(ax, bx)}
							y={Math.min(ay, by)}
							width={Math.abs(bx - ax)}
							height={Math.abs(by - ay)}
						/>
					{:else}
						<ellipse
							cx={(ax + bx) / 2}
							cy={(ay + by) / 2}
							rx={Math.abs(bx - ax) / 2}
							ry={Math.abs(by - ay) / 2}
						/>
					{/if}
				{/if}
			</svg>
		{/if}
		{#if poly}
			{@const r = 5 / Math.max(0.01, fit * zoom)}
			<svg
				class="marquee"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="lasso-points"
			>
				<polygon
					points={[...poly.pts, ...(poly.at ? [poly.at] : [])].map((p) => p.join(',')).join(' ')}
				/>
				<circle class="first" cx={poly.pts[0][0]} cy={poly.pts[0][1]} {r} />
			</svg>
		{/if}
		{#if camera}
			{@const c = camera}
			<svg
				class="camera"
				viewBox="0 0 {width} {height}"
				preserveAspectRatio="none"
				aria-hidden="true"
				data-testid="camera"
			>
				<path
					class="outside"
					fill-rule="evenodd"
					d="M0 0H{width}V{height}H0Z M{c.x} {c.y}h{c.width}v{c.height}h{-c.width}Z"
				/>
				<rect class="frame" x={c.x} y={c.y} width={c.width} height={c.height} />
				{#if c.safe}
					{#each [0.9, 0.8] as k (k)}
						<rect
							class="safe"
							x={c.x + (c.width * (1 - k)) / 2}
							y={c.y + (c.height * (1 - k)) / 2}
							width={c.width * k}
							height={c.height * k}
						/>
					{/each}
					<line
						class="safe"
						x1={c.x + c.width / 2}
						y1={c.y + c.height / 2 - c.height * 0.03}
						x2={c.x + c.width / 2}
						y2={c.y + c.height / 2 + c.height * 0.03}
					/>
					<line
						class="safe"
						x1={c.x + c.width / 2 - c.height * 0.03}
						y1={c.y + c.height / 2}
						x2={c.x + c.width / 2 + c.height * 0.03}
						y2={c.y + c.height / 2}
					/>
				{/if}
			</svg>
		{/if}
		{#if onionImage}
			<div class="overlay-clip">
				<canvas class="overlay onion" bind:this={onionEl} data-testid="onion"></canvas>
			</div>
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
		{#each cursors as c (c.id)}
			<div
				class="remote-cursor"
				style="left: {(c.x / width) * 100}%; top: {(c.y / height) *
					100}%; --c: {c.color}; transform: scale({1 / Math.max(0.01, fit * zoom)})"
				aria-hidden="true"
			>
				<svg viewBox="0 0 16 16" width="16" height="16">
					<path d="M1 1 L1 13 L4.5 9.5 L7 15 L9 14 L6.5 8.5 L11.5 8.5 Z" />
				</svg>
				<span>{c.name}</span>
			</div>
		{/each}
	</div>
	{#if dropFill}
		<div
			class="drop-tol"
			role="group"
			aria-label="Drop fill spread"
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
			{#if !dropContinue}
				<button
					type="button"
					class="wide"
					title="Tap other areas to fill them with this colour too"
					onclick={() => {
						dropContinue = dropFill?.hex ?? null;
						dropFill = null;
					}}
				>
					Continue filling
				</button>
			{/if}
			<button type="button" aria-label="Done" onclick={() => (dropFill = null)}>✓</button>
		</div>
	{/if}
	{#if dropContinue}
		<div class="canvas-note" role="status" data-testid="colordrop-continue">
			<span class="swatch" style="background: {dropContinue}"></span>
			Tap to fill with this colour
			<button type="button" onclick={() => (dropContinue = null)}>Done</button>
		</div>
	{:else if tool === 'clone' && (!cloneFrom || cloneSetting)}
		<div class="canvas-note" role="status" data-testid="clone-hint">
			{cloneSetting
				? 'Tap where to copy from'
				: 'Alt+click (⌥+click) where to copy from, then paint'}
		</div>
	{/if}
	{#if editShape && (tool === 'brush' || lineTool) && ready}
		{@const name = shapeName(editShape)}
		{@const pair = (
			{
				circle: ['Ellipse', 'Circle'],
				ellipse: ['Ellipse', 'Circle'],
				rectangle: ['Rectangle', 'Square'],
				square: ['Rectangle', 'Square'],
				triangle: ['Triangle', 'Even triangle'],
				line: ['Line', 'Line at 15°']
			} as Record<string, [string, string]>
		)[name]}
		{@const bends = curvable(editShape) && editShape.kind === 'poly'}
		{#if pair || bends}
			<div
				class="shape-bar"
				role="toolbar"
				aria-label="Edit shape"
				bind:this={shapeBarEl}
				style:translate="{shapeBarOff.dx}px {shapeBarOff.dy}px"
			>
				<button
					type="button"
					class="grip"
					aria-label="Move this bar"
					title="Drag to move it out of the way (double-click: back to the top)"
					onpointerdown={dragShapeBar}
					onkeydown={keyShapeBar}
					ondblclick={() => placeShapeBar(0, 0)}
				>
					<svg viewBox="0 0 10 16" width="10" height="16" aria-hidden="true">
						{#each [3, 8, 13] as y (y)}
							<circle cx="2.5" cy={y} r="1.4" /><circle cx="7.5" cy={y} r="1.4" />
						{/each}
					</svg>
				</button>
				<span>Edit shape</span>
				{#if bends && editShape.kind === 'poly'}
					<button
						type="button"
						aria-pressed={!editShape.smooth}
						title="Straight lines between the corners"
						onclick={() => setShapeCurve(false)}
					>
						Straight
					</button>
					<button
						type="button"
						aria-pressed={!!editShape.smooth}
						title="A smooth curve through the corners (kept for your next shapes)"
						onclick={() => setShapeCurve(true)}
					>
						Curved
					</button>
				{/if}
				{#if pair && bends}<i class="sep" aria-hidden="true"></i>{/if}
				{#if pair}
					<button
						type="button"
						aria-pressed={!(name === 'circle' || name === 'square')}
						disabled={editShape.kind !== 'ellipse'}
						title={editShape.kind === 'ellipse'
							? 'Free: each axis drags on its own'
							: 'Drag the corners to change it'}
						onclick={() => setShapeForm('free')}
					>
						{pair[0]}
					</button>
					<button
						type="button"
						aria-pressed={name === 'circle' || name === 'square'}
						title="Perfect (tap with another finger, or press Shift, while holding)"
						onclick={() => setShapeForm('perfect')}
					>
						{pair[1]}
					</button>
				{/if}
			</div>
		{/if}
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
	{#if prefs.rulers && ready && rot === 0 && fit}
		{@const sx = fit * zoom * (flipped ? -1 : 1)}
		<Rulers
			{width}
			{height}
			viewW={vpW}
			viewH={vpH}
			originX={vpW / 2 + panX - (width / 2) * sx}
			originY={vpH / 2 + panY - (height / 2) * fit * zoom}
			scaleX={sx}
			scaleY={fit * zoom}
			unit={prefs.units}
			ppi={prefs.ppi}
		/>
	{/if}
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
		/* the app can choose the colour around the canvas */
		background: var(--pp-canvas-surround, var(--pp-surface-sunk));
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
	.viewport.texting {
		cursor: text;
	}
	.text-frame {
		fill: none;
		stroke: #2f6bff;
		stroke-width: 1.5;
		stroke-dasharray: 6 4;
		vector-effect: non-scaling-stroke;
	}
	.ants,
	.sel-tint,
	.marquee {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		pointer-events: none;
	}
	/* an overlay: it must never hide the drawing (the plain canvas rule paints paper white) */
	canvas.ants,
	canvas.sel-tint {
		background: transparent;
	}
	.shape-bar {
		position: absolute;
		top: 12px;
		left: 50%;
		z-index: 6;
		display: flex;
		align-items: center;
		gap: 4px;
		padding: 4px 4px 4px 4px;
		border-radius: 14px;
		background: var(--pp-ink, #16161a);
		color: var(--pp-on-ink, #fff);
		font: 600 13px var(--pp-font-body);
		transform: translateX(-50%);
		box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
	}
	.shape-bar .sep {
		width: 1px;
		height: 20px;
		margin: 0 4px;
		background: #3a3a42;
	}
	.shape-bar span {
		margin-right: 6px;
		color: var(--pp-on-ink-muted, #b9b9c2);
	}
	.shape-bar button {
		height: 30px;
		padding: 0 12px;
		border: 0;
		border-radius: 10px;
		background: #2a2a30;
		color: inherit;
		font: inherit;
		cursor: pointer;
	}
	.shape-bar button[aria-pressed='true'] {
		background: var(--pp-accent-strong, #e8492f);
		color: #fff;
	}
	.shape-bar .grip {
		width: 22px;
		padding: 0;
		display: grid;
		place-items: center;
		background: transparent;
		color: var(--pp-on-ink-muted, #b9b9c2);
		cursor: grab;
		touch-action: none;
	}
	.shape-bar .grip:active {
		cursor: grabbing;
	}
	.shape-bar .grip svg {
		fill: currentColor;
	}
	.shape-bar button:disabled {
		opacity: 0.45;
		cursor: default;
	}
	.marquee .first {
		fill: #fff;
		stroke: #3d6bff;
		stroke-width: 2;
		stroke-dasharray: none;
		vector-effect: non-scaling-stroke;
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
	.xf circle.ink-mark {
		fill: none;
		stroke: #3d6bff;
		stroke-width: 1.5;
		vector-effect: non-scaling-stroke;
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
		stroke-linecap: round;
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
	.xf .line-path,
	.xf .line-path-under {
		fill: none;
		stroke-linecap: round;
		stroke-linejoin: round;
		vector-effect: non-scaling-stroke;
	}
	.xf .line-path-under {
		stroke: rgba(255, 255, 255, 0.85);
		stroke-width: 3.5;
	}
	/* Move on a layer with editable strokes: each one, to pick */
	.stroke-hints .hint {
		fill: none;
		stroke: rgba(61, 107, 255, 0.75);
		stroke-width: 1.5;
		stroke-linecap: round;
		stroke-linejoin: round;
		vector-effect: non-scaling-stroke;
	}
	.stroke-hints .hint.hover {
		stroke: #3d6bff;
		stroke-width: 2.5;
	}
	.xf .line-path {
		stroke: #3d6bff;
		stroke-width: 1.5;
	}
	.xf .line-box {
		fill: none;
		stroke: #3d6bff;
		stroke-width: 1;
		stroke-dasharray: 5 4;
		vector-effect: non-scaling-stroke;
	}
	.xf .line-h {
		fill: #fff;
		stroke: #3d6bff;
		stroke-width: 1.5;
		vector-effect: non-scaling-stroke;
	}
	.xf .line-pt {
		fill: #3d6bff;
		stroke: #fff;
		stroke-width: 1.5;
		vector-effect: non-scaling-stroke;
	}
	.xf .line-marquee {
		fill: rgba(61, 107, 255, 0.08);
		stroke: #3d6bff;
		stroke-width: 1;
		stroke-dasharray: 4 3;
		vector-effect: non-scaling-stroke;
	}
	.viewport.line-select {
		cursor: default;
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
	.camera {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		pointer-events: none;
	}
	.camera .outside {
		fill: rgba(22, 22, 26, 0.32);
	}
	.camera .frame {
		fill: none;
		stroke: #3d6bff;
		stroke-width: 2;
		vector-effect: non-scaling-stroke;
	}
	.camera .safe {
		fill: none;
		stroke: rgba(61, 107, 255, 0.55);
		stroke-width: 1;
		stroke-dasharray: 6 5;
		vector-effect: non-scaling-stroke;
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
		stroke: var(--grid, #3d6bff);
		stroke-opacity: 0.28;
		stroke-width: 1;
	}
	.guides .ray {
		stroke: var(--guide, #e8492f);
		stroke-opacity: 0.22;
		stroke-width: 1;
	}
	.guides .axis {
		stroke: rgba(139, 92, 246, 0.7);
		stroke-width: 1.5;
		stroke-dasharray: 8 6;
	}
	.guides .horizon {
		stroke: var(--guide, #e8492f);
		stroke-opacity: 0.6;
		stroke-width: 1.5;
	}
	.guides .comp-frame {
		fill: none;
		stroke: var(--guide, #e8492f);
		stroke-opacity: 0.5;
		stroke-width: 1;
		stroke-dasharray: 4 4;
		vector-effect: non-scaling-stroke;
	}
	.guides .vp-pin .arrow {
		fill: var(--guide, #e8492f);
	}
	.guides .ruler {
		stroke: rgba(22, 22, 26, 0.35);
		stroke-width: 10;
		stroke-linecap: round;
	}
	.guides .handle {
		fill: #fff;
		stroke: var(--guide, #e8492f);
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
	.remote-cursor {
		position: absolute;
		transform-origin: 0 0;
		pointer-events: none;
		z-index: 5;
		display: flex;
		align-items: flex-start;
		gap: 2px;
		transition:
			left 80ms linear,
			top 80ms linear;
	}
	.remote-cursor path {
		fill: var(--c);
		stroke: #fff;
		stroke-width: 1.2;
	}
	.remote-cursor span {
		margin-top: 12px;
		padding: 1px 6px;
		border-radius: 8px;
		background: var(--c);
		color: #fff;
		font:
			600 11px/1.4 system-ui,
			sans-serif;
		white-space: nowrap;
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
	.overlay.onion {
		object-fit: fill;
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
	.drop-tol button.wide {
		width: auto;
		padding: 0 14px;
		border-radius: 18px;
		background: #3a3a46;
	}
	/* a short note at the top of the canvas (ColorDrop carried on, the clone source) */
	.canvas-note {
		position: absolute;
		z-index: 5;
		top: 10px;
		left: 50%;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 6px 8px 6px 12px;
		transform: translateX(-50%);
		border-radius: 999px;
		background: #23232b;
		color: #fff;
		font: 600 13px var(--pp-font-body, sans-serif);
		box-shadow: 0 6px 18px rgba(0, 0, 0, 0.3);
		white-space: nowrap;
	}
	.canvas-note .swatch {
		width: 16px;
		height: 16px;
		border: 2px solid #fff;
		border-radius: 50%;
	}
	.canvas-note button {
		padding: 4px 12px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-accent, #2f6bff);
		color: #fff;
		font: inherit;
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
