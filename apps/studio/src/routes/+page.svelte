<script lang="ts">
	import { resolve } from '$app/paths';
	/**
	 * The drawing studio, in the app: the website's studio (same layout, tools and panels,
	 * from @pencilpilot/paint) without the website's practice parts, plus projects:
	 * Ctrl+S saves (.ppstudio), Ctrl+Shift+S saves as, Ctrl+O opens.
	 */
	import Studio from '@pencilpilot/paint/Studio.svelte';
	import ToolRail from '@pencilpilot/paint/ToolRail.svelte';
	import PaintCanvas, { type CanvasStatus } from '@pencilpilot/paint/PaintCanvas.svelte';
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
	import {
		askOpenPath,
		askSavePath,
		openProject,
		projectName,
		saveProject,
		type ProjectDoc
	} from '#lib/project.js';

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
	let canvasSize = $state({ width: 1920, height: 1080 });
	let fps = $state(24);
	/** bumps for a new, empty canvas (opening a project of another size remounts it) */
	let canvasKey = $state(0);

	// ---- the project file ----
	let path = $state<string | null>(null);
	let created = $state<string | undefined>(undefined);
	/** the canvas version when last saved or opened (anything newer is unsaved) */
	let savedVersion = $state(1);
	let busy = $state<'' | 'saving' | 'opening'>('');
	let fileNote = $state('');
	const unsaved = $derived(!!status && !status.blank && status.version !== savedVersion);
	/** a project being opened, waiting for its canvas */
	let pending = $state.raw<ProjectDoc | null>(null);

	async function save(as = false) {
		if (!canvas || busy) return;
		const target = as || !path ? await askSavePath(projectName(path)) : path;
		if (!target) return;
		busy = 'saving';
		fileNote = '';
		try {
			const snap = canvas.snapshotLayers();
			if (!snap) throw new Error('the canvas isn’t ready');
			const versionAtSave = status?.version ?? 0;
			path = await saveProject(target, {
				width: canvasSize.width,
				height: canvasSize.height,
				fps,
				layers: snap.layers,
				groups: snap.groups,
				activeLayer: status?.active ?? 0,
				strokes: snap.strokes,
				extra: { brushId, color, size, opacity, smoothing },
				created,
				thumbnail: await canvas.thumbnail()
			});
			created ??= new Date().toISOString();
			savedVersion = versionAtSave;
		} catch (e) {
			fileNote = `Couldn’t save: ${e}`;
		} finally {
			busy = '';
		}
	}

	async function open() {
		if (busy) return;
		const target = await askOpenPath();
		if (!target) return;
		busy = 'opening';
		fileNote = '';
		try {
			const doc = await openProject(target);
			path = target;
			created = doc.created;
			fps = doc.fps;
			const x = doc.extra;
			if (typeof x.brushId === 'string') brushId = x.brushId;
			if (typeof x.color === 'string') color = x.color;
			if (typeof x.size === 'number') size = x.size;
			if (typeof x.opacity === 'number') opacity = x.opacity;
			if (typeof x.smoothing === 'number') smoothing = x.smoothing;
			// a fresh canvas of the project's size, then its layers
			status = null;
			canvasSize = { width: doc.width, height: doc.height };
			canvasKey++;
			pending = doc;
			await tick();
		} catch (e) {
			fileNote = `Couldn’t open: ${e}`;
			busy = '';
		}
	}

	// the new canvas is ready: put the project's layers on it
	$effect(() => {
		const doc = pending;
		if (!doc || !canvas || !status) return;
		pending = null;
		canvas.restoreLayers(doc.layers, doc.groups, doc.strokes);
		if (doc.activeLayer) canvas.selectLayer(doc.activeLayer);
		// just opened: nothing unsaved (the canvas reported its new state as it restored)
		savedVersion = untrack(() => status?.version ?? 0);
		busy = '';
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
		{ id: 'layers', label: 'Layers', meta: `${status?.layers.length ?? 1} layers` },
		{ id: 'brushes', label: 'Brushes', meta: manifest ? brushLabel(manifest, brushId) : '…' },
		{ id: 'colour', label: 'Colour' },
		{ id: 'history', label: 'History' }
	]);

	const TOOL_KEYS: Record<string, string> = {
		i: 'picker',
		g: 'fill',
		f: 'lasso-fill',
		u: 'measure',
		s: 'smudge',
		p: 'pen',
		v: 'transform',
		k: 'liquify',
		m: 'select-rect',
		l: 'select-lasso',
		w: 'select-wand'
	};

	function onkey(e: KeyboardEvent) {
		if ((e.target as HTMLElement).closest('input, textarea, select')) return;
		const mod = e.ctrlKey || e.metaKey;
		const k = e.key.toLowerCase();
		if (mod && k === 's') {
			e.preventDefault();
			void save(e.shiftKey);
		} else if (mod && k === 'o') {
			e.preventDefault();
			void open();
		} else if (mod && k === 'z') {
			e.preventDefault();
			if (e.shiftKey) canvas?.redo();
			else canvas?.undo();
		} else if (mod && k === 'y') {
			e.preventDefault();
			canvas?.redo();
		} else if (!mod && k === 'b') {
			eraser = false;
			tool = 'brush';
		} else if (!mod && k === 'e') {
			eraser = true;
			tool = 'brush';
		} else if (!mod && TOOL_KEYS[k]) tool = tool === TOOL_KEYS[k] ? 'brush' : TOOL_KEYS[k];
		else if (!mod && e.key === '[') size = Math.max(0.25, +(size / 1.25).toFixed(2));
		else if (!mod && e.key === ']') size = Math.min(6, +(size * 1.25).toFixed(2));
		else if (tool === 'transform' && e.key === 'Enter') canvas?.transformCommit();
		else if (tool === 'transform' && e.key === 'Escape') canvas?.transformCancel();
		else if (tool === 'pen' && e.key === 'Enter') {
			if (canvas?.penStroke(true)) canvas.penClear();
		} else if (tool === 'pen' && e.key === 'Escape') canvas?.penClear();
	}
