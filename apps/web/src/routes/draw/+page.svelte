<script lang="ts">
	import { onMount } from 'svelte';
	import { warmGrader } from '#lib/grader.js';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import PaintCanvas, { BLANK_STATUS, type CanvasStatus } from '#lib/paint/PaintCanvas.svelte';
	import LayersPanel from '#lib/paint/LayersPanel.svelte';
	import CanvasSizePicker from '#lib/paint/CanvasSizePicker.svelte';
	import RefViewer from '#lib/paint/RefViewer.svelte';
	import TracePanel, { TRACE_DEFAULTS, type TraceState } from '#lib/paint/TracePanel.svelte';
	import { savedSize, saveSize, type CanvasSize } from '#lib/paint/canvas-size.js';
	import BrushPicker from '#lib/paint/BrushPicker.svelte';
	import { DEFAULT_BRUSH, loadManifest, type BrushManifest } from '#lib/paint/brushes.js';
	import { categories } from '#lib/categories.js';
	import { saveSubmission } from '#lib/submission.js';
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
	/** phones/tablets fold the brush panel away so the canvas comes first */
	let brushesOpen = $state(false);
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
	}

	// Reference panel: resizable sidebar, or a floating window.
	const SIDE_MIN = 260;
	const SIDE_MAX = 640;
	let sideW = $state(340);
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
	function setSide(w: number) {
		sideW = Math.round(Math.max(SIDE_MIN, Math.min(SIDE_MAX, w)));
		remember('pp.sideWidth', sideW);
	}
	let sideDrag: { x: number; w: number } | null = null;
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

	/** phones/tablets fold the layers panel away too */
	let layersOpen = $state(false);
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
	let confirmClear = $state(false);

	const SWATCHES = ['#16161a', '#6b6a70', '#b9b6ae', '#e8492f', '#3d6bff', '#1f9d74', '#f2a93b'];
	const brushName = $derived(manifest?.brushes.find((b) => b.id === brushId)?.name ?? '…');
	const clock = $derived(
		`${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
	);

	onMount(() => {
		warmGrader();
		canvasSize = savedSize();
		sideW = recall('pp.sideWidth', 340);
		refFloating = recall('pp.refFloating', false);
		floatBox = recall('pp.refFloat', floatBox);
		loadManifest().then((m) => (manifest = m));
		if (!reference) loadReference();
		const id = setInterval(() => {
			if (timerOn && !status.blank) seconds++;
		}, 1000);
		return () => clearInterval(id);
	});

	function onkey(e: KeyboardEvent) {
		const t = e.target as HTMLElement;
		if (t.closest('input, textarea, select')) return;
		const mod = e.ctrlKey || e.metaKey;
		const k = e.key.toLowerCase();
		if (mod && k === 'z') {
			e.preventDefault();
			if (e.shiftKey) canvas?.redo();
			else canvas?.undo();
		} else if (mod && k === 'y') {
			e.preventDefault();
			canvas?.redo();
		} else if (!mod && k === 'b') eraser = false;
		else if (!mod && k === 'e') eraser = true;
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

	async function submit() {
		const d = canvas?.exportDrawing();
		if (!d) return;
		saveSubmission({
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
			traced,
			trace: traced
				? {
						source: trace.source,
						transform: { x: trace.x, y: trace.y, scale: trace.scale, rotate: trace.rotate },
						ownUrl: trace.source === 'own' ? (trace.ownUrl ?? undefined) : undefined
					}
				: undefined
		});
		await goto('/results');
	}
</script>

<svelte:window onkeydown={onkey} />
<svelte:head><title>Draw · PencilPilot</title></svelte:head>

<div class="pp-page">
	<div class="top">
		<div>
			<span class="pp-eyebrow">
				{category ? `${category.name} · ` : ''}{reference
					? [topicName(reference.category, reference.topic), tierName(tierOf(reference))]
							.filter(Boolean)
							.join(' · ')
					: 'Free draw'}
			</span>
			<h1>{reference ? titleOf(reference) : 'Draw anything'}</h1>
		</div>
		<div class="top-actions">
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
			<CanvasSizePicker size={canvasSize} blank={status.blank} onpick={pickSize} />
			<Button onclick={submit} disabled={status.blank}>
				{traced ? 'Get tips →' : 'Submit drawing →'}
			</Button>
		</div>
	</div>

	<div class="layout" style="--side: {sideW}px; --ref-max-h: {Math.round(sideW * 0.95)}px">
		<div class="ref-area">
			{#if reference}
				<Tile as="section" label="Reference">
					<div class="tile-head">
						<h2>Reference</h2>
						{#if category}
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
						By <a href={reference.authorUrl} target="_blank" rel="noreferrer">{reference.author}</a>
						via
						<a href={reference.pageUrl} target="_blank" rel="noreferrer">Pixabay</a> ·
						<a href={reference.licenseUrl} target="_blank" rel="noreferrer">{reference.license}</a>
						{#if reference.live}· <span class="badge-live">Endless</span>{/if}
					</p>
					{#if reported}
						<p class="credit" role="status">Thanks! That picture is blocked for everyone.</p>
					{:else if reportOpen}
						<div class="report" role="group" aria-label="Report this picture">
							<span>What’s wrong?</span>
							<button type="button" onclick={() => report('unsafe')}>Not safe</button>
							<button type="button" onclick={() => report('ai_made')}>Looks AI-made</button>
							<button type="button" onclick={() => report('not_drawable')}>Can’t draw this</button>
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
		<!-- ARIA window-splitter pattern: a focusable separator resized with the arrow keys -->
		<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
		<div
			class="splitter"
			role="separator"
			aria-orientation="vertical"
			aria-label="Resize the side panel"
			aria-valuemin={SIDE_MIN}
			aria-valuemax={SIDE_MAX}
			aria-valuenow={sideW}
			tabindex="0"
			onpointerdown={(e) => {
				e.currentTarget.setPointerCapture(e.pointerId);
				sideDrag = { x: e.clientX, w: sideW };
			}}
			onpointermove={(e) => sideDrag && setSide(sideDrag.w + e.clientX - sideDrag.x)}
			onpointerup={() => (sideDrag = null)}
			onkeydown={(e) => {
				if (e.key === 'ArrowLeft') setSide(sideW - 20);
				if (e.key === 'ArrowRight') setSide(sideW + 20);
			}}
		></div>
		<div class="layers-area">
			<Tile as="section" label="Layers">
				<div class="tile-head">
					<h2>Layers</h2>
					<span class="current">{activeLayerName}</span>
					<button
						type="button"
						class="brush-toggle"
						aria-expanded={layersOpen}
						aria-controls="layers-body"
						onclick={() => (layersOpen = !layersOpen)}
					>
						{layersOpen ? 'Hide' : 'Show'}
					</button>
				</div>
				<div id="layers-body" class="brush-body" class:closed={!layersOpen}>
					<LayersPanel {status} controls={canvas} />
				</div>
			</Tile>
		</div>
		<div class="brush-area">
			<Tile as="section" label="Brushes">
				<div class="tile-head">
					<h2>Brushes</h2>
					<span class="current">{brushName}</span>
					<button
						type="button"
						class="brush-toggle"
						aria-expanded={brushesOpen}
						aria-controls="brush-body"
						onclick={() => (brushesOpen = !brushesOpen)}
					>
						{brushesOpen ? 'Hide' : 'Change'}
					</button>
				</div>
				<div id="brush-body" class="brush-body" class:closed={!brushesOpen}>
					{#if manifest}
						<BrushPicker {manifest} bind:selected={brushId} />
					{:else}
						<p class="muted">Loading brushes…</p>
					{/if}
				</div>
			</Tile>
		</div>

		<div class="canvas-col">
			<div class="canvas-tile">
				{#key `${canvasSize.width}x${canvasSize.height}`}
					<PaintCanvas
						bind:this={canvas}
						width={canvasSize.width}
						height={canvasSize.height}
						{brushId}
						{eraser}
						{size}
						{opacity}
						{color}
						overlaySrc={traceSrc}
						overlayOpacity={trace.opacity}
						overlayTransform={trace}
						adjustOverlay={trace.adjusting}
						onoverlaymove={(dx, dy) => (trace = { ...trace, x: trace.x + dx, y: trace.y + dy })}
						onchange={(s) => (status = s)}
						onblocked={onBlocked}
					/>
				{/key}
				{#if blocked}
					<p class="blocked" role="status">
						{blocked === 'hidden'
							? `“${activeLayerName}” is hidden. Show it or pick another layer to draw.`
							: `“${activeLayerName}” is locked. Unlock it or pick another layer to draw.`}
					</p>
				{/if}
			</div>

			<div class="toolbar" role="toolbar" aria-label="Drawing tools">
				<div class="group">
					<button
						type="button"
						class="tool"
						aria-pressed={!eraser}
						aria-label="Brush (B)"
						title="Brush (B)"
						onclick={() => (eraser = false)}
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
						aria-pressed={eraser}
						aria-label="Eraser (E)"
						title="Eraser (E) — uses the current brush shape"
						onclick={() => (eraser = true)}
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
				</div>
				<div class="group">
					<button
						type="button"
						class="tool"
						aria-label="Undo (Ctrl+Z)"
						title="Undo (Ctrl+Z)"
						disabled={!status.canUndo}
						onclick={() => canvas?.undo()}
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
				<label class="slider">
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
				<label class="slider">
					<span>Opacity <b>{Math.round(opacity * 100)}%</b></span>
					<input type="range" min="0.05" max="1" step="0.05" bind:value={opacity} />
				</label>
				<button
					type="button"
					class="tool"
					aria-pressed={tracing}
					title="Show a picture faintly inside the canvas to trace over (no score)"
					onclick={toggleTracing}
				>
					Trace
				</button>
				<div class="group swatches" role="radiogroup" aria-label="Colour">
					{#each SWATCHES as c (c)}
						<button
							type="button"
							role="radio"
							aria-checked={color === c}
							aria-label="Colour {c}"
							class="swatch"
							style="background: {c}"
							onclick={() => (color = c)}
						></button>
					{/each}
					<label class="swatch custom" title="Pick any colour">
						<span class="visually-hidden">Custom colour</span>
						<input type="color" bind:value={color} />
					</label>
				</div>
			</div>
			{#if tracing}
				<TracePanel bind:trace hasReference={!!reference} onclose={toggleTracing} />
			{/if}
		</div>
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

	<p class="hint">
		Shortcuts: <kbd>B</kbd> brush · <kbd>E</kbd> eraser · <kbd>[</kbd> <kbd>]</kbd> size ·
		<kbd>Ctrl</kbd>+<kbd>Z</kbd> undo · <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> redo. Pen pressure
		and tilt work with tablets.
	</p>
</div>

<style>
	.top {
		display: flex;
		flex-wrap: wrap;
		gap: 16px;
		align-items: center;
		justify-content: space-between;
		margin-bottom: 16px;
	}
	h1 {
		font-size: 30px;
		letter-spacing: -0.8px;
	}
	h2 {
		font-size: 20px;
		font-weight: 700;
		letter-spacing: -0.3px;
	}
	.top-actions {
		display: flex;
		gap: 12px;
		align-items: center;
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
	.layout {
		display: grid;
		grid-template-columns: var(--side, 340px) 0 minmax(0, 1fr);
		grid-template-rows: auto auto 1fr;
		grid-template-areas: 'ref split canvas' 'layers split canvas' 'brushes split canvas';
		column-gap: calc(var(--pp-gap) / 2);
		gap: var(--pp-gap);
		align-items: start;
	}
	.ref-area:empty {
		display: none;
	}
	.ref-area {
		grid-area: ref;
		min-width: 0;
	}
	.brush-area {
		grid-area: brushes;
		min-width: 0;
	}
	.layers-area {
		grid-area: layers;
		min-width: 0;
	}
	.splitter {
		grid-area: split;
		position: relative;
		align-self: stretch;
		cursor: col-resize;
		touch-action: none;
	}
	.splitter::after {
		content: '';
		position: absolute;
		top: 0;
		bottom: 0;
		left: -6px;
		width: 12px;
		border-radius: 999px;
	}
	.splitter:hover::after,
	.splitter:focus-visible::after {
		background: color-mix(in srgb, var(--pp-ink) 12%, transparent);
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
	.canvas-col {
		grid-area: canvas;
	}
	.brush-toggle {
		display: none;
	}
	/* Tablets and phones: reference, then the canvas, then a folded-away brush panel. */
	@media (max-width: 960px) {
		.layout {
			grid-template-columns: minmax(0, 1fr);
			grid-template-rows: none;
			grid-template-areas: 'ref' 'canvas' 'layers' 'brushes';
		}
		.splitter,
		.float-btn {
			display: none;
		}
		.layout {
			--ref-max-h: 34vh !important;
		}
		.brush-toggle {
			display: block;
			margin-left: auto;
			padding: 8px 14px;
			border: 0;
			border-radius: 999px;
			background: var(--pp-ink);
			color: #fff;
			font: 600 14px var(--pp-font-body);
			cursor: pointer;
		}
		.brush-body.closed {
			display: none;
		}
	}
	.tile-head {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		gap: 8px;
	}
	.current {
		font-size: 14px;
		font-weight: 600;
		color: var(--pp-accent-ink);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.muted {
		color: var(--pp-muted);
	}
	.canvas-col {
		display: flex;
		flex-direction: column;
		gap: var(--pp-gap);
		min-width: 0;
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

	.canvas-tile {
		position: relative;
		background: #fff;
		border-radius: var(--pp-radius-tile);
		overflow: hidden;
		box-shadow: 0 1px 0 rgba(22, 22, 26, 0.04);
	}
	.toolbar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 12px 20px;
		padding: 14px 16px;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		border-radius: var(--pp-radius-card);
	}
	.group {
		display: flex;
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
		background: conic-gradient(red, yellow, lime, cyan, blue, magenta, red);
	}
	.custom input {
		position: absolute;
		inset: -8px;
		opacity: 0;
		cursor: pointer;
	}
	.hint {
		margin: 16px 0 0;
		font-size: 13px;
		color: var(--pp-muted);
	}
	kbd {
		font: 600 12px var(--pp-font-body);
		padding: 1px 6px;
		border-radius: 6px;
		background: var(--pp-surface);
		border: 1px solid var(--pp-track);
	}
	.visually-hidden {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
</style>
