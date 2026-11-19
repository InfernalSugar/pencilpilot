<script lang="ts">
	import { onMount } from 'svelte';
	import { warmGrader } from '#lib/grader.js';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import PaintCanvas, {
		BLANK_STATUS,
		type CanvasStatus,
		type ImportResult
	} from '#lib/paint/PaintCanvas.svelte';
	import LayersPanel from '#lib/paint/LayersPanel.svelte';
	import CanvasSizePicker from '#lib/paint/CanvasSizePicker.svelte';
	import RefViewer from '#lib/paint/RefViewer.svelte';
	import { addToSession, clockOf, startSession, timeLabel } from '#lib/timed.js';
	import ColorPanel, { rememberColor } from '#lib/paint/ColorPanel.svelte';
	import {
		anyGuide,
		defaultGuides,
		SYMMETRY_MODES,
		symmetryOn,
		type GuideSettings
	} from '#lib/paint/guides.js';
	import TracePanel, { TRACE_DEFAULTS, type TraceState } from '#lib/paint/TracePanel.svelte';
	import { savedSize, saveSize, type CanvasSize } from '#lib/paint/canvas-size.js';
	import BrushPicker from '#lib/paint/BrushPicker.svelte';
	import { DEFAULT_BRUSH, loadManifest, type BrushManifest } from '#lib/paint/brushes.js';
	import { categories } from '#lib/categories.js';
	import { saveSubmission, type Submission } from '#lib/submission.js';
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
	import { fetchLiveReference, recentRefs, rememberRef, reportReference } from '#lib/live.js';
	import { topicName, topicsFor } from '#lib/topics.js';
	import { suggestTopic } from '#lib/progress.js';

	const category = categories.find((c) => c.id === page.url.searchParams.get('category'));
	const filter = {
		topic: page.url.searchParams.get('topic'),
		tier: Number(page.url.searchParams.get('tier')) || null
	};
	let reference = $state<Reference | undefined>(byId(page.url.searchParams.get('ref')));
	let refLoading = $state(false);
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
		| 'gradient';
	let tool = $state<Tool>('brush');
	let gradientKind = $state<'linear' | 'radial'>('linear');
	let importNote = $state('');
	const importText = (r: ImportResult) =>
		({
			ok: '',
			'too-many': 'You’ve reached the layer limit (32).',
			unreadable: 'That file couldn’t be opened as a picture.',
			busy: 'Finish what you’re doing first, then import again.',
			'too-big': 'That picture is too big (30 MB at most).'
		})[r];
	// paste a picture (Ctrl+V) to import it as a layer
	$effect(() => {
		const onPaste = (e: ClipboardEvent) => {
			const t = e.target as HTMLElement | null;
			if (t?.closest('input, textarea, [contenteditable]')) return;
			const file = [...(e.clipboardData?.files ?? [])].find((f) => f.type.startsWith('image/'));
			if (!file || !canvas) return;
			e.preventDefault();
			void canvas.importImage(file, 'Pasted picture').then((r) => (importNote = importText(r)));
		};
		window.addEventListener('paste', onPaste);
		return () => window.removeEventListener('paste', onPaste);
	});
	let gradientEnd = $state<string>('transparent');
	let gradientEndColor = $state('#3d6bff');

	// ---- Adjust panel: live preview on the active layer, Apply or Cancel ----
	const NO_ADJUST = { hue: 0, saturation: 0, brightness: 0, contrast: 0, blur: 0 };
	let adjustOpen = $state(false);
	let adjust = $state({ ...NO_ADJUST });
	let recolour = $state(false);
	let adjustMsg = $state('');
	function openAdjust() {
		if (adjustOpen) return closeAdjust(false);
		adjust = { ...NO_ADJUST };
		recolour = false;
		if (!canvas?.adjustBegin()) {
			adjustMsg = 'This layer is locked or hidden, so it can’t be adjusted.';
			return;
		}
		adjustMsg = '';
		tool = 'brush';
		adjustOpen = true;
	}
	function previewAdjust() {
		canvas?.adjustPreview({ ...adjust, recolour: recolour ? color : null });
	}
	function closeAdjust(keep: boolean) {
		if (keep) canvas?.adjustCommit();
		else canvas?.adjustCancel();
		adjustOpen = false;
	}
	let guides = $state<GuideSettings>(defaultGuides(800, 600));
	let guidesOpen = $state(false);
	function setGuides(g: GuideSettings) {
		guides = g;
		remember('pp.guides', { w: canvasSize.width, h: canvasSize.height, g });
	}
	/** guides saved for another canvas size are scaled to this one */
	function loadGuides() {
		const saved = recall<{ w: number; h: number; g: GuideSettings } | null>('pp.guides', null);
		const { width: w, height: h } = canvasSize;
		if (!saved?.g) return void (guides = defaultGuides(w, h));
		const [kx, ky] = [w / saved.w, h / saved.h];
		const sc = (p: [number, number]): [number, number] => [p[0] * kx, p[1] * ky];
		const g = saved.g;
		guides = {
			grid: { ...g.grid, size: Math.round(g.grid.size * Math.min(kx, ky)) },
			perspective: { ...g.perspective, vps: g.perspective.vps.map(sc) },
			ruler: { ...g.ruler, a: sc(g.ruler.a), b: sc(g.ruler.b) },
			symmetry: g.symmetry ? { ...g.symmetry, c: sc(g.symmetry.c) } : defaultGuides(w, h).symmetry
		};
	}
	/** pen pressure and tilt on/off (remembered) */
	let penPressure = $state(true);
	let liquifySize = $state(60);
	let liquifyStrength = $state(0.6);
	let selectMode = $state<'replace' | 'add' | 'subtract'>('replace');
	let wandTolerance = $state(0.15);
	const SELECT_TOOLS: { id: Tool; label: string; key: string; icon: string }[] = [
		{ id: 'select-rect', label: 'Rectangle select (M)', key: 'm', icon: 'M4 4h16v16H4Z' },
		{
			id: 'select-lasso',
			label: 'Lasso select (L)',
			key: 'l',
			icon: 'M7 20c-2 0-3-1-3-2.5S6 15 8 15c6 0 12-2 12-7 0-3-3-5-8-5S3 5 3 9c0 2 2 4 5 6'
		},
		{
			id: 'select-wand',
			label: 'Magic wand (W)',
			key: 'w',
			icon: 'm4 20 11-11M15 4v3M19 8h3M18 5l2-2M11 6 9 4'
		}
	];
	let fillTolerance = $state(0.12);
	let fillAllLayers = $state(true);
	let quickShape = $state(true);
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
		shapeNote = `Snapped to a ${kind}`;
		clearTimeout(shapeTimer);
		shapeTimer = setTimeout(() => (shapeNote = ''), 1800);
	}
	let smoothing = $state(0);
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
		if (tracing && !reference) trace = { ...trace, source: 'own' };
		if (!tracing) trace = { ...trace, adjusting: false };
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
			(await fetchLiveReference({
				category: category.id,
				topic,
				tier: filter.tier,
				exclude: avoid
			})) ?? undefined;
		r ??= pickCurated(avoid, f) ?? pickCurated(avoid) ?? pickCurated(avoid, { tier: filter.tier });
		reference = r;
		refLoading = false;
		if (r) rememberRef(r.id);
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
	let size = $state(1);
	let opacity = $state(1);
	let color = $state('#16161a');
	let status = $state<CanvasStatus>(BLANK_STATUS);
	let canvasSize = $state<CanvasSize>({ width: 800, height: 600 });
	function pickSize(next: CanvasSize) {
		canvasSize = next;
		saveSize(next);
		status = BLANK_STATUS;
		loadGuides();
	}

	// Side dock (reference + Layers / Brushes / Colour tabs). Always shown on wide
	// screens; a slide-over (tablet) or bottom sheet (phone) on narrow ones.
	const DOCK_TABS = [
		{ id: 'layers', label: 'Layers' },
		{ id: 'brushes', label: 'Brushes' },
		{ id: 'colour', label: 'Colour' }
	] as const;
	type DockTab = (typeof DOCK_TABS)[number]['id'];
	let dockTab = $state<DockTab>('layers');
	let dockOpen = $state(false);
	function setTab(t: DockTab) {
		dockTab = t;
		remember('pp.dockTab', t);
	}
	function openDock(t: DockTab) {
		setTab(t);
		dockOpen = true;
	}
	function toggleDock(t: DockTab) {
		if (dockOpen && dockTab === t) dockOpen = false;
		else openDock(t);
	}
	/** the studio fills the window below the site header */
	let studioH = $state('calc(100dvh - 72px)');
	function fitStudio() {
		const h = document.querySelector('body header')?.getBoundingClientRect().height ?? 72;
		studioH = `calc(100dvh - ${Math.round(h)}px)`;
	}
	let refFloating = $state(false);
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
	function setFloating(on: boolean) {
		refFloating = on;
		remember('pp.refFloating', on);
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
	let seconds = $state(0);
	let timerOn = $state(true);

	/** daily challenge day (?daily=YYYY-MM-DD): the picture is fixed */
	const daily = page.url.searchParams.get('daily');
	/** personal challenge day (?personal=YYYY-MM-DD): the picture is fixed too */
	const personal = page.url.searchParams.get('personal');
	/** where the back button goes */
	const backHref = $derived(daily ? '/daily' : category ? `/practice/${category.id}` : '/');

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

	const SWATCHES = ['#16161a', '#6b6a70', '#b9b6ae', '#e8492f', '#3d6bff', '#1f9d74', '#f2a93b'];
	const brushName = $derived(manifest?.brushes.find((b) => b.id === brushId)?.name ?? '…');
	const clock = $derived(
		`${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
	);

	onMount(() => {
		warmGrader();
		canvasSize = savedSize();
		dockTab = recall<DockTab>('pp.dockTab', 'layers');
		if (!DOCK_TABS.some((t) => t.id === dockTab)) dockTab = 'layers';
		fitStudio();
		window.addEventListener('resize', fitStudio);
		refFloating = recall('pp.refFloating', false);
		floatBox = recall('pp.refFloat', floatBox);
		smoothing = recall('pp.smoothing', 0);
		quickShape = recall('pp.quickShape', true);
		penPressure = recall('pp.penPressure', true);
		loadGuides();
		loadManifest().then((m) => (manifest = m));
		if (!reference) loadReference();
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
			window.removeEventListener('resize', fitStudio);
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
				if (adjustOpen) closeAdjust(false);
				canvas?.undo();
			}
		} else if (mod && k === 'y') {
			e.preventDefault();
			if (adjustOpen) closeAdjust(false);
			canvas?.redo();
		} else if (!mod && k === 'b') {
			eraser = false;
			tool = 'brush';
		} else if (!mod && k === 'e') {
			eraser = true;
			tool = 'brush';
		} else if (!mod && k === 'i') tool = tool === 'picker' ? 'brush' : 'picker';
		else if (!mod && k === 'g') tool = tool === 'fill' ? 'brush' : 'fill';
		else if (!mod && k === 'v') tool = tool === 'transform' ? 'brush' : 'transform';
		else if (!mod && k === 'k') tool = tool === 'liquify' ? 'brush' : 'liquify';
		else if (tool === 'transform' && e.key === 'Enter') canvas?.transformCommit();
		else if (tool === 'transform' && e.key === 'Escape') canvas?.transformCancel();
		else if (!mod && (k === 'm' || k === 'l' || k === 'w')) {
			const t = SELECT_TOOLS.find((x) => x.key === k)!.id;
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

	async function submit() {
		const sub = buildSubmission();
		if (!sub) return;
		saveSubmission(sub);
		await goto('/results');
	}
</script>

<svelte:window onkeydown={onkey} />
<svelte:head><title>Draw · PencilPilot</title></svelte:head>

<div class="studio" style="--studio-h: {studioH}">
	<header class="sbar">
		<a class="icon-btn back" href={backHref} aria-label="Back" title="Back">
			<svg
				width="20"
				height="20"
				viewBox="0 0 24 24"
				fill="none"
				stroke="currentColor"
				stroke-width="2.5"
				stroke-linecap="round"
				stroke-linejoin="round"
				aria-hidden="true"
			>
				<path d="M15 5l-7 7 7 7" />
			</svg>
		</a>
		<div class="title">
			<span class="pp-eyebrow">
				{daily ? 'Today’s challenge · ' : ''}{personal ? 'Your challenge · ' : ''}{drawMode ===
				'sketch'
					? 'Sketch · '
					: ''}{category ? `${category.name} · ` : ''}{reference
					? [topicName(reference.category, reference.topic), tierName(tierOf(reference))]
							.filter(Boolean)
							.join(' · ')
					: 'Free draw'}
			</span>
			<h1>{reference ? titleOf(reference) : 'Draw anything'}</h1>
		</div>
		<div class="sbar-actions">
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
						if (adjustOpen) closeAdjust(false);
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
			<button
				type="button"
				class="icon-btn dock-btn"
				aria-label="Layers"
				aria-pressed={dockOpen && dockTab === 'layers'}
				title="Layers"
				onclick={() => toggleDock('layers')}
			>
				<svg
					width="20"
					height="20"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.2"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path d="m12 3 9 5-9 5-9-5Z" /><path d="m3 13 9 5 9-5" />
				</svg>
			</button>
			<button
				type="button"
				class="icon-btn dock-btn"
				aria-label="Brushes"
				aria-pressed={dockOpen && dockTab === 'brushes'}
				title="Brushes"
				onclick={() => toggleDock('brushes')}
			>
				<svg
					width="20"
					height="20"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.2"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path d="M9.5 14.5 19 5a2 2 0 0 1 3 3l-9.5 9.5" /><path
						d="M7 16c-2 0-3 1.5-3 3.5 0 .8-.6 1.5-1.5 1.5H7a4 4 0 0 0 4-4c0-1.3-1-2-2-2Z"
					/>
				</svg>
			</button>
			{#if !sessionMode}
				<CanvasSizePicker size={canvasSize} blank={status.blank} onpick={pickSize} />
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
		</div>
	</header>

	<div class="work">
		<div class="rail" role="toolbar" aria-label="Drawing tools" aria-orientation="vertical">
			<div class="rail-tools">
				<button
					type="button"
					class="tool"
					aria-pressed={!eraser && tool === 'brush'}
					aria-label="Brush (B)"
					title="Brush (B)"
					onclick={() => {
						eraser = false;
						tool = 'brush';
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
						<path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
					</svg>
				</button>
				<button
					type="button"
					class="tool"
					aria-pressed={eraser && tool === 'brush'}
					aria-label="Eraser (E)"
					title="Eraser (E) — uses the current brush shape"
					onclick={() => {
						eraser = true;
						tool = 'brush';
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
						<path d="m7 21-4-4L14 6l5 5-8 10Z" /><path d="M22 21H7" />
					</svg>
				</button>
				<button
					type="button"
					class="tool"
					aria-pressed={tool === 'picker'}
					aria-label="Eyedropper (I)"
					title="Eyedropper (I): pick a colour from the canvas. Alt+click works too."
					onclick={() => (tool = tool === 'picker' ? 'brush' : 'picker')}
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
						<path d="m2 22 1-4 9-9 3 3-9 9Z" /><path d="m14 7 3-3a2.1 2.1 0 0 1 3 3l-3 3" /><path
							d="m11 6 7 7"
						/>
					</svg>
				</button>
				<button
					type="button"
					class="tool"
					aria-pressed={tool === 'fill'}
					aria-label="Fill (G)"
					title="Fill (G): fill an area with the colour. Or drag a colour swatch onto the canvas."
					onclick={() => (tool = tool === 'fill' ? 'brush' : 'fill')}
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
						<path d="m19 11-8-8-8.5 8.5a2.1 2.1 0 0 0 0 3L8 20a2.1 2.1 0 0 0 3 0Z" /><path
							d="m5 2 5 5"
						/><path d="M2 13h15" /><path d="M22 20a2 2 0 1 1-4 0c0-1.6 2-3 2-3s2 1.4 2 3Z" />
					</svg>
				</button>
				{#each SELECT_TOOLS as t (t.id)}
					<button
						type="button"
						class="tool"
						aria-pressed={tool === t.id}
						aria-label={t.label}
						title="{t.label}. Shift adds, Alt subtracts."
						onclick={() => (tool = tool === t.id ? 'brush' : t.id)}
					>
						<svg
							width="22"
							height="22"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="2.1"
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-dasharray={t.id === 'select-wand' ? undefined : '3 2.5'}
							aria-hidden="true"
						>
							<path d={t.icon} />
						</svg>
					</button>
				{/each}
				<button
					type="button"
					class="tool"
					aria-pressed={tool === 'transform'}
					aria-label="Transform (V)"
					title="Transform (V): move, scale, rotate or flip the selection (or the whole layer)"
					onclick={() => (tool = tool === 'transform' ? 'brush' : 'transform')}
				>
					<svg
						width="22"
						height="22"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="2.1"
						stroke-linecap="round"
						stroke-linejoin="round"
						aria-hidden="true"
					>
						<path
							d="M12 2v20M2 12h20M12 2l-3 3M12 2l3 3M12 22l-3-3M12 22l3-3M2 12l3-3M2 12l3 3M22 12l-3-3M22 12l-3 3"
						/>
					</svg>
				</button>
				<button
					type="button"
					class="tool"
					aria-pressed={tool === 'liquify'}
					aria-label="Liquify (K)"
					title="Liquify (K): push parts of the layer into place"
					onclick={() => (tool = tool === 'liquify' ? 'brush' : 'liquify')}
				>
					<svg
						width="22"
						height="22"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="2.1"
						stroke-linecap="round"
						stroke-linejoin="round"
						aria-hidden="true"
					>
						<path d="M3 12c3-6 6 6 9 0s6 6 9 0" /><path d="M3 18c3-6 6 6 9 0s6 6 9 0" />
					</svg>
				</button>
				<button
					type="button"
					class="tool"
					aria-pressed={tool === 'gradient'}
					aria-label="Gradient"
					title="Gradient: drag across the canvas"
					onclick={() => (tool = tool === 'gradient' ? 'brush' : 'gradient')}
				>
					<svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true">
						<defs>
							<linearGradient id="gr-ic" x1="0" x2="1">
								<stop offset="0" stop-color="currentColor" />
								<stop offset="1" stop-color="currentColor" stop-opacity="0" />
							</linearGradient>
						</defs>
						<rect
							x="3"
							y="5"
							width="18"
							height="14"
							rx="3"
							fill="url(#gr-ic)"
							stroke="currentColor"
							stroke-width="2"
						/>
					</svg>
				</button>
				<button
					type="button"
					class="tool"
					aria-pressed={adjustOpen}
					aria-expanded={adjustOpen}
					aria-label="Adjust"
					title="Adjust the layer: colour, brightness, blur"
					onclick={openAdjust}
				>
					<svg
						width="22"
						height="22"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="2.1"
						stroke-linecap="round"
						aria-hidden="true"
					>
						<path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0" /><circle
							cx="16"
							cy="6"
							r="2"
						/><circle cx="10" cy="12" r="2" /><circle cx="18" cy="18" r="2" />
					</svg>
				</button>
			</div>
			<div class="rail-sliders">
				<label class="slider rail-slider">
					<span>Size <b>{Math.round(size * 100)}%</b></span>
					<input
						type="range"
						min="-2"
						max="2.6"
						step="0.05"
						value={Math.log2(size)}
						oninput={(e) => (size = +(2 ** +e.currentTarget.value).toFixed(2))}
					/>
				</label>
				<label class="slider rail-slider">
					<span>Opacity <b>{Math.round(opacity * 100)}%</b></span>
					<input type="range" min="0.05" max="1" step="0.05" bind:value={opacity} />
				</label>
			</div>
		</div>

		<div class="center">
			<div class="opts" role="group" aria-label="Brush options">
				<span class="brush-name">{brushName}</span>
				<div class="group swatches" role="radiogroup" aria-label="Colour">
					{#each SWATCHES as c (c)}
						<button
							type="button"
							role="radio"
							aria-checked={color === c}
							aria-label="Colour {c}"
							draggable="true"
							ondragstart={(e) => e.dataTransfer?.setData('application/x-pp-color', c)}
							class="swatch"
							style="background: {c}"
							onclick={() => (color = c)}
						></button>
					{/each}
					<button
						type="button"
						class="swatch custom"
						aria-label="More colours"
						aria-expanded={dockTab === 'colour' && dockOpen}
						title="Colour wheel, recent colours and palettes"
						onclick={() => openDock('colour')}
					></button>
				</div>
				<label class="slider">
					<span>Smoothing <b>{Math.round(smoothing * 100)}%</b></span>
					<input
						type="range"
						min="0"
						max="1"
						step="0.05"
						bind:value={smoothing}
						onchange={() => remember('pp.smoothing', smoothing)}
						title="Steadies wobbly lines (like StreamLine)"
					/>
				</label>
				<button
					type="button"
					class="tool"
					aria-pressed={anyGuide(guides)}
					aria-expanded={guidesOpen}
					title="Guides: grid, perspective and ruler"
					onclick={() => (guidesOpen = !guidesOpen)}
				>
					Guides
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
					aria-pressed={quickShape}
					title="QuickShape: hold at the end of a stroke to snap it to a line, circle or rectangle"
					onclick={() => {
						quickShape = !quickShape;
						remember('pp.quickShape', quickShape);
					}}
				>
					QuickShape
				</button>
				<button
					type="button"
					class="tool"
					aria-pressed={tracing}
					title="Show a picture faintly inside the canvas to trace over (no score)"
					onclick={toggleTracing}
				>
					Trace
				</button>
			</div>
			<div class="ctx">
				{#if guidesOpen}
					<div class="fill-opts guides-opts" role="group" aria-label="Guides">
						<span class="opt-label">Grid</span>
						<div class="seg" role="radiogroup" aria-label="Grid">
							{#each [['off', 'Off'], ['square', 'Square'], ['isometric', 'Isometric']] as [id, label] (id)}
								<button
									type="button"
									class="tool"
									role="radio"
									aria-checked={id === 'off'
										? !guides.grid.on
										: guides.grid.on && guides.grid.kind === id}
									onclick={() =>
										setGuides({
											...guides,
											grid:
												id === 'off'
													? { ...guides.grid, on: false }
													: { ...guides.grid, on: true, kind: id as 'square' | 'isometric' }
										})}
								>
									{label}
								</button>
							{/each}
						</div>
						{#if guides.grid.on}
							<label class="slider">
								<span>Grid size <b>{guides.grid.size}</b></span>
								<input
									type="range"
									min="10"
									max="300"
									step="2"
									value={guides.grid.size}
									oninput={(e) =>
										setGuides({
											...guides,
											grid: { ...guides.grid, size: +e.currentTarget.value }
										})}
								/>
							</label>
						{/if}
						<span class="opt-label">Perspective</span>
						<div class="seg" role="radiogroup" aria-label="Perspective">
							{#each [0, 1, 2, 3] as n (n)}
								<button
									type="button"
									class="tool"
									role="radio"
									aria-checked={guides.perspective.points === n}
									onclick={() =>
										setGuides({
											...guides,
											perspective: { ...guides.perspective, points: n as 0 | 1 | 2 | 3 }
										})}
								>
									{n ? `${n}-point` : 'Off'}
								</button>
							{/each}
						</div>
						{#if guides.perspective.points}
							<button
								type="button"
								class="tool"
								aria-pressed={guides.perspective.assist}
								title="Assist: lines snap to the perspective directions"
								onclick={() =>
									setGuides({
										...guides,
										perspective: { ...guides.perspective, assist: !guides.perspective.assist }
									})}
							>
								Assist
							</button>
						{/if}
						<button
							type="button"
							class="tool"
							aria-pressed={guides.ruler.on}
							title="Ruler: strokes started next to it run straight along it"
							onclick={() =>
								setGuides({ ...guides, ruler: { ...guides.ruler, on: !guides.ruler.on } })}
						>
							Ruler
						</button>
						<span class="opt-label">Symmetry</span>
						<div class="seg" role="radiogroup" aria-label="Symmetry">
							{#each SYMMETRY_MODES as m (m)}
								<button
									type="button"
									class="tool"
									role="radio"
									aria-checked={guides.symmetry.mode === m}
									onclick={() =>
										setGuides({ ...guides, symmetry: { ...guides.symmetry, mode: m } })}
								>
									{m === 'off' ? 'Off' : m[0].toUpperCase() + m.slice(1)}
								</button>
							{/each}
						</div>
						{#if guides.symmetry.mode === 'radial'}
							<label class="slider">
								<span>Copies <b>{guides.symmetry.n}</b></span>
								<input
									type="range"
									min="2"
									max="24"
									step="1"
									value={guides.symmetry.n}
									oninput={(e) =>
										setGuides({
											...guides,
											symmetry: { ...guides.symmetry, n: +e.currentTarget.value }
										})}
								/>
							</label>
						{/if}
						{#if guides.perspective.points || guides.ruler.on || symmetryOn(guides)}
							<button
								type="button"
								class="tool"
								aria-pressed={tool === 'guides'}
								title="Drag vanishing points and the ruler"
								onclick={() => (tool = tool === 'guides' ? 'brush' : 'guides')}
							>
								Move guides
							</button>
						{/if}
					</div>
				{/if}
				{#if adjustMsg}<p class="adjust-msg" role="alert">{adjustMsg}</p>{/if}
				{#if importNote}<p class="adjust-msg" role="alert">{importNote}</p>{/if}
				{#if adjustOpen}
					<div class="fill-opts adjust-opts" role="group" aria-label="Adjust layer">
						{#each [['hue', 'Hue', -180, 180, 1, '°'], ['saturation', 'Saturation', -100, 100, 1, '%'], ['brightness', 'Brightness', -100, 100, 1, '%'], ['contrast', 'Contrast', -100, 100, 1, '%'], ['blur', 'Blur', 0, 40, 1, ' px']] as [key, label, min, max, st, u] (key)}
							{@const k = key as keyof typeof adjust}
							{@const scale = k === 'hue' || k === 'blur' ? 1 : 100}
							<label class="slider">
								<span>{label} <b>{Math.round(adjust[k] * scale)}{u}</b></span>
								<input
									type="range"
									{min}
									{max}
									step={st}
									value={Math.round(adjust[k] * scale)}
									oninput={(e) => {
										adjust = { ...adjust, [k]: +e.currentTarget.value / scale };
										previewAdjust();
									}}
								/>
							</label>
						{/each}
						<button
							type="button"
							class="tool"
							aria-pressed={recolour}
							title="Paint the whole layer in the current colour (keeps its shading by transparency)"
							onclick={() => {
								recolour = !recolour;
								previewAdjust();
							}}
						>
							Recolour
						</button>
						<button type="button" class="tool done" onclick={() => closeAdjust(true)}>Apply</button>
						<button type="button" class="tool" onclick={() => closeAdjust(false)}>Cancel</button>
					</div>
				{/if}
				{#if tool === 'gradient'}
					<div class="fill-opts" role="group" aria-label="Gradient">
						<div class="seg" role="radiogroup" aria-label="Gradient shape">
							{#each [['linear', 'Straight'], ['radial', 'Round']] as [id, label] (id)}
								<button
									type="button"
									class="tool"
									role="radio"
									aria-checked={gradientKind === id}
									onclick={() => (gradientKind = id as 'linear' | 'radial')}
								>
									{label}
								</button>
							{/each}
						</div>
						<div class="seg" role="radiogroup" aria-label="Gradient end">
							<button
								type="button"
								class="tool"
								role="radio"
								aria-checked={gradientEnd === 'transparent'}
								onclick={() => (gradientEnd = 'transparent')}
							>
								To transparent
							</button>
							<button
								type="button"
								class="tool"
								role="radio"
								aria-checked={gradientEnd !== 'transparent'}
								onclick={() => (gradientEnd = 'colour')}
							>
								To a colour
							</button>
						</div>
						{#if gradientEnd !== 'transparent'}
							<label class="end-colour">
								<input type="color" bind:value={gradientEndColor} aria-label="End colour" />
							</label>
						{/if}
						<span class="opt-label">Drag across the canvas</span>
					</div>
				{/if}
				{#if tool === 'liquify'}
					<div class="fill-opts" role="group" aria-label="Liquify">
						<label class="slider">
							<span>Size <b>{liquifySize}</b></span>
							<input type="range" min="10" max="200" step="2" bind:value={liquifySize} />
						</label>
						<label class="slider">
							<span>Strength <b>{Math.round(liquifyStrength * 100)}%</b></span>
							<input type="range" min="0.1" max="1" step="0.05" bind:value={liquifyStrength} />
						</label>
					</div>
				{/if}
				{#if tool === 'transform'}
					<div class="fill-opts" role="group" aria-label="Transform">
						<button type="button" class="tool" onclick={() => canvas?.transformFlip('x')}>
							Flip ↔
						</button>
						<button type="button" class="tool" onclick={() => canvas?.transformFlip('y')}>
							Flip ↕
						</button>
						<button type="button" class="tool" onclick={() => canvas?.transformRotate(90)}>
							Rotate 90°
						</button>
						<button type="button" class="tool" onclick={() => canvas?.transformReset()}>
							Reset
						</button>
						<button type="button" class="tool done" onclick={() => canvas?.transformCommit()}>
							Done
						</button>
						<button type="button" class="tool" onclick={() => canvas?.transformCancel()}>
							Cancel
						</button>
					</div>
				{/if}
				{#if tool.startsWith('select') || status.selection}
					<div class="fill-opts" role="group" aria-label="Selection">
						{#if tool.startsWith('select')}
							<div class="seg" role="radiogroup" aria-label="Selection mode">
								{#each [['replace', 'New'], ['add', 'Add'], ['subtract', 'Subtract']] as [id, label] (id)}
									<button
										type="button"
										class="tool"
										role="radio"
										aria-checked={selectMode === id}
										onclick={() => (selectMode = id as typeof selectMode)}
									>
										{label}
									</button>
								{/each}
							</div>
						{/if}
						{#if tool === 'select-wand'}
							<label class="slider">
								<span>Tolerance <b>{Math.round(wandTolerance * 100)}%</b></span>
								<input type="range" min="0" max="0.6" step="0.02" bind:value={wandTolerance} />
							</label>
						{/if}
						<button type="button" class="tool" onclick={() => canvas?.selectAll()}>All</button>
						{#if status.selection}
							<button type="button" class="tool" onclick={() => canvas?.deselect()}>None</button>
							<button type="button" class="tool" onclick={() => canvas?.invertSelection()}>
								Invert
							</button>
							<button type="button" class="tool" onclick={() => canvas?.clear()}>
								Clear selected
							</button>
						{/if}
					</div>
				{/if}
				{#if tool === 'fill'}
					<div class="fill-opts" role="group" aria-label="Fill options">
						<label class="slider">
							<span>Tolerance <b>{Math.round(fillTolerance * 100)}%</b></span>
							<input type="range" min="0" max="0.6" step="0.02" bind:value={fillTolerance} />
						</label>
						<button
							type="button"
							class="tool"
							aria-pressed={fillAllLayers}
							title="Fill what you see (all layers) or only this layer's own pixels"
							onclick={() => (fillAllLayers = !fillAllLayers)}
						>
							{fillAllLayers ? 'Sample: all layers' : 'Sample: this layer'}
						</button>
					</div>
				{/if}
			</div>
			{#if tracing}
				<div class="ctx">
					{#if tracing}
						<TracePanel bind:trace hasReference={!!reference} onclose={toggleTracing} />
					{/if}
				</div>
			{/if}
			{#if drawMode === 'sketch'}
				<p class="sketch-note">
					<b>Sketch mode:</b> rough, scratchy lines are fine. Put guide lines on the
					<b>Construction</b> layer: they’re never marked as mistakes. Scored on proportions, big shapes
					and angles.
				</p>
			{/if}
			<div class="stage canvas-tile">
				{#key `${canvasSize.width}x${canvasSize.height}-${round}`}
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
						{smoothing}
						onpick={picked}
						{fillTolerance}
						{fillAllLayers}
						{quickShape}
						onshape={onShape}
						{selectMode}
						{wandTolerance}
						ontransformend={onTransformEnd}
						{liquifySize}
						{liquifyStrength}
						allowImport
						onimport={(r) => (importNote = importText(r))}
						{gradientKind}
						gradientEnd={gradientEnd === 'transparent' ? 'transparent' : gradientEndColor}
						{penPressure}
						{guides}
						onguides={setGuides}
						overlaySrc={traceSrc}
						overlayOpacity={trace.opacity}
						overlayTransform={trace}
						adjustOverlay={trace.adjusting}
						onoverlaymove={(dx, dy) => (trace = { ...trace, x: trace.x + dx, y: trace.y + dy })}
						onchange={(s) => (status = s)}
						onblocked={onBlocked}
						locked={timeUp}
					/>
				{/key}
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
				{#if reference && !refFloating}
					<div class="pip" aria-label="Reference (small)">
						<div class="pip-bar">
							<span>Reference</span>
							<button
								type="button"
								aria-label="Show the reference in the panel"
								onclick={() => openDock('layers')}
							>
								⤢
							</button>
						</div>
						{#key reference.id}
							<img src={imageUrl(reference)} alt="" />
						{/key}
					</div>
				{/if}
			</div>
		</div>

		{#if dockOpen}
			<button
				type="button"
				class="scrim"
				aria-label="Close the panel"
				onclick={() => (dockOpen = false)}
			></button>
		{/if}
		<aside class="dock" class:open={dockOpen} aria-label="Side panel">
			<button
				type="button"
				class="dock-close"
				aria-label="Close the panel"
				onclick={() => (dockOpen = false)}
			>
				×
			</button>
			<div class="ref-card">
				{#if reference}
					<Tile as="section" label="Reference">
						<div class="tile-head">
							<h2>Reference</h2>
							{#if category && !daily && !personal}
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
						{#if refFloating}
							<p class="muted">The picture is in a floating window.</p>
							<button type="button" class="link-btn" onclick={() => setFloating(false)}>
								Put it back here
							</button>
						{:else}
							{#key reference.id}
								<RefViewer src={imageUrl(reference)} alt="Reference: {reference.tags.join(', ')}" />
							{/key}
							<button
								type="button"
								class="link-btn small float-btn"
								onclick={() => setFloating(true)}
							>
								Float this picture ⧉
							</button>
						{/if}
						<p class="credit">
							By <a href={reference.authorUrl} target="_blank" rel="noreferrer">
								{reference.author}
							</a>
							via
							<a href={reference.pageUrl} target="_blank" rel="noreferrer">Pixabay</a> ·
							<a href={reference.licenseUrl} target="_blank" rel="noreferrer">
								{reference.license}
							</a>
							{#if reference.live}· <span class="badge-live">Endless</span>{/if}
						</p>
						{#if reported}
							<p class="credit" role="status">Thanks! That picture is blocked for everyone.</p>
						{:else if reportOpen}
							<div class="report" role="group" aria-label="Report this picture">
								<span>What’s wrong?</span>
								<button type="button" onclick={() => report('unsafe')}>Not safe</button>
								<button type="button" onclick={() => report('ai_made')}>Looks AI-made</button>
								<button type="button" onclick={() => report('not_drawable')}>
									Can’t draw this
								</button>
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
						<div class="ref-skeleton" aria-busy="true">Finding a picture…</div>
					</Tile>
				{:else if category}
					<Tile tone="lilac" as="section" label="Reference">
						<h2>No {category.name.toLowerCase()} references yet</h2>
						<p class="muted-dark">Free draw for now. References are being curated.</p>
					</Tile>
				{/if}
			</div>
			<div class="tabs" role="tablist" aria-label="Panels">
				{#each DOCK_TABS as t (t.id)}
					<button
						type="button"
						role="tab"
						id="tab-{t.id}"
						aria-selected={dockTab === t.id}
						aria-controls="panel-{t.id}"
						class="tab"
						onclick={() => setTab(t.id)}
					>
						{t.label}
						{#if t.id === 'layers'}<span class="tab-meta">{activeLayerName}</span>{/if}
						{#if t.id === 'brushes'}<span class="tab-meta">{brushName}</span>{/if}
					</button>
				{/each}
			</div>
			<div class="tab-panel" role="tabpanel" id="panel-{dockTab}" aria-labelledby="tab-{dockTab}">
				{#if dockTab === 'layers'}
					<section aria-label="Layers">
						<LayersPanel {status} controls={canvas} />
					</section>
				{:else if dockTab === 'brushes'}
					<section aria-label="Brushes">
						{#if manifest}
							<BrushPicker {manifest} bind:selected={brushId} />
						{:else}
							<p class="muted">Loading brushes…</p>
						{/if}
					</section>
				{:else}
					<ColorPanel bind:color onclose={() => setTab('layers')} />
				{/if}
			</div>
			<p class="hint">
				Shortcuts: <kbd>B</kbd> brush · <kbd>E</kbd> eraser · <kbd>[</kbd> <kbd>]</kbd> size ·
				<kbd>Ctrl</kbd>+<kbd>Z</kbd> undo · <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> redo.
			</p>
		</aside>
	</div>
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
				<button type="button" onclick={() => setFloating(false)}>Dock</button>
			</div>
			{#key reference.id}
				<RefViewer src={imageUrl(reference)} alt="Reference: {reference.tags.join(', ')}" fill />
			{/key}
		</section>
	{/if}
</div>

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
		justify-content: space-between;
		gap: 8px;
		padding: 10px 2px 2px;
		cursor: move;
		touch-action: none;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-ink-2);
	}
	.float-bar button {
		min-height: 32px;
		padding: 4px 12px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-ink);
		color: #fff;
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.tool.done {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.seg {
		display: flex;
		gap: 2px;
	}
	.tool[role='radio'][aria-checked='true'] {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.adjust-msg {
		margin: 0;
		color: #ffd6cc;
		font-weight: 600;
		font-size: 14px;
	}
	.end-colour input {
		width: 44px;
		height: 36px;
		padding: 0;
		border: 0;
		border-radius: 10px;
		background: transparent;
		cursor: pointer;
	}
	.opt-label {
		font-size: 13px;
		font-weight: 600;
		color: rgba(255, 255, 255, 0.75);
	}
	.fill-opts {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 12px;
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

	.group {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		align-items: center;
	}
	.tool {
		min-width: 48px;
		height: 48px;
		padding: 0 12px;
		display: grid;
		place-items: center;
		border: 0;
		border-radius: 14px;
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
		min-width: 140px;
		flex: 1 1 140px;
	}
	.slider b {
		color: var(--pp-on-ink);
	}
	.slider input {
		accent-color: var(--pp-yellow);
	}
	.swatch {
		width: 30px;
		height: 30px;
		border-radius: 999px;
		border: 2px solid #2a2a30;
		cursor: pointer;
		padding: 0;
	}
	.swatch[aria-checked='true'] {
		outline: 3px solid var(--pp-yellow);
		outline-offset: 1px;
	}
	.custom {
		position: relative;
		overflow: hidden;
		cursor: pointer;
		background: conic-gradient(red, yellow, lime, cyan, blue, magenta, red);
	}

	kbd {
		font: 600 12px var(--pp-font-body);
		padding: 1px 6px;
		border-radius: 6px;
		background: var(--pp-surface);
		border: 1px solid var(--pp-track);
	}

	/* ---- Studio layout: top bar · tool rail | options + canvas | dock ---- */
	.studio {
		display: flex;
		flex-direction: column;
		gap: 8px;
		height: var(--studio-h);
		min-height: 540px;
		box-sizing: border-box;
		padding-block: 8px 12px;
		padding-left: max(12px, env(safe-area-inset-left, 0px));
		padding-right: max(12px, env(safe-area-inset-right, 0px));
		overflow: hidden;
	}
	.sbar {
		flex: none;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 12px;
		padding: 8px 10px;
		background: var(--pp-surface);
		border-radius: 18px;
	}
	.title {
		flex: 1 1 220px;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}
	.title :global(.pp-eyebrow) {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
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
	.sbar-actions :global(.btn) {
		min-height: 40px;
		padding: 8px 18px;
		white-space: nowrap;
	}
	.sbar-actions {
		flex: 0 1 auto;
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 8px;
		margin-left: auto;
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
	.icon-btn {
		flex: none;
		width: 40px;
		height: 40px;
		display: grid;
		place-items: center;
		border: 0;
		border-radius: 12px;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
		cursor: pointer;
	}
	.icon-btn[aria-pressed='true'] {
		background: var(--pp-ink);
		color: #fff;
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

	.work {
		flex: 1 1 0;
		min-height: 0;
		display: flex;
		gap: 8px;
		position: relative;
	}
	.rail {
		flex: none;
		display: flex;
		flex-direction: column;
		gap: 12px;
		padding: 8px;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		border-radius: 18px;
		overflow-y: auto;
		scrollbar-width: none;
	}
	.rail-tools {
		display: grid;
		grid-template-columns: repeat(2, 44px);
		gap: 6px;
	}
	.rail .tool {
		min-width: 44px;
		width: 44px;
		height: 44px;
		padding: 0;
		border-radius: 12px;
	}
	.rail-sliders {
		display: flex;
		justify-content: space-around;
		padding-top: 8px;
		border-top: 1px solid #34343c;
	}
	.slider.rail-slider {
		flex: none;
		min-width: 0;
		width: 44px;
		align-items: center;
		text-align: center;
		font-size: 11px;
	}
	.rail-slider input {
		writing-mode: vertical-lr;
		direction: rtl;
		width: 22px;
		height: 120px;
	}

	.center {
		flex: 1 1 0;
		min-width: 0;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.opts,
	.ctx {
		flex: none;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 14px;
		padding: 8px 12px;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		border-radius: 18px;
	}
	.opts {
		flex-wrap: nowrap;
		overflow-x: auto;
		scrollbar-width: thin;
	}
	.opts > :global(*) {
		flex: none;
	}
	.ctx {
		flex-direction: column;
		align-items: stretch;
	}
	.ctx:not(:has(*)) {
		display: none;
	}
	.opts .tool,
	.ctx .tool {
		height: 38px;
		min-width: 38px;
		border-radius: 12px;
	}
	.opts .slider {
		flex: 0 1 150px;
		min-width: 110px;
	}
	.brush-name {
		font: 700 15px var(--pp-font-display);
		white-space: nowrap;
	}
	.stage {
		flex: 1 1 0;
		min-height: 0;
		position: relative;
		background: var(--pp-surface-sunk);
		border-radius: 18px;
		overflow: hidden;
	}
	.pip {
		display: none;
	}

	.dock {
		--ref-max-h: min(28vh, 260px);
		flex: none;
		width: 330px;
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-height: 0;
		overflow-y: auto;
		scrollbar-width: thin;
	}
	.dock-close,
	.dock-btn,
	.scrim {
		display: none;
	}
	.ref-card {
		flex: none;
	}
	.ref-card :global(.pp-tile) {
		padding: 14px;
		gap: 8px;
	}
	.ref-card :global(.credit) {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.tabs {
		flex: none;
		display: flex;
		gap: 4px;
		padding: 4px;
		background: var(--pp-surface-sunk);
		border-radius: 14px;
	}
	.tab {
		flex: 1 1 0;
		min-width: 0;
		display: flex;
		flex-direction: column;
		align-items: center;
		padding: 7px 6px;
		border: 0;
		border-radius: 10px;
		background: transparent;
		font: 700 13px var(--pp-font-body);
		color: var(--pp-ink-2);
		cursor: pointer;
	}
	.tab[aria-selected='true'] {
		background: var(--pp-surface);
		color: var(--pp-ink);
		box-shadow: 0 1px 4px rgba(22, 22, 26, 0.08);
	}
	.tab-meta {
		max-width: 100%;
		font-size: 11px;
		font-weight: 600;
		color: var(--pp-ink-2);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.tab[aria-selected='true'] .tab-meta {
		color: var(--pp-accent-ink);
	}
	.tab-panel {
		padding: 14px;
		background: var(--pp-surface);
		border-radius: 20px;
	}
	.tab-panel :global(.panel) {
		width: auto;
		max-height: none;
		padding: 0;
		box-shadow: none;
	}
	.hint {
		margin: 0;
		padding: 0 6px 6px;
		font-size: 12px;
		color: var(--pp-muted);
	}

	/* Tablets: the dock slides in over the canvas; the reference floats on the canvas. */
	@media (max-width: 1100px) {
		.dock {
			position: absolute;
			top: 0;
			right: 0;
			bottom: 0;
			z-index: 20;
			width: min(340px, 86vw);
			padding: 10px;
			background: var(--pp-ground);
			border-radius: 18px;
			box-shadow: -14px 0 40px rgba(22, 22, 26, 0.18);
			transform: translateX(calc(100% + 24px));
			visibility: hidden;
			transition:
				transform 0.2s ease,
				visibility 0.2s;
		}
		.dock.open {
			transform: none;
			visibility: visible;
		}
		.dock-close {
			display: grid;
			place-items: center;
			align-self: flex-end;
			width: 36px;
			height: 36px;
			border: 0;
			border-radius: 10px;
			background: var(--pp-surface-sunk);
			font-size: 22px;
			cursor: pointer;
		}
		.dock-btn {
			display: grid;
		}
		.scrim {
			display: block;
			position: absolute;
			inset: 0;
			z-index: 19;
			border: 0;
			background: rgba(22, 22, 26, 0.12);
			cursor: pointer;
		}
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

	/* Phones: tools along the bottom, the dock is a bottom sheet. */
	@media (max-width: 700px) {
		.studio {
			padding-left: max(6px, env(safe-area-inset-left, 0px));
			padding-right: max(6px, env(safe-area-inset-right, 0px));
			gap: 6px;
		}
		.sbar {
			gap: 6px;
			padding: 6px;
		}
		.title {
			flex-basis: 180px;
		}
		.title :global(.pp-eyebrow) {
			font-size: 10px;
		}
		.sbar-actions {
			flex: 1 1 100%;
			gap: 6px;
		}
		.sbar-actions :global(button:last-child) {
			margin-left: auto;
		}
		.dock {
			--ref-max-h: 26vh;
		}
		.wide-only {
			display: none;
		}
		.sbar-actions :global(.btn) {
			min-height: 40px;
			padding: 8px 14px;
			font-size: 15px;
		}
		.pip {
			top: auto;
			bottom: 8px;
		}
		h1 {
			font-size: 16px;
		}
		.undo-group .tool:last-child,
		.sbar-actions :global(.picker) {
			display: none;
		}
		.work {
			flex-direction: column-reverse;
			gap: 6px;
		}
		.rail {
			flex-direction: column;
			gap: 8px;
			overflow: visible;
			padding: 6px;
		}
		.rail-tools {
			display: flex;
			gap: 6px;
			overflow-x: auto;
			scrollbar-width: none;
		}
		.rail .tool {
			flex: none;
		}
		.rail-sliders {
			order: -1;
			gap: 12px;
			padding: 0 4px;
			border: 0;
		}
		.slider.rail-slider {
			flex: 1 1 0;
			width: auto;
			align-items: stretch;
			text-align: left;
		}
		.rail-slider input {
			writing-mode: horizontal-tb;
			direction: ltr;
			width: auto;
			height: auto;
		}
		.opts {
			flex-wrap: nowrap;
			overflow-x: auto;
			scrollbar-width: none;
			padding: 6px 8px;
		}
		.opts > :global(*) {
			flex: none;
		}
		.dock {
			top: auto;
			left: 0;
			width: auto;
			max-height: 72%;
			border-radius: 22px 22px 0 0;
			box-shadow: 0 -12px 34px rgba(22, 22, 26, 0.2);
			transform: translateY(calc(100% + 24px));
		}
		.pip {
			left: auto;
			right: 8px;
			width: 110px;
		}
	}
</style>