</script>

<svelte:window onkeydown={onkey} />

<div class="app">
	<Studio tabs={TABS} bind:tab bind:dockOpen>
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
				title="Open a project (Ctrl+O)"
			>
				{busy === 'opening' ? 'Opening…' : 'Open'}
			</button>
			<button
				type="button"
				class="act"
				disabled={!!busy}
				onclick={() => save(false)}
				title="Save (Ctrl+S); Save as: Ctrl+Shift+S"
			>
				{busy === 'saving' ? 'Saving…' : 'Save'}
			</button>
			<button
				type="button"
				class="act"
				disabled={!status?.canUndo}
				onclick={() => canvas?.undo()}
				aria-label="Undo (Ctrl+Z)"
			>
				Undo
			</button>
			<button
				type="button"
				class="act"
				disabled={!status?.canRedo}
				onclick={() => canvas?.redo()}
				aria-label="Redo (Ctrl+Shift+Z)"
			>
				Redo
			</button>
			<a class="act" href={resolve('/pen-test')}>Pen test</a>
		{/snippet}
		{#snippet rail()}
			<ToolRail bind:tool bind:eraser bind:size bind:opacity />
		{/snippet}
		{#snippet options()}
			<span class="brush-name">{manifest ? brushLabel(manifest, brushId) : ''}</span>
			<Swatches bind:color onmore={() => ((tab = 'colour'), (dockOpen = true))} />
			<label class="slider">
				<span>Smoothing <b>{Math.round(smoothing * 100)}%</b></span>
				<input type="range" min="0" max="1" step="0.05" bind:value={smoothing} />
			</label>
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
					brushTexture={texture}
					allowImport
					onpick={(c) => (color = c)}
					onsize={(v) => (size = v)}
					onchange={(s) => (status = s)}
				/>
			{/key}
		{/snippet}
		{#snippet panel(t: string)}
			{#if t === 'layers' && status}
				<LayersPanel {status} controls={canvas} aspect={canvasSize.width / canvasSize.height} />
			{:else if t === 'brushes'}
				{#if manifest}
					<BrushPicker {manifest} bind:selected={brushId} />
					<BrushTexturePanel {brushId} onpick={(id) => (brushId = id)} />
				{:else}
					<p>Loading brushes…</p>
				{/if}
			{:else if t === 'history' && status}
				<HistoryPanel history={status.history} ongo={(n) => canvas?.historyGo(n)} />
			{:else if t === 'colour'}
				<ColorPanel bind:color onclose={() => (tab = 'layers')} />
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
</style>
