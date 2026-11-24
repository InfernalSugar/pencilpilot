<script lang="ts">
	import Studio from '@pencilpilot/paint/Studio.svelte';
	import ToolRail, { SELECT_TOOLS } from '@pencilpilot/paint/ToolRail.svelte';
	import ToolOptionsBar from '@pencilpilot/paint/ToolOptionsBar.svelte';
	import { ToolOptions } from '@pencilpilot/paint/tool-options.svelte.js';
	import { smoothTool, type SmoothTool } from '@pencilpilot/paint/stabilizer';
	import Swatches from '@pencilpilot/paint/Swatches.svelte';
	import { onMount } from 'svelte';
	import { warmGrader } from '#lib/grader.js';
	import { beforeNavigate, goto, replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import Tile from '@pencilpilot/ui/Tile.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import PaintCanvas, {
		BLANK_STATUS,
		type CanvasStatus
	} from '@pencilpilot/paint/PaintCanvas.svelte';
	import LayersPanel from '@pencilpilot/paint/LayersPanel.svelte';
	import BrushTexturePanel from '@pencilpilot/paint/BrushTexturePanel.svelte';
	import QuickMenu, { type QuickAction } from '@pencilpilot/paint/QuickMenu.svelte';
	import TimelapsePlayer from '@pencilpilot/paint/TimelapsePlayer.svelte';
	import { brushTexture, onTextures, textureOn } from '@pencilpilot/paint/brush-textures';
	import CanvasSizePicker from '@pencilpilot/paint/CanvasSizePicker.svelte';
	import RefViewer from '@pencilpilot/paint/RefViewer.svelte';
	import HistoryPanel from '@pencilpilot/paint/HistoryPanel.svelte';
	import { gradable, gradeSubmission } from '#lib/grade-submission.js';
	import { addToSession, clockOf, startSession, timeLabel } from '#lib/timed.js';
	import ColorPanel, { rememberColor } from '@pencilpilot/paint/ColorPanel.svelte';
	import {
		compositionOn,
		drawingGuide,
		defaultGuides,
		gridLines as canvasGridLines,
		type GuideSettings
	} from '@pencilpilot/paint/guides';
	import TracePanel, {
		TRACE_DEFAULTS,
		type TraceState
	} from '@pencilpilot/paint/TracePanel.svelte';
	import { savedSize, saveSize, type CanvasSize } from '@pencilpilot/paint/canvas-size';
	import BrushPicker from '@pencilpilot/paint/BrushPicker.svelte';
	import {
		DEFAULT_BRUSH,
		brushLabel,
		loadManifest,
		type BrushManifest
	} from '@pencilpilot/paint/brushes';
	import { categories } from '#lib/categories.js';
	import { loadSubmission, saveSubmission, type Submission } from '#lib/submission.js';
	import { ownReference } from '#lib/own-picture.js';
	import {
		getDrawing,
		grayToPng,
		pngToGray,
		pngToRgba,
		rgbaToPng,
		saveDrawing,
		type SavedDrawing
	} from '#lib/history.js';
	import {
		byId,
		filtered,
		imageUrl,
		tierName,
		tierOf,
		titleOf,
		type RefFilter,
		type Reference
	} from '#lib/references.js';
	import {
		fetchLiveReference,
		liveAvailable,
		recentRefs,
		rememberRef,
		reportReference
	} from '#lib/live.js';
	import { topicName, topicsFor } from '#lib/topics.js';
	import { suggestTopic } from '#lib/progress.js';
	import RefCredit from '#lib/RefCredit.svelte';

	const category = categories.find((c) => c.id === page.url.searchParams.get('category'));
	const filter = {
		topic: page.url.searchParams.get('topic'),
		tier: Number(page.url.searchParams.get('tier')) || null
	};
	/**
	 * The picture asked for in the address (?ref=): a hand-approved one, the one from the
	 * last submission (Try again), or the one this tab was drawing (a reload).
	 */
	function referenceFromUrl(): Reference | undefined {
		const id = page.url.searchParams.get('ref');
		if (!id) return undefined;
		const curated = byId(id);
		if (curated) return curated;
		if (typeof sessionStorage === 'undefined') return undefined;
		const last = loadSubmission()?.reference;
		if (last?.id === id) return last;
		try {
			const kept = JSON.parse(sessionStorage.getItem(KEPT_REF) ?? 'null') as Reference | null;
			if (kept?.id === id) return kept;
		} catch {
			/* storage blocked */
		}
		return undefined;
	}
	const KEPT_REF = 'pp.drawRef';
	/** remember the picture in the address and this tab, so a reload keeps it */
	function keepReference(r: Reference) {
		try {
			sessionStorage.setItem(KEPT_REF, JSON.stringify(r));
		} catch {
			/* storage blocked */
		}
		if (daily || personal || sessionMode || r.own) return;
		const url = new URL(page.url.href);
		if (url.searchParams.get('ref') === r.id) return;
		url.searchParams.set('ref', r.id);
		replaceState(url, page.state);
	}
	/** "Practise with your own picture": it lives in this tab only (see own-picture.ts) */
	const ownMode = page.url.searchParams.has('own');
	let reference = $state<Reference | undefined>(
		ownMode
			? typeof sessionStorage === 'undefined'
				? undefined
				: (ownReference() ?? undefined)
			: referenceFromUrl()
	);
	let refLoading = $state(false);
	/** the ref-service is still gathering pictures for this category */
	let refWaiting = $state(false);
	/** brush (draw / erase) or the eyedropper */
	type Tool =
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
		| 'clone'
		| 'pen';
	let tool = $state<Tool>('brush');
	// paste a picture (Ctrl+V) to import it as a layer
	$effect(() => {
		const onPaste = (e: ClipboardEvent) => {
			const t = e.target as HTMLElement | null;
			if (t?.closest('input, textarea, [contenteditable]')) return;
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

	/** the quick menu's actions */
	function quickActions(): QuickAction[] {
		return [
			{
				id: 'undo',
				label: 'Undo',
				icon: 'M9 14 4 9l5-5M4 9h11a5 5 0 0 1 0 10h-3',
				disabled: !status.canUndo,
				run: () => canvas?.undo()
			},
			{
				id: 'redo',
				label: 'Redo',
				icon: 'm15 14 5-5-5-5M20 9H9a5 5 0 0 0 0 10h3',
				disabled: !status.canRedo,
				run: () => canvas?.redo()
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
				disabled: !status.selection,
				run: () => canvas?.deselect()
			}
		];
	}

	/** guides saved for another canvas size are scaled to this one */
	function loadGuides() {
		const saved = recall<{ w: number; h: number; g: GuideSettings } | null>('pp.guides', null);
		const { width: w, height: h } = canvasSize;
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
			symmetry: g.symmetry ? { ...g.symmetry, c: sc(g.symmetry.c) } : defaultGuides(w, h).symmetry,
			composition: g.composition ?? defaultGuides(w, h).composition
		};
	}
	/** pen pressure and tilt on/off (remembered) */
	let penPressure = $state(true);
	/** a new, empty canvas (a new size from the picker, or a saved drawing): remounts it */
	let canvasEpoch = $state(0);
	/** the quick menu (right-click / long-press), at screen x, y */
	let quickMenu = $state<{ x: number; y: number } | null>(null);
	let timelapseOpen = $state(false);
	// the current brush's grain / shape / dual brush (changes in the Brushes tab apply at once)
	let texVersion = $state(0);
	$effect(() => onTextures(() => texVersion++));
	const currentTexture = $derived.by(() => {
		void texVersion;
		const t = brushTexture(brushId);
		return textureOn(t) ? t : null;
	});
	/**
	 * The canvas grid on the reference (the grid method): the same lines, placed
	 * where the reference sits when it's fitted inside the canvas (as the grader
	 * fits it), in fractions of the picture.
	 */
	const refGrid = $derived.by((): [number, number, number, number][] | null => {
		if (!reference || !toolOpts.guides.grid.on || !reference.width || !reference.height)
			return null;
		const { width: W, height: H } = canvasSize;
		const k = Math.min(W / reference.width, H / reference.height);
		const [w, h] = [reference.width * k, reference.height * k];
		const [x0, y0] = [(W - w) / 2, (H - h) / 2];
		return canvasGridLines(toolOpts.guides, W, H).map(([a, b, c, d]) => [
			(a - x0) / w,
			(b - y0) / h,
			(c - x0) / w,
			(d - y0) / h
		]);
	});
	/** grayscale view of the canvas and the reference (values only) */
	let grayView = $state(false);
	/** notan (2 or 3 flat tones) and squint (blur) views, on the canvas and the reference */
	let notan = $state<0 | 2 | 3>(0);
	let squint = $state(false);
	/** a unit and an angle marked on the reference (fractions of the picture) */
	type Mark = [[number, number], [number, number]];
	let refUnit = $state<Mark | null>(null);
	let refAngle = $state<Mark | null>(null);
	/** where the reference sits when fitted inside the canvas (as the grader places it) */
	const refPlaced = $derived.by(() => {
		if (!reference?.width || !reference.height) return null;
		const { width: W, height: H } = canvasSize;
		const k = Math.min(W / reference.width, H / reference.height);
		const [w, h] = [reference.width * k, reference.height * k];
		return { x: (W - w) / 2, y: (H - h) / 2, w, h };
	});
	/** the unit's length on the canvas, in canvas px */
	const measureUnit = $derived.by(() => {
		if (!refUnit || !refPlaced) return 0;
		const [[ax, ay], [bx, by]] = refUnit;
		return Math.hypot((bx - ax) * refPlaced.w, (by - ay) * refPlaced.h);
	});
	/** the reference angle, as a guide line where that edge sits on the canvas */
	const angleGuide = $derived.by(() => {
		if (!refAngle || !refPlaced) return null;
		const [[ax, ay], [bx, by]] = refAngle;
		const deg = (Math.atan2(-(by - ay) * refPlaced.h, (bx - ax) * refPlaced.w) * 180) / Math.PI;
		return {
			p: [
				refPlaced.x + ((ax + bx) / 2) * refPlaced.w,
				refPlaced.y + ((ay + by) / 2) * refPlaced.h
			] as [number, number],
			deg
		};
	});
	// a new picture: its marks no longer fit
	$effect(() => {
		void reference?.id;
		refUnit = refAngle = null;
	});

	// ---- "How am I doing?": a quick look at the biggest problem, without handing in ----
	let checkNote = $state<{ title: string; text: string } | null>(null);
	let checking = $state(false);
	async function checkNow() {
		const sub = buildSubmission();
		if (!sub || !gradable(sub) || checking) return;
		checking = true;
		try {
			const g = await gradeSubmission(sub);
			const fix = g.fb.fixes[0];
			checkNote = g.grade.flags.includes('unrelated')
				? {
						title: 'Not quite the picture yet',
						text: 'Your lines don’t follow the reference’s shapes yet. Start with its main outline.'
					}
				: fix
					? { title: fix.title, text: fix.text }
					: {
							title: 'Looking good so far',
							text: g.fb.praise?.text ?? 'Nothing big to fix yet. Keep going!'
						};
		} catch {
			checkNote = { title: 'Couldn’t check right now', text: 'Try again in a moment.' };
		} finally {
			checking = false;
		}
	}

	let shapeNote = $state('');
	let shapeTimer: ReturnType<typeof setTimeout> | undefined;
	function onTransformEnd(r: 'done' | 'cancelled' | 'empty') {
		tool = 'brush';
		if (r === 'empty') onShapeNote('Nothing to move on this layer (or it’s locked).');
	}
	function onShapeNote(text: string) {
		shapeNote = text;
		clearTimeout(shapeTimer);
		shapeTimer = setTimeout(() => (shapeNote = ''), 2200);
	}
	function onShape(kind: string) {
		shapeNote = `Snapped to ${/^[aeiou]/.test(kind) ? 'an' : 'a'} ${kind}`;
		clearTimeout(shapeTimer);
		shapeTimer = setTimeout(() => (shapeNote = ''), 1800);
	}
	function picked(c: string) {
		color = c;
		eraser = false;
		tool = 'brush';
	}
	// recent colours: the colour of each new (non-eraser) stroke
	let strokesForColor = 0;
	$effect(() => {
		const n = status.strokes;
		if (n > strokesForColor && !eraser) rememberColor(color);
		strokesForColor = n;
	});

	let tracing = $state(false);
	let trace = $state<TraceState>({ ...TRACE_DEFAULTS });
	/** a stroke was made while the tracing picture showed: tips only, no score */
	let traced = $state(false);
	const traceSrc = $derived(
		!tracing ? null : trace.source === 'own' ? trace.ownUrl : reference ? imageUrl(reference) : null
	);
	let strokesSeen = 0;
	$effect(() => {
		const n = status.strokes;
		if (status.blank)
			traced = false; // a fresh start
		else if (n > strokesSeen && traceSrc) traced = true;
		strokesSeen = n;
	});
	function toggleTracing() {
		tracing = !tracing;
		traceBarOpen = tracing;
		if (tracing && !reference) trace = { ...trace, source: 'own' };
		if (!tracing) trace = { ...trace, adjusting: false };
	}
	/** tracing stays on while its settings bar is tucked away (less clutter) */
	let traceBarOpen = $state(true);
	function traceButton() {
		if (!tracing) toggleTracing();
		else traceBarOpen = !traceBarOpen;
	}
	let reportOpen = $state(false);
	let reported = $state(false);

	/** A hand-approved reference the student hasn't seen recently (or any, if all were seen). */
	function pickCurated(avoid: string[], f: RefFilter = filter) {
		if (!category) return undefined;
		const pool = filtered(category.id, f);
		const unseen = pool.filter((r) => !avoid.includes(r.id));
		const from = unseen.length ? unseen : pool;
		return from[Math.floor(Math.random() * from.length)];
	}

	/**
	 * Beginners get hand-approved pictures first; everyone else (and beginners who have
	 * seen them all) gets Endless pictures from the ref-service, falling back to curated
	 * ones if the service has nothing ready within ~1.5 s.
	 */
	async function loadReference(skip?: string) {
		if (!category) return;
		refLoading = true;
		if (trace.source === 'reference') trace = { ...trace, x: 0, y: 0, scale: 1, rotate: 0 };
		reported = false;
		reportOpen = false;
		const avoid = [...recentRefs(), ...(skip ? [skip] : [])];
		// "Surprise me": lean towards topics the student finds hardest (or hasn't tried).
		const topic =
			filter.topic ??
			suggestTopic(
				category.id,
				topicsFor(category.id).map((t) => t.id)
			) ??
			null;
		const f: RefFilter = { topic, tier: filter.tier };
		const unseenCurated = filtered(category.id, f).filter((r) => !avoid.includes(r.id));
		let r: Reference | undefined =
			filter.tier === 1 && unseenCurated.length ? pickCurated(avoid, f) : undefined;
		r ??=
			(await fetchLiveReference(
				{
					category: category.id,
					topic,
					tier: filter.tier,
					exclude: avoid
				},
				4000
			)) ?? undefined;
		// nothing ready that fits: any hand-approved picture in the category beats none
		r ??=
			pickCurated(avoid, f) ??
			pickCurated(avoid) ??
			pickCurated(avoid, { tier: filter.tier }) ??
			pickCurated(avoid, {});
		// still nothing (a category with no hand-approved pictures, e.g. right after the
		// service starts): any live picture in the category, waiting a little while the
		// service gathers some
		const serviceUp = !r && (await liveAvailable());
		for (let tries = 0; !r && serviceUp && tries < 10; tries++) {
			refWaiting = tries > 0;
			r = (await fetchLiveReference({ category: category.id, exclude: avoid }, 4000)) ?? undefined;
			if (!r) await new Promise((done) => setTimeout(done, 3000));
		}
		refWaiting = false;
		reference = r;
		refLoading = false;
		if (r) {
			rememberRef(r.id);
			keepReference(r);
		}
	}

	function nextReference() {
		if (!category || !status.blank) return;
		loadReference(reference?.id);
	}

	async function report(reason: string) {
		if (!reference) return;
		await reportReference(reference.id, reason).catch(() => false);
		reported = true;
		reportOpen = false;
		if (status.blank) loadReference(reference.id);
	}

	let manifest = $state<BrushManifest>();
	let brushId = $state(DEFAULT_BRUSH);
	let eraser = $state(false);
	/** the painting tool whose smoothing the slider shows */
	const smoothKind = $derived(smoothTool(tool, eraser));
	let size = $state(1);
	let opacity = $state(1);
	let color = $state('#16161a');
	let status = $state<CanvasStatus>(BLANK_STATUS);
	let canvasSize = $state<CanvasSize>({ width: 800, height: 600 });
	function pickSize(next: CanvasSize) {
		canvasEpoch++;
		canvasSize = next;
		saveSize(next);
		status = BLANK_STATUS;
		loadGuides();
	}

	// Side dock (reference + Layers / Brushes / Colour tabs). Always shown on wide
	// screens; a slide-over (tablet) or bottom sheet (phone) on narrow ones.
	const DOCK_TABS = ['layers', 'brushes', 'colour', 'history'];
	let dockTab = $state('layers');
	let dockOpen = $state(false);
	// remember the chosen tab
	$effect(() => remember('pp.dockTab', dockTab));
	function setTab(t: string) {
		dockTab = t;
	}
	function openDock(t: string) {
		setTab(t);
		dockOpen = true;
	}
	/**
	 * Where the reference shows: in the side panel, beside the canvas (its own pane,
	 * resized with the divider) or in a floating window.
	 */
	type RefPlace = 'dock' | 'beside' | 'float';
	let refPlace = $state<RefPlace>('dock');
	const refFloating = $derived(refPlace === 'float');
	const showBeside = $derived(refPlace === 'beside' && (!!reference || refLoading));
	/** the reference pane's share of the drawing area in "beside" mode */
	let refSplit = $state(0.4);
	const SPLIT_MIN = 0.15;
	const SPLIT_MAX = 0.75;
	let floatBox = $state({ x: 24, y: 120, w: 360, h: 420 });
	function remember(key: string, value: unknown) {
		try {
			localStorage.setItem(key, JSON.stringify(value));
		} catch {
			/* storage blocked */
		}
	}
	function recall<T>(key: string, fallback: T): T {
		try {
			return JSON.parse(localStorage.getItem(key) ?? 'null') ?? fallback;
		} catch {
			return fallback;
		}
	}
	function setPlace(p: RefPlace) {
		refPlace = p;
		remember('pp.refPlace', p);
		if (p === 'beside' && dockOpen && window.innerWidth <= 1100) dockOpen = false;
	}
	/** First visit: beside the canvas when there's room; older saved choices still count. */
	function savedPlace(): RefPlace {
		const p = recall<string | null>('pp.refPlace', null);
		if (p === 'dock' || p === 'beside' || p === 'float') return p;
		if (recall('pp.refFloating', false)) return 'float';
		return window.innerWidth > 700 ? 'beside' : 'dock';
	}
	let splitEl = $state<HTMLDivElement>();
	let splitDrag = false;
	function setSplit(v: number) {
		refSplit = Math.min(SPLIT_MAX, Math.max(SPLIT_MIN, v));
	}
	function splitMove(e: PointerEvent) {
		if (!splitDrag || !splitEl) return;
		const r = splitEl.getBoundingClientRect();
		const column = getComputedStyle(splitEl).flexDirection === 'column';
		setSplit(column ? (e.clientY - r.top) / r.height : (e.clientX - r.left) / r.width);
	}
	function splitEnd() {
		if (!splitDrag) return;
		splitDrag = false;
		remember('pp.refSplit', refSplit);
	}
	function splitKey(e: KeyboardEvent) {
		const step = e.shiftKey ? 0.1 : 0.02;
		const d =
			e.key === 'ArrowLeft' || e.key === 'ArrowUp'
				? -step
				: e.key === 'ArrowRight' || e.key === 'ArrowDown'
					? step
					: 0;
		if (e.key === 'Home') setSplit(SPLIT_MIN);
		else if (e.key === 'End') setSplit(SPLIT_MAX);
		else if (d) setSplit(refSplit + d);
		else return;
		e.preventDefault();
		remember('pp.refSplit', refSplit);
	}
	let floatDrag: { x: number; y: number; bx: number; by: number } | null = null;
	function keepFloatOnScreen() {
		const b = floatBox;
		floatBox = {
			...b,
			x: Math.max(0, Math.min(window.innerWidth - 120, b.x)),
			y: Math.max(0, Math.min(window.innerHeight - 60, b.y))
		};
		remember('pp.refFloat', floatBox);
	}

	/** "this layer is locked/hidden" notice after a refused stroke */
	let blocked = $state<'' | 'locked' | 'hidden'>('');
	let blockedTimer: ReturnType<typeof setTimeout> | undefined;
	function onBlocked(why: 'locked' | 'hidden') {
		blocked = why;
		clearTimeout(blockedTimer);
		blockedTimer = setTimeout(() => (blocked = ''), 3500);
	}
	const activeLayerName = $derived(status.layers.find((l) => l.id === status.active)?.name ?? '');
	let canvas: ReturnType<typeof PaintCanvas> | undefined = $state();
	/** the tools' own settings and the Adjust panel (shared with the app) */
	const toolOpts = new ToolOptions({
		canvas: () => canvas,
		status: () => status,
		color: () => color,
		setTool: (t: string) => {
			if (!timeUp) tool = t as Tool;
		},
		onguides: (g: GuideSettings) =>
			remember('pp.guides', { w: canvasSize.width, h: canvasSize.height, g }),
		size: () => canvasSize
	});
	let seconds = $state(0);
	let timerOn = $state(true);

	/** daily challenge day (?daily=YYYY-MM-DD): the picture is fixed */
	const daily = page.url.searchParams.get('daily');
	/** personal challenge day (?personal=YYYY-MM-DD): the picture is fixed too */
	const personal = page.url.searchParams.get('personal');
	/** where the back button goes */
	const backHref = $derived(
		ownMode ? '/practice/own' : daily ? '/daily' : category ? `/practice/${category.id}` : '/'
	);

	// Sketch mode (?mode=sketch, ADR-0007): a Construction layer under a Sketch layer.
	const drawMode: 'finished' | 'sketch' =
		page.url.searchParams.get('mode') === 'sketch' ? 'sketch' : 'finished';
	const CONSTRUCTION_BLUE = '#3d6bff';
	let constructionId = $state(0);
	let setupFor = '';
	let colorBeforeConstruction = '';
	$effect(() => {
		const key = `${canvasSize.width}x${canvasSize.height}-${round}`;
		if (drawMode !== 'sketch' || !canvas || status.layers.length !== 1 || setupFor === key) return;
		setupFor = key;
		const sketchId = status.layers[0].id;
		canvas.setLayer(sketchId, { name: 'Sketch' });
		canvas.addLayer('Construction');
		const cid = status.active;
		canvas.moveLayer(cid, 0);
		canvas.setLayer(cid, { opacity: 0.5 });
		canvas.clearHistory();
		constructionId = cid;
	});
	// Construction lines in blue; back to the student's colour on other layers.
	$effect(() => {
		if (drawMode !== 'sketch' || !constructionId) return;
		if (status.active === constructionId) {
			if (color !== CONSTRUCTION_BLUE) {
				colorBeforeConstruction = color;
				color = CONSTRUCTION_BLUE;
			}
		} else if (color === CONSTRUCTION_BLUE && colorBeforeConstruction) {
			color = colorBeforeConstruction;
			colorBeforeConstruction = '';
		}
	});

	// Timed rounds (?time=120) and gesture sessions (?session=gesture&count=10&time=60).
	const timeLimit = Math.max(0, Math.min(3600, Number(page.url.searchParams.get('time')) || 0));
	const sessionMode =
		page.url.searchParams.get('session') === 'gesture' && timeLimit > 0 && !!category;
	const sessionCount = Math.max(1, Math.min(50, Number(page.url.searchParams.get('count')) || 10));
	/** gesture session: which picture (also refreshes the canvas between pictures) */
	let round = $state(0);
	/** seconds used in this timed round */
	let used = $state(0);
	let timeUp = $state(false);
	const remaining = $derived(timeLimit - used);
	const canvasReady = $derived(status.layers.length > 0);
	/** the clock runs once the canvas and the picture are there */
	const roundLive = $derived(canvasReady && !refLoading && (!!reference || !category));

	function tickTimed() {
		if (!timeLimit || timeUp || !roundLive) return;
		used++;
		if (used >= timeLimit) onTimeUp();
	}

	function onTimeUp() {
		timeUp = true;
		if (sessionMode) {
			const done = addToSession(status.blank ? null : buildSubmission());
			setTimeout(async () => {
				if (done >= sessionCount) return goto('/session');
				round++;
				used = 0;
				traced = false;
				status = BLANK_STATUS;
				timeUp = false;
				await loadReference(reference?.id);
			}, 1200);
		} else if (!status.blank) setTimeout(submit, 1200);
	}

	function tryAgain() {
		used = 0;
		timeUp = false;
	}
	let confirmClear = $state(false);

	const brushName = $derived(brushLabel(manifest, brushId));
	const clock = $derived(
		`${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
	);

	onMount(() => {
		warmGrader();
		canvasSize = savedSize();
		dockTab = recall('pp.dockTab', 'layers');
		if (!DOCK_TABS.includes(dockTab)) dockTab = 'layers';
		refPlace = savedPlace();
		setSplit(Number(recall('pp.refSplit', 0.4)) || 0.4);
		floatBox = recall('pp.refFloat', floatBox);
		// (one smoothing for every tool before each kept its own)
		const smoothing = Number(recall('pp.smoothing', 0)) || 0;
		toolOpts.smoothing = {
			brush: smoothing,
			eraser: smoothing,
			smudge: smoothing,
			...recall<Partial<Record<SmoothTool, number>>>('pp.smoothingTools', {})
		};
		toolOpts.quickShape = recall('pp.quickShape', true);
		toolOpts.closeFill = recall('pp.closeFill', false);
		penPressure = recall('pp.penPressure', true);
		loadGuides();
		loadManifest().then((m) => (manifest = m));
		if (historyParam) void openFromHistory(historyParam);
		else if (!reference) loadReference();
		else keepReference(reference);
		if (sessionMode && category)
			startSession({
				count: sessionCount,
				time: timeLimit,
				category: category.id,
				topic: filter.topic,
				tier: filter.tier,
				mode: drawMode
			});
		const id = setInterval(() => {
			if (timerOn && !status.blank) seconds++;
			tickTimed();
		}, 1000);
		return () => {
			clearInterval(id);
		};
	});

	function onkey(e: KeyboardEvent) {
		const t = e.target as HTMLElement;
		if (t.closest('input, textarea, select')) return;
		const mod = e.ctrlKey || e.metaKey;
		const k = e.key.toLowerCase();
		if (mod && k === 'z') {
			e.preventDefault();
			if (e.shiftKey) canvas?.redo();
			else {
				if (toolOpts.adjustOpen) toolOpts.closeAdjust(false);
				canvas?.undo();
			}
		} else if (mod && k === 'y') {
			e.preventDefault();
			if (toolOpts.adjustOpen) toolOpts.closeAdjust(false);
			canvas?.redo();
		} else if (!mod && k === 'b') {
			eraser = false;
			tool = 'brush';
		} else if (!mod && k === 'e') {
			eraser = true;
			tool = 'brush';
		} else if (!mod && k === 'i') tool = tool === 'picker' ? 'brush' : 'picker';
		else if (!mod && k === 'g') tool = tool === 'fill' ? 'brush' : 'fill';
		else if (!mod && k === 'f') tool = tool === 'lasso-fill' ? 'brush' : 'lasso-fill';
		else if (!mod && k === 'u') tool = tool === 'measure' ? 'brush' : 'measure';
		else if (!mod && k === 's') tool = tool === 'smudge' ? 'brush' : 'smudge';
		else if (!mod && k === 'j') tool = tool === 'clone' ? 'brush' : 'clone';
		else if (!mod && k === 'p') tool = tool === 'pen' ? 'brush' : 'pen';
		else if (tool === 'pen' && e.key === 'Enter') {
			if (canvas?.penStroke(toolOpts.penTaper)) canvas.penClear();
		} else if (tool === 'pen' && e.key === 'Escape') canvas?.penClear();
		else if (tool === 'pen' && !mod && e.key === 'Backspace') {
			e.preventDefault();
			canvas?.penBack();
		} else if (mod && (k === 'c' || k === 'x') && !e.shiftKey) {
			e.preventDefault();
			toolOpts.copyCut(k === 'x');
		} else if (mod && k === 'j') {
			e.preventDefault();
			if (canvas?.copySelection(false, false)) toolOpts.pasteCopied();
		} else if (!mod && k === 'v') tool = tool === 'transform' ? 'brush' : 'transform';
		else if (!mod && k === 'k') tool = tool === 'liquify' ? 'brush' : 'liquify';
		else if (tool === 'transform' && e.key === 'Enter') canvas?.transformCommit();
		else if (tool === 'transform' && e.key === 'Escape') canvas?.transformCancel();
		else if (!mod && (k === 'm' || k === 'l' || k === 'w')) {
			const t = SELECT_TOOLS.find((x) => x.key === k)!.id as Tool;
			tool = tool === t ? 'brush' : t;
		} else if (mod && k === 'a') {
			e.preventDefault();
			canvas?.selectAll();
		} else if (mod && k === 'd') {
			e.preventDefault();
			canvas?.deselect();
		} else if (mod && e.shiftKey && k === 'i') {
			e.preventDefault();
			canvas?.invertSelection();
		} else if (!mod && (e.key === 'Delete' || e.key === 'Backspace') && status.selection) {
			e.preventDefault();
			canvas?.clear();
		} else if (e.key === 'Escape' && status.selection) canvas?.deselect();
		else if (!mod && e.key === '[') size = Math.max(0.25, +(size / 1.25).toFixed(2));
		else if (!mod && e.key === ']') size = Math.min(6, +(size * 1.25).toFixed(2));
		else if (!mod && (e.key === '+' || e.key === '=')) canvas?.zoomIn();
		else if (!mod && e.key === '-') canvas?.zoomOut();
		else if (!mod && e.key === '0') canvas?.resetView();
		else if (!mod && e.key === '4') canvas?.rotateBy(-15);
		else if (!mod && e.key === '6') canvas?.rotateBy(15);
		else if (!mod && e.key === '5') canvas?.resetRotation();
		else if (!mod && k === 'm') canvas?.toggleMirror();
	}

	function buildSubmission(): Submission | null {
		// construction lines are never graded
		const d = canvas?.exportDrawing({
			exclude: drawMode === 'sketch' && constructionId ? [constructionId] : []
		});
		if (!d) return null;
		return {
			at: Date.now(),
			png: d.png,
			width: d.width,
			height: d.height,
			seconds,
			category: category?.id ?? null,
			referenceId: reference?.id ?? null,
			reference: reference ? $state.snapshot(reference) : null,
			topic: filter.topic,
			tier: reference ? tierOf(reference) : null,
			strokes: d.strokes.length,
			points: d.strokes.reduce((n, s) => n + s.points.length, 0),
			brushes: [...new Set(d.strokes.map((s) => s.brushId))],
			strokeData: d.strokes,
			layers: d.layers.map((l) => ({
				id: l.id,
				opacity: l.opacity,
				blend: l.blend,
				...(l.clip ? { clip: true } : {})
			})),
			traced: traced || d.layers.some((l) => l.imported),
			...(d.layers.some((l) => l.imported) ? { imported: true } : {}),
			trace: traced
				? {
						source: trace.source,
						transform: { x: trace.x, y: trace.y, scale: trace.scale, rotate: trace.rotate },
						ownUrl: trace.source === 'own' ? (trace.ownUrl ?? undefined) : undefined
					}
				: undefined,
			...(timeLimit ? { timeLimit } : {}),
			...(drawMode === 'sketch' ? { mode: 'sketch' as const } : {}),
			...(daily ? { daily } : {}),
			...(personal ? { personal } : {})
		};
	}

	// ---- Save: a PNG picture or a layered Photoshop file ----
	let saveOpen = $state(false);
	let saving = $state(false);
	function fileName(ext: string) {
		const base = (reference ? titleOf(reference) : 'drawing')
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-|-$/g, '');
		return `pencilpilot-${base || 'drawing'}-${new Date().toISOString().slice(0, 10)}.${ext}`;
	}
	function download(blob: Blob, name: string) {
		const a = document.createElement('a');
		a.href = URL.createObjectURL(blob);
		a.download = name;
		a.click();
		setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
	}
	async function savePng() {
		saveOpen = false;
		const d = canvas?.exportDrawing();
		if (d) download(await (await fetch(d.png)).blob(), fileName('png'));
	}
	async function savePsd() {
		saveOpen = false;
		saving = true;
		try {
			const doc = canvas?.exportLayered();
			if (!doc) return;
			const { writePsd } = await import('@pencilpilot/paint/psd');
			const bytes = writePsd(doc);
			download(
				new Blob([bytes as BlobPart], { type: 'image/vnd.adobe.photoshop' }),
				fileName('psd')
			);
		} finally {
			saving = false;
		}
	}

	async function submit() {
		const sub = buildSubmission();
		if (!sub) return;
		handedIn = true;
		await saveToHistory(true);
		saveSubmission({ ...sub, historyId: drawingId });
		await goto('/results');
	}

	// ---- drawing history: the last 10 drawings, kept in this browser (lib/history.ts) ----
	const historyParam = page.url.searchParams.get('history');
	/** this drawing's history entry (continuing one keeps its id) */
	let drawingId = historyParam ?? `d${Date.now()}`;
	let handedIn = false;
	let historyTimer: ReturnType<typeof setTimeout> | undefined;
	let historySaving = false;

	async function saveToHistory(submitted: boolean) {
		if (!canvas || status.blank || historySaving) return;
		const snap = canvas.snapshotLayers();
		const d = canvas.exportDrawing();
		if (!snap || !d) return;
		historySaving = true;
		try {
			const { width: w, height: h } = canvasSize;
			const layers = await Promise.all(
				snap.layers.map(async (l) => ({
					id: l.id,
					name: l.name,
					opacity: l.opacity,
					blend: l.blend,
					visible: l.visible,
					locked: l.locked,
					alphaLock: l.alphaLock,
					clip: l.clip,
					imported: l.imported,
					group: l.group,
					png: await rgbaToPng(l.rgba, w, h),
					...(l.mask ? { mask: await grayToPng(l.mask, w, h) } : {})
				}))
			);
			const png = await (await fetch(d.png)).blob();
			await saveDrawing({
				id: drawingId,
				savedAt: Date.now(),
				title: reference ? titleOf(reference) : 'Free drawing',
				submitted,
				width: w,
				height: h,
				thumb: await thumbOf(d.png),
				png,
				layers,
				groups: snap.groups,
				strokes: snap.strokes,
				...(reference ? { reference: $state.snapshot(reference) as Reference } : {}),
				...(category ? { category: category.id } : {}),
				mode: drawMode,
				traced
			});
		} finally {
			historySaving = false;
		}
	}

	/** a small JPEG preview */
	async function thumbOf(png: string) {
		const img = new Image();
		img.src = png;
		await img.decode();
		const k = Math.min(1, 320 / img.naturalWidth);
		const c = document.createElement('canvas');
		c.width = Math.round(img.naturalWidth * k);
		c.height = Math.round(img.naturalHeight * k);
		c.getContext('2d')!.drawImage(img, 0, 0, c.width, c.height);
		return c.toDataURL('image/jpeg', 0.8);
	}

	// keep a draft: a few seconds after the last change, and when leaving the page
	$effect(() => {
		void status.version;
		if (status.blank || handedIn || restoring) return;
		clearTimeout(historyTimer);
		historyTimer = setTimeout(() => void saveToHistory(false), 3000);
	});
	$effect(() => {
		const leave = () => {
			if (document.visibilityState === 'hidden' && !handedIn) void saveToHistory(false);
		};
		const closing = () => {
			if (!handedIn) void saveToHistory(false);
		};
		document.addEventListener('visibilitychange', leave);
		window.addEventListener('pagehide', closing);
		return () => {
			document.removeEventListener('visibilitychange', leave);
			window.removeEventListener('pagehide', closing);
			clearTimeout(historyTimer);
		};
	});
	// leaving for another page in the app: snapshot now, while the canvas is still here
	beforeNavigate(() => {
		clearTimeout(historyTimer);
		if (!handedIn) void saveToHistory(false);
	});

	// continuing a drawing from the history
	// raw: the saved drawing must stay plain data (proxies can't be stored or cloned)
	let restoring = $state.raw<SavedDrawing | null>(null);
	async function openFromHistory(id: string) {
		const entry = await getDrawing(id);
		if (!entry) return loadReference();
		if (entry.width !== canvasSize.width || entry.height !== canvasSize.height) {
			canvasEpoch++;
			canvasSize = { width: entry.width, height: entry.height };
			status = BLANK_STATUS; // wait for the new canvas
		}
		reference = entry.reference;
		if (reference) keepReference(reference);
		traced = !!entry.traced;
		restoring = entry;
	}
	$effect(() => {
		const entry = restoring;
		if (
			!entry ||
			!canvas ||
			!status.layers.length ||
			entry.width !== canvasSize.width ||
			entry.height !== canvasSize.height
		)
			return;
		restoring = null; // once
		void (async () => {
			const { width: w, height: h } = entry;
			const layers = await Promise.all(
				entry.layers.map(async (l) => ({
					...l,
					rgba: await pngToRgba(l.png, w, h),
					mask: l.mask ? await pngToGray(l.mask, w, h) : undefined
				}))
			);
			canvas?.restoreLayers(layers, entry.groups, entry.strokes);
		})();
	});
</script>

<svelte:window onkeydown={onkey} />
<svelte:head><title>Draw · PencilPilot</title></svelte:head>

<Studio
	{backHref}
	tabs={[
		{ id: 'layers', label: 'Layers', meta: activeLayerName },
		{ id: 'brushes', label: 'Brushes', meta: brushName },
		{ id: 'colour', label: 'Colour' },
		{ id: 'history', label: 'History', meta: status.history.undo.at(-1) }
	]}
	bind:tab={dockTab}
	bind:dockOpen
>
	{#snippet title()}
		<span class="pp-eyebrow">
			{ownMode ? 'Your own picture · ' : ''}{daily ? 'Today’s challenge · ' : ''}{personal
				? 'Your challenge · '
				: ''}{drawMode === 'sketch' ? 'Sketch · ' : ''}{category
				? `${category.name} · `
				: ''}{reference?.own
				? 'Practice'
				: reference
					? [topicName(reference.category, reference.topic), tierName(tierOf(reference))]
							.filter(Boolean)
							.join(' · ')
					: 'Free draw'}
		</span>
		<h1>{reference ? titleOf(reference) : 'Draw anything'}</h1>
	{/snippet}
	{#snippet actions()}
		{#if sessionMode}
			<span class="round" aria-label="Picture {round + 1} of {sessionCount}">
				{round + 1} / {sessionCount}
			</span>
		{/if}
		{#if timeLimit}
			<span
				class="timer"
				class:urgent={remaining <= 10}
				role="timer"
				aria-label="Time left {clockOf(remaining)}"
			>
				<svg
					width="20"
					height="20"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.4"
					stroke-linecap="round"
					aria-hidden="true"
				>
					<circle cx="12" cy="13" r="8" /><path d="M12 9v4l2.5 2M9 2h6" />
				</svg>
				{clockOf(remaining)}
			</span>
		{:else}
			<button
				type="button"
				class="timer"
				aria-pressed={timerOn}
				title={timerOn ? 'Pause timer' : 'Resume timer'}
				onclick={() => (timerOn = !timerOn)}
			>
				<svg
					width="20"
					height="20"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.4"
					stroke-linecap="round"
					aria-hidden="true"
				>
					<circle cx="12" cy="13" r="8" /><path d="M12 9v4l2.5 2M9 2h6" />
				</svg>
				<span aria-label="Time spent">{clock}</span>
			</button>
		{/if}
		<div class="undo-group" role="group" aria-label="History">
			<button
				type="button"
				class="tool"
				aria-label="Undo (Ctrl+Z)"
				title="Undo (Ctrl+Z)"
				disabled={!status.canUndo}
				onclick={() => {
					if (toolOpts.adjustOpen) toolOpts.closeAdjust(false);
					canvas?.undo();
				}}
			>
				<svg
					width="22"
					height="22"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.3"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path d="M9 14 4 9l5-5" /><path d="M4 9h11a5 5 0 0 1 0 10h-3" />
				</svg>
			</button>
			<button
				type="button"
				class="tool"
				aria-label="Redo (Ctrl+Shift+Z)"
				title="Redo (Ctrl+Shift+Z)"
				disabled={!status.canRedo}
				onclick={() => canvas?.redo()}
			>
				<svg
					width="22"
					height="22"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.3"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path d="m15 14 5-5-5-5" /><path d="M20 9H9a5 5 0 0 0 0 10h3" />
				</svg>
			</button>
			{#if confirmClear}
				<button
					type="button"
					class="tool danger"
					onclick={() => {
						canvas?.clear();
						confirmClear = false;
					}}
				>
					Clear layer?
				</button>
				<button type="button" class="tool" onclick={() => (confirmClear = false)}>Keep</button>
			{:else}
				<button
					type="button"
					class="tool"
					aria-label="Clear layer"
					title="Clear the current layer"
					disabled={status.blank}
					onclick={() => (confirmClear = true)}
				>
					<svg
						width="22"
						height="22"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="2.3"
						stroke-linecap="round"
						stroke-linejoin="round"
						aria-hidden="true"
					>
						<path d="M4 7h16M6 7l1 13h10l1-13M9 7V4h6v3" />
					</svg>
				</button>
			{/if}
		</div>
		{#if !sessionMode}
			<CanvasSizePicker
				size={canvasSize}
				blank={status.blank}
				onpick={pickSize}
				onresize={(next, how) =>
					canvas?.resizeCanvas(
						next.width,
						next.height,
						(next.width - canvasSize.width) * how.anchor[0],
						(next.height - canvasSize.height) * how.anchor[1],
						how.scale
					)}
			/>
			<div class="save">
				<button
					type="button"
					class="save-btn"
					aria-expanded={saveOpen}
					aria-haspopup="true"
					disabled={status.blank || saving}
					onclick={() => (saveOpen = !saveOpen)}
				>
					{saving ? 'Saving…' : 'Save'}
				</button>
				{#if saveOpen}
					<div class="save-menu" role="group" aria-label="Save as">
						<button type="button" onclick={savePng}>
							<b>PNG picture</b><span>One flat image</span>
						</button>
						<button type="button" onclick={savePsd}>
							<b>Photoshop file (.psd)</b><span>Keeps layers, groups and masks</span>
						</button>
						<button
							type="button"
							onclick={() => {
								saveOpen = false;
								timelapseOpen = true;
							}}
						>
							<b>Timelapse</b><span>Watch it being drawn, save it as a video</span>
						</button>
						<a class="menu-link" href="/history">Your last 10 drawings →</a>
					</div>
				{/if}
			</div>
			{#if reference && !timeLimit && !sessionMode}
				<div class="check-wrap">
					<button
						type="button"
						class="check-btn"
						disabled={status.blank || checking}
						title="How am I doing? See the biggest thing to fix so far (it isn't handed in)"
						onclick={checkNow}
					>
						{checking ? 'Checking…' : 'How am I doing?'}
					</button>
					{#if checkNote}
						<div class="check-pop" role="status" aria-label="Quick check">
							<b>{checkNote.title}</b>
							<p>{checkNote.text}</p>
							<button type="button" onclick={() => (checkNote = null)}>Got it</button>
						</div>
					{/if}
				</div>
			{/if}
			<Button
				onclick={submit}
				disabled={status.blank || timeUp}
				aria-label={traced ? 'Get tips' : timeLimit ? 'Done early' : 'Submit drawing'}
			>
				{#if traced}Get tips →{:else if timeLimit}Done early →{:else}<span>
						Submit<span class="wide-only">&nbsp;drawing</span> →
					</span>{/if}
			</Button>
		{/if}
	{/snippet}
	{#snippet rail()}
		<ToolRail
			bind:tool
			bind:eraser
			bind:size
			bind:opacity
			adjustOpen={toolOpts.adjustOpen}
			onadjust={() => toolOpts.openAdjust()}
		/>
	{/snippet}
	{#snippet options()}
		<span class="brush-name">{brushName}</span>
		<Swatches
			bind:color
			onmore={() => openDock('colour')}
			moreOpen={dockTab === 'colour' && dockOpen}
		/>
		{#if smoothKind}
			<label class="slider">
				<span>Smoothing <b>{Math.round(toolOpts.smoothing[smoothKind] * 100)}%</b></span>
				<input
					type="range"
					min="0"
					max="1"
					step="0.05"
					bind:value={toolOpts.smoothing[smoothKind]}
					onchange={() => remember('pp.smoothingTools', $state.snapshot(toolOpts.smoothing))}
					title="Steadies wobbly lines. The brush, eraser and smudge each keep their own."
				/>
			</label>
		{/if}
		<button
			type="button"
			class="tool"
			aria-pressed={drawingGuide(toolOpts.guides)}
			aria-expanded={toolOpts.guidesOpen}
			title="Guides: grid, perspective and ruler"
			onclick={() => {
				toolOpts.guidesOpen = !toolOpts.guidesOpen;
				if (toolOpts.guidesOpen) toolOpts.compositionOpen = false;
			}}
		>
			Guides
		</button>
		<button
			type="button"
			class="tool"
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
			class="tool"
			aria-pressed={penPressure}
			title="Pen pressure and tilt: off draws every stroke at a steady medium pressure"
			onclick={() => {
				penPressure = !penPressure;
				remember('pp.penPressure', penPressure);
			}}
		>
			Pen pressure
		</button>
		<button
			type="button"
			class="tool"
			aria-pressed={toolOpts.quickShape}
			title="Snap shapes: hold at the end of a stroke to snap it to a line, circle or rectangle"
			onclick={() => {
				toolOpts.quickShape = !toolOpts.quickShape;
				remember('pp.quickShape', toolOpts.quickShape);
			}}
		>
			Snap shapes
		</button>
		<button
			type="button"
			class="tool"
			aria-pressed={toolOpts.closeFill}
			title="Close and fill: a stroke that ends where it began fills inside with its colour"
			onclick={() => {
				toolOpts.closeFill = !toolOpts.closeFill;
				remember('pp.closeFill', toolOpts.closeFill);
			}}
		>
			Close &amp; fill
		</button>
		<button
			type="button"
			class="tool"
			aria-pressed={tracing}
			aria-expanded={tracing ? traceBarOpen : undefined}
			title={tracing
				? traceBarOpen
					? 'Hide the tracing settings (tracing stays on)'
					: 'Show the tracing settings'
				: 'Show a picture faintly inside the canvas to trace over (no score)'}
			onclick={traceButton}
		>
			Trace{#if tracing}<span class="caret" aria-hidden="true">
					{traceBarOpen ? '▴' : '▾'}
				</span>{/if}
		</button>
	{/snippet}
	{#snippet context()}
		<ToolOptionsBar opts={toolOpts} bind:tool {status} {canvas} {measureUnit} />
		{#if tracing && traceBarOpen}
			<TracePanel
				bind:trace
				hasReference={!!reference}
				onclose={toggleTracing}
				onhide={() => (traceBarOpen = false)}
			/>
		{/if}
	{/snippet}
	{#snippet banner()}
		{#if drawMode === 'sketch'}
			<p class="sketch-note">
				<b>Sketch mode:</b> rough, scratchy lines are fine. Put guide lines on the
				<b>Construction</b> layer: they’re never marked as mistakes. Scored on proportions, big shapes
				and angles.
			</p>
		{/if}
	{/snippet}
	{#snippet stage()}
		<div
			class="split"
			class:beside={showBeside}
			bind:this={splitEl}
			style="--split: {refSplit * 100}%"
		>
			{#if showBeside}
				<section class="ref-pane" aria-label="Reference beside the canvas">
					<div class="ref-pane-bar">
						<span>Reference</span>
						<button type="button" class="tool" onclick={() => setPlace('float')}>Float</button>
						<button type="button" class="tool" onclick={() => setPlace('dock')}>
							To the panel
						</button>
					</div>
					{#if reference}
						{#key reference.id}
							<RefViewer
								src={imageUrl(reference)}
								alt="Reference: {reference.tags.join(', ')}"
								fill
								gray={grayView}
								grid={refGrid}
								onpick={picked}
								{notan}
								{squint}
								unit={refUnit}
								angle={refAngle}
								onunit={(m) => {
									refUnit = m;
									if (m) tool = 'measure';
								}}
								onangle={(m) => (refAngle = m)}
							/>
						{/key}
					{:else}
						<div class="ref-skeleton" aria-busy="true">Finding a picture…</div>
					{/if}
				</section>
				<!-- a window splitter: drag, or arrow keys when focused -->
				<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
				<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
				<div
					class="divider"
					role="separator"
					tabindex="0"
					aria-label="Resize the reference"
					aria-orientation="vertical"
					aria-valuemin={SPLIT_MIN * 100}
					aria-valuemax={SPLIT_MAX * 100}
					aria-valuenow={Math.round(refSplit * 100)}
					onpointerdown={(e) => {
						e.currentTarget.setPointerCapture(e.pointerId);
						splitDrag = true;
					}}
					onpointermove={splitMove}
					onpointerup={splitEnd}
					onpointercancel={splitEnd}
					onkeydown={splitKey}
				></div>
			{/if}
			<div class="canvas-pane">
				{#key `${canvasEpoch}-${round}`}
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
						{tool}
						smoothing={toolOpts.smoothingFor(tool, eraser)}
						onpick={picked}
						bind:grayView
						bind:notan
						bind:squint
						{measureUnit}
						{angleGuide}
						onsize={(v) => (size = v)}
						onshape={onShape}
						ontransformend={onTransformEnd}
						allowImport
						brushTexture={currentTexture}
						oncanvassize={(s) => {
							canvasSize = s;
							saveSize(s);
						}}
						onquickmenu={(x, y) => (quickMenu = { x, y })}
						{penPressure}
						overlaySrc={traceSrc}
						overlayOpacity={trace.opacity}
						overlayTransform={trace}
						adjustOverlay={trace.adjusting}
						onoverlaymove={(dx, dy) => (trace = { ...trace, x: trace.x + dx, y: trace.y + dy })}
						fillTolerance={toolOpts.fillTolerance}
						fillAllLayers={toolOpts.fillAllLayers}
						fillReference={toolOpts.fillReference}
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
						onchange={(s) => (status = s)}
						onblocked={onBlocked}
						locked={timeUp}
					/>
				{/key}
			</div>
		</div>
		{#if timeUp}
			<div class="times-up" role="alert">
				<b>Time’s up!</b>
				{#if sessionMode}
					<span>{round + 1 >= sessionCount ? 'Grading your session…' : 'Next picture…'}</span>
				{:else if status.blank}
					<span>Nothing drawn this time.</span>
					<button type="button" onclick={tryAgain}>Try again</button>
				{:else}
					<span>Grading your {timeLabel(timeLimit)} drawing…</span>
				{/if}
			</div>
		{/if}
		{#if shapeNote}
			<p class="blocked" role="status">{shapeNote}</p>
		{/if}
		{#if blocked}
			<p class="blocked" role="status">
				{blocked === 'hidden'
					? `“${activeLayerName}” is hidden. Show it or pick another layer to draw.`
					: `“${activeLayerName}” is locked. Unlock it or pick another layer to draw.`}
			</p>
		{/if}
		{#if reference && refPlace === 'dock'}
			<div class="pip" aria-label="Reference (small)">
				<div class="pip-bar">
					<span>Reference</span>
					<button
						type="button"
						aria-label="Show the reference bigger, beside the canvas"
						title="Bigger: beside the canvas"
						onclick={() => setPlace('beside')}
					>
						⤢
					</button>
				</div>
				{#key reference.id}
					<img src={imageUrl(reference)} alt="" />
				{/key}
			</div>
		{/if}
	{/snippet}
	{#snippet dockTop()}
		{#if reference}
			<Tile as="section" label="Reference">
				<div class="tile-head">
					<h2>Reference</h2>
					{#if reference.own}
						<a class="link-btn" href="/practice/own">Choose another</a>
					{:else if category && !daily && !personal}
						<button
							type="button"
							class="link-btn"
							disabled={!status.blank}
							title={status.blank ? 'Show a different reference' : 'Clear the canvas to switch'}
							onclick={nextReference}
						>
							New one ↻
						</button>
					{/if}
				</div>
				{#if refPlace !== 'dock'}
					<p class="muted">
						{refPlace === 'float'
							? 'The picture is in a floating window.'
							: 'The picture is beside the canvas.'}
					</p>
					<button type="button" class="link-btn" onclick={() => setPlace('dock')}>
						Put it back here
					</button>
				{:else}
					{#key reference.id}
						<RefViewer
							src={imageUrl(reference)}
							alt="Reference: {reference.tags.join(', ')}"
							gray={grayView}
							grid={refGrid}
							onpick={picked}
							{notan}
							{squint}
							unit={refUnit}
							angle={refAngle}
							onunit={(m) => {
								refUnit = m;
								if (m) tool = 'measure';
							}}
							onangle={(m) => (refAngle = m)}
						/>
					{/key}
					<div class="place-btns">
						<button
							type="button"
							class="tool"
							title="Show the reference bigger, in its own pane beside the canvas"
							onclick={() => setPlace('beside')}
						>
							Beside the canvas
						</button>
						<button
							type="button"
							class="tool"
							title="Show the reference in a window you can move and resize"
							onclick={() => setPlace('float')}
						>
							Float
						</button>
					</div>
				{/if}
				{#if reference.own}
					<p class="credit">Your own picture. It stays on this device and isn’t uploaded.</p>
				{:else}
					<p class="credit one-line">
						<RefCredit {reference} />
						{#if reference.live}· <span class="badge-live">Endless</span>{/if}
					</p>
				{/if}
				{#if reference.own}
					<!-- nothing to report: it's the student's own picture -->
				{:else if reported}
					<p class="credit" role="status">Thanks! That picture is blocked for everyone.</p>
				{:else if reportOpen}
					<div class="report" role="group" aria-label="Report this picture">
						<span>What’s wrong?</span>
						<button type="button" onclick={() => report('unsafe')}>Not safe</button>
						<button type="button" onclick={() => report('ai_made')}>Looks AI-made</button>
						<button type="button" onclick={() => report('not_drawable')}> Can’t draw this </button>
						<button type="button" class="cancel" onclick={() => (reportOpen = false)}>
							Cancel
						</button>
					</div>
				{:else}
					<button type="button" class="link-btn small" onclick={() => (reportOpen = true)}>
						Report picture
					</button>
				{/if}
			</Tile>
		{:else if refLoading}
			<Tile as="section" label="Reference">
				<div class="tile-head"><h2>Reference</h2></div>
				<div class="ref-skeleton" aria-busy="true">
					{refWaiting ? 'Gathering pictures for this category… one moment' : 'Finding a picture…'}
				</div>
			</Tile>
		{:else if ownMode}
			<Tile tone="lilac" as="section" label="Reference">
				<h2>No picture chosen</h2>
				<p class="muted-dark">
					Your picture is kept only while this tab is open. <a href="/practice/own">Choose one</a>
					to draw from.
				</p>
			</Tile>
		{:else if category}
			<Tile tone="lilac" as="section" label="Reference">
				<h2>No {category.name.toLowerCase()} references yet</h2>
				<p class="muted-dark">Free draw for now. References are being curated.</p>
			</Tile>
		{/if}
	{/snippet}
	{#snippet panel(t: string)}
		{#if t === 'layers'}
			<section aria-label="Layers">
				<LayersPanel
					{status}
					controls={canvas}
					onadjustment={(id) => toolOpts.adjustmentLayer(id)}
				/>
			</section>
		{:else if t === 'brushes'}
			<section aria-label="Brushes">
				{#if manifest}
					<BrushPicker {manifest} bind:selected={brushId} />
					<BrushTexturePanel {brushId} />
				{:else}
					<p class="muted">Loading brushes…</p>
				{/if}
			</section>
		{:else if t === 'history'}
			<HistoryPanel history={status.history} ongo={(n) => canvas?.historyGo(n)} />
		{:else}
			<ColorPanel bind:color onclose={() => setTab('layers')} />
		{/if}
	{/snippet}
	{#snippet dockBottom()}
		<p class="hint">
			Shortcuts: <kbd>B</kbd> brush · <kbd>E</kbd> eraser · <kbd>[</kbd> <kbd>]</kbd> size ·
			<kbd>Ctrl</kbd>+<kbd>Z</kbd> undo · <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> redo.
		</p>
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
		{#if refFloating && reference}
			<section
				class="floating"
				aria-label="Reference picture"
				style="left: {floatBox.x}px; top: {floatBox.y}px; width: {floatBox.w}px; height: {floatBox.h}px"
				onpointerup={(e) => {
					const el = e.currentTarget as HTMLElement;
					floatBox = { ...floatBox, w: el.offsetWidth, h: el.offsetHeight };
					remember('pp.refFloat', floatBox);
				}}
			>
				<div
					class="float-bar"
					role="presentation"
					onpointerdown={(e) => {
						if ((e.target as HTMLElement).closest('button')) return;
						e.currentTarget.setPointerCapture(e.pointerId);
						floatDrag = { x: e.clientX, y: e.clientY, bx: floatBox.x, by: floatBox.y };
					}}
					onpointermove={(e) => {
						if (!floatDrag) return;
						floatBox = {
							...floatBox,
							x: floatDrag.bx + e.clientX - floatDrag.x,
							y: floatDrag.by + e.clientY - floatDrag.y
						};
					}}
					onpointerup={() => {
						floatDrag = null;
						keepFloatOnScreen();
					}}
				>
					<span>Reference · drag to move</span>
					<button type="button" class="tool" onclick={() => setPlace('beside')}>Beside</button>
					<button type="button" class="tool" onclick={() => setPlace('dock')}>Dock</button>
				</div>
				{#key reference.id}
					<RefViewer
						src={imageUrl(reference)}
						alt="Reference: {reference.tags.join(', ')}"
						fill
						gray={grayView}
						grid={refGrid}
						onpick={picked}
						{notan}
						{squint}
						unit={refUnit}
						angle={refAngle}
						onunit={(m) => {
							refUnit = m;
							if (m) tool = 'measure';
						}}
						onangle={(m) => (refAngle = m)}
					/>
				{/key}
			</section>
		{/if}
	{/snippet}
</Studio>

<style>
	h1 {
		font-size: 30px;
		letter-spacing: -0.8px;
	}
	h2 {
		font-size: 20px;
		font-weight: 700;
		letter-spacing: -0.3px;
	}
	.timer {
		display: flex;
		align-items: center;
		gap: 8px;
		min-height: 48px;
		padding: 10px 18px;
		background: var(--pp-yellow);
		border: 0;
		border-radius: var(--pp-radius-btn);
		font: 700 22px var(--pp-font-display);
		color: var(--pp-ink);
		cursor: pointer;
	}
	.timer[aria-pressed='false'] {
		background: var(--pp-surface-sunk);
	}
	.split {
		position: absolute;
		inset: 0;
		display: flex;
	}
	.canvas-pane {
		position: relative;
		flex: 1 1 0;
		min-width: 0;
		min-height: 0;
		height: 100%;
	}
	.ref-pane {
		flex: 0 0 var(--split);
		min-width: 0;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 8px;
		box-sizing: border-box;
		background: var(--pp-surface);
	}
	.ref-pane :global(.ref) {
		flex: 1;
	}
	.ref-pane-bar {
		flex: none;
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 13px;
		font-weight: 700;
	}
	.ref-pane-bar span {
		flex: 1;
	}
	.float-bar span {
		margin-right: auto;
	}
	.ref-pane .ref-skeleton {
		flex: 1;
	}
	.divider {
		flex: none;
		width: 12px;
		position: relative;
		cursor: col-resize;
		touch-action: none;
		background: var(--pp-surface-sunk);
	}
	.divider::after {
		content: '';
		position: absolute;
		left: 50%;
		top: 50%;
		width: 4px;
		height: 44px;
		transform: translate(-50%, -50%);
		border-radius: 4px;
		background: var(--pp-ink-2);
		opacity: 0.45;
		transition: opacity 0.15s;
	}
	.divider:hover::after,
	.divider:focus-visible::after,
	.divider:active::after {
		opacity: 1;
		background: var(--pp-accent);
	}
	.divider:focus-visible {
		outline: 2px solid var(--pp-accent);
		outline-offset: -2px;
	}
	@media (max-width: 700px) {
		.split.beside {
			flex-direction: column;
		}
		.divider {
			width: auto;
			height: 14px;
			cursor: row-resize;
		}
		.divider::after {
			width: 44px;
			height: 4px;
		}
	}
	.place-btns {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}
	.floating {
		position: fixed;
		z-index: 30;
		min-width: 200px;
		min-height: 200px;
		max-width: calc(100vw - 16px);
		max-height: calc(100vh - 16px);
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 0 12px 12px;
		border-radius: 20px;
		background: var(--pp-surface);
		box-shadow: 0 10px 40px rgba(22, 22, 26, 0.25);
		resize: both;
		overflow: hidden;
	}
	.float-bar {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 8px;
		padding: 10px 2px 2px;
		cursor: move;
		touch-action: none;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-ink-2);
	}
	.check-wrap {
		position: relative;
	}
	.check-btn {
		min-height: 40px;
		padding: 8px 14px;
		border: 0;
		border-radius: var(--pp-radius-btn, 999px);
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
		font: 600 15px var(--pp-font-body);
		white-space: nowrap;
		cursor: pointer;
	}
	.check-btn:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}
	.check-pop {
		position: absolute;
		right: 0;
		top: calc(100% + 8px);
		z-index: 40;
		width: min(320px, 80vw);
		padding: 14px;
		border-radius: 16px;
		background: var(--pp-surface);
		box-shadow: 0 10px 40px rgba(22, 22, 26, 0.25);
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.check-pop p {
		margin: 0;
		font-size: 14px;
		color: var(--pp-ink-2);
	}
	.check-pop button {
		align-self: flex-end;
		padding: 6px 12px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-ink);
		color: #fff;
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}

	.sketch-note {
		flex: none;
		margin: 0;
		padding: 6px 12px;
		border-radius: 12px;
		font-size: 13px !important;
		background: var(--pp-lilac);
		color: var(--pp-lilac-ink);
		font-size: 14px;
	}
	.sketch-note b {
		color: var(--pp-ink);
	}
	.round {
		display: flex;
		align-items: center;
		min-height: 48px;
		padding: 10px 16px;
		border-radius: var(--pp-radius-btn);
		background: var(--pp-lilac);
		color: var(--pp-lilac-ink);
		font: 700 20px var(--pp-font-display);
	}
	span.timer {
		cursor: default;
	}
	.timer.urgent {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.times-up {
		position: absolute;
		inset: 0;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 8px;
		background: color-mix(in srgb, var(--pp-ink) 55%, transparent);
		color: #fff;
		font-size: 18px;
		font-weight: 600;
		text-align: center;
	}
	.times-up b {
		font: 800 48px var(--pp-font-display);
		letter-spacing: -1px;
	}
	.times-up button {
		min-height: 44px;
		padding: 8px 20px;
		border: 0;
		border-radius: 999px;
		background: #fff;
		color: var(--pp-ink);
		font: 600 16px var(--pp-font-body);
		cursor: pointer;
	}
	.blocked {
		position: absolute;
		left: 50%;
		bottom: 16px;
		transform: translateX(-50%);
		margin: 0;
		padding: 10px 16px;
		max-width: calc(100% - 32px);
		border-radius: 999px;
		background: var(--pp-ink);
		color: #fff;
		font-weight: 600;
		font-size: 14px;
		text-align: center;
		pointer-events: none;
	}
	.tile-head {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		gap: 8px;
	}
	.muted {
		color: var(--pp-muted);
	}

	.credit {
		margin: 0;
		font-size: 12px;
		color: var(--pp-muted);
	}
	.muted-dark {
		margin: 0;
		color: var(--pp-lilac-ink);
	}
	.link-btn {
		border: 0;
		background: none;
		font: 600 14px var(--pp-font-body);
		color: var(--pp-accent-ink);
		cursor: pointer;
		padding: 6px 0;
	}
	.link-btn:disabled {
		color: var(--pp-muted);
		cursor: not-allowed;
	}

	.ref-skeleton {
		height: 240px;
		border-radius: 18px;
		display: grid;
		place-items: center;
		color: var(--pp-muted);
		font-weight: 600;
		background: linear-gradient(
			90deg,
			var(--pp-surface-sunk),
			var(--pp-ground),
			var(--pp-surface-sunk)
		);
		background-size: 200% 100%;
		animation: shimmer 1.2s linear infinite;
	}
	@keyframes shimmer {
		to {
			background-position: -200% 0;
		}
	}
	.badge-live {
		padding: 1px 8px;
		border-radius: 999px;
		background: var(--pp-lilac);
		color: var(--pp-lilac-ink);
		font-weight: 600;
	}
	.link-btn.small {
		align-self: flex-start;
		font-size: 12px;
		color: var(--pp-muted);
		padding: 0;
	}
	.report {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		align-items: center;
		font-size: 13px;
	}
	.report button {
		padding: 6px 10px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-ground);
		font: 600 12px var(--pp-font-body);
		cursor: pointer;
	}
	.report button:not(.cancel) {
		color: var(--pp-accent-ink);
	}

	.tool {
		min-width: 38px;
		height: 38px;
		padding: 0 12px;
		display: grid;
		place-items: center;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.tool[aria-pressed='true'] {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.tool:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}
	.tool.danger {
		background: var(--pp-accent);
		color: #fff;
	}
	.slider {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 13px;
		color: var(--pp-on-ink-muted);
		flex: none;
		width: 150px;
	}
	.slider b {
		color: var(--pp-on-ink);
	}
	.slider input {
		accent-color: var(--pp-yellow);
	}

	kbd {
		font: 600 12px var(--pp-font-body);
		padding: 1px 6px;
		border-radius: 6px;
		background: var(--pp-surface);
		border: 1px solid var(--pp-track);
	}

	/* ---- Studio layout: top bar · tool rail | options + canvas | dock ---- */
	h1 {
		margin: 0;
		font-size: 21px;
		letter-spacing: -0.5px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	h2 {
		font-size: 18px;
		font-weight: 700;
		letter-spacing: -0.3px;
	}
	.undo-group {
		display: flex;
		gap: 4px;
	}
	.undo-group .tool {
		min-width: 40px;
		height: 40px;
		padding: 0 8px;
		border-radius: 12px;
	}
	.timer {
		display: flex;
		align-items: center;
		gap: 6px;
		min-height: 40px;
		padding: 6px 12px;
		background: var(--pp-yellow);
		border: 0;
		border-radius: 12px;
		font: 700 17px var(--pp-font-display);
		color: var(--pp-ink);
		cursor: pointer;
	}
	.timer[aria-pressed='false'] {
		background: var(--pp-surface-sunk);
	}

	.brush-name {
		font: 700 15px var(--pp-font-display);
		white-space: nowrap;
	}
	.pip {
		display: none;
	}

	.credit.one-line {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.caret {
		margin-left: 4px;
		font-size: 11px;
	}
	.save {
		position: relative;
	}
	.save-btn {
		min-height: 40px;
		padding: 0 14px;
		border: 0;
		border-radius: 12px;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
		font: 600 15px var(--pp-font-body);
		cursor: pointer;
	}
	.save-btn:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}
	.save-menu {
		position: absolute;
		right: 0;
		top: calc(100% + 6px);
		z-index: 40;
		display: flex;
		flex-direction: column;
		gap: 4px;
		width: 250px;
		padding: 6px;
		background: var(--pp-surface);
		border-radius: 14px;
		box-shadow: 0 12px 34px rgba(22, 22, 26, 0.2);
	}
	.save-menu button {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 2px;
		padding: 10px 12px;
		border: 0;
		border-radius: 10px;
		background: transparent;
		color: var(--pp-ink);
		font: 14px var(--pp-font-body);
		text-align: left;
		cursor: pointer;
	}
	.save-menu button:hover,
	.save-menu button:focus-visible {
		background: var(--pp-surface-sunk);
	}
	.menu-link {
		padding: 8px 12px;
		font-size: 13px;
		font-weight: 700;
		color: var(--pp-accent-ink);
		text-decoration: none;
	}
	.save-menu span {
		font-size: 12px;
		color: var(--pp-ink-2);
	}
	.hint {
		margin: 0;
		padding: 0 6px 6px;
		font-size: 12px;
		color: var(--pp-muted);
	}

	@media (max-width: 1100px) {
		.pip {
			display: flex;
			flex-direction: column;
			position: absolute;
			left: 12px;
			top: 12px;
			z-index: 5;
			width: clamp(120px, 24%, 220px);
			background: var(--pp-surface);
			border-radius: 14px;
			overflow: hidden;
			box-shadow: 0 10px 30px rgba(22, 22, 26, 0.22);
		}
		.pip-bar {
			display: flex;
			align-items: center;
			justify-content: space-between;
			padding: 4px 4px 4px 10px;
			font-size: 12px;
			font-weight: 700;
		}
		.pip-bar button {
			width: 28px;
			height: 28px;
			border: 0;
			border-radius: 8px;
			background: var(--pp-surface-sunk);
			cursor: pointer;
		}
		.pip img {
			display: block;
			width: 100%;
			max-height: 200px;
			object-fit: contain;
			background: #fff;
		}
	}

	@media (max-width: 700px) {
		.wide-only {
			display: none;
		}
		.pip {
			top: auto;
			bottom: 8px;
		}
		h1 {
			font-size: 16px;
		}
		.undo-group .tool:last-child {
			display: none;
		}
		.pip {
			left: auto;
			right: 8px;
			width: 110px;
		}
	}
</style>
