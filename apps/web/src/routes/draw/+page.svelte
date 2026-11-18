<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import PaintCanvas, { type CanvasStatus } from '#lib/paint/PaintCanvas.svelte';
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
	import { topicName } from '#lib/topics.js';

	const category = categories.find((c) => c.id === page.url.searchParams.get('category'));
	const filter = {
		topic: page.url.searchParams.get('topic'),
		tier: Number(page.url.searchParams.get('tier')) || null
	};
	let reference = $state<Reference | undefined>(byId(page.url.searchParams.get('ref')));
	let refLoading = $state(false);
	let peek = $state(false);
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
		peek = false;
		reported = false;
		reportOpen = false;
		const avoid = [...recentRefs(), ...(skip ? [skip] : [])];
		const unseenCurated = filtered(category.id, filter).filter((r) => !avoid.includes(r.id));
		let r: Reference | undefined =
			filter.tier === 1 && unseenCurated.length ? pickCurated(avoid) : undefined;
		r ??=
			(await fetchLiveReference({
				category: category.id,
				topic: filter.topic,
				tier: filter.tier,
				exclude: avoid
			})) ?? undefined;
		r ??= pickCurated(avoid) ?? pickCurated(avoid, { tier: filter.tier });
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
	let status = $state<CanvasStatus>({ canUndo: false, canRedo: false, blank: true, strokes: 0 });
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
			strokeData: d.strokes
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
			<Button onclick={submit} disabled={status.blank}>Submit drawing →</Button>
		</div>
	</div>

	<div class="layout">
		<div class="side">
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
					{#key reference.id}
						<a class="ref-img" href={imageUrl(reference)} target="_blank" rel="noreferrer">
							<img
								class="fade-in"
								src={imageUrl(reference)}
								alt="Reference: {reference.tags.join(', ')}"
							/>
						</a>
					{/key}
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
			<Tile as="section" label="Brushes">
				<div class="tile-head">
					<h2>Brushes</h2>
					<span class="current">{brushName}</span>
				</div>
				{#if manifest}
					<BrushPicker {manifest} bind:selected={brushId} />
				{:else}
					<p class="muted">Loading brushes…</p>
				{/if}
			</Tile>
		</div>

		<div class="canvas-col">
			<div class="canvas-tile">
				<PaintCanvas
					bind:this={canvas}
					{brushId}
					{eraser}
					{size}
					{opacity}
					{color}
					onchange={(s) => (status = s)}
				/>
				{#if reference && peek}
					<img class="peek" src={imageUrl(reference)} alt="" />
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
							Clear all?
						</button>
						<button type="button" class="tool" onclick={() => (confirmClear = false)}>Keep</button>
					{:else}
						<button
							type="button"
							class="tool"
							aria-label="Clear canvas"
							title="Clear canvas"
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
				{#if reference}
					<button
						type="button"
						class="tool"
						aria-pressed={peek}
						title="Show the reference faintly over your canvas"
						onclick={() => (peek = !peek)}
					>
						Peek
					</button>
				{/if}
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
		</div>
	</div>
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
		grid-template-columns: 340px minmax(0, 1fr);
		gap: var(--pp-gap);
		align-items: start;
	}
	@media (max-width: 960px) {
		.layout {
			grid-template-columns: minmax(0, 1fr);
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
	.side {
		display: flex;
		flex-direction: column;
		gap: var(--pp-gap);
		min-width: 0;
	}
	.ref-img img {
		display: block;
		width: 100%;
		max-height: 320px;
		object-fit: contain;
		border-radius: 18px;
		background: var(--pp-surface-sunk);
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
	.fade-in {
		animation: fade 0.35s ease-out;
	}
	@keyframes fade {
		from {
			opacity: 0;
			transform: scale(0.98);
		}
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
	.peek {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		object-fit: contain;
		opacity: 0.3;
		pointer-events: none;
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
		background: var(--pp-accent);
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
